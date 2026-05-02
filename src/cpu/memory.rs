verus! {

// ---------------------------------------------------------------------------
// Memory helpers — fully verified.
// ---------------------------------------------------------------------------

impl CpuState {
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
}

} // verus!
