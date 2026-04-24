/// LWIR VLIW processor state and execution engine.
use crate::bundle::Bundle;
use crate::isa::{Opcode, Syllable};
use crate::latency::LatencyTable;
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

pub const NUM_GPRS:  usize = 32;
pub const NUM_PREDS: usize = 16;
pub const MEM_SIZE:  usize = 65536;

/// Scoreboard: tracks when each GPR's in-flight write completes.
#[derive(Clone, Copy, Debug)]
pub struct ScoreboardEntry {
    pub ready_cycle: u64,
}

/// Full architectural state — all fields pub so callers can snapshot freely.
#[derive(Clone, Debug)]
pub struct CpuState<const W: usize> {
    pub gprs:       Vec<u64>,
    pub preds:      Vec<bool>,
    pub pc:         usize,
    pub cycle:      u64,
    pub scoreboard: Vec<ScoreboardEntry>,
    pub memory:     Vec<u8>,
    pub halted:     bool,
    pub latencies:  LatencyTable,
}

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

// ---------------------------------------------------------------------------
// Well-formedness predicate
// ---------------------------------------------------------------------------

impl<const W: usize> CpuState<W> {
    /// The processor state is well-formed when all register files and memory
    /// have the expected sizes, and the hardwired values (r0=0, p0=true) hold.
    pub open spec fn wf(&self) -> bool {
        &&& self.gprs.len()       == NUM_GPRS
        &&& self.preds.len()      == NUM_PREDS
        &&& self.scoreboard.len() == NUM_GPRS
        &&& self.memory.len()     == MEM_SIZE
        &&& self.gprs[0int]       == 0u64
        &&& self.preds[0int]      == true
    }

    // -----------------------------------------------------------------------
    // Constructor
    // -----------------------------------------------------------------------

    /// Create a reset CPU.
    ///
    /// Postconditions:
    ///   - wf() holds
    ///   - every GPR is zero, every predicate is false except p0=true
    ///   - every byte of memory is zero
    ///   - every scoreboard entry has ready_cycle 0
    ///   - pc == 0, cycle == 0, halted == false
    pub fn new(latencies: LatencyTable) -> (ret: Self)
        ensures
            ret.wf(),
            ret.pc    == 0,
            ret.cycle == 0,
            !ret.halted,
            forall|i: int| 0 <= i < NUM_GPRS  ==> ret.gprs[i] == 0u64,
            forall|i: int| 0 <= i < NUM_PREDS ==> ret.preds[i] == (i == 0),
            forall|i: int| 0 <= i < MEM_SIZE  ==> ret.memory[i] == 0u8,
            forall|i: int| 0 <= i < NUM_GPRS  ==> ret.scoreboard[i].ready_cycle == 0u64,
    {
        let mut gprs: Vec<u64> = Vec::new();
        let mut scoreboard: Vec<ScoreboardEntry> = Vec::new();
        let mut i = 0usize;
        while i < NUM_GPRS
            invariant
                i <= NUM_GPRS,
                gprs.len() == i,
                scoreboard.len() == i,
                forall|j: int| 0 <= j < i ==> gprs[j] == 0u64,
                forall|j: int| 0 <= j < i ==> scoreboard[j].ready_cycle == 0u64,
            decreases NUM_GPRS - i,
        {
            gprs.push(0u64);
            scoreboard.push(ScoreboardEntry { ready_cycle: 0 });
            i += 1;
        }

        let mut preds = Vec::new();
        let mut j = 0usize;
        while j < NUM_PREDS
            invariant
                j <= NUM_PREDS,
                preds.len() == j,
                forall|k: int| 0 <= k < j ==> preds[k] == (k == 0),
            decreases NUM_PREDS - j,
        {
            preds.push(j == 0);
            j += 1;
        }

        let mut memory = Vec::new();
        let mut k = 0usize;
        while k < MEM_SIZE
            invariant
                k <= MEM_SIZE,
                memory.len() == k,
                forall|m: int| 0 <= m < k ==> memory[m] == 0u8,
            decreases MEM_SIZE - k,
        {
            memory.push(0u8);
            k += 1;
        }

        CpuState { gprs, preds, pc: 0, cycle: 0, scoreboard, memory, halted: false, latencies }
    }

    // -----------------------------------------------------------------------
    // Register accessors — weak precondition, strong postcondition
    // -----------------------------------------------------------------------

    /// Read GPR at `idx`.
    ///
    /// Precondition: wf() (weak).
    /// Postcondition: r0 always reads 0; any in-range index returns its exact value;
    ///                out-of-range returns 0.
    pub fn read_gpr(&self, idx: usize) -> (ret: u64)
        requires self.wf(),
        ensures
            idx == 0 || idx >= NUM_GPRS ==> ret == 0u64,
            0 < idx < NUM_GPRS          ==> ret == self.gprs[idx as int],
    {
        if idx == 0 || idx >= NUM_GPRS { 0u64 } else { self.gprs[idx] }
    }

