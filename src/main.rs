/// Hello-world demo for the LWIR VLIW simulator.
///
/// Builds a tiny 4-wide program that:
///   1. Loads immediate 6 into r1 and 7 into r2.
///   2. Multiplies them and stores the result in r3.
///   3. Stores r3 to memory address 0x100.
///   4. Halts.
///
/// Then prints CPU state so you can verify r3 == 42.
use lwir_simulator::bundle::Bundle;
use lwir_simulator::cpu::{print_cpu_state, CpuState};
use lwir_simulator::isa::{Opcode, Syllable};
use lwir_simulator::latency::LatencyTable;

/// Bundle width — change to 8, 16, 32, 64, 128, or 256 and recompile.
const W: usize = 4;

fn main() {
    println!("LWIR VLIW Simulator — hello world (W={W})");

    // --- Build program ---------------------------------------------------

    // Bundle 0: r1 = 6  (slot 0),  r2 = 7  (slot 1)
    let mut b0 = Bundle::<W>::nop_bundle();
    b0.set_slot(0, Syllable {
        opcode: Opcode::MovImm,
        dst: Some(1),
        src: [None, None],
        imm: 6,
        predicate: 0,
        pred_negated: false,
    });
    b0.set_slot(1, Syllable {
        opcode: Opcode::MovImm,
        dst: Some(2),
        src: [None, None],
        imm: 7,
        predicate: 0,
        pred_negated: false,
    });

    // Bundle 1: r3 = r1 * r2  (X slot = slot 3)
    let mut b1 = Bundle::<W>::nop_bundle();
    b1.set_slot(3, Syllable {
        opcode: Opcode::Mul,
        dst: Some(3),
        src: [Some(1), Some(2)],
        imm: 0,
        predicate: 0,
        pred_negated: false,
    });

    // Bundle 2: mem[0x100] = r3  (M slot = slot 2)
    let mut b2 = Bundle::<W>::nop_bundle();
    b2.set_slot(2, Syllable {
        opcode: Opcode::StoreD,
        dst: None,
        src: [Some(0), Some(3)], // base r0=0, data r3
        imm: 0x100,
        predicate: 0,
        pred_negated: false,
    });

    // Bundle 3: RET halts when lr (r31) == 0
    let mut b3 = Bundle::<W>::nop_bundle();
    b3.set_slot(3, Syllable {
        opcode: Opcode::Ret,
        dst: None,
        src: [None, None],
        imm: 0,
        predicate: 0,
        pred_negated: false,
    });

    let program: Vec<Bundle<W>> = vec![b0, b1, b2, b3];

    // --- Configure processor ---------------------------------------------
    let mut latencies = LatencyTable::default();
    // Model a heavier multiply unit (5 cycles instead of 3).
    latencies.set(Opcode::Mul, 5);

    let mut cpu = CpuState::<W>::new(latencies);

    // --- Run until halt --------------------------------------------------
    println!("\nRunning {} bundles…\n", program.len());
    while cpu.step(&program) {}

    // --- Print final state -----------------------------------------------
    print_cpu_state(&cpu);

    assert_eq!(cpu.read_gpr(1), 6,  "r1 should hold 6");
    assert_eq!(cpu.read_gpr(2), 7,  "r2 should hold 7");
    assert_eq!(cpu.read_gpr(3), 42, "r3 should hold 6*7 = 42");

    let stored = u64::from_le_bytes(
        cpu.memory[0x100..0x108].try_into().unwrap()
    );
    assert_eq!(stored, 42, "memory[0x100] should hold 42");

    println!("\nAll assertions passed — 6 × 7 = {} ✓", cpu.read_gpr(3));
}
