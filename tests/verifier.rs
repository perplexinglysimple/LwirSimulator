use lwir_simulator::asm::parse_program;
use lwir_simulator::bundle::Bundle;
use lwir_simulator::isa::{Opcode, Syllable};
use lwir_simulator::latency::LatencyTable;
use lwir_simulator::verifier::{verify_program, Rule};

const W: usize = 4;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn nop_bundle() -> Bundle<W> {
    Bundle::<W>::nop_bundle()
}

fn syl(opcode: Opcode, dst: Option<usize>, src0: Option<usize>, src1: Option<usize>, imm: i64) -> Syllable {
    Syllable { opcode, dst, src: [src0, src1], imm, predicate: 0, pred_negated: false }
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

fn has_rule(diags: &[lwir_simulator::verifier::Diagnostic], r: Rule) -> bool {
    diags.iter().any(|d| d.rule == r)
}

fn write_temp_lwir(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("target").join("test-lwir");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}-{}.lwir", std::process::id()));
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
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn detects_memory_op_in_integer_slot() {
    let mut b = nop_bundle();
    b.set_slot(0, syl(Opcode::StoreD, None, Some(0), Some(1), 0)); // StoreD in slot 0
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn detects_integer_op_in_memory_slot() {
    let mut b = nop_bundle();
    b.set_slot(2, movi(1, 5)); // MovImm (Integer) in slot 2 (Memory)
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn detects_integer_op_in_control_slot() {
    let mut b = nop_bundle();
    b.set_slot(3, add(1, 2, 3)); // Add (Integer) in slot 3 (Control)
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

#[test]
fn nop_in_any_slot_is_legal() {
    let program = vec![nop_bundle()];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn slot_legality_example_file_is_flagged() {
    let source = std::fs::read_to_string("examples/illegal_wrong_slot.lwir").unwrap();
    let program = parse_program::<W>(&source).unwrap();
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SlotOpcodeLegality), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Rule 3: same-bundle GPR RAW
// ---------------------------------------------------------------------------

#[test]
fn detects_same_bundle_gpr_raw() {
    let mut b = nop_bundle();
    b.set_slot(0, movi(1, 42));                        // slot 0 writes r1
    b.set_slot(1, add(2, 1, 0));                        // slot 1 reads r1 → RAW
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

#[test]
fn detects_same_bundle_raw_via_ret_reads_link_reg() {
    let mut b = nop_bundle();
    b.set_slot(1, movi(31, 3)); // slot 1 writes r31 (link)
    b.set_slot(3, ret());        // slot 3 ret implicitly reads r31
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

#[test]
fn detects_same_bundle_raw_via_call_then_ret_in_wide_bundle() {
    let mut b = Bundle::<8>::nop_bundle();
    b.set_slot(3, call(0)); // slot 3 implicitly writes r31 (link)
    b.set_slot(7, ret());   // slot 7 ret implicitly reads r31
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprRaw), "{diags:?}");
}

#[test]
fn raw_example_file_is_flagged() {
    let source = std::fs::read_to_string("examples/illegal_raw_same_bundle.lwir").unwrap();
    let program = parse_program::<W>(&source).unwrap();
    let diags = verify_program(&program, &LatencyTable::default());
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
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundleGprWaw), "{diags:?}");
}

#[test]
fn waw_on_r0_is_not_flagged() {
    // r0 is hardwired zero; writes to it are silently dropped.
    let mut b = nop_bundle();
    b.set_slot(0, movi(0, 6));
    b.set_slot(1, movi(0, 7));
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(!has_rule(&diags, Rule::SameBundleGprWaw), "{diags:?}");
}

#[test]
fn detects_same_bundle_waw_via_call_and_explicit_link_write_in_wide_bundle() {
    let mut b = Bundle::<8>::nop_bundle();
    b.set_slot(3, call(0));      // slot 3 implicitly writes r31
    b.set_slot(4, movi(31, 0));  // slot 4 also writes r31
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
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
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn detects_same_bundle_pred_raw_pnot() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 0, 0)); // slot 0 writes p1
    b.set_slot(3, p_not(2, 1));      // slot 3 pnot reads p1 → pred RAW
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn detects_same_bundle_pred_raw_pand() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 0, 0)); // slot 0 writes p1
    b.set_slot(3, p_and(2, 1, 0));  // slot 3 pand reads p1 as src0 → pred RAW
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn detects_same_bundle_pred_waw() {
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(1, 0, 0)); // slot 0 writes p1
    b.set_slot(3, p_not(1, 2));      // slot 3 also writes p1 → pred WAW
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::SameBundlePredHazard), "{diags:?}");
}