    /// Write GPR at `idx` with `val`.
    ///
    /// Precondition: wf() (weak).
    /// Postconditions:
    ///   - wf() is preserved
    ///   - r0 is immutable: writes are silently dropped
    ///   - writing a valid non-zero index sets exactly that register to val
    ///   - every other register is unchanged
    pub fn write_gpr(&mut self, idx: usize, val: u64)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.preds      == old(self).preds,
            self.memory     == old(self).memory,
            self.pc         == old(self).pc,
            self.cycle      == old(self).cycle,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            idx == 0 || idx >= NUM_GPRS ==>
                forall|i: int| 0 <= i < NUM_GPRS ==> self.gprs[i] == old(self).gprs[i],
            0 < idx < NUM_GPRS ==> self.gprs[idx as int] == val,
            0 < idx < NUM_GPRS ==>
                forall|i: int| 0 <= i < NUM_GPRS && i != idx ==>
                    self.gprs[i] == old(self).gprs[i],
    {
        if idx != 0 && idx < NUM_GPRS {
            self.gprs.set(idx, val);
        }
    }

    /// Read predicate register at `idx`.
    ///
    /// Precondition: wf() (weak).
    /// Postcondition: p0 always reads true; in-range returns exact value;
    ///                out-of-range returns false.
    pub fn read_pred(&self, idx: usize) -> (ret: bool)
        requires self.wf(),
        ensures
            idx == 0              ==> ret == true,
            idx >= NUM_PREDS      ==> ret == false,
            0 < idx < NUM_PREDS   ==> ret == self.preds[idx as int],
    {
        if idx == 0 { true } else if idx < NUM_PREDS { self.preds[idx] } else { false }
    }

    /// Read an optional GPR source operand with the architectural r0/out-of-range behavior.
    pub fn read_src_gpr(&self, r: Option<usize>) -> (ret: u64)
        requires self.wf(),
        ensures ret == spec_src(self, r),
    {
        match r {
            Some(i) => self.read_gpr(i),
            None => 0u64,
        }
    }

    /// Read an optional predicate source operand with the architectural p0/out-of-range behavior.
    pub fn read_src_pred(&self, r: Option<usize>) -> (ret: bool)
        requires self.wf(),
        ensures ret == spec_pred_src(self, r),
    {
        match r {
            Some(i) => self.read_pred(i),
            None => false,
        }
    }

    /// Write predicate register at `idx` with `val`.
    ///
    /// Precondition: wf() (weak).
    /// Postconditions:
    ///   - wf() preserved
    ///   - p0 is immutable
    ///   - valid non-zero index is set to val
    ///   - every other predicate is unchanged
    pub fn write_pred(&mut self, idx: usize, val: bool)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.gprs       == old(self).gprs,
            self.memory     == old(self).memory,
            self.pc         == old(self).pc,
            self.cycle      == old(self).cycle,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            idx == 0 || idx >= NUM_PREDS ==>
                forall|i: int| 0 <= i < NUM_PREDS ==> self.preds[i] == old(self).preds[i],
            0 < idx < NUM_PREDS ==> self.preds[idx as int] == val,
            0 < idx < NUM_PREDS ==>
                forall|i: int| 0 <= i < NUM_PREDS && i != idx ==>
                    self.preds[i] == old(self).preds[i],
    {
        if idx != 0 && idx < NUM_PREDS {
            self.preds.set(idx, val);
        }
    }

    // -----------------------------------------------------------------------
    // Memory helpers — fully verified.
    // -----------------------------------------------------------------------

    fn load8(&self, addr: usize) -> (ret: u8)
        requires self.memory.len() == MEM_SIZE,
        ensures
            addr < MEM_SIZE  ==> ret == self.memory[addr as int],
            addr >= MEM_SIZE ==> ret == 0u8,
    {
        if addr < MEM_SIZE { self.memory[addr] } else { 0 }
    }

    fn load16(&self, addr: usize) -> (ret: u16)
        requires self.memory.len() == MEM_SIZE,
        ensures
            addr + 1 < MEM_SIZE  ==> ret == (self.memory[addr as int] as u16)
                                         | ((self.memory[addr as int + 1] as u16) << 8),
            addr + 1 >= MEM_SIZE ==> ret == 0u16,
    {
        // Use addr < MEM_SIZE - 1 to avoid usize overflow in addr + 1.
        if addr < MEM_SIZE - 1 {
            let lo = self.memory[addr] as u16;
            let hi = self.memory[addr + 1] as u16;
            lo | (hi << 8)
        } else { 0 }
    }

