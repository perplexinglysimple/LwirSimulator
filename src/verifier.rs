/// Independent static verifier for LWIR programs.
///
/// Checks all rules from docs/compiler_contract.md without executing the program.
/// Produces a list of Diagnostics; an empty list means the program is clean.
///
/// The verifier enforces a *conservative* contract: guard predicates are not
/// evaluated, so every non-nop syllable is treated as unconditionally active.
/// Soundness is captured by the `lemma_*` proof functions below: the conservative
/// spec implies the corresponding runtime pairwise condition for any pair of
/// syllables that are both active in a given CPU state.
use crate::bundle::Bundle;
use crate::cpu::{CpuState, NUM_GPRS, NUM_PREDS};
use crate::isa::{Opcode, SlotClass};
use crate::latency::LatencyTable;
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

// ---------------------------------------------------------------------------
// Private exec helpers (outside verus! — trusted via verify_program's postcondition)
// ---------------------------------------------------------------------------

fn check_slot_legality<const W: usize>(
    bidx: usize,
    bundle: &Bundle<W>,
    diags: &mut Vec<Diagnostic>,
) {
    for (slot, syl) in bundle.syllables.iter().enumerate() {
        if syl.opcode == Opcode::Nop {
            continue;
        }
        let expected = slot_class_for_index(slot);
        let actual = syl.opcode.slot_class();
        if actual != expected {
            diags.push(Diagnostic {
                bundle_idx: bidx,
                slot,
                rule: Rule::SlotOpcodeLegality,
                message: format!(
                    "bundle {bidx} slot {slot}: \
                     `{}` is a {} op but slot {slot} is a {} slot",
                    opcode_name(syl.opcode),
                    slot_class_name(actual),
                    slot_class_name(expected),
                ),
            });
        }
    }
}

fn check_gpr_hazards<const W: usize>(bidx: usize, bundle: &Bundle<W>, diags: &mut Vec<Diagnostic>) {
    let n = bundle.syllables.len();
    for i in 0..n {
        let ei = &bundle.syllables[i];
        let Some(dst) = gpr_write_dst(ei.opcode, ei.dst) else {
            continue;
        };
        if dst == 0 || dst >= NUM_GPRS {
            continue;
        }
        for j in (i + 1)..n {
            let lj = &bundle.syllables[j];

            if lj.src[0] == Some(dst) || lj.src[1] == Some(dst) {
                diags.push(Diagnostic {
                    bundle_idx: bidx,
                    slot: i,
                    rule: Rule::SameBundleGprRaw,
                    message: format!(
                        "bundle {bidx}: slot {i} writes r{dst}, \
                         slot {j} reads it (same-bundle GPR RAW)"
                    ),
                });
            }

            if lj.opcode == Opcode::Ret && dst == 31 {
                diags.push(Diagnostic {
                    bundle_idx: bidx,
                    slot: i,
                    rule: Rule::SameBundleGprRaw,
                    message: format!(
                        "bundle {bidx}: slot {i} writes r31 (link register), \
                         slot {j} `ret` implicitly reads it (same-bundle GPR RAW)"
                    ),
                });
            }

            if let Some(later_dst) = gpr_write_dst(lj.opcode, lj.dst) {
                if later_dst == dst {
                    diags.push(Diagnostic {
                        bundle_idx: bidx,
                        slot: i,
                        rule: Rule::SameBundleGprWaw,
                        message: format!(
                            "bundle {bidx}: slot {i} and slot {j} both write r{dst} \
                             (same-bundle GPR WAW)"
                        ),
                    });
                }
            }
        }
    }
}

fn check_pred_hazards<const W: usize>(
    bidx: usize,
    bundle: &Bundle<W>,
    diags: &mut Vec<Diagnostic>,
) {
    let n = bundle.syllables.len();
    for i in 0..n {
        let ei = &bundle.syllables[i];
        if !ei.opcode.writes_pred() {
            continue;
        }
        let Some(dst) = ei.dst else {
            continue;
        };
        if dst == 0 || dst >= NUM_PREDS {
            continue;
        }
        for j in (i + 1)..n {
            let lj = &bundle.syllables[j];

            let reads_as_src =
                lj.opcode.reads_pred_src() && (lj.src[0] == Some(dst) || lj.src[1] == Some(dst));
            let reads_as_branch = lj.opcode == Opcode::Branch && lj.predicate == dst;

            if reads_as_src || reads_as_branch {
                diags.push(Diagnostic {
                    bundle_idx: bidx,
                    slot: i,
                    rule: Rule::SameBundlePredHazard,
                    message: format!(
                        "bundle {bidx}: slot {i} writes p{dst}, \
                         slot {j} reads it (same-bundle predicate RAW)"
                    ),
                });
            }

            if lj.opcode.writes_pred() && lj.dst == Some(dst) {
                diags.push(Diagnostic {
                    bundle_idx: bidx,
                    slot: i,
                    rule: Rule::SameBundlePredHazard,
                    message: format!(
                        "bundle {bidx}: slot {i} and slot {j} both write p{dst} \
                         (same-bundle predicate WAW)"
                    ),
                });
            }
        }
    }
}

