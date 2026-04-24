use lwir_simulator::bundle::Bundle;
use lwir_simulator::cpu::CpuState;
use lwir_simulator::isa::{Opcode, Syllable};
use lwir_simulator::latency::LatencyTable;

const W: usize = 4;

fn mov_imm(dst: usize, imm: i64) -> Syllable {
    Syllable {
        opcode: Opcode::MovImm,
        dst: Some(dst),
        src: [None, None],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn mul(dst: usize, lhs: usize, rhs: usize) -> Syllable {
    Syllable {
        opcode: Opcode::Mul,
        dst: Some(dst),
        src: [Some(lhs), Some(rhs)],
        imm: 0,
        predicate: 0,
        pred_negated: false,
    }
}

fn store_d(base: usize, data: usize, imm: i64) -> Syllable {
    Syllable {
        opcode: Opcode::StoreD,
        dst: None,
        src: [Some(base), Some(data)],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn ret() -> Syllable {
    Syllable {
        opcode: Opcode::Ret,
        dst: None,
        src: [None, None],
        imm: 0,
        predicate: 0,
        pred_negated: false,
    }
}

fn hello_world_program() -> Vec<Bundle<W>> {
    let mut b0 = Bundle::<W>::nop_bundle();
    b0.set_slot(0, mov_imm(1, 6));
    b0.set_slot(1, mov_imm(2, 7));

    let mut b1 = Bundle::<W>::nop_bundle();
    b1.set_slot(3, mul(3, 1, 2));

    let mut b2 = Bundle::<W>::nop_bundle();
    b2.set_slot(2, store_d(0, 3, 0x100));

    let mut b3 = Bundle::<W>::nop_bundle();
    b3.set_slot(3, ret());

    vec![b0, b1, b2, b3]
}

#[test]
fn hello_world_program_completes_and_writes_result() {
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);

    let program = hello_world_program();
    let mut cpu = CpuState::<W>::new(latencies);

    while cpu.step(&program) {}

    assert!(cpu.halted);
    assert_eq!(cpu.pc, program.len());
    assert_eq!(cpu.read_gpr(1), 6);
    assert_eq!(cpu.read_gpr(2), 7);
    assert_eq!(cpu.read_gpr(3), 42);

    let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
    assert_eq!(stored, 42);
}

#[test]
fn register_zero_and_predicate_zero_are_hardwired() {
    let mut cpu = CpuState::<W>::new(LatencyTable::default());

    cpu.write_gpr(0, 99);
    cpu.write_pred(0, false);

    assert_eq!(cpu.read_gpr(0), 0);
    assert!(cpu.read_pred(0));
}