    fn load32(&self, addr: usize) -> (ret: u32)
        requires self.memory.len() == MEM_SIZE,
        ensures
            addr + 3 < MEM_SIZE  ==> ret == (self.memory[addr as int] as u32)
                | ((self.memory[addr as int + 1] as u32) << 8)
                | ((self.memory[addr as int + 2] as u32) << 16)
                | ((self.memory[addr as int + 3] as u32) << 24),
            addr + 3 >= MEM_SIZE ==> ret == 0u32,
    {
        if addr < MEM_SIZE - 3 {
            let b0 = self.memory[addr]     as u32;
            let b1 = self.memory[addr + 1] as u32;
            let b2 = self.memory[addr + 2] as u32;
            let b3 = self.memory[addr + 3] as u32;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
        } else { 0 }
    }

    fn load64(&self, addr: usize) -> (ret: u64)
        requires self.memory.len() == MEM_SIZE,
        ensures
            addr + 7 < MEM_SIZE  ==> ret == (self.memory[addr as int] as u64)
                | ((self.memory[addr as int + 1] as u64) << 8)
                | ((self.memory[addr as int + 2] as u64) << 16)
                | ((self.memory[addr as int + 3] as u64) << 24)
                | ((self.memory[addr as int + 4] as u64) << 32)
                | ((self.memory[addr as int + 5] as u64) << 40)
                | ((self.memory[addr as int + 6] as u64) << 48)
                | ((self.memory[addr as int + 7] as u64) << 56),
            addr + 7 >= MEM_SIZE ==> ret == 0u64,
    {
        if addr < MEM_SIZE - 7 {
            let b0 = self.memory[addr]     as u64;
            let b1 = self.memory[addr + 1] as u64;
            let b2 = self.memory[addr + 2] as u64;
            let b3 = self.memory[addr + 3] as u64;
            let b4 = self.memory[addr + 4] as u64;
            let b5 = self.memory[addr + 5] as u64;
            let b6 = self.memory[addr + 6] as u64;
            let b7 = self.memory[addr + 7] as u64;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
               | (b4 << 32) | (b5 << 40) | (b6 << 48) | (b7 << 56)
        } else { 0 }
    }

    fn store8(&mut self, addr: usize, v: u8)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.gprs       == old(self).gprs,
            self.preds      == old(self).preds,
            self.pc         == old(self).pc,
            self.cycle      == old(self).cycle,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            addr < MEM_SIZE  ==> self.memory[addr as int] == v,
            addr < MEM_SIZE  ==>
                forall|i: int| 0 <= i < MEM_SIZE && i != addr ==>
                    self.memory[i] == old(self).memory[i],
            addr >= MEM_SIZE ==> self.memory == old(self).memory,
    {
        if addr < MEM_SIZE { self.memory.set(addr, v); }
    }

    fn store16(&mut self, addr: usize, v: u16)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.gprs       == old(self).gprs,
            self.preds      == old(self).preds,
            self.pc         == old(self).pc,
            self.cycle      == old(self).cycle,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            addr + 1 < MEM_SIZE ==> self.memory[addr as int]     == (v & 0xffu16) as u8,
            addr + 1 < MEM_SIZE ==> self.memory[addr as int + 1] == (v >> 8) as u8,
            addr + 1 < MEM_SIZE ==>
                forall|i: int| 0 <= i < MEM_SIZE && i != addr && i != addr + 1 ==>
                    self.memory[i] == old(self).memory[i],
            addr + 1 >= MEM_SIZE ==> self.memory == old(self).memory,
    {
        if addr < MEM_SIZE - 1 {
            assert(v & 0xffu16 <= u8::MAX as u16) by (bit_vector);
            assert(v >> 8u16 <= u8::MAX as u16) by (bit_vector);
            self.memory.set(addr,     (v & 0xffu16) as u8);
            self.memory.set(addr + 1, (v >> 8u16) as u8);
        }
    }

    fn store32(&mut self, addr: usize, v: u32)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.gprs       == old(self).gprs,
            self.preds      == old(self).preds,
            self.pc         == old(self).pc,
            self.cycle      == old(self).cycle,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            addr + 3 < MEM_SIZE ==> self.memory[addr as int]     == (v & 0xffu32) as u8,
            addr + 3 < MEM_SIZE ==> self.memory[addr as int + 1] == ((v >>  8) & 0xffu32) as u8,
            addr + 3 < MEM_SIZE ==> self.memory[addr as int + 2] == ((v >> 16) & 0xffu32) as u8,
            addr + 3 < MEM_SIZE ==> self.memory[addr as int + 3] == (v >> 24) as u8,
            addr + 3 < MEM_SIZE ==>
                forall|i: int| 0 <= i < MEM_SIZE
                    && i != addr && i != addr+1 && i != addr+2 && i != addr+3 ==>
                    self.memory[i] == old(self).memory[i],
            addr + 3 >= MEM_SIZE ==> self.memory == old(self).memory,
    {
        if addr < MEM_SIZE - 3 {
            assert(v & 0xffu32 <= u8::MAX as u32) by (bit_vector);
            assert((v >>  8) & 0xffu32 <= u8::MAX as u32) by (bit_vector);
            assert((v >> 16) & 0xffu32 <= u8::MAX as u32) by (bit_vector);
            assert(v >> 24u32 <= u8::MAX as u32) by (bit_vector);
            let ghost m0 = self.memory@;
            self.memory.set(addr,     (v & 0xffu32) as u8);
            let ghost m1 = self.memory@;
            self.memory.set(addr + 1, ((v >>  8) & 0xffu32) as u8);
            let ghost m2 = self.memory@;
            self.memory.set(addr + 2, ((v >> 16) & 0xffu32) as u8);
            let ghost m3 = self.memory@;
            self.memory.set(addr + 3, (v >> 24u32) as u8);
            assert forall|i: int| 0 <= i < MEM_SIZE
                && i != addr && i != addr+1 && i != addr+2 && i != addr+3
            implies self.memory@[i] == m0[i] by {
                assert(self.memory@[i] == m3[i]);
                assert(m3[i] == m2[i]);
                assert(m2[i] == m1[i]);
                assert(m1[i] == m0[i]);
            };
        }
    }