#[test]
fn pred_waw_on_p0_is_not_flagged() {
    // p0 is the always-true constant; co-writes are ignored.
    let mut b = nop_bundle();
    b.set_slot(0, cmp_lt(0, 1, 2));
    b.set_slot(3, p_not(0, 1));
    let program = vec![b];
    let diags = verify_program(&program, &LatencyTable::default());
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
    let diags = verify_program(&program, &lats);
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
    let diags = verify_program(&program, &lats);
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
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(!has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
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
    let diags = verify_program(&program, &lats);
    assert!(has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
}

#[test]
fn detects_timing_violation_in_hello_lwir() {
    // hello.lwir: mul r3 at bundle 1 (lat=3), store r3 at bundle 2 — not enough gap.
    let source = std::fs::read_to_string("examples/hello.lwir").unwrap();
    let program = parse_program::<W>(&source).unwrap();
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(has_rule(&diags, Rule::GprReadyCycle), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Clean programs produce no diagnostics
// ---------------------------------------------------------------------------

#[test]
fn clean_program_produces_no_diagnostics() {
    let source = std::fs::read_to_string("examples/clean_schedule.lwir").unwrap();
    let program = parse_program::<W>(&source).unwrap();
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "expected clean program but got: {diags:?}");
}

#[test]
fn empty_program_is_clean() {
    let program: Vec<Bundle<W>> = vec![];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn all_nop_program_is_clean() {
    let program = vec![nop_bundle(), nop_bundle(), nop_bundle()];
    let diags = verify_program(&program, &LatencyTable::default());
    assert!(diags.is_empty(), "{diags:?}");
}

// ---------------------------------------------------------------------------
// Verifier binary integration tests
// ---------------------------------------------------------------------------

#[test]
fn verifier_binary_exits_clean_on_clean_program() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lwir_verify"))
        .arg("examples/clean_schedule.lwir")
        .output()
        .expect("binary should run");

    assert!(out.status.success(), "stdout: {}", String::from_utf8_lossy(&out.stdout));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("CLEAN"), "{stdout}");
}

#[test]
fn verifier_binary_accepts_width_8_program() {
    let path = write_temp_lwir(
        "width8-clean",
        r#".width 8
{
  0: movi r1, 1
  4: movi r2, 2
  7: ret
}
"#,
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lwir_verify"))
        .arg(&path)
        .output()
        .expect("binary should run");

    assert!(out.status.success(), "stdout: {}", String::from_utf8_lossy(&out.stdout));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("W=8"), "{stdout}");
    assert!(stdout.contains("CLEAN"), "{stdout}");
}

#[test]
fn verifier_binary_exits_one_on_illegal_slot() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lwir_verify"))
        .arg("examples/illegal_wrong_slot.lwir")
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("slot-opcode-legality"), "{stdout}");
}

#[test]
fn verifier_binary_exits_one_on_raw_hazard() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lwir_verify"))
        .arg("examples/illegal_raw_same_bundle.lwir")
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("same-bundle-gpr-raw"), "{stdout}");
}

#[test]
fn verifier_binary_exits_two_on_no_args() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lwir_verify"))
        .output()
        .expect("binary should run");

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("usage:"), "{stderr}");
}

#[test]
fn verifier_binary_prints_bundle_count_and_header() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_lwir_verify"))
        .arg("examples/clean_schedule.lwir")
        .output()
        .expect("binary should run");

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("LWIR Verifier"), "{stdout}");
    assert!(stdout.contains("Bundles"), "{stdout}");
}
