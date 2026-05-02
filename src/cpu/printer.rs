verus! {

/// Pretty-print the processor state.
#[verifier::external]
pub fn print_cpu_state(state: &CpuState) {
    println!("=== VLIW Processor State (width={}) ===", state.width);
    println!("  PC: {}  Cycle: {}  Halted: {}", state.pc, state.cycle, state.halted);
    println!("  GPRs:");
    for (i, v) in state.gprs.iter().enumerate() {
        if *v != 0 {
            println!("    r{i:<2} = {v:#018x}  ({v})");
        }
    }
    println!("  Predicate registers:");
    for (i, v) in state.preds.iter().enumerate() {
        if *v || i == 0 {
            println!("    p{i} = {v}");
        }
    }
    println!("==========================================");
}

} // verus!