    fn store64(&mut self, addr: usize, v: u64)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.gprs       == old(self).gprs,
            self.preds      == old(self).preds,
            self.pc         == old(self).pc,
            self.cycle      == old(self).cycle,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int]     == (v & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 1] == ((v >>  8) & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 2] == ((v >> 16) & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 3] == ((v >> 24) & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 4] == ((v >> 32) & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 5] == ((v >> 40) & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 6] == ((v >> 48) & 0xffu64) as u8,
            addr + 7 < MEM_SIZE ==> self.memory[addr as int + 7] == (v >> 56) as u8,
            addr + 7 < MEM_SIZE ==>
                forall|i: int| 0 <= i < MEM_SIZE
                    && i != addr   && i != addr+1 && i != addr+2 && i != addr+3
                    && i != addr+4 && i != addr+5 && i != addr+6 && i != addr+7 ==>
                    self.memory[i] == old(self).memory[i],
            addr + 7 >= MEM_SIZE ==> self.memory == old(self).memory,
    {
        if addr < MEM_SIZE - 7 {
            assert(v & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert((v >>  8) & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert((v >> 16) & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert((v >> 24) & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert((v >> 32) & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert((v >> 40) & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert((v >> 48) & 0xffu64 <= u8::MAX as u64) by (bit_vector);
            assert(v >> 56u64 <= u8::MAX as u64) by (bit_vector);
            let ghost m0 = self.memory@;
            self.memory.set(addr,     (v & 0xffu64) as u8);
            let ghost m1 = self.memory@;
            self.memory.set(addr + 1, ((v >>  8) & 0xffu64) as u8);
            let ghost m2 = self.memory@;
            self.memory.set(addr + 2, ((v >> 16) & 0xffu64) as u8);
            let ghost m3 = self.memory@;
            self.memory.set(addr + 3, ((v >> 24) & 0xffu64) as u8);
            let ghost m4 = self.memory@;
            self.memory.set(addr + 4, ((v >> 32) & 0xffu64) as u8);
            let ghost m5 = self.memory@;
            self.memory.set(addr + 5, ((v >> 40) & 0xffu64) as u8);
            let ghost m6 = self.memory@;
            self.memory.set(addr + 6, ((v >> 48) & 0xffu64) as u8);
            let ghost m7 = self.memory@;
            self.memory.set(addr + 7, (v >> 56u64) as u8);
            assert forall|i: int| 0 <= i < MEM_SIZE
                && i != addr   && i != addr+1 && i != addr+2 && i != addr+3
                && i != addr+4 && i != addr+5 && i != addr+6 && i != addr+7
            implies self.memory@[i] == m0[i] by {
                assert(self.memory@[i] == m7[i]);
                assert(m7[i] == m6[i]);
                assert(m6[i] == m5[i]);
                assert(m5[i] == m4[i]);
                assert(m4[i] == m3[i]);
                assert(m3[i] == m2[i]);
                assert(m2[i] == m1[i]);
                assert(m1[i] == m0[i]);
            };
        }
    }

    // -----------------------------------------------------------------------
    // Execution engine
    // -----------------------------------------------------------------------

    /// Record a writeback: update the destination GPR.
    fn writeback(&mut self, syl: &Syllable, val: u64, latency: u32)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.preds  == old(self).preds,
            self.memory == old(self).memory,
            self.pc     == old(self).pc,
            self.cycle  == old(self).cycle,
            self.halted == old(self).halted,
            syl.dst.is_none() ==>
                forall|i: int| 0 <= i < NUM_GPRS ==> self.gprs[i] == old(self).gprs[i],
            syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] == val,
            syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                forall|i: int| 0 <= i < NUM_GPRS && i != syl.dst.unwrap() ==>
                    self.gprs[i] == old(self).gprs[i],
    {
        if let Some(dst) = syl.dst {
            self.write_gpr(dst, val);
            if dst < NUM_GPRS {
                self.scoreboard.set(dst, ScoreboardEntry {
                    ready_cycle: self.cycle.wrapping_add(latency as u64),
                });
            }
        }
    }

    /// Execute a GPR-writing opcode whose effect is fully captured by writeback.
    fn exec_gpr_writer(&mut self, syl: &Syllable, lat: u32)
        requires
            old(self).wf(),
            spec_is_gpr_writer(syl.opcode),
        ensures
            self.wf(),
            self.cycle == old(self).cycle,
            self.preds  == old(self).preds,
            self.memory == old(self).memory,
            self.pc     == old(self).pc,
            self.halted == old(self).halted,
            syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                forall|i: int| 0 <= i < NUM_GPRS && i != syl.dst.unwrap() ==>
                    #[trigger] self.gprs[i] == old(self).gprs[i],
            syl.opcode == Opcode::Add &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_add(spec_src(old(self), syl.src[1])),
            syl.opcode == Opcode::Sub &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_sub(spec_src(old(self), syl.src[1])),
            syl.opcode == Opcode::And &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]) & spec_src(old(self), syl.src[1]),
            syl.opcode == Opcode::Or &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]) | spec_src(old(self), syl.src[1]),
            syl.opcode == Opcode::Xor &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]) ^ spec_src(old(self), syl.src[1]),
            syl.opcode == Opcode::Mov &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] == spec_src(old(self), syl.src[0]),
            syl.opcode == Opcode::MovImm &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] == syl.imm as u64,
            syl.opcode == Opcode::Mul &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_mul(spec_src(old(self), syl.src[1])),
            syl.opcode == Opcode::Lea &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_add(syl.imm as u64),
    {
        let src0 = self.read_src_gpr(syl.src[0]);
        let src1 = self.read_src_gpr(syl.src[1]);
        let imm  = syl.imm as u64;
        assert(src0 == spec_src(old(self), syl.src[0]));
        assert(src1 == spec_src(old(self), syl.src[1]));
        let sh64 = src1 & 63;
        let sh = if sh64 < 64u64 { sh64 as u32 } else { 0u32 };

        match syl.opcode {
            Opcode::Add     => self.writeback(syl, src0.wrapping_add(src1), lat),
            Opcode::Sub     => self.writeback(syl, src0.wrapping_sub(src1), lat),
            Opcode::And     => self.writeback(syl, src0 & src1, lat),
            Opcode::Or      => self.writeback(syl, src0 | src1, lat),
            Opcode::Xor     => self.writeback(syl, src0 ^ src1, lat),
            Opcode::Shl     => self.writeback(syl, src0 << sh, lat),
            Opcode::Srl     => self.writeback(syl, src0 >> sh, lat),
            Opcode::Sra     => self.writeback(syl, ((src0 as i64) >> sh) as u64, lat),
            Opcode::Mov     => self.writeback(syl, src0, lat),
            Opcode::MovImm  => self.writeback(syl, imm, lat),
            Opcode::Mul     => self.writeback(syl, src0.wrapping_mul(src1), lat),
            Opcode::MulH    => {
                let v = (src0 as u128).wrapping_mul(src1 as u128);
                self.writeback(syl, (v >> 64) as u64, lat);
            }
            Opcode::LoadD   => {
                let a = src0.wrapping_add(imm) as usize;
                let v = self.load64(a);
                self.writeback(syl, v, lat);
            }
            Opcode::LoadW   => {
                let a = src0.wrapping_add(imm) as usize;
                let v = self.load32(a);
                self.writeback(syl, v as u64, lat);
            }
            Opcode::LoadH   => {
                let a = src0.wrapping_add(imm) as usize;
                let v = self.load16(a);
                self.writeback(syl, v as u64, lat);
            }
            Opcode::LoadB   => {
                let a = src0.wrapping_add(imm) as usize;
                let v = self.load8(a);
                self.writeback(syl, v as u64, lat);
            }
            Opcode::Lea     => self.writeback(syl, src0.wrapping_add(imm), lat),
            _ => {},
        }
    }

    fn exec_compare(&mut self, syl: &Syllable)
        requires
            old(self).wf(),
            syl.opcode == Opcode::CmpEq || syl.opcode == Opcode::CmpLt || syl.opcode == Opcode::CmpUlt,
        ensures
            self.wf(),
            self.cycle      == old(self).cycle,
            self.gprs       == old(self).gprs,
            self.memory     == old(self).memory,
            self.pc         == old(self).pc,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            syl.opcode == Opcode::CmpEq &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_src(old(self), syl.src[0]) == spec_src(old(self), syl.src[1])),
            syl.opcode == Opcode::CmpLt &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    ((spec_src(old(self), syl.src[0]) as i64) <
                     (spec_src(old(self), syl.src[1]) as i64)),
            syl.opcode == Opcode::CmpUlt &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_src(old(self), syl.src[0]) < spec_src(old(self), syl.src[1])),
    {
        let src0 = self.read_src_gpr(syl.src[0]);
        let src1 = self.read_src_gpr(syl.src[1]);
        assert(src0 == spec_src(old(self), syl.src[0]));
        assert(src1 == spec_src(old(self), syl.src[1]));
        match syl.opcode {
            Opcode::CmpEq  => self.write_pred(syl.dst.unwrap_or(0), src0 == src1),
            Opcode::CmpLt  => self.write_pred(syl.dst.unwrap_or(0), (src0 as i64) < (src1 as i64)),
            Opcode::CmpUlt => self.write_pred(syl.dst.unwrap_or(0), src0 < src1),
            _ => {},
        }
    }

    fn exec_store(&mut self, syl: &Syllable)
        requires
            old(self).wf(),
            spec_is_store(syl.opcode),
        ensures
            self.wf(),
            self.cycle      == old(self).cycle,
            self.gprs       == old(self).gprs,
            self.preds      == old(self).preds,
            self.pc         == old(self).pc,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            syl.opcode == Opcode::StoreD && spec_addr(old(self), syl) + 7 < MEM_SIZE ==>
                self.memory[spec_addr(old(self), syl) as int]     == (spec_src(old(self), syl.src[1]) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 1] == ((spec_src(old(self), syl.src[1]) >>  8) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 2] == ((spec_src(old(self), syl.src[1]) >> 16) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 3] == ((spec_src(old(self), syl.src[1]) >> 24) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 4] == ((spec_src(old(self), syl.src[1]) >> 32) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 5] == ((spec_src(old(self), syl.src[1]) >> 40) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 6] == ((spec_src(old(self), syl.src[1]) >> 48) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 7] == (spec_src(old(self), syl.src[1]) >> 56) as u8,
    {
        let src0 = self.read_src_gpr(syl.src[0]);
        let src1 = self.read_src_gpr(syl.src[1]);
        let imm  = syl.imm as u64;
        assert(src0 == spec_src(old(self), syl.src[0]));
        assert(src1 == spec_src(old(self), syl.src[1]));
        let a = src0.wrapping_add(imm) as usize;
        match syl.opcode {
            Opcode::StoreD => self.store64(a, src1),
            Opcode::StoreW => self.store32(a, src1 as u32),
            Opcode::StoreH => self.store16(a, src1 as u16),
            Opcode::StoreB => self.store8(a, src1 as u8),
            _ => {},
        }
    }

    fn exec_predicate_logic(&mut self, syl: &Syllable)
        requires
            old(self).wf(),
            syl.opcode == Opcode::PAnd || syl.opcode == Opcode::POr || syl.opcode == Opcode::PXor || syl.opcode == Opcode::PNot,
        ensures
            self.wf(),
            self.cycle      == old(self).cycle,
            self.gprs       == old(self).gprs,
            self.memory     == old(self).memory,
            self.pc         == old(self).pc,
            self.halted     == old(self).halted,
            self.scoreboard == old(self).scoreboard,
            syl.opcode == Opcode::PAnd &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_pred_src(old(self), syl.src[0]) && spec_pred_src(old(self), syl.src[1])),
            syl.opcode == Opcode::POr &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_pred_src(old(self), syl.src[0]) || spec_pred_src(old(self), syl.src[1])),
            syl.opcode == Opcode::PXor &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_pred_src(old(self), syl.src[0]) ^ spec_pred_src(old(self), syl.src[1])),
            syl.opcode == Opcode::PNot &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] == !spec_pred_src(old(self), syl.src[0]),
    {
        match syl.opcode {
            Opcode::PAnd => {
                let a = self.read_src_pred(syl.src[0]);
                let b = self.read_src_pred(syl.src[1]);
                assert(a == spec_pred_src(old(self), syl.src[0]));
                assert(b == spec_pred_src(old(self), syl.src[1]));
                self.write_pred(syl.dst.unwrap_or(0), a && b);
            }
            Opcode::POr => {
                let a = self.read_src_pred(syl.src[0]);
                let b = self.read_src_pred(syl.src[1]);
                assert(a == spec_pred_src(old(self), syl.src[0]));
                assert(b == spec_pred_src(old(self), syl.src[1]));
                self.write_pred(syl.dst.unwrap_or(0), a || b);
            }
            Opcode::PXor => {
                let a = self.read_src_pred(syl.src[0]);
                let b = self.read_src_pred(syl.src[1]);
                assert(a == spec_pred_src(old(self), syl.src[0]));
                assert(b == spec_pred_src(old(self), syl.src[1]));
                self.write_pred(syl.dst.unwrap_or(0), a ^ b);
            }
            Opcode::PNot => {
                let a = self.read_src_pred(syl.src[0]);
                assert(a == spec_pred_src(old(self), syl.src[0]));
                self.write_pred(syl.dst.unwrap_or(0), !a);
            }
            _ => {},
        }
    }

    fn exec_control(&mut self, syl: &Syllable)
        requires
            old(self).wf(),
            syl.opcode == Opcode::Branch || syl.opcode == Opcode::Jump || syl.opcode == Opcode::Call || syl.opcode == Opcode::Ret,
        ensures
            self.wf(),
            self.cycle      == old(self).cycle,
            self.memory     == old(self).memory,
            self.preds      == old(self).preds,
            self.scoreboard == old(self).scoreboard,
            syl.opcode == Opcode::Branch ==>
                self.gprs   == old(self).gprs &&
                self.halted == old(self).halted,
            syl.opcode == Opcode::Branch &&
                spec_pred(old(self), syl.predicate) != syl.pred_negated ==>
                self.pc == syl.imm as usize,
            syl.opcode == Opcode::Branch &&
                spec_pred(old(self), syl.predicate) == syl.pred_negated ==>
                self.pc == old(self).pc,
            syl.opcode == Opcode::Jump ==>
                self.pc     == syl.imm as usize &&
                self.gprs   == old(self).gprs &&
                self.halted == old(self).halted,
            syl.opcode == Opcode::Call ==>
                self.pc     == syl.imm as usize &&
                self.gprs[31int] == old(self).pc as u64 &&
                self.halted == old(self).halted,
            syl.opcode == Opcode::Ret ==>
                self.gprs == old(self).gprs,
            syl.opcode == Opcode::Ret && old(self).gprs[31int] == 0u64 ==>
                self.halted,
            syl.opcode == Opcode::Ret && old(self).gprs[31int] != 0u64 ==>
                self.pc == old(self).gprs[31int] as usize &&
                !self.halted,
    {
        match syl.opcode {
            Opcode::Branch => {
                if self.read_pred(syl.predicate) != syl.pred_negated {
                    self.pc = syl.imm as usize;
                }
            }
            Opcode::Jump => {
                self.pc = syl.imm as usize;
            }
            Opcode::Call => {
                let rpc = self.pc;
                self.write_gpr(31, rpc as u64);
                self.pc = syl.imm as usize;
            }
            Opcode::Ret => {
                let t = self.read_gpr(31);
                assert(t == old(self).gprs[31int]);
                if t == 0u64 {
                    self.halted = true;
                } else {
                    self.pc = t as usize;
                    self.halted = false;
                }
            }
            _ => {},
        }
    }

    /// Execute one syllable.
    fn execute_syllable(&mut self, syl: &Syllable)
        requires old(self).wf(),
        ensures
            self.wf(),
            self.cycle == old(self).cycle,

            // ── Inactive: full state freeze ───────────────────────────────
            !spec_syl_active(old(self), syl) ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            // ── Nop / Prefetch: no-ops ────────────────────────────────────
            spec_syl_active(old(self), syl) &&
                (syl.opcode == Opcode::Nop || syl.opcode == Opcode::Prefetch) ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            // ── Active GPR-writing ops: frame preservation ─────────────────
            // Preds, memory, pc, and halted are unchanged.
            spec_syl_active(old(self), syl) && spec_is_gpr_writer(syl.opcode) ==>
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            // Non-destination GPRs are unchanged.
            spec_syl_active(old(self), syl) && spec_is_gpr_writer(syl.opcode) &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                forall|i: int| 0 <= i < NUM_GPRS && i != syl.dst.unwrap() ==>
                    #[trigger] self.gprs[i] == old(self).gprs[i],

            // ── Per-opcode result values ───────────────────────────────────
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Add &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_add(
                    spec_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Sub &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_sub(
                    spec_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::And &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]) & spec_src(old(self), syl.src[1]),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Or &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]) | spec_src(old(self), syl.src[1]),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Xor &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]) ^ spec_src(old(self), syl.src[1]),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Mov &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] == spec_src(old(self), syl.src[0]),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::MovImm &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] == syl.imm as u64,

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Mul &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_mul(
                    spec_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Lea &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                self.gprs[syl.dst.unwrap() as int] ==
                    spec_src(old(self), syl.src[0]).wrapping_add(syl.imm as u64),

            // ── Compare ops (write predicates, not GPRs) ───────────────────
            spec_syl_active(old(self), syl) &&
                (syl.opcode == Opcode::CmpEq || syl.opcode == Opcode::CmpLt ||
                 syl.opcode == Opcode::CmpUlt) ==>
                self.gprs   == old(self).gprs &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::CmpEq &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_src(old(self), syl.src[0]) == spec_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::CmpLt &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    ((spec_src(old(self), syl.src[0]) as i64) <
                     (spec_src(old(self), syl.src[1]) as i64)),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::CmpUlt &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_src(old(self), syl.src[0]) < spec_src(old(self), syl.src[1])),

            // ── Store ops: GPRs and preds unchanged, PC/halted unchanged ───
            spec_syl_active(old(self), syl) && spec_is_store(syl.opcode) ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            // StoreD: 8 bytes written at addr = src0 + imm
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::StoreD &&
                spec_addr(old(self), syl) + 7 < MEM_SIZE ==>
                self.memory[spec_addr(old(self), syl) as int]     == (spec_src(old(self), syl.src[1]) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 1] == ((spec_src(old(self), syl.src[1]) >>  8) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 2] == ((spec_src(old(self), syl.src[1]) >> 16) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 3] == ((spec_src(old(self), syl.src[1]) >> 24) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 4] == ((spec_src(old(self), syl.src[1]) >> 32) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 5] == ((spec_src(old(self), syl.src[1]) >> 40) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 6] == ((spec_src(old(self), syl.src[1]) >> 48) & 0xffu64) as u8 &&
                self.memory[spec_addr(old(self), syl) as int + 7] == (spec_src(old(self), syl.src[1]) >> 56) as u8,

            // ── Predicate logic ops ────────────────────────────────────────
            spec_syl_active(old(self), syl) &&
                (syl.opcode == Opcode::PAnd || syl.opcode == Opcode::POr ||
                 syl.opcode == Opcode::PXor || syl.opcode == Opcode::PNot) ==>
                self.gprs   == old(self).gprs &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::PAnd &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_pred_src(old(self), syl.src[0]) && spec_pred_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::POr &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_pred_src(old(self), syl.src[0]) || spec_pred_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::PXor &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] ==
                    (spec_pred_src(old(self), syl.src[0]) ^ spec_pred_src(old(self), syl.src[1])),

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::PNot &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_PREDS ==>
                self.preds[syl.dst.unwrap() as int] == !spec_pred_src(old(self), syl.src[0]),

            // ── Branch ────────────────────────────────────────────────────
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Branch ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.halted == old(self).halted,
            // Branch taken when predicate condition holds (re-evaluated).
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Branch &&
                spec_pred(old(self), syl.predicate) != syl.pred_negated ==>
                self.pc == syl.imm as usize,
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Branch &&
                spec_pred(old(self), syl.predicate) == syl.pred_negated ==>
                self.pc == old(self).pc,

            // ── Jump ──────────────────────────────────────────────────────
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Jump ==>
                self.pc     == syl.imm as usize &&
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.halted == old(self).halted,

            // ── Call ──────────────────────────────────────────────────────
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Call ==>
                self.pc     == syl.imm as usize &&
                self.gprs[31int] == old(self).pc as u64 &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.halted == old(self).halted,

            // ── Ret ───────────────────────────────────────────────────────
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Ret ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory,
            // lr == 0 → halt
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Ret &&
                old(self).gprs[31int] == 0u64 ==>
                self.halted,
            // lr != 0 → jump to lr
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Ret &&
                old(self).gprs[31int] != 0u64 ==>
                self.pc == old(self).gprs[31int] as usize &&
                !self.halted,
    {
        let pred_val = self.read_pred(syl.predicate);
        let active = if syl.pred_negated { !pred_val } else { pred_val };
        if !active { return; }

        let lat  = self.latencies.get(syl.opcode);

        match syl.opcode {
            Opcode::Nop     => {}
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor |
            Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm |
            Opcode::Mul | Opcode::MulH | Opcode::LoadD | Opcode::LoadW |
            Opcode::LoadH | Opcode::LoadB | Opcode::Lea => {
                self.exec_gpr_writer(syl, lat);
            }
            Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt => {
                self.exec_compare(syl);
            }
            Opcode::StoreD | Opcode::StoreW | Opcode::StoreH | Opcode::StoreB => {
                self.exec_store(syl);
            }
            Opcode::Prefetch => {}
            Opcode::Branch | Opcode::Jump | Opcode::Call | Opcode::Ret => self.exec_control(syl),
            Opcode::PAnd | Opcode::POr | Opcode::PXor | Opcode::PNot => self.exec_predicate_logic(syl),
        }
    }

    /// Advance by one bundle.
    ///
    /// Postconditions:
    ///   - returns false iff already halted or pc was out of range
    ///   - on true: cycle increased by 1, pc advanced past the bundle
    ///   - wf() is preserved throughout
    pub fn step(&mut self, program: &Vec<Bundle<W>>) -> (ret: bool)
        requires
            old(self).wf(),
            old(self).cycle < u64::MAX,
        ensures
            self.wf(),
            !ret ==> self.halted || old(self).pc >= program.len(),
            ret  ==> old(self).cycle + 1 == self.cycle || self.halted,
    {
        if self.halted || self.pc >= program.len() {
            return false;
        }
        let bundle = &program[self.pc];
        self.pc    = self.pc + 1;
        self.cycle = self.cycle + 1;

        let mut slot = 0usize;
        while slot < bundle.syllables.len()
            invariant
                self.wf(),
                self.cycle == old(self).cycle + 1,
            decreases bundle.syllables.len() - slot,
        {
            let syl = &bundle.syllables[slot];
            self.execute_syllable(syl);
            if self.halted { break; }
            slot = slot + 1;
        }
        true
    }
}

/// Pretty-print the processor state.
#[verifier::external]
pub fn print_cpu_state<const W: usize>(state: &CpuState<W>) {
    println!("=== LWIR Processor State (width={W}) ===");
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
