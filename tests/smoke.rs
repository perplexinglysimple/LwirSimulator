use vliw_simulator::asm::parse_program;
use vliw_simulator::bundle::Bundle;
use vliw_simulator::cpu::CpuState;
use vliw_simulator::isa::{Opcode, Syllable};
use vliw_simulator::latency::LatencyTable;
use vliw_simulator::layout::{canonical_layout, ProcessorLayout};
use vliw_simulator::system::{Bus, BusAccessKind, BusReq, System};

const W: usize = 4;

fn processor_header(width: usize) -> String {
    let mut slots = String::new();
    for slot in 0..width {
        let units = match slot % 4 {
            0 | 1 => "alu",
            2 => "mem",
            _ => "ctrl, mul",
        };
        slots.push_str(&format!("    {slot} = {{ {units} }}\n"));
    }
    format!(
        ".processor {{\n  width {width}\n\n  hardware {{\n    unit alu = integer_alu\n    unit mem = memory\n    unit ctrl = control\n    unit mul = multiplier\n  }}\n\n  layout slots {{\n{slots}  }}\n\n  cache {{ }}\n  topology {{ cpus 1 }}\n}}\n"
    )
}

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

fn call(target: i64) -> Syllable {
    Syllable {
        opcode: Opcode::Call,
        dst: None,
        src: [None, None],
        imm: target,
        predicate: 0,
        pred_negated: false,
    }
}

fn branch(predicate: usize, pred_negated: bool, target: i64) -> Syllable {
    Syllable {
        opcode: Opcode::Branch,
        dst: None,
        src: [None, None],
        imm: target,
        predicate,
        pred_negated,
    }
}

fn cmp_lt(dst: usize, lhs: usize, rhs: usize) -> Syllable {
    Syllable {
        opcode: Opcode::CmpLt,
        dst: Some(dst),
        src: [Some(lhs), Some(rhs)],
        imm: 0,
        predicate: 0,
        pred_negated: false,
    }
}

fn p_not(dst: usize, src: usize) -> Syllable {
    Syllable {
        opcode: Opcode::PNot,
        dst: Some(dst),
        src: [Some(src), None],
        imm: 0,
        predicate: 0,
        pred_negated: false,
    }
}