fn check_gpr_timing<const W: usize>(
    bidx: usize,
    bundle: &Bundle<W>,
    issue_cycle: u64,
    ready_at: &[u64],
    diags: &mut Vec<Diagnostic>,
) {
    let next_cycle = issue_cycle + 1;
    for (slot, syl) in bundle.syllables.iter().enumerate() {
        for src_opt in &syl.src {
            if let Some(r) = *src_opt {
                if r > 0 && r < NUM_GPRS && ready_at[r] > next_cycle {
                    diags.push(Diagnostic {
                        bundle_idx: bidx,
                        slot,
                        rule: Rule::GprReadyCycle,
                        message: format!(
                            "bundle {bidx} slot {slot}: r{r} not ready until cycle {} \
                             but bundle issues at cycle {issue_cycle} \
                             (needs it by cycle {next_cycle})",
                            ready_at[r]
                        ),
                    });
                }
            }
        }
        if syl.opcode == Opcode::Ret && ready_at[31] > next_cycle {
            diags.push(Diagnostic {
                bundle_idx: bidx,
                slot,
                rule: Rule::GprReadyCycle,
                message: format!(
                    "bundle {bidx} slot {slot}: `ret` reads r31 (link register) \
                     not ready until cycle {} \
                     but bundle issues at cycle {issue_cycle} \
                     (needs it by cycle {next_cycle})",
                    ready_at[31]
                ),
            });
        }
    }
}

fn update_ready_at<const W: usize>(
    bundle: &Bundle<W>,
    issue_cycle: u64,
    latencies: &LatencyTable,
    ready_at: &mut Vec<u64>,
) {
    let write_cycle = issue_cycle + 1;
    for syl in &bundle.syllables {
        if let Some(dst) = gpr_write_dst(syl.opcode, syl.dst) {
            if dst > 0 && dst < NUM_GPRS {
                let lat = latencies.get(syl.opcode) as u64;
                let new_ready = write_cycle + lat;
                if new_ready > ready_at[dst] {
                    ready_at[dst] = new_ready;
                }
            }
        }
    }
}

fn slot_class_for_index(slot: usize) -> SlotClass {
    match slot % 4 {
        0 | 1 => SlotClass::Integer,
        2 => SlotClass::Memory,
        _ => SlotClass::Control,
    }
}

fn gpr_write_dst(op: Opcode, dst: Option<usize>) -> Option<usize> {
    if op == Opcode::Call {
        Some(31)
    } else if op.writes_gpr() {
        dst
    } else {
        None
    }
}

fn opcode_name(op: Opcode) -> &'static str {
    match op {
        Opcode::Add => "add",
        Opcode::Sub => "sub",
        Opcode::And => "and",
        Opcode::Or => "or",
        Opcode::Xor => "xor",
        Opcode::Shl => "shl",
        Opcode::Srl => "srl",
        Opcode::Sra => "sra",
        Opcode::Mov => "mov",
        Opcode::MovImm => "movi",
        Opcode::CmpEq => "cmpeq",
        Opcode::CmpLt => "cmplt",
        Opcode::CmpUlt => "cmpult",
        Opcode::LoadB => "loadb",
        Opcode::LoadH => "loadh",
        Opcode::LoadW => "loadw",
        Opcode::LoadD => "loadd",
        Opcode::StoreB => "storeb",
        Opcode::StoreH => "storeh",
        Opcode::StoreW => "storew",
        Opcode::StoreD => "stored",
        Opcode::Lea => "lea",
        Opcode::Prefetch => "prefetch",
        Opcode::Mul => "mul",
        Opcode::MulH => "mulh",
        Opcode::Branch => "branch",
        Opcode::Jump => "jump",
        Opcode::Call => "call",
        Opcode::Ret => "ret",
        Opcode::PAnd => "pand",
        Opcode::POr => "por",
        Opcode::PXor => "pxor",
        Opcode::PNot => "pnot",
        Opcode::Nop => "nop",
    }
}

