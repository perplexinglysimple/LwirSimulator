verus! {

// ---------------------------------------------------------------------------
// Bundle legality and stall checks
// ---------------------------------------------------------------------------

impl CpuState {
    /// Runtime legality check for one bundle in the current architectural state.
    ///
    /// Invalid bundles are rejected before execution so compiler bugs are not
    /// silently accepted by the simulator.
    fn bundle_is_legal(&self, layout: &ProcessorLayout, bundle: &Bundle) -> (ret: bool)
        requires self.wf(),
    {
        let mut slot = 0usize;
        while slot < bundle.syllables.len()
            invariant
                self.wf(),
                slot <= bundle.syllables.len(),
            decreases bundle.syllables.len() - slot,
        {
            let syl = &bundle.syllables[slot];
            let active = self.syl_is_active_runtime(syl);
            if active && !layout.slot_can_execute(slot, syl.opcode) {
                return false;
            }

            let mut later = slot + 1;
            while later < bundle.syllables.len()
                invariant
                    self.wf(),
                    slot < bundle.syllables.len(),
                    later <= bundle.syllables.len(),
                decreases bundle.syllables.len() - later,
            {
                let earlier = &bundle.syllables[slot];
                let later_syl = &bundle.syllables[later];
                let earlier_active = self.syl_is_active_runtime(earlier);
                let later_active = self.syl_is_active_runtime(later_syl);

                if earlier_active && later_active {
                    if let Some(dst) = Self::opcode_gpr_write_dst(earlier.opcode, earlier.dst) {
                        if dst > 0 && dst < self.num_gprs {
                            if later_syl.src[0] == Some(dst) || later_syl.src[1] == Some(dst) {
                                return false;
                            }
                            if let Some(later_dst) =
                                Self::opcode_gpr_write_dst(later_syl.opcode, later_syl.dst)
                            {
                                if later_dst == dst {
                                    return false;
                                }
                            }
                            if later_syl.opcode == Opcode::Ret && dst == 31 {
                                return false;
                            }
                        }
                    }

                    if Self::opcode_writes_pred(earlier.opcode) {
                        if let Some(dst) = earlier.dst {
                            if dst > 0 && dst < self.num_preds {
                                if Self::opcode_reads_pred(later_syl.opcode)
                                    && (later_syl.src[0] == Some(dst)
                                        || later_syl.src[1] == Some(dst)
                                        || (later_syl.opcode == Opcode::Branch && later_syl.predicate == dst))
                                {
                                    return false;
                                }
                                if Self::opcode_writes_pred(later_syl.opcode) && later_syl.dst == Some(dst) {
                                    return false;
                                }
                            }
                        }
                    }
                }

                later += 1;
            }

            slot += 1;
        }

        true
    }

    /// Does this bundle have an active GPR read whose producer is not ready
    /// by the next cycle boundary?
    fn bundle_has_unready_gpr_sources(&self, bundle: &Bundle) -> (ret: bool)
        requires
            self.wf(),
            self.cycle < u64::MAX,
    {
        let next_cycle = self.cycle + 1;
        let mut slot = 0usize;
        while slot < bundle.syllables.len()
            invariant
                self.wf(),
                self.cycle < u64::MAX,
                slot <= bundle.syllables.len(),
            decreases bundle.syllables.len() - slot,
        {
            let syl = &bundle.syllables[slot];
            if self.syl_is_active_runtime(syl) {
                let mut src_idx = 0usize;
                while src_idx < 2
                    invariant
                        self.wf(),
                        self.cycle < u64::MAX,
                        slot < bundle.syllables.len(),
                        src_idx <= 2,
                    decreases 2 - src_idx,
                {
                    if let Some(src) = syl.src[src_idx] {
                        if src > 0 && src < self.num_gprs
                            && self.scoreboard[src].ready_cycle > next_cycle
                        {
                            return true;
                        }
                    }
                    src_idx += 1;
                }

                if syl.opcode == Opcode::Ret
                    && self.scoreboard[31].ready_cycle > next_cycle
                {
                    return true;
                }
            }
            slot += 1;
        }
        false
    }
}

} // verus!
