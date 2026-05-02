verus! {

// ---------------------------------------------------------------------------
// Well-formedness predicate and basic accessors
// ---------------------------------------------------------------------------

impl CpuState {
    /// The processor state is well-formed when all register files and memory
    /// have the expected sizes, and the hardwired values (r0=0, p0=true) hold.
    pub open spec fn wf(&self) -> bool {
        &&& self.gprs.len()       == NUM_GPRS
        &&& self.preds.len()      == NUM_PREDS
        &&& self.scoreboard.len() == NUM_GPRS
        &&& self.memory.len()     == MEM_SIZE
        &&& crate::bundle::is_valid_width(self.width)
        &&& self.gprs[0int]       == 0u64
        &&& self.preds[0int]      == true
    }

    /// Create a reset CPU.
    pub fn new(width: usize, latencies: LatencyTable) -> (ret: Self)
        requires crate::bundle::is_valid_width(width),
        ensures
            ret.wf(),
            ret.width == width,
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

        CpuState { width, gprs, preds, pc: 0, cycle: 0, scoreboard, memory, halted: false, latencies }
    }

    /// Read GPR at `idx`.
    pub fn read_gpr(&self, idx: usize) -> (ret: u64)
        requires self.wf(),
        ensures
            idx == 0 || idx >= NUM_GPRS ==> ret == 0u64,
            0 < idx < NUM_GPRS          ==> ret == self.gprs[idx as int],
    {
        if idx == 0 || idx >= NUM_GPRS { 0u64 } else { self.gprs[idx] }
    }

    /// Write GPR at `idx` with `val`.
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

    /// Expected slot class for a slot index in a bundle.
    fn slot_class_for_index(slot: usize) -> (ret: SlotClass)
        ensures
            ret == spec_slot_class_for_index(slot as int),
    {
        match slot % 4 {
            0 | 1 => SlotClass::Integer,
            2 => SlotClass::Memory,
            _ => SlotClass::Control,
        }
    }

    fn opcode_writes_pred(op: Opcode) -> (ret: bool)
        ensures ret == spec_opcode_writes_pred(op),
    {
        op == Opcode::CmpEq || op == Opcode::CmpLt || op == Opcode::CmpUlt
            || op == Opcode::PAnd || op == Opcode::POr || op == Opcode::PXor || op == Opcode::PNot
    }

    fn opcode_writes_gpr(op: Opcode) -> (ret: bool)
        ensures ret == spec_is_gpr_writer(op),
    {
        op == Opcode::Add  || op == Opcode::Sub  || op == Opcode::And ||
        op == Opcode::Or   || op == Opcode::Xor  || op == Opcode::Shl ||
        op == Opcode::Srl  || op == Opcode::Sra  || op == Opcode::Mov ||
        op == Opcode::MovImm || op == Opcode::Mul || op == Opcode::MulH ||
        op == Opcode::Lea  || op == Opcode::LoadB || op == Opcode::LoadH ||
        op == Opcode::LoadW || op == Opcode::LoadD
    }

    fn opcode_gpr_write_dst(op: Opcode, dst: Option<usize>) -> (ret: Option<usize>)
        ensures ret == spec_gpr_write_dst(op, dst),
    {
        if op == Opcode::Call {
            Some(31)
        } else if Self::opcode_writes_gpr(op) {
            dst
        } else {
            None
        }
    }

    fn opcode_reads_pred(op: Opcode) -> (ret: bool)
        ensures ret == (spec_opcode_reads_pred_src(op) || op == Opcode::Branch),
    {
        op == Opcode::Branch || op == Opcode::PAnd || op == Opcode::POr
            || op == Opcode::PXor || op == Opcode::PNot
    }

    fn syl_is_active_runtime(&self, syl: &Syllable) -> (ret: bool)
        requires self.wf(),
        ensures ret == spec_syl_active(self, syl),
    {
        let pred_val = self.read_pred(syl.predicate);
        if syl.pred_negated { !pred_val } else { pred_val }
    }
}

} // verus!