fn load_b(dst: usize, base: usize, imm: i64) -> Syllable {
    Syllable {
        opcode: Opcode::LoadB,
        dst: Some(dst),
        src: [Some(base), None],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn load_h(dst: usize, base: usize, imm: i64) -> Syllable {
    Syllable {
        opcode: Opcode::LoadH,
        dst: Some(dst),
        src: [Some(base), None],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn load_w(dst: usize, base: usize, imm: i64) -> Syllable {
    Syllable {
        opcode: Opcode::LoadW,
        dst: Some(dst),
        src: [Some(base), None],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn load_d(dst: usize, base: usize, imm: i64) -> Syllable {
    Syllable {
        opcode: Opcode::LoadD,
        dst: Some(dst),
        src: [Some(base), None],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn sparse_latency_table() -> LatencyTable {
    LatencyTable { entries: vec![] }
}

fn step(cpu: &mut CpuState, program: &Vec<Bundle>) -> bool {
    let layout = canonical_layout(cpu.width);
    cpu.step(&layout, program)
}

fn trace_program(
    cpu: &mut CpuState,
    layout: &ProcessorLayout,
    program: &[Bundle],
) -> vliw_simulator::cpu::TraceLog {
    cpu.trace_program(layout, program)
}

fn bus_req(cpu_id: usize, address: usize, value: u64) -> BusReq {
    BusReq {
        cpu_id,
        slot: 2,
        opcode: Opcode::StoreD,
        kind: BusAccessKind::Store,
        address,
        width_bytes: 8,
        value,
        dst: None,
    }
}

fn contended_store_system(address: i64, cpu0_value: i64, cpu1_value: i64) -> System {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;

    let mut cpu0_b0 = Bundle::nop_bundle(W);
    cpu0_b0.set_slot(0, mov_imm(1, cpu0_value));
    let cpu0_b1 = Bundle::nop_bundle(W);
    let mut cpu0_b2 = Bundle::nop_bundle(W);
    cpu0_b2.set_slot(2, store_d(0, 1, address));
    let mut cpu0_b3 = Bundle::nop_bundle(W);
    cpu0_b3.set_slot(3, ret());

    let mut cpu1_b0 = Bundle::nop_bundle(W);
    cpu1_b0.set_slot(0, mov_imm(1, cpu1_value));
    let mut cpu1_b1 = Bundle::nop_bundle(W);
    cpu1_b1.set_slot(2, store_d(0, 1, address));
    let mut cpu1_b2 = Bundle::nop_bundle(W);
    cpu1_b2.set_slot(3, ret());

    System::new(
        layout,
        vec![
            vec![cpu0_b0, cpu0_b1, cpu0_b2, cpu0_b3],
            vec![cpu1_b0, cpu1_b1, cpu1_b2],
        ],
        LatencyTable::default(),
    )
    .unwrap()
}

fn hello_world_program() -> Vec<Bundle> {
    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(0, mov_imm(1, 6));
    b0.set_slot(1, mov_imm(2, 7));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(3, mul(3, 1, 2));

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(2, store_d(0, 3, 0x100));

    let mut b3 = Bundle::nop_bundle(W);
    b3.set_slot(3, ret());

    vec![b0, b1, b2, b3]
}

#[test]
fn hello_world_program_completes_and_writes_result() {
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);

    let program = hello_world_program();
    let mut cpu = CpuState::new(W, latencies);

    while step(&mut cpu, &program) {}

    assert!(cpu.halted);
    assert_eq!(cpu.pc, program.len());
    assert_eq!(cpu.read_gpr(1), 6);
    assert_eq!(cpu.read_gpr(2), 7);
    assert_eq!(cpu.read_gpr(3), 42);

    let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
    assert_eq!(stored, 42);
}

#[test]
fn system_single_cpu_matches_direct_cpu_execution() {
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);

    let program = hello_world_program();
    let mut direct = CpuState::new(W, latencies.clone());
    while step(&mut direct, &program) {}

    let layout = canonical_layout(W);
    let mut system = System::new(layout, vec![program], latencies).unwrap();
    system.run_until_quiescent();

    let cpu = &system.cpus[0];
    assert_eq!(system.cycle, direct.cycle);
    assert_eq!(cpu.halted, direct.halted);
    assert_eq!(cpu.pc, direct.pc);
    assert_eq!(cpu.read_gpr(1), direct.read_gpr(1));
    assert_eq!(cpu.read_gpr(2), direct.read_gpr(2));
    assert_eq!(cpu.read_gpr(3), direct.read_gpr(3));
    assert_eq!(
        &system.memory.bytes()[0x100..0x108],
        &direct.memory[0x100..0x108]
    );
}

#[test]
fn system_two_cpus_run_independent_programs_against_shared_memory() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;

    let mut cpu0_b0 = Bundle::nop_bundle(W);
    cpu0_b0.set_slot(0, mov_imm(1, 0x11));
    let cpu0_b1 = Bundle::nop_bundle(W);
    let mut cpu0_b2 = Bundle::nop_bundle(W);
    cpu0_b2.set_slot(2, store_d(0, 1, 0x100));
    let mut cpu0_b3 = Bundle::nop_bundle(W);
    cpu0_b3.set_slot(3, ret());

    let mut cpu1_b0 = Bundle::nop_bundle(W);
    cpu1_b0.set_slot(0, mov_imm(1, 0x22));
    let mut cpu1_b1 = Bundle::nop_bundle(W);
    cpu1_b1.set_slot(2, store_d(0, 1, 0x108));
    let mut cpu1_b2 = Bundle::nop_bundle(W);
    cpu1_b2.set_slot(3, ret());

    let mut system = System::new(
        layout,
        vec![
            vec![cpu0_b0, cpu0_b1, cpu0_b2, cpu0_b3],
            vec![cpu1_b0, cpu1_b1, cpu1_b2],
        ],
        LatencyTable::default(),
    )
    .unwrap();

    system.run_until_quiescent();

    assert_eq!(system.cycle, 4);
    assert!(system.cpus[0].halted);
    assert!(system.cpus[1].halted);
    assert_eq!(system.cpus[0].read_gpr(1), 0x11);
    assert_eq!(system.cpus[1].read_gpr(1), 0x22);

    let stored0 = u64::from_le_bytes(system.memory.bytes()[0x100..0x108].try_into().unwrap());
    let stored1 = u64::from_le_bytes(system.memory.bytes()[0x108..0x110].try_into().unwrap());
    assert_eq!(stored0, 0x11);
    assert_eq!(stored1, 0x22);
}

#[test]
fn bus_arbitration_uses_closed_form_cycle_owner() {
    let bus = Bus::new(3);
    let requests = vec![
        bus_req(2, 0x120, 0xaa),
        bus_req(0, 0x100, 0xbb),
        bus_req(1, 0x110, 0xcc),
    ];

    assert_eq!(bus.owner(0), 0);
    assert_eq!(bus.owner(1), 1);
    assert_eq!(bus.owner(2), 2);
    assert_eq!(bus.owner(3), 0);
    assert_eq!(bus.arbitrate(2, &requests).unwrap().cpu_id, 2);
    assert_eq!(bus.arbitrate(1, &requests).unwrap().cpu_id, 1);
}

#[test]
fn system_bus_commits_at_most_one_memory_request_per_cycle() {
    let mut system = contended_store_system(0x100, 0x11, 0x22);
    system.run_until_quiescent();

    let grants = system
        .bus
        .events
        .iter()
        .filter(|event| event.granted.is_some())
        .count();
    assert_eq!(grants, 2);
    assert!(system
        .bus
        .events
        .iter()
        .all(|event| event.granted.as_ref().map_or(0, |_| 1) <= 1));
}

#[test]
fn system_bus_serializes_contended_writes_in_cpu_id_order() {
    let mut system = contended_store_system(0x100, 0x11, 0x22);
    system.run_until_quiescent();

    let granted = system
        .bus
        .events
        .iter()
        .filter_map(|event| event.granted.as_ref().map(|req| req.cpu_id))
        .collect::<Vec<_>>();
    assert_eq!(granted, vec![1, 0]);

    let stored = u64::from_le_bytes(system.memory.bytes()[0x100..0x108].try_into().unwrap());
    assert_eq!(stored, 0x11);

    let trace = system.bus.to_string();
    assert!(trace.contains("bus trace v1"), "{trace}");
    assert!(
        trace.contains("bus cycle=1 winner=cpu1 grant=cpu1:slot2:store@0x00000100/8"),
        "{trace}"
    );
    assert!(
        trace.contains("bus cycle=2 winner=cpu0 grant=cpu0:slot2:store@0x00000100/8"),
        "{trace}"
    );
}

#[test]
fn register_zero_and_predicate_zero_are_hardwired() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    cpu.write_gpr(0, 99);
    cpu.write_pred(0, false);

    assert_eq!(cpu.read_gpr(0), 0);
    assert!(cpu.read_pred(0));
}

#[test]
fn processor_arch_config_sizes_cpu_state() {
    let source = r#"
.processor {
  width 4

  arch { gprs 64 preds 32 memory 131072 }

  hardware {
    unit alu = integer_alu
    unit mem = memory
    unit ctrl = control
    unit mul = multiplier
  }

  layout slots {
    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { ctrl, mul }
  }

  cache { }
  topology { cpus 1 }
}

{
  i0: movi r63, 7
  i1: nop
  m : nop
  x : ret
}
"#;

    let program = parse_program(source).unwrap();
    let cpu = CpuState::new_for_layout(&program.layout, LatencyTable::default());

    assert_eq!(program.layout.arch.gprs, 64);
    assert_eq!(program.layout.arch.preds, 32);
    assert_eq!(program.layout.arch.memory_bytes, 131072);
    assert_eq!(cpu.gprs.len(), 64);
    assert_eq!(cpu.preds.len(), 32);
    assert_eq!(cpu.scoreboard.len(), 64);
    assert_eq!(cpu.memory.len(), 131072);
}

#[test]
fn illegal_bundle_wrong_slot_halts_before_execution() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut bad = Bundle::nop_bundle(W);
    bad.set_slot(0, ret());
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
    assert_eq!(cpu.cycle, 0);
}

#[test]
fn illegal_same_bundle_raw_halts_before_execution() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut bad = Bundle::nop_bundle(W);
    bad.set_slot(0, mov_imm(1, 6));
    bad.set_slot(
        1,
        Syllable {
            opcode: Opcode::Add,
            dst: Some(2),
            src: [Some(1), Some(0)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
    assert_eq!(cpu.cycle, 0);
    assert_eq!(cpu.read_gpr(1), 0);
    assert_eq!(cpu.read_gpr(2), 0);
}

#[test]
fn scoreboard_stalls_until_producer_ready() {
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);

    let mut cpu = CpuState::new(W, latencies);
    cpu.write_gpr(1, 6);
    cpu.write_gpr(2, 7);

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(3, mul(3, 1, 2));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(2, store_d(0, 3, 0x100));

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(3, ret());

    let program = vec![b0, b1, b2];

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 1);
    assert_eq!(cpu.read_gpr(3), 42);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 2);
    let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
    assert_eq!(stored, 0);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 3);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 4);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 5);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 2);
    assert_eq!(cpu.cycle, 6);
    let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
    assert_eq!(stored, 42);
}