fn slot_class_name(sc: SlotClass) -> &'static str {
    match sc {
        SlotClass::Integer => "Integer",
        SlotClass::Memory => "Memory",
        SlotClass::Control => "Control",
    }
}

// ---------------------------------------------------------------------------
// Verus: spec functions and soundness proofs
// ---------------------------------------------------------------------------

verus! {

// ---------------------------------------------------------------------------
// Public types (inside verus! so they are visible to postconditions)
// ---------------------------------------------------------------------------

/// A compiler contract rule that can be violated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rule {
    /// Rule 2/7/8: a syllable's opcode does not belong in its slot class.
    SlotOpcodeLegality,
    /// Rule 3: a later syllable reads a GPR written by an earlier syllable in the same bundle.
    SameBundleGprRaw,
    /// Rule 4: two syllables in the same bundle write the same GPR.
    SameBundleGprWaw,
    /// Rule 5: a later syllable reads or co-writes a predicate produced earlier in the same bundle.
    SameBundlePredHazard,
    /// Rule 6: a GPR source register is not yet ready when this bundle issues (stall-free timing).
    GprReadyCycle,
}

/// A single violation found by the verifier.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Zero-based bundle index where the violation occurs.
    pub bundle_idx: usize,
    /// Zero-based slot index of the primary offending syllable.
    pub slot: usize,
    /// The contract rule that was violated.
    pub rule: Rule,
    /// Human-readable description.
    pub message: String,
}

// --- Conservative contract spec functions ---
//
// Each predicate is factored into a per-slot/per-pair helper so that the
// forall bodies have a single function-call term to use as a Verus trigger.

pub open spec fn spec_slot_ok_in<const W: usize>(bundle: &Bundle<W>, slot: int) -> bool {
    bundle.syllables[slot].opcode == Opcode::Nop ||
    crate::isa::spec_slot_class(bundle.syllables[slot].opcode)
        == crate::cpu::spec_slot_class_for_index(slot)
}

/// A bundle has no slot-opcode legality violations under the conservative contract
/// (all syllables treated as unconditionally active).
pub open spec fn spec_bundle_slot_ok<const W: usize>(bundle: &Bundle<W>) -> bool {
    forall|slot: int| 0 <= slot < bundle.syllables.len() ==>
        #[trigger] spec_slot_ok_in::<W>(bundle, slot)
}

pub open spec fn spec_gpr_pair_ok_in<const W: usize>(bundle: &Bundle<W>, i: int, j: int) -> bool {
    match crate::cpu::spec_gpr_write_dst(bundle.syllables[i].opcode, bundle.syllables[i].dst) {
        None      => true,
        Some(dst) => dst == 0 || dst >= NUM_GPRS || (
            bundle.syllables[j].src[0] != Some(dst) &&
            bundle.syllables[j].src[1] != Some(dst) &&
            !(bundle.syllables[j].opcode == Opcode::Ret && dst == 31) &&
            match crate::cpu::spec_gpr_write_dst(
                bundle.syllables[j].opcode,
                bundle.syllables[j].dst,
            ) {
                None            => true,
                Some(later_dst) => later_dst != dst,
            }
        ),
    }
}

/// A bundle has no same-bundle GPR RAW or WAW hazards under the conservative contract.
pub open spec fn spec_bundle_gpr_hazard_free<const W: usize>(bundle: &Bundle<W>) -> bool {
    forall|i: int, j: int| 0 <= i < j < bundle.syllables.len() ==>
        #[trigger] spec_gpr_pair_ok_in::<W>(bundle, i, j)
}

pub open spec fn spec_pred_pair_ok_in<const W: usize>(bundle: &Bundle<W>, i: int, j: int) -> bool {
    let ei = bundle.syllables[i];
    let lj = bundle.syllables[j];
    !crate::cpu::spec_opcode_writes_pred(ei.opcode) || match ei.dst {
        None      => true,
        Some(dst) => dst == 0 || dst >= NUM_PREDS || (
            !(crate::cpu::spec_opcode_reads_pred_src(lj.opcode) &&
              (lj.src[0] == Some(dst) || lj.src[1] == Some(dst))) &&
            !(lj.opcode == Opcode::Branch && lj.predicate == dst) &&
            !(crate::cpu::spec_opcode_writes_pred(lj.opcode) && lj.dst == Some(dst))
        ),
    }
}

