verus! {

// ---------------------------------------------------------------------------
// Execution engine
// ---------------------------------------------------------------------------

impl<const W: usize> CpuState<W> {
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

            !spec_syl_active(old(self), syl) ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) &&
                (syl.opcode == Opcode::Nop || syl.opcode == Opcode::Prefetch) ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) && spec_is_gpr_writer(syl.opcode) ==>
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) && spec_is_gpr_writer(syl.opcode) &&
                syl.dst.is_some() && syl.dst.unwrap() > 0 && syl.dst.unwrap() < NUM_GPRS ==>
                forall|i: int| 0 <= i < NUM_GPRS && i != syl.dst.unwrap() ==>
                    #[trigger] self.gprs[i] == old(self).gprs[i],

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

            spec_syl_active(old(self), syl) && spec_is_store(syl.opcode) ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.pc     == old(self).pc &&
                self.halted == old(self).halted,

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

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Branch ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.halted == old(self).halted,
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Branch &&
                spec_pred(old(self), syl.predicate) != syl.pred_negated ==>
                self.pc == syl.imm as usize,
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Branch &&
                spec_pred(old(self), syl.predicate) == syl.pred_negated ==>
                self.pc == old(self).pc,

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Jump ==>
                self.pc     == syl.imm as usize &&
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Call ==>
                self.pc     == syl.imm as usize &&
                self.gprs[31int] == old(self).pc as u64 &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory &&
                self.halted == old(self).halted,

            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Ret ==>
                self.gprs   == old(self).gprs &&
                self.preds  == old(self).preds &&
                self.memory == old(self).memory,
            spec_syl_active(old(self), syl) && syl.opcode == Opcode::Ret &&
                old(self).gprs[31int] == 0u64 ==>
                self.halted,
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
        if !self.bundle_is_legal(bundle) {
            self.halted = true;
            return false;
        }
        if self.bundle_has_unready_gpr_sources(bundle) {
            self.cycle = self.cycle + 1;
            return true;
        }
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

} // verus!