#[test]
fn shifts_and_load_widths_behave_as_expected() {
    let mut cpu = CpuState::new(W, LatencyTable::default());
    cpu.write_gpr(1, 0xf000_0000_0000_0000);
    cpu.write_gpr(2, 4);

    cpu.memory[0x120] = 0x88;
    cpu.memory[0x121] = 0x77;
    cpu.memory[0x122] = 0x66;
    cpu.memory[0x123] = 0x55;
    cpu.memory[0x124] = 0x44;
    cpu.memory[0x125] = 0x33;
    cpu.memory[0x126] = 0x22;
    cpu.memory[0x127] = 0x11;

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(
        0,
        Syllable {
            opcode: Opcode::Shl,
            dst: Some(3),
            src: [Some(2), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(
        0,
        Syllable {
            opcode: Opcode::Srl,
            dst: Some(4),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(
        0,
        Syllable {
            opcode: Opcode::Sra,
            dst: Some(5),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b3 = Bundle::nop_bundle(W);
    b3.set_slot(2, load_b(6, 0, 0x120));

    let mut b4 = Bundle::nop_bundle(W);
    b4.set_slot(2, load_h(7, 0, 0x120));

    let mut b5 = Bundle::nop_bundle(W);
    b5.set_slot(2, load_w(8, 0, 0x120));

    let mut b6 = Bundle::nop_bundle(W);
    b6.set_slot(2, load_d(9, 0, 0x120));

    let mut b7 = Bundle::nop_bundle(W);
    b7.set_slot(3, ret());

    let program = vec![b0, b1, b2, b3, b4, b5, b6, b7];
    while step(&mut cpu, &program) {}

    assert_eq!(cpu.read_gpr(3), 64);
    assert_eq!(cpu.read_gpr(4), 0x0f00_0000_0000_0000);
    assert_eq!(cpu.read_gpr(5), 0xff00_0000_0000_0000);
    assert_eq!(cpu.read_gpr(6), 0x88);
    assert_eq!(cpu.read_gpr(7), 0x7788);
    assert_eq!(cpu.read_gpr(8), 0x5566_7788);
    assert_eq!(cpu.read_gpr(9), 0x1122_3344_5566_7788);
}

#[test]
fn predicate_logic_and_branch_control_skip_work() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(0, mov_imm(1, 5));
    b0.set_slot(1, mov_imm(2, 7));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(0, cmp_lt(1, 1, 2));

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(3, p_not(2, 1));

    let mut b3 = Bundle::nop_bundle(W);
    b3.set_slot(3, branch(2, true, 5));

    let mut b4 = Bundle::nop_bundle(W);
    b4.set_slot(0, mov_imm(3, 99));

    let mut b5 = Bundle::nop_bundle(W);
    b5.set_slot(3, ret());

    let program = vec![b0, b1, b2, b3, b4, b5];
    while step(&mut cpu, &program) {}

    assert!(cpu.read_pred(1));
    assert!(!cpu.read_pred(2));
    assert_eq!(cpu.read_gpr(3), 0);
    assert_eq!(cpu.pc, program.len());
}

#[test]
fn call_and_return_follow_link_register() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(3, call(2));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(0, mov_imm(5, 1));
    b1.set_slot(1, mov_imm(31, 0));

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(0, mov_imm(6, 2));
    b2.set_slot(3, ret());

    let program = vec![b0, b1, b2];

    let mut steps = 0usize;
    while step(&mut cpu, &program) {
        steps += 1;
        assert!(steps < 10, "call/return flow should terminate");
    }

    assert!(cpu.halted);
    assert_eq!(cpu.read_gpr(5), 1);
    assert_eq!(cpu.read_gpr(6), 2);
    assert_eq!(cpu.read_gpr(31), 0);
}

#[test]
fn main_binary_runs_example_program_successfully() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_simulator"))
        .arg("examples/hello.vliw")
        .output()
        .expect("binary should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("VLIW Simulator"));
    assert!(stdout.contains("Program: examples/hello.vliw"));
    assert!(stdout.contains("Halted: true"));
    assert!(stdout.contains("42"));
}

#[test]
fn main_binary_requires_program_path() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_simulator"))
        .output()
        .expect("binary should run");

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("usage:"));
    assert!(stderr.contains("<program.vliw>"));
}

#[test]
fn deterministic_trace_records_scheduler_visible_events() {
    let source = format!(
        "{}{}",
        processor_header(W),
        r#"
{
  I0: movi r1, 6
  I1: movi r2, 7
  M : nop
  X : nop
}

{
  I0: nop
  I1: nop
  M : nop
  X : mul r3, r1, r2
}

{
  I0: nop
  I1: nop
  M : std [r0 + 0x100], r3
  X : nop
}

{
  I0: cmplt p1, r1, r2
  I1: nop
  M : nop
  X : nop
}

{
  I0: nop
  I1: nop
  M : nop
  X : branch p1, call_site
}

{
  I0: movi r9, 99
  I1: nop
  M : nop
  X : nop
}

call_site:
{
  I0: nop
  I1: nop
  M : nop
  X : call worker
}

after_call:
{
  I0: movi r31, 0
  I1: nop
  M : nop
  X : nop
}

worker:
{
  I0: nop
  I1: nop
  M : ldd r4, [r0 + 0x100]
  X : ret
}
"#
    );

    let program = parse_program(&source).expect("trace program should parse");
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);
    let mut cpu = CpuState::new(W, latencies);

    let trace = trace_program(&mut cpu, &program.layout, &program.bundles);
    let rendered = trace.to_string();

    assert!(cpu.halted);
    assert!(rendered.starts_with("trace v1 width=4\n"), "{rendered}");
    assert!(rendered.contains("event kind=stall bundle=2"), "{rendered}");
    assert!(
        rendered.contains("gpr slot=3 reg=r3 value=0x000000000000002a"),
        "{rendered}"
    );
    assert!(
        rendered.contains("pred slot=0 reg=p1 value=true"),
        "{rendered}"
    );
    assert!(
        rendered.contains("mem slot=2 kind=store width=8 addr=0x00000100"),
        "{rendered}"
    );
    assert!(
        rendered.contains("mem slot=2 kind=load width=8 addr=0x00000100"),
        "{rendered}"
    );
    assert!(
        rendered.contains("control slot=3 kind=branch pred=p1 taken=true"),
        "{rendered}"
    );
    assert!(rendered.contains("control slot=3 kind=call"), "{rendered}");
    assert!(
        rendered.contains("control slot=3 kind=ret target=halt halted=true"),
        "{rendered}"
    );
    assert!(rendered.contains("final pc=9"), "{rendered}");
}

#[test]
fn direct_mapped_cache_reports_load_hit_streak() {
    let source = r#"
.processor {
  width 4

  hardware {
    unit alu = integer_alu
    unit mem = memory
    unit ctrl = control
    unit mul = multiplier
  }

  layout slots {
    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { ctrl, mul }
  }

  cache {
    l1d {
      line_bytes 64
      capacity 64
      associativity 1
      write_policy write_back
      hit_latency 1
      miss_latency 4
      writeback_latency 5
    }
  }
  topology { cpus 1 }
}

{
  i0: nop
  i1: nop
  m : ldd r1, [r0 + 0]
  x : nop
}

{
  i0: nop
  i1: nop
  m : nop
  x : nop
}

{
  i0: nop
  i1: nop
  m : ldd r2, [r0 + 8]
  x : ret
}
"#;

    let program = parse_program(source).expect("cache program should parse");
    let mut cpu = CpuState::new_for_layout(&program.layout, LatencyTable::default());
    let trace = trace_program(&mut cpu, &program.layout, &program.bundles).to_string();

    assert!(
        trace.contains(
            "kind=load width=8 addr=0x00000000 value=0x0000000000000000 in_bounds=true cache=miss"
        ),
        "{trace}"
    );
    assert!(
        trace.contains(
            "kind=load width=8 addr=0x00000008 value=0x0000000000000000 in_bounds=true cache=hit"
        ),
        "{trace}"
    );
    assert!(
        trace.contains("gpr slot=2 reg=r1 value=0x0000000000000000 ready=5"),
        "{trace}"
    );
    assert!(
        trace.contains("gpr slot=2 reg=r2 value=0x0000000000000000 ready=4"),
        "{trace}"
    );
}

#[test]
fn direct_mapped_cache_reports_dirty_eviction() {
    let source = r#"
.processor {
  width 4

  hardware {
    unit alu = integer_alu
    unit mem = memory
    unit ctrl = control
    unit mul = multiplier
  }

  layout slots {
    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { ctrl, mul }
  }

  cache {
    l1d {
      line_bytes 64
      capacity 64
      associativity 1
      write_policy write_back
      hit_latency 1
      miss_latency 4
      writeback_latency 5
    }
  }
  topology { cpus 1 }
}

{
  i0: movi r1, 99
  i1: nop
  m : nop
  x : nop
}

{
  i0: nop
  i1: nop
  m : std [r0 + 0], r1
  x : nop
}

{
  i0: nop
  i1: nop
  m : ldd r2, [r0 + 64]
  x : ret
}
"#;

    let program = parse_program(source).expect("dirty eviction program should parse");
    let mut cpu = CpuState::new_for_layout(&program.layout, LatencyTable::default());
    let trace = trace_program(&mut cpu, &program.layout, &program.bundles).to_string();

    assert!(
        trace.contains(
            "kind=store width=8 addr=0x00000000 value=0x0000000000000063 in_bounds=true cache=miss"
        ),
        "{trace}"
    );
    assert!(trace.contains("kind=load width=8 addr=0x00000040 value=0x0000000000000000 in_bounds=true cache=miss_dirty"), "{trace}");
    assert!(
        trace.contains("gpr slot=2 reg=r2 value=0x0000000000000000 ready=12"),
        "{trace}"
    );
}

#[test]
fn main_binary_trace_mode_emits_stable_log() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_simulator"))
        .arg("--trace")
        .arg("examples/hello.vliw")
        .output()
        .expect("binary should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.starts_with("trace v1 width=4\n"), "{stdout}");
    assert!(stdout.contains("event kind=stall bundle=2"), "{stdout}");
    assert!(stdout.contains("mem slot=2 kind=store"), "{stdout}");
    assert!(stdout.contains("final pc="), "{stdout}");
}

#[test]
fn bundle_helpers_preserve_expected_structure() {
    let mut bundle = Bundle::nop_bundle(W);

    assert_eq!(bundle.width(), W);
    assert!(bundle.is_all_nop());

    bundle.set_slot(0, mov_imm(1, 9));
    assert!(!bundle.is_all_nop());
    assert_eq!(bundle.syllables[0].opcode, Opcode::MovImm);
    assert_eq!(bundle.syllables[0].dst, Some(1));
    assert_eq!(bundle.syllables[0].imm, 9);

    for slot in 1..W {
        assert_eq!(bundle.syllables[slot].opcode, Opcode::Nop);
        assert_eq!(bundle.syllables[slot].predicate, 0);
    }
}

#[test]
fn latency_table_defaults_and_overrides_are_visible() {
    let mut latencies = LatencyTable::default();

    assert_eq!(latencies.get(Opcode::Add), 1);
    assert_eq!(latencies.get(Opcode::LoadD), 3);
    assert_eq!(latencies.get(Opcode::MulH), 3);
    assert_eq!(latencies.get(Opcode::Nop), 0);

    latencies.set(Opcode::LoadD, 10);
    latencies.set(Opcode::Branch, 2);

    assert_eq!(latencies.get(Opcode::LoadD), 10);
    assert_eq!(latencies.get(Opcode::Branch), 2);
    assert_eq!(latencies.get(Opcode::Add), 1);
}

#[test]
fn fp_and_aes_units_parse_verify_and_execute_placeholders() {
    let source = r#"
.processor {
  width 4

  hardware {
    unit alu = integer_alu
    unit mem = memory
    unit ctrl = control
    unit mul = multiplier
    unit fp = fp { variant fp64 latency 6 }
    unit aes = aes { variant aes_ni latency 4 }
  }

  layout slots {
    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { fp, aes, ctrl, mul }
  }

  cache { }
  topology { cpus 1 }
}

{
  i0: movi r1, 10
  i1: movi r2, 32
  m : nop
  x : nop
}

{
  i0: nop
  i1: nop
  m : nop
  x : fpadd64 r3, r1, r2
}

{
  i0: nop
  i1: nop
  m : nop
  x : aesenc r4, r1, r2
}

{
  i0: nop
  i1: nop
  m : nop
  x : ret
}
"#;

    let program = parse_program(source).unwrap();
    assert_eq!(program.layout.units[4].latency, Some(6));
    assert_eq!(program.layout.units[5].latency, Some(4));
    assert!(program.layout.slot_can_execute(3, Opcode::FpAdd64));
    assert!(program.layout.slot_can_execute(3, Opcode::AesEnc));

    let mut cpu = CpuState::new(W, LatencyTable::default());
    while cpu.step(&program.layout, &program.bundles) {}

    assert!(cpu.halted);
    assert_eq!(cpu.read_gpr(3), 42);
    assert_eq!(cpu.read_gpr(4), 10 ^ 32 ^ 0x63);
}

#[test]
fn opcode_matrix_covers_remaining_execution_paths() {
    let mut cpu = CpuState::new(W, LatencyTable::default());
    cpu.write_gpr(11, u64::MAX);
    cpu.write_gpr(12, 2);
    cpu.write_gpr(13, 0x1122_3344_5566_7788);

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(0, mov_imm(1, 0x55));
    b0.set_slot(1, mov_imm(2, 0x0f));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(
        0,
        Syllable {
            opcode: Opcode::Add,
            dst: Some(3),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );
    b1.set_slot(
        1,
        Syllable {
            opcode: Opcode::Sub,
            dst: Some(4),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(
        0,
        Syllable {
            opcode: Opcode::And,
            dst: Some(5),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );
    b2.set_slot(
        1,
        Syllable {
            opcode: Opcode::Or,
            dst: Some(6),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b3 = Bundle::nop_bundle(W);
    b3.set_slot(
        0,
        Syllable {
            opcode: Opcode::Xor,
            dst: Some(7),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );
    b3.set_slot(
        1,
        Syllable {
            opcode: Opcode::Mov,
            dst: Some(8),
            src: [Some(1), None],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b4 = Bundle::nop_bundle(W);
    b4.set_slot(
        2,
        Syllable {
            opcode: Opcode::Lea,
            dst: Some(9),
            src: [Some(0), None],
            imm: 0x200,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b5 = Bundle::nop_bundle(W);
    b5.set_slot(
        2,
        Syllable {
            opcode: Opcode::Prefetch,
            dst: None,
            src: [Some(9), None],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b6 = Bundle::nop_bundle(W);
    b6.set_slot(
        3,
        Syllable {
            opcode: Opcode::MulH,
            dst: Some(10),
            src: [Some(11), Some(12)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b7 = Bundle::nop_bundle(W);
    b7.set_slot(
        2,
        Syllable {
            opcode: Opcode::StoreB,
            dst: None,
            src: [Some(9), Some(13)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b8 = Bundle::nop_bundle(W);
    b8.set_slot(
        2,
        Syllable {
            opcode: Opcode::StoreH,
            dst: None,
            src: [Some(9), Some(13)],
            imm: 2,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b9 = Bundle::nop_bundle(W);
    b9.set_slot(
        2,
        Syllable {
            opcode: Opcode::StoreW,
            dst: None,
            src: [Some(9), Some(13)],
            imm: 8,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b10 = Bundle::nop_bundle(W);
    b10.set_slot(
        0,
        Syllable {
            opcode: Opcode::CmpEq,
            dst: Some(1),
            src: [Some(1), Some(8)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b11 = Bundle::nop_bundle(W);
    b11.set_slot(
        0,
        Syllable {
            opcode: Opcode::CmpUlt,
            dst: Some(2),
            src: [Some(2), Some(1)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b12 = Bundle::nop_bundle(W);
    b12.set_slot(
        3,
        Syllable {
            opcode: Opcode::PAnd,
            dst: Some(3),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b13 = Bundle::nop_bundle(W);
    b13.set_slot(
        3,
        Syllable {
            opcode: Opcode::POr,
            dst: Some(4),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b14 = Bundle::nop_bundle(W);
    b14.set_slot(
        3,
        Syllable {
            opcode: Opcode::PXor,
            dst: Some(5),
            src: [Some(1), Some(2)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b15 = Bundle::nop_bundle(W);
    b15.set_slot(
        3,
        Syllable {
            opcode: Opcode::Jump,
            dst: None,
            src: [None, None],
            imm: 17,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b16 = Bundle::nop_bundle(W);
    b16.set_slot(0, mov_imm(14, 999));

    let mut b17 = Bundle::nop_bundle(W);
    b17.set_slot(3, ret());

    let program = vec![
        b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15, b16, b17,
    ];

    while step(&mut cpu, &program) {}

    assert_eq!(cpu.read_gpr(1), 0x55);
    assert_eq!(cpu.read_gpr(2), 0x0f);
    assert_eq!(cpu.read_gpr(3), 0x64);
    assert_eq!(cpu.read_gpr(4), 0x46);
    assert_eq!(cpu.read_gpr(5), 0x05);
    assert_eq!(cpu.read_gpr(6), 0x5f);
    assert_eq!(cpu.read_gpr(7), 0x5a);
    assert_eq!(cpu.read_gpr(8), 0x55);
    assert_eq!(cpu.read_gpr(9), 0x200);
    assert_eq!(cpu.read_gpr(10), 1);
    assert_eq!(cpu.read_gpr(14), 0);

    assert_eq!(cpu.memory[0x200], 0x88);
    assert_eq!(cpu.memory[0x202], 0x88);
    assert_eq!(cpu.memory[0x203], 0x77);
    assert_eq!(cpu.memory[0x208], 0x88);
    assert_eq!(cpu.memory[0x209], 0x77);
    assert_eq!(cpu.memory[0x20a], 0x66);
    assert_eq!(cpu.memory[0x20b], 0x55);

    assert!(cpu.read_pred(1));
    assert!(cpu.read_pred(2));
    assert!(cpu.read_pred(3));
    assert!(cpu.read_pred(4));
    assert!(!cpu.read_pred(5));
}

#[test]
fn illegal_same_bundle_gpr_waw_halts_before_execution() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut bad = Bundle::nop_bundle(W);
    bad.set_slot(0, mov_imm(1, 6));
    bad.set_slot(1, mov_imm(1, 7));
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
    assert_eq!(cpu.read_gpr(1), 0);
}

#[test]
fn illegal_same_bundle_ret_dependency_halts_before_execution() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut bad = Bundle::nop_bundle(W);
    bad.set_slot(0, mov_imm(31, 2));
    bad.set_slot(3, ret());
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
    assert_eq!(cpu.read_gpr(31), 0);
}

#[test]
fn illegal_same_bundle_call_ret_dependency_halts_wide_bundle() {
    const W8: usize = 8;
    let mut cpu = CpuState::new(W8, LatencyTable::default());

    let mut bad = Bundle::nop_bundle(W8);
    bad.set_slot(3, call(0));
    bad.set_slot(7, ret());
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
    assert_eq!(cpu.cycle, 0);
    assert_eq!(cpu.read_gpr(31), 0);
}

#[test]
fn illegal_same_bundle_predicate_raw_halts_before_execution() {
    let mut cpu = CpuState::new(W, LatencyTable::default());
    cpu.write_pred(1, true);

    let mut bad = Bundle::nop_bundle(W);
    bad.set_slot(0, cmp_lt(1, 0, 0));
    bad.set_slot(3, branch(1, false, 0));
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
}

#[test]
fn illegal_same_bundle_predicate_waw_halts_before_execution() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut bad = Bundle::nop_bundle(W);
    bad.set_slot(0, cmp_lt(1, 0, 0));
    bad.set_slot(3, p_not(1, 0));
    let program = vec![bad];

    assert!(!step(&mut cpu, &program));
    assert!(cpu.halted);
    assert_eq!(cpu.pc, 0);
}

#[test]
fn ret_stalls_until_link_register_ready() {
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::MovImm, 3);

    let mut cpu = CpuState::new(W, latencies);

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(1, mov_imm(31, 3));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(3, ret());

    let program = vec![b0, b1];

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 1);
    assert_eq!(cpu.read_gpr(31), 3);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 2);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 3);

    assert!(!cpu.halted);
    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 3);
    assert_eq!(cpu.cycle, 4);
    assert!(!cpu.halted);

    assert!(!step(&mut cpu, &program));
    assert_eq!(cpu.pc, 3);
    assert!(!cpu.halted);
}

#[test]
fn ret_stalls_until_call_link_register_ready() {
    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Call, 3);

    let mut cpu = CpuState::new(W, latencies);

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(3, call(1));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(3, ret());

    let program = vec![b0, b1];

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 1);
    assert_eq!(cpu.read_gpr(31), 1);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 2);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 3);

    assert!(step(&mut cpu, &program));
    assert_eq!(cpu.pc, 1);
    assert_eq!(cpu.cycle, 4);
}

#[test]
fn out_of_bounds_loads_return_zero() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(2, load_h(1, 0, 65535));

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(2, load_w(2, 0, 65533));

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(2, load_d(3, 0, 65529));

    let mut b3 = Bundle::nop_bundle(W);
    b3.set_slot(3, ret());

    let program = vec![b0, b1, b2, b3];
    while step(&mut cpu, &program) {}

    assert_eq!(cpu.read_gpr(1), 0);
    assert_eq!(cpu.read_gpr(2), 0);
    assert_eq!(cpu.read_gpr(3), 0);
}

#[test]
fn predicate_ops_with_none_sources_use_false_default() {
    let mut cpu = CpuState::new(W, LatencyTable::default());

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(
        3,
        Syllable {
            opcode: Opcode::PNot,
            dst: Some(1),
            src: [None, None],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(
        3,
        Syllable {
            opcode: Opcode::PAnd,
            dst: Some(2),
            src: [None, Some(1)],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        },
    );

    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(3, ret());

    let program = vec![b0, b1, b2];
    while step(&mut cpu, &program) {}

    assert!(cpu.read_pred(1));
    assert!(!cpu.read_pred(2));
}

#[test]
fn canonical_layout_allows_nop_in_any_slot() {
    let layout = canonical_layout(W);
    for slot in 0..W {
        assert!(layout.slot_can_execute(slot, Opcode::Nop));
    }
}

#[test]
fn sparse_latency_table_uses_default_get_and_append_set() {
    let mut latencies = sparse_latency_table();

    assert_eq!(latencies.get(Opcode::Add), 1);
    assert_eq!(latencies.get(Opcode::Nop), 1);

    latencies.set(Opcode::Add, 9);
    assert_eq!(latencies.get(Opcode::Add), 9);
    assert_eq!(latencies.entries.len(), 1);
}
