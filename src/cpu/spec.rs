verus! {

// ---------------------------------------------------------------------------
// Spec helpers used in execute_syllable postconditions
// ---------------------------------------------------------------------------

/// Is `syl` active in state `cpu`?
pub open spec fn spec_syl_active<const W: usize>(cpu: &CpuState<W>, syl: &Syllable) -> bool {
    let pv = if syl.predicate == 0 { true }
             else if syl.predicate < NUM_PREDS { cpu.preds[syl.predicate as int] }
             else { false };
    if syl.pred_negated { !pv } else { pv }
}

/// Read GPR `idx` with r0-is-zero and out-of-range clamping.
pub open spec fn spec_gpr<const W: usize>(cpu: &CpuState<W>, idx: usize) -> u64 {
    if idx == 0 || idx >= NUM_GPRS { 0u64 } else { cpu.gprs[idx as int] }
}

/// Read source operand (`None` → 0).
pub open spec fn spec_src<const W: usize>(cpu: &CpuState<W>, r: Option<usize>) -> u64 {
    match r { Some(i) => spec_gpr(cpu, i), None => 0u64 }
}

/// Read predicate register (`idx` 0 → true, out-of-range → false).
pub open spec fn spec_pred<const W: usize>(cpu: &CpuState<W>, idx: usize) -> bool {
    if idx == 0 { true } else if idx < NUM_PREDS { cpu.preds[idx as int] } else { false }
}

/// Read predicate source (`None` → false).
pub open spec fn spec_pred_src<const W: usize>(cpu: &CpuState<W>, r: Option<usize>) -> bool {
    match r { Some(i) => spec_pred(cpu, i), None => false }
}

/// Is `op` an opcode that writes its result to a GPR via writeback?
pub open spec fn spec_is_gpr_writer(op: Opcode) -> bool {
    op == Opcode::Add  || op == Opcode::Sub  || op == Opcode::And ||
    op == Opcode::Or   || op == Opcode::Xor  || op == Opcode::Shl ||
    op == Opcode::Srl  || op == Opcode::Sra  || op == Opcode::Mov ||
    op == Opcode::MovImm || op == Opcode::Mul || op == Opcode::MulH ||
    op == Opcode::Lea  || op == Opcode::LoadB || op == Opcode::LoadH ||
    op == Opcode::LoadW || op == Opcode::LoadD
}

/// Is `op` a store opcode (writes memory, not a GPR)?
pub open spec fn spec_is_store(op: Opcode) -> bool {
    op == Opcode::StoreB || op == Opcode::StoreH ||
    op == Opcode::StoreW || op == Opcode::StoreD
}

/// Spec: address used by a store/load (src0 + imm, wrapping).
pub open spec fn spec_addr<const W: usize>(cpu: &CpuState<W>, syl: &Syllable) -> usize {
    (spec_src(cpu, syl.src[0]).wrapping_add(syl.imm as u64)) as usize
}

} // verus!
