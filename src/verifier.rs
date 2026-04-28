/// Independent static verifier for LWIR programs.
///
/// Checks all rules from docs/compiler_contract.md without executing the program.
/// Produces a list of Diagnostics; an empty list means the program is clean.
use crate::bundle::Bundle;
use crate::isa::{Opcode, SlotClass};
use crate::latency::LatencyTable;

const NUM_GPRS: usize = 32;
const NUM_PREDS: usize = 16;

// ---------------------------------------------------------------------------
// Public types
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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Verify `program` against all compiler contract rules.
///
/// `latencies` is used for the GPR ready-cycle timing check (rule 6).
/// Returns a possibly-empty list of diagnostics sorted by bundle index.
pub fn verify_program<const W: usize>(
    program: &[Bundle<W>],
    latencies: &LatencyTable,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    // ready_at[r]: earliest cycle at which GPR r is available (0 = always ready).
    // In stall-free execution bundle N issues at simulator cycle N.
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

// ---------------------------------------------------------------------------
// Rule 2 / 7 / 8 — slot opcode legality
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

// ---------------------------------------------------------------------------
// Rules 3 & 4 — same-bundle GPR RAW and WAW
// ---------------------------------------------------------------------------

fn check_gpr_hazards<const W: usize>(
    bidx: usize,
    bundle: &Bundle<W>,
    diags: &mut Vec<Diagnostic>,
) {
    let n = bundle.syllables.len();
    for i in 0..n {
        let ei = &bundle.syllables[i];
        if !ei.opcode.writes_gpr() {
            continue;
        }
        let Some(dst) = ei.dst else {
            continue;
        };
        if dst == 0 || dst >= NUM_GPRS {
            continue;
        }
        for j in (i + 1)..n {
            let lj = &bundle.syllables[j];

            // Rule 3: RAW — later slot reads the GPR written by the earlier slot.
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

            // Implicit r31 read by ret.
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

            // Rule 4: WAW — later slot also writes the same GPR.
            if lj.opcode.writes_gpr() && lj.dst == Some(dst) {
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

// ---------------------------------------------------------------------------
// Rule 5 — same-bundle predicate hazards
// ---------------------------------------------------------------------------

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

            // Predicate RAW: later slot reads the predicate as an ALU source.
            let reads_as_src = lj.opcode.reads_pred_src()
                && (lj.src[0] == Some(dst) || lj.src[1] == Some(dst));
            // Branch reads its predicate via the `predicate` field.
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

            // Predicate WAW: later slot also writes the same predicate.
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

// ---------------------------------------------------------------------------
// Rule 6 — GPR ready-cycle timing
// ---------------------------------------------------------------------------

fn check_gpr_timing<const W: usize>(
    bidx: usize,
    bundle: &Bundle<W>,
    issue_cycle: u64,
    ready_at: &[u64],
    diags: &mut Vec<Diagnostic>,
) {
    // In the simulator the stall check uses `cycle` (before increment) and
    // `next_cycle = cycle + 1`.  With stall-free execution, bundle bidx
    // runs at simulator cycle bidx, so next_cycle = bidx + 1.
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
        // `ret` implicitly reads r31 (the link register).
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
    // The simulator increments `cycle` before executing, so writes happen at
    // cycle = issue_cycle + 1.  ready_cycle = write_cycle + latency.
    let write_cycle = issue_cycle + 1;
    for syl in &bundle.syllables {
        if syl.opcode.writes_gpr() {
            if let Some(dst) = syl.dst {
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
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn slot_class_for_index(slot: usize) -> SlotClass {
    match slot % 4 {
        0 | 1 => SlotClass::Integer,
        2 => SlotClass::Memory,
        _ => SlotClass::Control,
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
