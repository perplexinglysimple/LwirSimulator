verus! {

// ---------------------------------------------------------------------------
// Spec helpers used in execute_syllable postconditions
// ---------------------------------------------------------------------------

/// Is `syl` active in state `cpu`?
pub open spec fn spec_syl_active(cpu: &CpuState, syl: &Syllable) -> bool {
    let pv = if syl.predicate == 0 { true }
             else if syl.predicate < cpu.num_preds { cpu.preds[syl.predicate as int] }
             else { false };
    if syl.pred_negated { !pv } else { pv }
}

/// Read GPR `idx` with r0-is-zero and out-of-range clamping.
pub open spec fn spec_gpr(cpu: &CpuState, idx: usize) -> u64 {
    if idx == 0 || idx >= cpu.num_gprs { 0u64 } else { cpu.gprs[idx as int] }
}

/// Read source operand (`None` → 0).
pub open spec fn spec_src(cpu: &CpuState, r: Option<usize>) -> u64 {
    match r { Some(i) => spec_gpr(cpu, i), None => 0u64 }
}

/// Read predicate register (`idx` 0 → true, out-of-range → false).
pub open spec fn spec_pred(cpu: &CpuState, idx: usize) -> bool {
    if idx == 0 { true } else if idx < cpu.num_preds { cpu.preds[idx as int] } else { false }
}

/// Read predicate source (`None` → false).
pub open spec fn spec_pred_src(cpu: &CpuState, r: Option<usize>) -> bool {
    match r { Some(i) => spec_pred(cpu, i), None => false }
}

/// Is `op` an opcode that writes its result to a GPR via writeback?
pub open spec fn spec_is_gpr_writer(op: Opcode) -> bool {
    op == Opcode::Add  || op == Opcode::AddImm || op == Opcode::Sub  || op == Opcode::SubImm || op == Opcode::And ||
    op == Opcode::Or   || op == Opcode::Xor  || op == Opcode::Shl ||
    op == Opcode::Srl  || op == Opcode::Sra  || op == Opcode::Mov ||
    op == Opcode::MovImm || op == Opcode::Mul || op == Opcode::MulH ||
    op == Opcode::Lea  || op == Opcode::LoadB || op == Opcode::LoadH ||
    op == Opcode::LoadW || op == Opcode::LoadD || op == Opcode::AcqLoad ||
    op == Opcode::FpAdd32 || op == Opcode::FpSub32 ||
    op == Opcode::FpMul32 || op == Opcode::FpDiv32 ||
    op == Opcode::FpCvt32To64 || op == Opcode::FpCvtI32ToFp32 ||
    op == Opcode::FpCvtFp32ToI32 ||
    op == Opcode::FpAdd64 || op == Opcode::FpSub64 ||
    op == Opcode::FpMul64 || op == Opcode::FpDiv64 ||
    op == Opcode::FpCvt64To32 || op == Opcode::FpCvtI64ToFp64 ||
    op == Opcode::FpCvtFp64ToI64 ||
    op == Opcode::AesEnc || op == Opcode::AesDec
}

/// Is `op` a store opcode (writes memory, not a GPR)?
pub open spec fn spec_is_store(op: Opcode) -> bool {
    op == Opcode::StoreB || op == Opcode::StoreH ||
    op == Opcode::StoreW || op == Opcode::StoreD || op == Opcode::RelStore
}

/// Spec: address used by a store/load (src0 + imm, wrapping).
pub open spec fn spec_addr(cpu: &CpuState, syl: &Syllable) -> usize {
    (spec_src(cpu, syl.src[0]).wrapping_add(syl.imm as u64)) as usize
}

/// Spec: GPR destination written by (opcode, dst), including `call`'s implicit r31 write.
pub open spec fn spec_gpr_write_dst(op: Opcode, dst: Option<usize>) -> Option<usize> {
    if op == Opcode::Call { Some(31usize) }
    else if spec_is_gpr_writer(op) { dst }
    else { None }
}

/// Spec: does this opcode write a predicate register destination?
pub open spec fn spec_opcode_writes_pred(op: Opcode) -> bool {
    op == Opcode::CmpEq || op == Opcode::CmpLt || op == Opcode::CmpUlt
    || op == Opcode::FpCmp32 || op == Opcode::FpCmp64
    || op == Opcode::PAnd || op == Opcode::POr || op == Opcode::PXor || op == Opcode::PNot
}

/// Spec: does this opcode read predicate registers as ALU source operands?
/// `branch` reads its predicate via the dedicated `predicate` field, not src[], so it is excluded.
pub open spec fn spec_opcode_reads_pred_src(op: Opcode) -> bool {
    op == Opcode::PAnd || op == Opcode::POr || op == Opcode::PXor || op == Opcode::PNot
}

} // verus!
