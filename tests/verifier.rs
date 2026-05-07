use vliw_simulator::asm::parse_program;
use vliw_simulator::bundle::Bundle;
use vliw_simulator::isa::{Opcode, Syllable};
use vliw_simulator::latency::LatencyTable;
use vliw_simulator::layout::canonical_layout;
use vliw_simulator::system::worst_case_visibility;
use vliw_simulator::verifier::{
    verify_program, verify_program_for_cpu, verify_system, Diagnostic, Rule,
};

const W: usize = 4;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn nop_bundle() -> Bundle {
    Bundle::nop_bundle(W)
}

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

fn processor_header_with_memory(width: usize, memory_size: &str) -> String {
    let mut header = processor_header(width);
    let marker = "  cache { }\n";
    header = header.replace(
        marker,
        &format!("  memory {{ size {memory_size} }}\n  cache {{ }}\n"),
    );
    header
}

fn sparse_no_multiplier_header() -> String {
    ".processor {
  width 4

  hardware {
    unit alu = integer_alu
    unit mem = memory
    unit ctrl = control
  }

  layout slots {
    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { ctrl }
  }

  cache { }
  topology { cpus 1 }
}
"
    .to_string()
}

fn syl(
    opcode: Opcode,
    dst: Option<usize>,
    src0: Option<usize>,
    src1: Option<usize>,
    imm: i64,
) -> Syllable {
    Syllable {
        opcode,
        dst,
        src: [src0, src1],
        imm,
        predicate: 0,
        pred_negated: false,
    }
}

fn movi(dst: usize, imm: i64) -> Syllable {
    syl(Opcode::MovImm, Some(dst), None, None, imm)
}

fn add(dst: usize, a: usize, b: usize) -> Syllable {
    syl(Opcode::Add, Some(dst), Some(a), Some(b), 0)
}

fn mul(dst: usize, a: usize, b: usize) -> Syllable {
    syl(Opcode::Mul, Some(dst), Some(a), Some(b), 0)
}

fn store_d(base: usize, data: usize, imm: i64) -> Syllable {
    syl(Opcode::StoreD, None, Some(base), Some(data), imm)
}

fn load_d(dst: usize, base: usize, imm: i64) -> Syllable {
    syl(Opcode::LoadD, Some(dst), Some(base), None, imm)
}

fn ret() -> Syllable {
    syl(Opcode::Ret, None, None, None, 0)
}

fn call(target: i64) -> Syllable {
    syl(Opcode::Call, None, None, None, target)
}

fn cmp_lt(dst: usize, a: usize, b: usize) -> Syllable {
    syl(Opcode::CmpLt, Some(dst), Some(a), Some(b), 0)
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

fn p_not(dst: usize, src: usize) -> Syllable {
    syl(Opcode::PNot, Some(dst), Some(src), None, 0)
}

fn p_and(dst: usize, a: usize, b: usize) -> Syllable {
    syl(Opcode::PAnd, Some(dst), Some(a), Some(b), 0)
}

fn has_rule(diags: &[Diagnostic], r: Rule) -> bool {
    diags.iter().any(|d| d.rule == r)
}

fn verify_bundles(program: &[Bundle], latencies: &LatencyTable) -> Vec<Diagnostic> {
    let width = program.first().map_or(W, |bundle| bundle.width());
    let layout = canonical_layout(width);
    verify_program(&layout, program, latencies)
}

fn write_temp_vliw(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("target").join("test-vliw");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}-{}.vliw", std::process::id()));
    std::fs::write(&path, source).unwrap();
    path
}

// ---------------------------------------------------------------------------
// Rule 2/7/8: slot opcode legality
// ---------------------------------------------------------------------------

