verus! {

// ---------------------------------------------------------------------------
// Well-formedness predicate and basic accessors
// ---------------------------------------------------------------------------

impl CpuState {
    /// The processor state is well-formed when all register files and memory
    /// have the expected sizes, and the hardwired values (r0=0, p0=true) hold.
    pub open spec fn wf(&self) -> bool {
        &&& self.gprs.len()       == self.num_gprs
        &&& self.preds.len()      == self.num_preds
        &&& self.scoreboard.len() == self.num_gprs
        &&& self.memory.len()     == self.mem_size
        &&& self.num_gprs >= 32
        &&& self.num_preds >= 1
        &&& self.mem_size >= 8
        &&& crate::bundle::is_valid_width(self.width)
        &&& self.gprs[0int]       == 0u64
        &&& self.preds[0int]      == true
    }

    /// Create a reset CPU.
    pub fn new(width: usize, latencies: LatencyTable) -> (ret: Self)
        requires crate::bundle::is_valid_width(width),
    {
        Self::new_configured(width, NUM_GPRS, NUM_PREDS, MEM_SIZE, latencies)
    }

    pub fn new_for_layout(layout: &ProcessorLayout, latencies: LatencyTable) -> (ret: Self)
        requires
            crate::bundle::is_valid_width(layout.width),
            crate::layout::arch_supported(layout),
    {
        let mut cpu = Self::new_configured(
            layout.width,
            layout.arch.gprs,
            layout.arch.preds,
            layout.arch.memory_bytes,
            latencies,
        );
        cpu.cache = CacheState::new(layout.cache);
        cpu
    }

    /// Create a reset CPU with explicit architectural resource sizes.
    pub fn new_configured(
        width: usize,
        num_gprs: usize,
        num_preds: usize,
        mem_size: usize,
        latencies: LatencyTable,
    ) -> (ret: Self)
        requires
            crate::bundle::is_valid_width(width),
            num_gprs >= 32,
            num_preds >= 1,
            mem_size >= 8,
        ensures
            ret.wf(),
            ret.width == width,
            ret.num_gprs == num_gprs,
            ret.num_preds == num_preds,
            ret.mem_size == mem_size,
            ret.pc    == 0,
            ret.cycle == 0,
            !ret.halted,
            forall|i: int| 0 <= i < num_gprs  ==> ret.gprs[i] == 0u64,
            forall|i: int| 0 <= i < num_preds ==> ret.preds[i] == (i == 0),
            forall|i: int| 0 <= i < mem_size  ==> ret.memory[i] == 0u8,
            forall|i: int| 0 <= i < num_gprs  ==> ret.scoreboard[i].ready_cycle == 0u64,
    {
        let mut gprs: Vec<u64> = Vec::new();
        let mut scoreboard: Vec<ScoreboardEntry> = Vec::new();
        let mut i = 0usize;
        while i < num_gprs
            invariant
                i <= num_gprs,
                gprs.len() == i,
                scoreboard.len() == i,
                forall|j: int| 0 <= j < i ==> gprs[j] == 0u64,
                forall|j: int| 0 <= j < i ==> scoreboard[j].ready_cycle == 0u64,
            decreases num_gprs - i,
        {
            gprs.push(0u64);
            scoreboard.push(ScoreboardEntry { ready_cycle: 0 });
            i += 1;
        }

        let mut preds = Vec::new();
        let mut j = 0usize;
        while j < num_preds
            invariant
                j <= num_preds,
                preds.len() == j,
                forall|k: int| 0 <= k < j ==> preds[k] == (k == 0),
            decreases num_preds - j,
        {
            preds.push(j == 0);
            j += 1;
        }

        let mut memory = Vec::new();
        let mut k = 0usize;
        while k < mem_size
            invariant
                k <= mem_size,
                memory.len() == k,
                forall|m: int| 0 <= m < k ==> memory[m] == 0u8,
            decreases mem_size - k,
        {
            memory.push(0u8);
            k += 1;
        }

        CpuState {
            width,
            num_gprs,
            num_preds,
            mem_size,
            gprs,
            preds,
            pc: 0,
            cycle: 0,
            scoreboard,
            memory,
            cache: CacheState::new(crate::cache::CacheConfig::default_l1d()),
            halted: false,
            latencies,
        }
    }

    /// Read GPR at `idx`.
    pub fn read_gpr(&self, idx: usize) -> (ret: u64)
        requires self.wf(),
        ensures
            idx == 0 || idx >= self.num_gprs ==> ret == 0u64,
            0 < idx < self.num_gprs          ==> ret == self.gprs[idx as int],
    {
        if idx == 0 || idx >= self.num_gprs { 0u64 } else { self.gprs[idx] }
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
            self.width      == old(self).width,
            self.num_gprs   == old(self).num_gprs,
            self.num_preds  == old(self).num_preds,
            self.mem_size   == old(self).mem_size,
            idx == 0 || idx >= old(self).num_gprs ==>
                forall|i: int| 0 <= i < old(self).num_gprs ==> self.gprs[i] == old(self).gprs[i],
            0 < idx < old(self).num_gprs ==> self.gprs[idx as int] == val,
            0 < idx < old(self).num_gprs ==>
                forall|i: int| 0 <= i < old(self).num_gprs && i != idx ==>
                    self.gprs[i] == old(self).gprs[i],
    {
        if idx != 0 && idx < self.num_gprs {
            self.gprs.set(idx, val);
        }
    }

    /// Read predicate register at `idx`.
    pub fn read_pred(&self, idx: usize) -> (ret: bool)
        requires self.wf(),
        ensures
            idx == 0              ==> ret == true,
            idx >= self.num_preds      ==> ret == false,
            0 < idx < self.num_preds   ==> ret == self.preds[idx as int],
    {
        if idx == 0 { true } else if idx < self.num_preds { self.preds[idx] } else { false }
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
            self.width      == old(self).width,
            self.num_gprs   == old(self).num_gprs,
            self.num_preds  == old(self).num_preds,
            self.mem_size   == old(self).mem_size,
            idx == 0 || idx >= old(self).num_preds ==>
                forall|i: int| 0 <= i < old(self).num_preds ==> self.preds[i] == old(self).preds[i],
            0 < idx < old(self).num_preds ==> self.preds[idx as int] == val,
            0 < idx < old(self).num_preds ==>
                forall|i: int| 0 <= i < old(self).num_preds && i != idx ==>
                    self.preds[i] == old(self).preds[i],
    {
        if idx != 0 && idx < self.num_preds {
            self.preds.set(idx, val);
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
        op == Opcode::LoadW || op == Opcode::LoadD ||
        op == Opcode::FpAdd32 || op == Opcode::FpMul32 ||
        op == Opcode::FpAdd64 || op == Opcode::FpMul64 ||
        op == Opcode::AesEnc || op == Opcode::AesDec
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
