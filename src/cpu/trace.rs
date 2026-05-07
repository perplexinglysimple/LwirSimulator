use std::fmt;

/// A deterministic execution trace for one program run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceLog {
    pub width: usize,
    pub events: Vec<TraceEvent>,
    pub final_pc: usize,
    pub final_cycle: u64,
    pub final_halted: bool,
}

/// One attempted bundle issue in the execution trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub kind: TraceEventKind,
    pub bundle_index: usize,
    pub cycle: u64,
    pub active_syllables: Vec<TraceActiveSyllable>,
    pub stalls: Vec<TraceStall>,
    pub gpr_writes: Vec<TraceGprWrite>,
    pub pred_writes: Vec<TracePredWrite>,
    pub memory_effects: Vec<TraceMemoryEffect>,
    pub control_flow: Vec<TraceControlFlow>,
    pub pc_after: usize,
    pub cycle_after: u64,
    pub halted_after: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceEventKind {
    Issue,
    Stall,
    IllegalBundle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceActiveSyllable {
    pub slot: usize,
    pub opcode: Opcode,
    pub predicate: usize,
    pub pred_negated: bool,
    pub dst: Option<usize>,
    pub src: [Option<usize>; 2],
    pub imm: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceStall {
    pub slot: usize,
    pub register: usize,
    pub ready_cycle: u64,
    pub needed_cycle: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceGprWrite {
    pub slot: usize,
    pub register: usize,
    pub value: u64,
    pub ready_cycle: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TracePredWrite {
    pub slot: usize,
    pub predicate: usize,
    pub value: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceMemoryEffect {
    Load {
        slot: usize,
        width_bytes: usize,
        address: usize,
        value: u64,
        in_bounds: bool,
        cache_outcome: CacheOutcome,
    },
    Store {
        slot: usize,
        width_bytes: usize,
        address: usize,
        value: u64,
        in_bounds: bool,
        cache_outcome: CacheOutcome,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceControlFlow {
    Branch {
        slot: usize,
        predicate: usize,
        pred_negated: bool,
        taken: bool,
        target: usize,
        fallthrough: usize,
    },
    Jump {
        slot: usize,
        target: usize,
    },
    Call {
        slot: usize,
        target: usize,
        return_pc: usize,
    },
    Ret {
        slot: usize,
        target: Option<usize>,
        halted: bool,
    },
}

impl CpuState {
    /// Run until the program halts or PC leaves the program, returning a stable trace.
    pub fn trace_program(&mut self, layout: &ProcessorLayout, program: &[Bundle]) -> TraceLog {
        let mut events = Vec::new();
        while let Some(event) = self.step_trace(layout, program) {
            events.push(event);
        }

        TraceLog {
            width: self.width,
            events,
            final_pc: self.pc,
            final_cycle: self.cycle,
            final_halted: self.halted,
        }
    }

    /// Execute one traced bundle attempt. Returns `None` when no bundle can issue.
    pub fn step_trace(&mut self, layout: &ProcessorLayout, program: &[Bundle]) -> Option<TraceEvent> {
        if self.halted || self.pc >= program.len() {
            return None;
        }

        let bundle_index = self.pc;
        let cycle = self.cycle;
        let bundle = &program[self.pc];

        if !self.bundle_is_legal(layout, bundle) {
            self.halted = true;
            return Some(TraceEvent {
                kind: TraceEventKind::IllegalBundle,
                bundle_index,
                cycle,
                active_syllables: self.collect_active_syllables(bundle),
                stalls: Vec::new(),
                gpr_writes: Vec::new(),
                pred_writes: Vec::new(),
                memory_effects: Vec::new(),
                control_flow: Vec::new(),
                pc_after: self.pc,
                cycle_after: self.cycle,
                halted_after: self.halted,
            });
        }

        if self.bundle_has_unready_gpr_sources(bundle) {
            let stalls = self.collect_trace_stalls(bundle);
            self.cycle = self.cycle.wrapping_add(1);
            return Some(TraceEvent {
                kind: TraceEventKind::Stall,
                bundle_index,
                cycle,
                active_syllables: self.collect_active_syllables(bundle),
                stalls,
                gpr_writes: Vec::new(),
                pred_writes: Vec::new(),
                memory_effects: Vec::new(),
                control_flow: Vec::new(),
                pc_after: self.pc,
                cycle_after: self.cycle,
                halted_after: self.halted,
            });
        }

        self.pc = self.pc + 1;
        self.cycle = self.cycle + 1;

        let mut active_syllables = Vec::new();
        let mut gpr_writes = Vec::new();
        let mut pred_writes = Vec::new();
        let mut memory_effects = Vec::new();
        let mut control_flow = Vec::new();

        for (slot, syl) in bundle.syllables.iter().enumerate() {
            if syl.opcode == Opcode::Nop || !self.syl_is_active_runtime(syl) {
                continue;
            }

            active_syllables.push(trace_active_syllable(slot, syl));
            if let Some(effect) = self.trace_memory_effect(slot, syl) {
                memory_effects.push(effect);
            }
            if let Some(decision) = self.trace_control_flow(slot, syl) {
                control_flow.push(decision);
            }

            self.execute_syllable(syl);

            if let Some(register) = gpr_write_dst_for_trace(syl) {
                if register > 0 && register < self.num_gprs {
                    gpr_writes.push(TraceGprWrite {
                        slot,
                        register,
                        value: self.read_gpr(register),
                        ready_cycle: self.scoreboard[register].ready_cycle,
                    });
                }
            }

            if syl.opcode.writes_pred() {
                if let Some(predicate) = syl.dst {
                    if predicate > 0 && predicate < self.num_preds {
                        pred_writes.push(TracePredWrite {
                            slot,
                            predicate,
                            value: self.read_pred(predicate),
                        });
                    }
                }
            }

            if self.halted {
                break;
            }
        }

        Some(TraceEvent {
            kind: TraceEventKind::Issue,
            bundle_index,
            cycle,
            active_syllables,
            stalls: Vec::new(),
            gpr_writes,
            pred_writes,
            memory_effects,
            control_flow,
            pc_after: self.pc,
            cycle_after: self.cycle,
            halted_after: self.halted,
        })
    }

    fn collect_active_syllables(&self, bundle: &Bundle) -> Vec<TraceActiveSyllable> {
        let mut active = Vec::new();
        for (slot, syl) in bundle.syllables.iter().enumerate() {
            if syl.opcode != Opcode::Nop && self.syl_is_active_runtime(syl) {
                active.push(trace_active_syllable(slot, syl));
            }
        }
        active
    }

    fn collect_trace_stalls(&self, bundle: &Bundle) -> Vec<TraceStall> {
        let needed_cycle = self.cycle + 1;
        let mut stalls = Vec::new();

        for (slot, syl) in bundle.syllables.iter().enumerate() {
            if !self.syl_is_active_runtime(syl) {
                continue;
            }

            for src in syl.src {
                if let Some(register) = src {
                    if register > 0
                        && register < self.num_gprs
                        && self.scoreboard[register].ready_cycle > needed_cycle
                    {
                        stalls.push(TraceStall {
                            slot,
                            register,
                            ready_cycle: self.scoreboard[register].ready_cycle,
                            needed_cycle,
                        });
                    }
                }
            }

            if syl.opcode == Opcode::Ret && self.scoreboard[31].ready_cycle > needed_cycle {
                stalls.push(TraceStall {
                    slot,
                    register: 31,
                    ready_cycle: self.scoreboard[31].ready_cycle,
                    needed_cycle,
                });
            }
        }

        stalls
    }

    fn trace_memory_effect(&self, slot: usize, syl: &Syllable) -> Option<TraceMemoryEffect> {
        let width_bytes = memory_width_bytes(syl.opcode)?;
        let base = self.read_src_gpr(syl.src[0]);
        let address = base.wrapping_add(syl.imm as u64) as usize;
        let in_bounds = memory_access_in_bounds(self.mem_size, address, width_bytes);

        if is_load_opcode(syl.opcode) {
            if !in_bounds {
                panic!("{}", memory_bounds_error("load", address, width_bytes, self.mem_size));
            }
            let value = match width_bytes {
                1 => self.load8(address) as u64,
                2 => self.load16(address) as u64,
                4 => self.load32(address) as u64,
                8 => self.load64(address),
                _ => 0,
            };
            Some(TraceMemoryEffect::Load {
                slot,
                width_bytes,
                address,
                value,
                in_bounds,
                cache_outcome: self.cache.peek_outcome(address),
            })
        } else {
            if !in_bounds {
                panic!("{}", memory_bounds_error("store", address, width_bytes, self.mem_size));
            }
            let raw = self.read_src_gpr(syl.src[1]);
            let value = mask_to_width(raw, width_bytes);
            Some(TraceMemoryEffect::Store {
                slot,
                width_bytes,
                address,
                value,
                in_bounds,
                cache_outcome: self.cache.peek_outcome(address),
            })
        }
    }

    fn trace_control_flow(&self, slot: usize, syl: &Syllable) -> Option<TraceControlFlow> {
        match syl.opcode {
            Opcode::Branch => {
                let pred = self.read_pred(syl.predicate);
                Some(TraceControlFlow::Branch {
                    slot,
                    predicate: syl.predicate,
                    pred_negated: syl.pred_negated,
                    taken: pred != syl.pred_negated,
                    target: syl.imm as usize,
                    fallthrough: self.pc,
                })
            }
            Opcode::Jump => Some(TraceControlFlow::Jump {
                slot,
                target: syl.imm as usize,
            }),
            Opcode::Call => Some(TraceControlFlow::Call {
                slot,
                target: syl.imm as usize,
                return_pc: self.pc,
            }),
            Opcode::Ret => {
                let target = self.read_gpr(31);
                Some(TraceControlFlow::Ret {
                    slot,
                    target: if target == 0 {
                        None
                    } else {
                        Some(target as usize)
                    },
                    halted: target == 0,
                })
            }
            _ => None,
        }
    }
}

impl fmt::Display for TraceLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "trace v1 width={}", self.width)?;
        for event in &self.events {
            writeln!(
                f,
                "event kind={} bundle={} cycle={} pc_after={} cycle_after={} halted={}",
                event.kind.as_str(),
                event.bundle_index,
                event.cycle,
                event.pc_after,
                event.cycle_after,
                event.halted_after
            )?;
            for active in &event.active_syllables {
                writeln!(
                    f,
                    "  active slot={} op={} guard={} dst={} src0={} src1={} imm={}",
                    active.slot,
                    opcode_mnemonic(active.opcode),
                    format_guard(active.predicate, active.pred_negated),
                    format_active_dst(active),
                    format_active_src(active, 0),
                    format_active_src(active, 1),
                    active.imm
                )?;
            }
            for stall in &event.stalls {
                writeln!(
                    f,
                    "  stall slot={} reg=r{} ready={} needed={}",
                    stall.slot, stall.register, stall.ready_cycle, stall.needed_cycle
                )?;
            }
            for write in &event.gpr_writes {
                writeln!(
                    f,
                    "  gpr slot={} reg=r{} value={:#018x} ready={}",
                    write.slot, write.register, write.value, write.ready_cycle
                )?;
            }
            for write in &event.pred_writes {
                writeln!(
                    f,
                    "  pred slot={} reg=p{} value={}",
                    write.slot, write.predicate, write.value
                )?;
            }
            for effect in &event.memory_effects {
                match effect {
                    TraceMemoryEffect::Load {
                        slot,
                        width_bytes,
                        address,
                        value,
                        in_bounds,
                        cache_outcome,
                    } => writeln!(
                        f,
                        "  mem slot={} kind=load width={} addr={:#010x} value={:#018x} in_bounds={} cache={}",
                        slot, width_bytes, address, value, in_bounds, format_cache_outcome(*cache_outcome)
                    )?,
                    TraceMemoryEffect::Store {
                        slot,
                        width_bytes,
                        address,
                        value,
                        in_bounds,
                        cache_outcome,
                    } => writeln!(
                        f,
                        "  mem slot={} kind=store width={} addr={:#010x} value={:#018x} in_bounds={} cache={}",
                        slot, width_bytes, address, value, in_bounds, format_cache_outcome(*cache_outcome)
                    )?,
                }
            }
            for decision in &event.control_flow {
                match decision {
                    TraceControlFlow::Branch {
                        slot,
                        predicate,
                        pred_negated,
                        taken,
                        target,
                        fallthrough,
                    } => writeln!(
                        f,
                        "  control slot={} kind=branch pred={} taken={} target={} fallthrough={}",
                        slot,
                        format_guard(*predicate, *pred_negated),
                        taken,
                        target,
                        fallthrough
                    )?,
                    TraceControlFlow::Jump { slot, target } => {
                        writeln!(f, "  control slot={} kind=jump target={}", slot, target)?
                    }
                    TraceControlFlow::Call {
                        slot,
                        target,
                        return_pc,
                    } => writeln!(
                        f,
                        "  control slot={} kind=call target={} return={}",
                        slot, target, return_pc
                    )?,
                    TraceControlFlow::Ret {
                        slot,
                        target,
                        halted,
                    } => writeln!(
                        f,
                        "  control slot={} kind=ret target={} halted={}",
                        slot,
                        target.map_or_else(|| "halt".to_string(), |target| target.to_string()),
                        halted
                    )?,
                }
            }
        }
        writeln!(
            f,
            "final pc={} cycle={} halted={}",
            self.final_pc, self.final_cycle, self.final_halted
        )
    }
}

fn format_cache_outcome(outcome: CacheOutcome) -> &'static str {
    match outcome {
        CacheOutcome::Hit => "hit",
        CacheOutcome::Miss => "miss",
        CacheOutcome::MissDirty => "miss_dirty",
    }
}

impl TraceEventKind {
    fn as_str(self) -> &'static str {
        match self {
            TraceEventKind::Issue => "issue",
            TraceEventKind::Stall => "stall",
            TraceEventKind::IllegalBundle => "illegal",
        }
    }
}

fn trace_active_syllable(slot: usize, syl: &Syllable) -> TraceActiveSyllable {
    TraceActiveSyllable {
        slot,
        opcode: syl.opcode,
        predicate: syl.predicate,
        pred_negated: syl.pred_negated,
        dst: syl.dst,
        src: syl.src,
        imm: syl.imm,
    }
}

fn gpr_write_dst_for_trace(syl: &Syllable) -> Option<usize> {
    if syl.opcode == Opcode::Call {
        Some(31)
    } else if syl.opcode.writes_gpr() {
        syl.dst
    } else {
        None
    }
}

fn memory_width_bytes(opcode: Opcode) -> Option<usize> {
    match opcode {
        Opcode::LoadB | Opcode::StoreB => Some(1),
        Opcode::LoadH | Opcode::StoreH => Some(2),
        Opcode::LoadW | Opcode::StoreW => Some(4),
        Opcode::LoadD | Opcode::StoreD | Opcode::AcqLoad | Opcode::RelStore => Some(8),
        _ => None,
    }
}

fn is_load_opcode(opcode: Opcode) -> bool {
    matches!(
        opcode,
        Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD | Opcode::AcqLoad
    )
}

fn memory_access_in_bounds(mem_size: usize, address: usize, width_bytes: usize) -> bool {
    width_bytes <= mem_size && address <= mem_size - width_bytes
}

fn memory_bounds_error(kind: &str, address: usize, width_bytes: usize, mem_size: usize) -> String {
    format!(
        "error: {kind} at 0x{address:x} (width={width_bytes}) is out of bounds (memory size=0x{mem_size:x})"
    )
}

fn mask_to_width(value: u64, width_bytes: usize) -> u64 {
    match width_bytes {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        _ => value,
    }
}

fn format_guard(predicate: usize, pred_negated: bool) -> String {
    if pred_negated {
        format!("!p{predicate}")
    } else {
        format!("p{predicate}")
    }
}

fn format_optional_reg(prefix: &str, value: Option<usize>) -> String {
    value.map_or_else(|| "-".to_string(), |idx| format!("{prefix}{idx}"))
}

fn format_active_dst(active: &TraceActiveSyllable) -> String {
    let prefix = if active.opcode.writes_pred() {
        "p"
    } else {
        "r"
    };
    format_optional_reg(prefix, active.dst)
}

fn format_active_src(active: &TraceActiveSyllable, src_idx: usize) -> String {
    let prefix = if active.opcode.reads_pred_src() {
        "p"
    } else {
        "r"
    };
    format_optional_reg(prefix, active.src[src_idx])
}

fn opcode_mnemonic(op: Opcode) -> &'static str {
    match op {
        Opcode::Add => "add",
        Opcode::Sub => "sub",
        Opcode::And => "and",
        Opcode::Or => "or",
        Opcode::Xor => "xor",
        Opcode::Shl => "shl",
        Opcode::Srl => "srl",
        Opcode::Sra => "sra",
        Opcode::Mov => "mov",
        Opcode::MovImm => "movi",
        Opcode::CmpEq => "cmpeq",
        Opcode::CmpLt => "cmplt",
        Opcode::CmpUlt => "cmpult",
        Opcode::LoadB => "loadb",
        Opcode::LoadH => "loadh",
        Opcode::LoadW => "loadw",
        Opcode::LoadD => "loadd",
        Opcode::StoreB => "storeb",
        Opcode::StoreH => "storeh",
        Opcode::StoreW => "storew",
        Opcode::StoreD => "stored",
        Opcode::Lea => "lea",
        Opcode::Prefetch => "prefetch",
        Opcode::Mul => "mul",
        Opcode::MulH => "mulh",
        Opcode::Branch => "branch",
        Opcode::Jump => "jump",
        Opcode::Call => "call",
        Opcode::Ret => "ret",
        Opcode::PAnd => "pand",
        Opcode::POr => "por",
        Opcode::PXor => "pxor",
        Opcode::PNot => "pnot",
        Opcode::FpAdd32 => "fpadd32",
        Opcode::FpMul32 => "fpmul32",
        Opcode::FpAdd64 => "fpadd64",
        Opcode::FpMul64 => "fpmul64",
        Opcode::AesEnc => "aesenc",
        Opcode::AesDec => "aesdec",
        Opcode::AcqLoad => "acqload",
        Opcode::RelStore => "relstore",
        Opcode::Nop => "nop",
    }
}