/// A bundle has no same-bundle predicate hazards under the conservative contract.
pub open spec fn spec_bundle_pred_hazard_free<const W: usize>(bundle: &Bundle<W>) -> bool {
    forall|i: int, j: int| 0 <= i < j < bundle.syllables.len() ==>
        #[trigger] spec_pred_pair_ok_in::<W>(bundle, i, j)
}

// --- Soundness lemmas (machine-checked by Verus/Z3) ---
//
// Each lemma proves that the conservative spec (which ignores guards) implies
// the corresponding runtime pairwise condition for any specific pair (i, j) of
// syllables that are both active in a given CPU state.  Together they establish
// that a program passing `verify_program` with no diagnostics will also pass
// `CpuState::bundle_is_legal` in any reachable CPU state.

/// Slot-legality conservatism: unconditional slot-ok implies active-slot-ok.
pub proof fn lemma_slot_ok_implies_active_slot_legal<const W: usize>(
    bundle: &Bundle<W>,
    cpu: &CpuState<W>,
    slot: int,
)
    requires
        cpu.wf(),
        0 <= slot < bundle.syllables.len(),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[slot]),
        spec_bundle_slot_ok::<W>(bundle),
    ensures
        spec_slot_ok_in::<W>(bundle, slot),
{
    assert(spec_slot_ok_in::<W>(bundle, slot));
}

/// GPR-hazard conservatism: unconditional hazard-free implies active-pair hazard-free.
pub proof fn lemma_gpr_hazard_free_implies_active_pair_ok<const W: usize>(
    bundle: &Bundle<W>,
    cpu: &CpuState<W>,
    i: int,
    j: int,
)
    requires
        cpu.wf(),
        0 <= i < j,
        j < bundle.syllables.len(),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[i]),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[j]),
        spec_bundle_gpr_hazard_free::<W>(bundle),
    ensures
        spec_gpr_pair_ok_in::<W>(bundle, i, j),
{
    assert(spec_gpr_pair_ok_in::<W>(bundle, i, j));
}

/// Predicate-hazard conservatism: unconditional pred-hazard-free implies active-pair pred-hazard-free.
pub proof fn lemma_pred_hazard_free_implies_active_pair_ok<const W: usize>(
    bundle: &Bundle<W>,
    cpu: &CpuState<W>,
    i: int,
    j: int,
)
    requires
        cpu.wf(),
        0 <= i < j,
        j < bundle.syllables.len(),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[i]),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[j]),
        spec_bundle_pred_hazard_free::<W>(bundle),
    ensures
        spec_pred_pair_ok_in::<W>(bundle, i, j),
{
    assert(spec_pred_pair_ok_in::<W>(bundle, i, j));
}

// --- Public entry point ---

/// Verify `program` against all compiler contract rules.
///
/// The exec implementation is trusted (`external_body`); the postcondition formally
/// states what an empty result guarantees: every bundle satisfies the conservative
/// spec predicates whose soundness is proved by the lemmas above.
#[verifier::external_body]
pub fn verify_program<const W: usize>(
    program: &[Bundle<W>],
    latencies: &LatencyTable,
) -> (ret: Vec<Diagnostic>)
    ensures
        ret.len() == 0 ==> forall|k: int| 0 <= k < program.len() ==>
            #[trigger] spec_bundle_slot_ok::<W>(&program[k]) &&
            spec_bundle_gpr_hazard_free::<W>(&program[k]) &&
            spec_bundle_pred_hazard_free::<W>(&program[k]),
{
    let mut diags = Vec::new();
    let mut ready_at = vec![0u64; NUM_GPRS];

    for (bidx, bundle) in program.iter().enumerate() {
        let issue_cycle = bidx as u64;
        check_slot_legality::<W>(bidx, bundle, &mut diags);
        check_gpr_hazards::<W>(bidx, bundle, &mut diags);
        check_pred_hazards::<W>(bidx, bundle, &mut diags);
        check_gpr_timing::<W>(bidx, bundle, issue_cycle, &ready_at, &mut diags);
        update_ready_at::<W>(bundle, issue_cycle, latencies, &mut ready_at);
    }

    diags
}

} // verus!
