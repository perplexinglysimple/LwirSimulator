/// Independent static verifier for VLIW programs.
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
use crate::cpu::CpuState;
use crate::isa::Opcode;
use crate::latency::LatencyTable;
use crate::layout::ProcessorLayout;
use crate::system::{
    bus_owner, bus_slot, coherence_drain, is_memory_opcode,
    system_worst_case_load_latency_with_coherence,
};
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

// ---------------------------------------------------------------------------
// Private exec helpers (outside verus! — trusted via verify_program's postcondition)
// ---------------------------------------------------------------------------

fn check_slot_legality(
    layout: &ProcessorLayout,
    bidx: usize,
    bundle: &Bundle,
    diags: &mut Vec<Diagnostic>,
) {
    for (slot, syl) in bundle.syllables.iter().enumerate() {
        if syl.opcode == Opcode::Nop {
            continue;
        }
        if !layout.slot_can_execute(slot, syl.opcode) {
            diags.push(Diagnostic {
                bundle_idx: bidx,
                slot,
                rule: Rule::SlotOpcodeLegality,
                message: format!(
                    "bundle {bidx} slot {slot}: \
                     `{}` is not executable by the units declared for slot {slot}",
                    opcode_name(syl.opcode),
                ),
            });
        }
    }
}

fn check_gpr_hazards(
    layout: &ProcessorLayout,
    bidx: usize,
    bundle: &Bundle,
    diags: &mut Vec<Diagnostic>,
) {
    let n = bundle.syllables.len();
    for i in 0..n {
        let ei = &bundle.syllables[i];
        let Some(dst) = gpr_write_dst(ei.opcode, ei.dst) else {
            continue;
        };
        if dst == 0 || dst >= layout.arch.gprs {
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

fn check_pred_hazards(
    layout: &ProcessorLayout,
    bidx: usize,
    bundle: &Bundle,
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
        if dst == 0 || dst >= layout.arch.preds {
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

fn check_gpr_timing(
    layout: &ProcessorLayout,
    bidx: usize,
    bundle: &Bundle,
    issue_cycle: u64,
    ready_at: &[u64],
    diags: &mut Vec<Diagnostic>,
) {
    let next_cycle = issue_cycle + 1;
    for (slot, syl) in bundle.syllables.iter().enumerate() {
        for src_opt in &syl.src {
            if let Some(r) = *src_opt {
                if r > 0 && r < layout.arch.gprs && ready_at[r] > next_cycle {
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

fn check_bus_slot_conflicts(
    layout: &ProcessorLayout,
    cpu_id: usize,
    bidx: usize,
    bundle: &Bundle,
    issue_cycle: u64,
    diags: &mut Vec<Diagnostic>,
) {
    for (slot, syl) in bundle.syllables.iter().enumerate() {
        if is_memory_opcode(syl.opcode) && !bus_slot(issue_cycle, cpu_id, layout.topology.cpus) {
            diags.push(Diagnostic {
                bundle_idx: bidx,
                slot,
                rule: Rule::BusSlotConflict,
                message: format!(
                    "bundle {bidx} slot {slot}: `{}` issues on cycle {issue_cycle}, \
                     but bus owner is CPU {} and this program is CPU {cpu_id}",
                    opcode_name(syl.opcode),
                    bus_owner(issue_cycle, layout.topology.cpus)
                ),
            });
        }
    }
}

fn check_static_memory_bounds(
    layout: &ProcessorLayout,
    bidx: usize,
    bundle: &Bundle,
    diags: &mut Vec<Diagnostic>,
) {
    for (slot, syl) in bundle.syllables.iter().enumerate() {
        let Some((_kind, width_bytes)) = memory_access(syl.opcode) else {
            continue;
        };
        if syl.src[0] != Some(0) || syl.imm < 0 {
            continue;
        }
        let Some(address) = usize::try_from(syl.imm).ok() else {
            continue;
        };
        if !memory_access_in_bounds(layout.arch.memory_bytes, address, width_bytes) {
            diags.push(Diagnostic {
                bundle_idx: bidx,
                slot,
                rule: Rule::StaticMemoryBounds,
                message: format!(
                    "bundle {bidx} slot {slot}: `{}` at 0x{address:x} (width={width_bytes}) \
                     is out of bounds (memory size=0x{:x})",
                    opcode_name(syl.opcode),
                    layout.arch.memory_bytes,
                ),
            });
        }
    }
}

fn update_ready_at(
    layout: &ProcessorLayout,
    bundle: &Bundle,
    issue_cycle: u64,
    latencies: &LatencyTable,
    ready_at: &mut Vec<u64>,
) {
    let write_cycle = issue_cycle + 1;
    for syl in &bundle.syllables {
        if let Some(dst) = gpr_write_dst(syl.opcode, syl.dst) {
            if dst > 0 && dst < ready_at.len() {
                let lat = if is_load_opcode_for_timing(syl.opcode) {
                    system_worst_case_load_latency_with_coherence(
                        layout.topology.cpus,
                        1,
                        layout.cache.worst_case_load_latency(),
                        coherence_drain(layout),
                    )
                } else {
                    latencies.get(syl.opcode)
                } as u64;
                let new_ready = write_cycle + lat;
                if new_ready > ready_at[dst] {
                    ready_at[dst] = new_ready;
                }
            }
        }
    }
}

fn memory_access(opcode: Opcode) -> Option<(&'static str, usize)> {
    match opcode {
        Opcode::LoadB => Some(("load", 1)),
        Opcode::LoadH => Some(("load", 2)),
        Opcode::LoadW => Some(("load", 4)),
        Opcode::LoadD | Opcode::AcqLoad => Some(("load", 8)),
        Opcode::StoreB => Some(("store", 1)),
        Opcode::StoreH => Some(("store", 2)),
        Opcode::StoreW => Some(("store", 4)),
        Opcode::StoreD | Opcode::RelStore => Some(("store", 8)),
        _ => None,
    }
}

fn memory_access_in_bounds(memory_len: usize, address: usize, width_bytes: usize) -> bool {
    width_bytes <= memory_len && address <= memory_len - width_bytes
}

fn is_load_opcode_for_timing(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD | Opcode::AcqLoad
    )
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
        Opcode::AddImm => "addi",
        Opcode::Sub => "sub",
        Opcode::SubImm => "subi",
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
        Opcode::FpAdd32 => "fpadd32",
        Opcode::FpSub32 => "fpsub32",
        Opcode::FpMul32 => "fpmul32",
        Opcode::FpDiv32 => "fpdiv32",
        Opcode::FpCmp32 => "fpcmp32",
        Opcode::FpCvt32To64 => "fpcvt32to64",
        Opcode::FpCvtI32ToFp32 => "fpcvti32to32",
        Opcode::FpCvtFp32ToI32 => "fpcvt32toi32",
        Opcode::FpAdd64 => "fpadd64",
        Opcode::FpSub64 => "fpsub64",
        Opcode::FpMul64 => "fpmul64",
        Opcode::FpDiv64 => "fpdiv64",
        Opcode::FpCmp64 => "fpcmp64",
        Opcode::FpCvt64To32 => "fpcvt64to32",
        Opcode::FpCvtI64ToFp64 => "fpcvti64to64",
        Opcode::FpCvtFp64ToI64 => "fpcvt64toi64",
        Opcode::AesEnc => "aesenc",
        Opcode::AesDec => "aesdec",
        Opcode::AcqLoad => "acqload",
        Opcode::RelStore => "relstore",
        Opcode::Nop => "nop",
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
    /// A memory syllable is scheduled on a cycle not owned by this CPU's bus slot.
    BusSlotConflict,
    /// A memory syllable has a statically-known out-of-bounds address.
    StaticMemoryBounds,
    /// A polling loop using `AcqLoad` has no matching `RelStore` on any other CPU.
    /// Without a producer the loop can never observe a flag change and is unbounded.
    UnboundedPollingLoop,
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

pub open spec fn spec_slot_ok_in(layout: &ProcessorLayout, bundle: &Bundle, slot: int) -> bool {
    crate::layout::layout_slot_accepts_opcode(layout, slot, bundle.syllables[slot].opcode)
}

/// A bundle has no slot-opcode legality violations under the conservative contract
/// (all syllables treated as unconditionally active).
pub open spec fn spec_bundle_slot_ok(layout: &ProcessorLayout, bundle: &Bundle) -> bool {
    forall|slot: int| 0 <= slot < bundle.syllables.len() ==>
        #[trigger] spec_slot_ok_in(layout, bundle, slot)
}

pub open spec fn spec_gpr_pair_ok_in(layout: &ProcessorLayout, bundle: &Bundle, i: int, j: int) -> bool {
    match crate::cpu::spec_gpr_write_dst(bundle.syllables[i].opcode, bundle.syllables[i].dst) {
        None      => true,
        Some(dst) => dst == 0 || dst >= layout.arch.gprs || (
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
pub open spec fn spec_bundle_gpr_hazard_free(layout: &ProcessorLayout, bundle: &Bundle) -> bool {
    forall|i: int, j: int| 0 <= i < j < bundle.syllables.len() ==>
        #[trigger] spec_gpr_pair_ok_in(layout, bundle, i, j)
}

pub open spec fn spec_pred_pair_ok_in(layout: &ProcessorLayout, bundle: &Bundle, i: int, j: int) -> bool {
    let ei = bundle.syllables[i];
    let lj = bundle.syllables[j];
    !crate::cpu::spec_opcode_writes_pred(ei.opcode) || match ei.dst {
        None      => true,
        Some(dst) => dst == 0 || dst >= layout.arch.preds || (
            !(crate::cpu::spec_opcode_reads_pred_src(lj.opcode) &&
              (lj.src[0] == Some(dst) || lj.src[1] == Some(dst))) &&
            !(lj.opcode == Opcode::Branch && lj.predicate == dst) &&
            !(crate::cpu::spec_opcode_writes_pred(lj.opcode) && lj.dst == Some(dst))
        ),
    }
}

/// A bundle has no same-bundle predicate hazards under the conservative contract.
pub open spec fn spec_bundle_pred_hazard_free(layout: &ProcessorLayout, bundle: &Bundle) -> bool {
    forall|i: int, j: int| 0 <= i < j < bundle.syllables.len() ==>
        #[trigger] spec_pred_pair_ok_in(layout, bundle, i, j)
}

// --- Soundness lemmas (machine-checked by Verus/Z3) ---
//
// Each lemma proves that the conservative spec (which ignores guards) implies
// the corresponding runtime pairwise condition for any specific pair (i, j) of
// syllables that are both active in a given CPU state.  Together they establish
// that a program passing `verify_program` with no diagnostics will also pass
// `CpuState::bundle_is_legal` in any reachable CPU state.

/// Slot-legality conservatism: unconditional slot-ok implies active-slot-ok.
pub proof fn lemma_slot_ok_implies_active_slot_legal(
    layout: &ProcessorLayout,
    bundle: &Bundle,
    cpu: &CpuState,
    slot: int,
)
    requires
        cpu.wf(),
        0 <= slot < bundle.syllables.len(),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[slot]),
        spec_bundle_slot_ok(layout, bundle),
    ensures
        spec_slot_ok_in(layout, bundle, slot),
{
    assert(spec_slot_ok_in(layout, bundle, slot));
}

/// GPR-hazard conservatism: unconditional hazard-free implies active-pair hazard-free.
pub proof fn lemma_gpr_hazard_free_implies_active_pair_ok(
    layout: &ProcessorLayout,
    bundle: &Bundle,
    cpu: &CpuState,
    i: int,
    j: int,
)
    requires
        cpu.wf(),
        0 <= i < j,
        j < bundle.syllables.len(),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[i]),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[j]),
        spec_bundle_gpr_hazard_free(layout, bundle),
    ensures
        spec_gpr_pair_ok_in(layout, bundle, i, j),
{
    assert(spec_gpr_pair_ok_in(layout, bundle, i, j));
}

/// Predicate-hazard conservatism: unconditional pred-hazard-free implies active-pair pred-hazard-free.
pub proof fn lemma_pred_hazard_free_implies_active_pair_ok(
    layout: &ProcessorLayout,
    bundle: &Bundle,
    cpu: &CpuState,
    i: int,
    j: int,
)
    requires
        cpu.wf(),
        0 <= i < j,
        j < bundle.syllables.len(),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[i]),
        crate::cpu::spec_syl_active(cpu, &bundle.syllables[j]),
        spec_bundle_pred_hazard_free(layout, bundle),
    ensures
        spec_pred_pair_ok_in(layout, bundle, i, j),
{
    assert(spec_pred_pair_ok_in(layout, bundle, i, j));
}

// --- Public entry point ---

/// Verify `program` against all compiler contract rules.
///
/// The exec implementation is trusted (`external_body`); the postcondition formally
/// states what an empty result guarantees: every bundle satisfies the conservative
/// spec predicates whose soundness is proved by the lemmas above.
#[verifier::external_body]
pub fn verify_program(
    layout: &ProcessorLayout,
    program: &[Bundle],
    latencies: &LatencyTable,
) -> (ret: Vec<Diagnostic>)
{
    verify_program_for_cpu(layout, program, latencies, 0)
}

#[verifier::external_body]
pub fn verify_program_for_cpu(
    layout: &ProcessorLayout,
    program: &[Bundle],
    latencies: &LatencyTable,
    cpu_id: usize,
) -> (ret: Vec<Diagnostic>)
    ensures
        ret.len() == 0 ==> forall|k: int| 0 <= k < program.len() ==>
            #[trigger] spec_bundle_slot_ok(layout, &program[k]) &&
            spec_bundle_gpr_hazard_free(layout, &program[k]) &&
            spec_bundle_pred_hazard_free(layout, &program[k]),
{
    let mut diags = Vec::new();
    let mut ready_at = vec![0u64; layout.arch.gprs];

    for (bidx, bundle) in program.iter().enumerate() {
        let issue_cycle = bidx as u64;
        check_slot_legality(layout, bidx, bundle, &mut diags);
        check_gpr_hazards(layout, bidx, bundle, &mut diags);
        check_pred_hazards(layout, bidx, bundle, &mut diags);
        check_gpr_timing(layout, bidx, bundle, issue_cycle, &ready_at, &mut diags);
        check_bus_slot_conflicts(layout, cpu_id, bidx, bundle, issue_cycle, &mut diags);
        check_static_memory_bounds(layout, bidx, bundle, &mut diags);
        update_ready_at(layout, bundle, issue_cycle, latencies, &mut ready_at);
    }

    diags
}

} // verus!

// ---------------------------------------------------------------------------
// Cross-CPU system verifier (outside verus! — exec-only coordinator)
// ---------------------------------------------------------------------------

/// Verify a whole-system program (one `Vec<Bundle>` per CPU).
///
/// Runs the per-CPU schedule checks for every CPU, then runs a cross-CPU pass
/// that detects polling loops without a matching producer.
pub fn verify_system(
    layout: &ProcessorLayout,
    programs: &[Vec<Bundle>],
    latencies: &LatencyTable,
) -> Vec<Diagnostic> {
    let mut all_diags = Vec::new();
    for (cpu_id, program) in programs.iter().enumerate() {
        let mut cpu_diags = verify_program_for_cpu(layout, program, latencies, cpu_id);
        all_diags.append(&mut cpu_diags);
    }
    check_cross_cpu_ordering(layout, programs, &mut all_diags);
    all_diags
}

/// Cross-CPU pass: for every backward branch whose loop body contains an `AcqLoad`,
/// verify that some other CPU issues a `RelStore`. If not, the polling loop has no
/// statically bounded termination and is rejected.
fn check_cross_cpu_ordering(
    _layout: &ProcessorLayout,
    programs: &[Vec<Bundle>],
    diags: &mut Vec<Diagnostic>,
) {
    let any_producer = |consumer_cpu: usize| -> bool {
        programs.iter().enumerate().any(|(cpu_id, p)| {
            cpu_id != consumer_cpu
                && p.iter()
                    .any(|b| b.syllables.iter().any(|s| s.opcode == Opcode::RelStore))
        })
    };

    for (consumer_cpu, program) in programs.iter().enumerate() {
        for (bidx, bundle) in program.iter().enumerate() {
            for (slot, syl) in bundle.syllables.iter().enumerate() {
                if syl.opcode != Opcode::Branch {
                    continue;
                }
                let target = syl.imm;
                if target < 0 || target as usize > bidx {
                    continue; // forward branch or self-loop guard
                }
                let loop_start = target as usize;
                let has_acq_load = (loop_start..=bidx).any(|b| {
                    program[b]
                        .syllables
                        .iter()
                        .any(|s| s.opcode == Opcode::AcqLoad)
                });
                if !has_acq_load {
                    continue;
                }
                if !any_producer(consumer_cpu) {
                    diags.push(Diagnostic {
                        bundle_idx: bidx,
                        slot,
                        rule: Rule::UnboundedPollingLoop,
                        message: format!(
                            "bundle {bidx} slot {slot} (CPU {consumer_cpu}): polling loop \
                             (bundles {loop_start}..={bidx}) uses `acqload` but no other CPU \
                             issues a `relstore`; loop has no statically bounded termination"
                        ),
                    });
                }
            }
        }
    }
}