#[test]
fn detects_control_op_in_integer_slot() {
    let mut b = nop_bundle();
    b.set_slot(0, ret()); // ret (Control) in slot 0 (Integer)
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn detects_memory_op_in_integer_slot() {
    let mut b = nop_bundle();
    b.set_slot(0, syl(Opcode::StoreD, None, Some(0), Some(1), 0)); // StoreD in slot 0
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn detects_integer_op_in_memory_slot() {
    let mut b = nop_bundle();
    b.set_slot(2, movi(1, 5)); // MovImm (Integer) in slot 2 (Memory)
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn detects_integer_op_in_control_slot() {
    let mut b = nop_bundle();
    b.set_slot(3, add(1, 2, 3)); // Add (Integer) in slot 3 (Control)
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn nop_in_any_slot_is_legal() {
    let program = vec![nop_bundle()];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn canonical_layout_matches_legacy_slot_positions() {
    let layout = canonical_layout(W);
    let integer_ops = [
        Opcode::Add,
        Opcode::Sub,
        Opcode::And,
        Opcode::Or,
        Opcode::Xor,
        Opcode::Shl,
        Opcode::Srl,
        Opcode::Sra,
        Opcode::Mov,
        Opcode::MovImm,
        Opcode::CmpEq,
        Opcode::CmpLt,
        Opcode::CmpUlt,
    ];
    let memory_ops = [
        Opcode::LoadB,
        Opcode::LoadH,
        Opcode::LoadW,
        Opcode::LoadD,
        Opcode::StoreB,
        Opcode::StoreH,
        Opcode::StoreW,
        Opcode::StoreD,
        Opcode::Lea,
        Opcode::Prefetch,
        Opcode::AcqLoad,
        Opcode::RelStore,
    ];
    let control_ops = [
        Opcode::Branch,
        Opcode::Jump,
        Opcode::Call,
        Opcode::Ret,
        Opcode::PAnd,
        Opcode::POr,
        Opcode::PXor,
        Opcode::PNot,
    ];
    let multiply_ops = [Opcode::Mul, Opcode::MulH];

    for op in integer_ops {
        assert!(layout.slot_can_execute(0, op), "{op:?}");
        assert!(layout.slot_can_execute(1, op), "{op:?}");
        assert!(!layout.slot_can_execute(2, op), "{op:?}");
        assert!(!layout.slot_can_execute(3, op), "{op:?}");
    }
    for op in memory_ops {
        assert!(!layout.slot_can_execute(0, op), "{op:?}");
        assert!(!layout.slot_can_execute(1, op), "{op:?}");
        assert!(layout.slot_can_execute(2, op), "{op:?}");
        assert!(!layout.slot_can_execute(3, op), "{op:?}");
    }
    for op in control_ops.into_iter().chain(multiply_ops) {
        assert!(!layout.slot_can_execute(0, op), "{op:?}");
        assert!(!layout.slot_can_execute(1, op), "{op:?}");
        assert!(!layout.slot_can_execute(2, op), "{op:?}");
        assert!(layout.slot_can_execute(3, op), "{op:?}");
    }
}

#[test]
fn sparse_layout_is_clean_when_program_uses_only_declared_units() {
    let source = format!(
        "{}{}",
        sparse_no_multiplier_header(),
        "\nentry:\n{\n  i0: movi r1, 1\n  i1: nop\n  m : nop\n  x : ret\n}\n"
    );
    let program = parse_program(&source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn sparse_layout_rejects_missing_multiplier_unit() {
    let source = format!(
        "{}{}",
        sparse_no_multiplier_header(),
        "\nentry:\n{\n  i0: nop\n  i1: nop\n  m : nop\n  x : mul r1, r0, r0\n}\n"
    );
    let program = parse_program(&source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn slot_legality_example_file_is_flagged() {
    let source = std::fs::read_to_string("examples/illegal_wrong_slot.vliw").unwrap();
    let program = parse_program(&source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Rule 3: same-bundle GPR RAW
// ---------------------------------------------------------------------------

#[test]
fn detects_same_bundle_gpr_raw() {
    let mut b = nop_bundle();
    b.set_slot(0, movi(1, 42)); // slot 0 writes r1
    b.set_slot(1, add(2, 1, 0)); // slot 1 reads r1 → RAW
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

#[test]
fn detects_same_bundle_raw_via_ret_reads_link_reg() {
    let mut b = nop_bundle();
    b.set_slot(1, movi(31, 3)); // slot 1 writes r31 (link)
    b.set_slot(3, ret()); // slot 3 ret implicitly reads r31
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

#[test]
fn detects_same_bundle_raw_via_call_then_ret_in_wide_bundle() {
    let mut b = Bundle::nop_bundle(8);
    b.set_slot(3, call(0)); // slot 3 implicitly writes r31 (link)
    b.set_slot(7, ret()); // slot 7 ret implicitly reads r31
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

#[test]
fn raw_example_file_is_flagged() {
    let source = std::fs::read_to_string("examples/illegal_raw_same_bundle.vliw").unwrap();
    let program = parse_program(&source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Rule 4: same-bundle GPR WAW
// ---------------------------------------------------------------------------

#[test]
fn detects_same_bundle_gpr_waw() {
    let mut b = nop_bundle();
    b.set_slot(0, movi(1, 6)); // slot 0 writes r1
    b.set_slot(1, movi(1, 7)); // slot 1 also writes r1 → WAW
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprWaw), "{diags:?}");
}

#[test]
fn waw_on_r0_is_not_flagged() {
    // r0 is hardwired zero; writes to it are silently dropped.
    let mut b = nop_bundle();
    b.set_slot(0, movi(0, 6));
    b.set_slot(1, movi(0, 7));
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(!has_rule(&diags, Rule::SameBundleGprWaw), "{diags:?}");
}

#[test]
fn detects_same_bundle_waw_via_call_and_explicit_link_write_in_wide_bundle() {
    let mut b = Bundle::nop_bundle(8);
    b.set_slot(3, call(0)); // slot 3 implicitly writes r31
    b.set_slot(4, movi(31, 0)); // slot 4 also writes r31
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprWaw), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Rule 5: same-bundle predicate hazards
// ---------------------------------------------------------------------------

#[test]
fn detects_same_bundle_pred_raw_branch() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 2, 3)); // slot 0 writes p1
    b.set_slot(3, branch(1, false, 5)); // slot 3 branch reads p1 → pred RAW
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn detects_same_bundle_pred_raw_pnot() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 0, 0)); // slot 0 writes p1
    b.set_slot(3, p_not(2, 1)); // slot 3 pnot reads p1 → pred RAW
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn detects_same_bundle_pred_raw_pand() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 0, 0)); // slot 0 writes p1
    b.set_slot(3, p_and(2, 1, 0)); // slot 3 pand reads p1 as src0 → pred RAW
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn detects_same_bundle_pred_waw() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 0, 0)); // slot 0 writes p1
    b.set_slot(3, p_not(1, 2)); // slot 3 also writes p1 → pred WAW
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn pred_waw_on_p0_is_not_flagged() {
    // p0 is the always-true constant; co-writes are ignored.
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(0, 1, 2));
    b.set_slot(3, p_not(0, 1));
    let program = vec![b];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(!has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Rule 6: GPR ready-cycle timing
// ---------------------------------------------------------------------------

#[test]
fn detects_gpr_timing_violation_mul_then_immediate_use() {
    // mul at bundle 0, consumer at bundle 1 — mul latency=3 means r3 isn't
    // ready until cycle 4, but bundle 1 issues at cycle 1 (needs it by cycle 2).
    let mut b0 = nop_bundle();
    b0.set_slot(0, movi(1, 6));
    b0.set_slot(1, movi(2, 7));

    let mut b1 = nop_bundle();
    b1.set_slot(3, mul(3, 1, 2)); // writes r3 with latency 3

    let mut b2 = nop_bundle();
    b2.set_slot(2, store_d(0, 3, 0x100)); // reads r3 immediately → too soon

    let mut b3 = nop_bundle();
    b3.set_slot(3, ret());

    let program = vec![b0, b1, b2, b3];
    let mut lats = LatencyTable::default();
    lats.set(Opcode::Mul, 3);
    let diags = verify_bundles(&program, &lats);
    assert!(has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
}

#[test]
fn no_timing_violation_when_gap_is_sufficient() {
    // mul at bundle 0 (lat=3) with two NOP bundles before the consumer.
    // ready_at[r3] = (0+1)+3 = 4; consumer at bundle 3: next_cycle=4, 4>4? NO → clean.
    let mut b0 = nop_bundle();
    b0.set_slot(0, movi(1, 6));
    b0.set_slot(1, movi(2, 7));

    let mut b1 = nop_bundle();
    b1.set_slot(3, mul(3, 1, 2)); // writes r3, lat=3, ready_at[3]=4

    // b2, b3: NOPs

    let mut b4 = nop_bundle();
    b4.set_slot(2, store_d(0, 3, 0x100)); // issue_cycle=4, next_cycle=5, 4>5? NO

    let mut b5 = nop_bundle();
    b5.set_slot(3, ret());

    let program = vec![b0, b1, nop_bundle(), nop_bundle(), b4, b5];
    let mut lats = LatencyTable::default();
    lats.set(Opcode::Mul, 3);
    let diags = verify_bundles(&program, &lats);
    assert!(!has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
}

#[test]
fn back_to_back_latency1_ops_are_clean() {
    // movi r1 at bundle 0 (lat=1), add reads r1 at bundle 1.
    // ready_at[1] = (0+1)+1 = 2; bundle 1 next_cycle = 2; 2>2? NO → clean.
    let mut b0 = nop_bundle();
    b0.set_slot(0, movi(1, 10));
    b0.set_slot(1, movi(2, 20));

    let mut b1 = nop_bundle();
    b1.set_slot(0, add(3, 1, 2));

    let mut b2 = nop_bundle();
    b2.set_slot(3, ret());

    let program = vec![b0, b1, b2];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(!has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
}

#[test]
fn verifier_uses_cache_worst_case_for_load_timing() {
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
  i0: add r2, r1, r0
  i1: nop
  m : nop
  x : ret
}
"#;
    let program = parse_program(source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());

    assert!(has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
    assert!(
        diags
            .iter()
            .any(|diag| diag.message.contains("not ready until cycle 10")),
        "{diags:?}"
    );
}

#[test]
fn verifier_uses_system_worst_case_load_latency() {
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
  topology { cpus 3 }
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
  i0: add r2, r1, r0
  i1: nop
  m : nop
  x : ret
}
"#;
    let program = parse_program(source).unwrap();
    let diags = verify_program_for_cpu(
        &program.layout,
        &program.bundles,
        &LatencyTable::default(),
        0,
    );

    assert!(has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
    assert!(
        diags
            .iter()
            .any(|diag| diag.message.contains("not ready until cycle 17")),
        "{diags:?}"
    );
}

#[test]
fn verifier_rejects_memory_op_outside_cpu_bus_slot() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;

    let mut b0 = nop_bundle();
    b0.set_slot(2, store_d(0, 1, 0));
    let program = vec![b0];

    let diags = verify_program_for_cpu(&layout, &program, &LatencyTable::default(), 1);
    assert!(has_rule(&diags, Rule::BusSlotConflict), "{diags:?}");
}

#[test]
fn verifier_accepts_memory_op_inside_cpu_bus_slot() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;

    let mut b0 = nop_bundle();
    b0.set_slot(2, store_d(0, 1, 0));
    let program = vec![b0];

    let diags = verify_program_for_cpu(&layout, &program, &LatencyTable::default(), 0);
    assert!(!has_rule(&diags, Rule::BusSlotConflict), "{diags:?}");
}

#[test]
fn detects_timing_violation_via_call_link_register_write() {
    let mut b0 = nop_bundle();
    b0.set_slot(3, call(1));

    let mut b1 = nop_bundle();
    b1.set_slot(3, ret());

    let program = vec![b0, b1];
    let mut lats = LatencyTable::default();
    lats.set(Opcode::Call, 3);
    let diags = verify_bundles(&program, &lats);
    assert!(has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
}

#[test]
fn rejects_static_out_of_bounds_store_from_zero_base() {
    let mut b = nop_bundle();
    b.set_slot(2, store_d(0, 1, 0x10000));
    let diags = verify_bundles(&[b], &LatencyTable::default());

    assert!(has_rule(&diags, Rule::StaticMemoryBounds), "{diags:?}");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("0x10000") && d.message.contains("memory size=0x10000")),
        "{diags:?}"
    );
}

#[test]
fn rejects_static_out_of_bounds_load_that_crosses_memory_end() {
    let mut b = nop_bundle();
    b.set_slot(2, load_d(1, 0, 0xffff));
    let diags = verify_bundles(&[b], &LatencyTable::default());

    assert!(has_rule(&diags, Rule::StaticMemoryBounds), "{diags:?}");
}

#[test]
fn accepts_static_memory_operand_inside_memory() {
    let mut b = nop_bundle();
    b.set_slot(2, store_d(0, 1, 0xfff8));
    let diags = verify_bundles(&[b], &LatencyTable::default());

    assert!(!has_rule(&diags, Rule::StaticMemoryBounds), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Clean programs produce no diagnostics
// ---------------------------------------------------------------------------

#[test]
fn top_level_positive_examples_are_clean() {
    for path in [
        "examples/clean_schedule.vliw",
        "examples/hello.vliw",
        "examples/mul_latency.vliw",
        "examples/predication.vliw",
    ] {
        let source = std::fs::read_to_string(path).unwrap();
        let program = parse_program(&source).unwrap();
        let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
        assert!(diags.is_empty(), "{path} should be clean: {diags:?}");
    }
}

#[test]
fn top_level_illegal_examples_report_expected_rules() {
    assert_illegal_fixture::<4>(
        "examples/illegal_raw_same_bundle.vliw",
        &[Rule::SameBundleGprRaw],
    );
    assert_illegal_fixture::<4>(
        "examples/illegal_wrong_slot.vliw",
        &[Rule::SlotOpcodeLegality],
    );
}

#[test]
fn empty_program_is_clean() {
    let program: Vec<Bundle> = vec![];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn all_nop_program_is_clean() {
    let program = vec![nop_bundle(), nop_bundle(), nop_bundle()];
    let diags = verify_bundles(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Verifier binary integration tests
// ---------------------------------------------------------------------------

#[test]
fn verifier_binary_exits_clean_on_clean_program() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .arg("examples/clean_schedule.vliw")
        .output()
        .expect("binary should run");

    assert!(
        out.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("CLEAN"), "{stdout}");
}

#[test]
fn verifier_binary_accepts_width_8_program() {
    let path = write_temp_vliw(
        "width8-clean",
        &format!(
            "{}{}",
            processor_header(8),
            r#"
{
  0: movi r1, 1
  4: movi r2, 2
  7: ret
}
"#
        ),
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .arg(&path)
        .output()
        .expect("binary should run");

    assert!(
        out.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("W=8"), "{stdout}");
    assert!(stdout.contains("CLEAN"), "{stdout}");
}

#[test]
fn verifier_binary_accepts_small_widths() {
    for fixture in [
        ("examples/fixtures/legal/w1_single_slot.vliw", "W=1"),
        ("examples/fixtures/legal/w2_dual_slot.vliw", "W=2"),
        ("examples/fixtures/legal/w3_triple_slot.vliw", "W=3"),
    ] {
        let (path, banner) = fixture;
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
            .arg(path)
            .output()
            .expect("binary should run");

        assert!(
            out.status.success(),
            "{path}: stdout: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.contains(banner), "{path}: {stdout}");
        assert!(stdout.contains("CLEAN"), "{path}: {stdout}");
    }
}

#[test]
fn verifier_accepts_non_power_of_two_widths_via_canonical_layout() {
    // canonical_layout used to be restricted to powers of two; widths 1..=256
    // are now valid and the verifier should accept a NOP-only program at any
    // such width.
    for width in [1usize, 2, 3, 5, 7, 13, 17, 100, 255, 256] {
        let layout = canonical_layout(width);
        assert!(layout.validate(), "layout invalid for width {width}");
        let program = vec![Bundle::nop_bundle(width)];
        let diags = verify_program(&layout, &program, &LatencyTable::default());
        assert!(diags.is_empty(), "width {width}: {diags:?}");
    }
}

#[test]
fn verifier_binary_exits_one_on_illegal_slot() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .arg("examples/illegal_wrong_slot.vliw")
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("slot-opcode-legality"), "{stdout}");
}

#[test]
fn verifier_binary_exits_one_on_raw_hazard() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .arg("examples/illegal_raw_same_bundle.vliw")
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("same-bundle-gpr-raw"), "{stdout}");
}

#[test]
fn verifier_binary_exits_one_on_static_memory_bounds() {
    let path = write_temp_vliw(
        "static-memory-bounds",
        &format!(
            "{}{}",
            processor_header_with_memory(W, "0x10000"),
            r#"
{
  m : store_d r0, r0, 0x10000
}
"#
        ),
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .arg(&path)
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("static-memory-bounds"), "{stdout}");
    assert!(stdout.contains("0x10000"), "{stdout}");
}

#[test]
fn verifier_binary_exits_two_on_no_args() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("usage:"), "{stderr}");
}

#[test]
fn verifier_binary_prints_bundle_count_and_header() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vliw_verify"))
        .arg("examples/clean_schedule.vliw")
        .output()
        .expect("binary should run");

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("VLIW Verifier"), "{stdout}");
    assert!(stdout.contains("Bundles"), "{stdout}");
}

// ---------------------------------------------------------------------------
// Backend golden fixtures
// ---------------------------------------------------------------------------

#[test]
fn backend_legal_fixtures_are_clean() {
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_control_pred_mem_latency.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_composed_slot.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_fp_unit.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_aes_unit.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_cache_hit_streak.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_cache_dirty_eviction.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_acqload_relstore.vliw");
    assert_clean_fixture::<4>("examples/fixtures/legal/w4_msi_coherence_cpu0.vliw");
    assert_clean_fixture::<8>("examples/fixtures/legal/w8_pred_mem_latency.vliw");
    assert_clean_fixture::<16>("examples/fixtures/legal/w16_call_mem_latency.vliw");
}

#[test]
fn backend_two_cpu_coherence_fixture_pair_is_clean() {
    let cpu0_source =
        std::fs::read_to_string("examples/fixtures/legal/w4_msi_coherence_cpu0.vliw").unwrap();
    let cpu1_source =
        std::fs::read_to_string("examples/fixtures/legal/w4_msi_coherence_cpu1.vliw").unwrap();
    let cpu0 = parse_program(&cpu0_source).unwrap();
    let cpu1 = parse_program(&cpu1_source).unwrap();
    assert_eq!(cpu0.layout, cpu1.layout);

    let programs = vec![cpu0.bundles, cpu1.bundles];
    let diags = verify_system(&cpu0.layout, &programs, &LatencyTable::default());
    assert!(
        diags.is_empty(),
        "coherence fixture pair should be clean: {diags:?}"
    );
}

#[test]
fn backend_illegal_fixtures_report_expected_rules() {
    assert_illegal_fixture::<4>(
        "examples/fixtures/illegal/w4_latency_mul_use.vliw",
        &[Rule::GprReadyCycle],
    );
    assert_illegal_fixture::<8>(
        "examples/fixtures/illegal/w8_call_ret_same_bundle.vliw",
        &[Rule::SameBundleGprRaw],
    );
    assert_illegal_fixture::<16>(
        "examples/fixtures/illegal/w16_predicate_and_slot.vliw",
        &[Rule::SameBundlePredHazard, Rule::SlotOpcodeLegality],
    );
    assert_illegal_fixture::<4>(
        "examples/fixtures/illegal/w4_missing_fp_unit.vliw",
        &[Rule::SlotOpcodeLegality],
    );
}

#[test]
fn processor_layout_parse_error_fixtures_are_rejected() {
    for path in [
        "examples/fixtures/illegal/w4_no_processor_header.vliw",
        "examples/fixtures/illegal/w4_unknown_unit.vliw",
        "examples/fixtures/illegal/w4_layout_width_mismatch.vliw",
    ] {
        let source = std::fs::read_to_string(path).unwrap();
        let err = parse_program(&source).expect_err("fixture should fail during parsing");
        assert!(
            err.contains("processor") || err.contains("layout"),
            "{path}: {err}"
        );
    }
}

fn assert_clean_fixture<const WIDTH: usize>(path: &str) {
    let source = std::fs::read_to_string(path).unwrap();
    let program = parse_program(&source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(
        diags.is_empty(),
        "{path} should be clean but got: {diags:?}"
    );
}

fn assert_illegal_fixture<const WIDTH: usize>(path: &str, expected_rules: &[Rule]) {
    let source = std::fs::read_to_string(path).unwrap();
    let program = parse_program(&source).unwrap();
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(
        !diags.is_empty(),
        "{path} should produce verifier diagnostics"
    );

    for rule in expected_rules {
        assert!(
            has_rule(&diags, rule.clone()),
            "{path} should report {rule:?}, got: {diags:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Stage 4C helpers
// ---------------------------------------------------------------------------

fn acq_load(dst: usize, base: usize, imm: i64) -> Syllable {
    syl(Opcode::AcqLoad, Some(dst), Some(base), None, imm)
}

fn rel_store(base: usize, data: usize, imm: i64) -> Syllable {
    syl(Opcode::RelStore, None, Some(base), Some(data), imm)
}

fn cmp_eq(dst: usize, a: usize, b: usize) -> Syllable {
    syl(Opcode::CmpEq, Some(dst), Some(a), Some(b), 0)
}

// ---------------------------------------------------------------------------
// Stage 4C: AcqLoad / RelStore slot routing
// ---------------------------------------------------------------------------

#[test]
fn acqload_and_relstore_route_to_memory_slot() {
    let layout = canonical_layout(W);
    assert!(layout.slot_can_execute(2, Opcode::AcqLoad));
    assert!(layout.slot_can_execute(2, Opcode::RelStore));
    assert!(!layout.slot_can_execute(0, Opcode::AcqLoad));
    assert!(!layout.slot_can_execute(3, Opcode::RelStore));
}

#[test]
fn acqload_in_wrong_slot_is_rejected() {
    let mut b = nop_bundle();
    b.set_slot(0, acq_load(1, 0, 0)); // AcqLoad in integer slot
    let diags = verify_bundles(&[b], &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn relstore_in_wrong_slot_is_rejected() {
    let mut b = nop_bundle();
    b.set_slot(3, rel_store(0, 1, 0)); // RelStore in control slot
    let diags = verify_bundles(&[b], &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn acqload_bus_slot_conflict_rejected() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;

    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(2, acq_load(1, 0, 0)); // AcqLoad at bundle 0 (CPU 0's cycle) issued by CPU 1

    let diags = verify_program_for_cpu(&layout, &[b0], &LatencyTable::default(), 1);
    assert!(has_rule(&diags, Rule::BusSlotConflict), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Stage 4C: worst_case_visibility
// ---------------------------------------------------------------------------

#[test]
fn worst_case_visibility_single_cpu_equals_cache_cost() {
    let layout = canonical_layout(W); // cpus=1, default cache (miss=3, writeback=0)
    let vis = worst_case_visibility(&layout);
    // (1-1)*1 + 3 + 0 = 3
    assert_eq!(vis, 3);
}

#[test]
fn worst_case_visibility_two_cpus_adds_bus_slot() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;
    let vis = worst_case_visibility(&layout);
    // (2-1)*1 + 3 + 0 = 4
    assert_eq!(vis, 4);
}

#[test]
fn worst_case_visibility_folds_in_coherence_drain() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;
    layout.cache.writeback_latency = 5;
    let vis = worst_case_visibility(&layout);
    // (2-1)*1 + miss_latency(3) + writeback_latency(5) + coherence_drain(5) = 14
    assert_eq!(vis, 14);
}

// ---------------------------------------------------------------------------
// Stage 4C: cross-CPU polling loop verification
// ---------------------------------------------------------------------------

/// Two-CPU producer/consumer: CPU 0 writes with RelStore, CPU 1 polls with AcqLoad.
/// Memory ops must be on cycles owned by each CPU (bundle_idx % cpus == cpu_id).
/// CPU 0 owns even indices (0, 2, 4, ...); CPU 1 owns odd indices (1, 3, 5, ...).
/// With cpus=2 and default cache, worst_case_load_latency = 4, so AcqLoad at
/// bundle_idx=1 is ready at cycle 1+1+4=6; CmpEq must be at bundle_idx≥5.
#[test]
fn verify_system_bounded_polling_loop_is_clean() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 2;

    // Producer (CPU 0): RelStore at bundle_idx=2 (even → CPU 0's cycle)
    let mut p_b0 = Bundle::nop_bundle(W);
    p_b0.set_slot(0, syl(Opcode::MovImm, Some(1), None, None, 99));
    let p_b1 = Bundle::nop_bundle(W);
    let mut p_b2 = Bundle::nop_bundle(W);
    p_b2.set_slot(2, rel_store(0, 1, 0x100));
    let p_b3 = Bundle::nop_bundle(W);
    let mut p_b4 = Bundle::nop_bundle(W);
    p_b4.set_slot(3, ret());

    // Consumer (CPU 1): AcqLoad at bundle_idx=1 (odd → CPU 1's cycle).
    // r2 ready at cycle 6 (1+1+4). CmpEq at bundle_idx=5 (issue=5, needed by 6 → OK).
    // Backward branch at bundle_idx=7 targeting bundle_idx=1.
    let c_b0 = Bundle::nop_bundle(W);
    let mut c_b1 = Bundle::nop_bundle(W);
    c_b1.set_slot(2, acq_load(2, 0, 0x100)); // AcqLoad at odd cycle 1
    let c_b2 = Bundle::nop_bundle(W);
    let c_b3 = Bundle::nop_bundle(W);
    let c_b4 = Bundle::nop_bundle(W);
    let mut c_b5 = Bundle::nop_bundle(W);
    c_b5.set_slot(0, cmp_eq(1, 2, 0)); // cmpeq p1, r2, r0 at cycle 5 (r2 ready by 6 ✓)
    let c_b6 = Bundle::nop_bundle(W);
    let mut c_b7 = Bundle::nop_bundle(W);
    c_b7.set_slot(3, branch(1, false, 1)); // branch p1, 1 (loop while r2==0)
    let mut c_b8 = Bundle::nop_bundle(W);
    c_b8.set_slot(3, ret());

    let programs = vec![
        vec![p_b0, p_b1, p_b2, p_b3, p_b4],
        vec![c_b0, c_b1, c_b2, c_b3, c_b4, c_b5, c_b6, c_b7, c_b8],
    ];

    let diags = verify_system(&layout, &programs, &LatencyTable::default());
    assert!(
        !has_rule(&diags, Rule::UnboundedPollingLoop),
        "bounded loop should not be flagged: {diags:?}"
    );
    assert!(
        diags.is_empty(),
        "bounded loop should have no diagnostics: {diags:?}"
    );
}

/// Single-CPU polling loop with `AcqLoad` is unbounded (no other CPU can produce).
#[test]
fn verify_system_unbounded_polling_loop_is_rejected() {
    let layout = canonical_layout(W); // cpus=1: CPU 0 owns all cycles

    // AcqLoad at bundle 0; with cpus=1 the worst-case load latency is 3,
    // so r1 is ready at cycle 4. CmpEq at bundle 3 (issue=3, needed by 4) is just in time.
    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(2, acq_load(1, 0, 0)); // acqload r1, [r0+0]

    let b1 = Bundle::nop_bundle(W);
    let b2 = Bundle::nop_bundle(W);

    let mut b3 = Bundle::nop_bundle(W);
    b3.set_slot(0, cmp_eq(1, 1, 0)); // cmpeq p1, r1, r0 (p1=true while r1==0)

    let mut b4 = Bundle::nop_bundle(W);
    b4.set_slot(3, branch(1, false, 0)); // branch p1, 0 (loop while flag not set)

    let mut b5 = Bundle::nop_bundle(W);
    b5.set_slot(3, ret());

    let programs = vec![vec![b0, b1, b2, b3, b4, b5]];
    let diags = verify_system(&layout, &programs, &LatencyTable::default());
    assert!(
        has_rule(&diags, Rule::UnboundedPollingLoop),
        "single-CPU polling loop should be flagged as unbounded: {diags:?}"
    );
}

/// Forward branches that contain AcqLoad are NOT polling loops.
#[test]
fn acqload_without_backward_branch_is_not_a_polling_loop() {
    let mut layout = canonical_layout(W);
    layout.topology.cpus = 1;

    // Bundle 0: acqload + forward branch
    let mut b0 = Bundle::nop_bundle(W);
    b0.set_slot(2, acq_load(1, 0, 0));
    let mut b1 = Bundle::nop_bundle(W);
    b1.set_slot(3, branch(0, false, 2)); // forward branch to bundle 2 (p0 = always true)
    let mut b2 = Bundle::nop_bundle(W);
    b2.set_slot(3, ret());

    let programs = vec![vec![b0, b1, b2]];
    let diags = verify_system(&layout, &programs, &LatencyTable::default());
    assert!(
        !has_rule(&diags, Rule::UnboundedPollingLoop),
        "forward branch should not trigger unbounded-loop check: {diags:?}"
    );
}

/// Parses acqload and relstore mnemonics from .vliw text.
#[test]
fn acqload_relstore_parse_and_verify_clean() {
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

  cache { }
  topology { cpus 1 }
}

{
  i0: movi r1, 42
  i1: nop
  m : nop
  x : nop
}

{
  i0: nop
  i1: nop
  m : relstore r0, r1, 0
  x : nop
}

{
  i0: nop
  i1: nop
  m : acqload r2, r0, 0
  x : ret
}
"#;
    let program = parse_program(source).expect("should parse");
    assert_eq!(program.bundles[1].syllables[2].opcode, Opcode::RelStore);
    assert_eq!(program.bundles[2].syllables[2].opcode, Opcode::AcqLoad);
    let diags = verify_program(&program.layout, &program.bundles, &LatencyTable::default());
    assert!(diags.is_empty(), "should be clean: {diags:?}");
}
