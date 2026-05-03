use crate::bundle::Bundle;
use crate::cache::{at_most_one_modified_cache_states, CacheState};
use crate::cpu::{CpuState, ScoreboardEntry};
use crate::isa::{Opcode, Syllable};
use crate::latency::LatencyTable;
use crate::layout::{program_layout_compatible_runtime, ProcessorLayout};
use crate::program::Program;
use builtin::*;
use builtin_macros::*;
use std::fmt;
use vstd::prelude::*;

verus! {

pub open spec fn bus_serializes_commits(commits_in_cycle: nat) -> bool {
    commits_in_cycle <= 1
}

pub open spec fn spec_bus_owner(cycle: nat, cpus: nat) -> nat
    recommends cpus > 0
{
    cycle % cpus
}

pub open spec fn spec_bus_slot(cycle: nat, cpu_id: nat, cpus: nat) -> bool {
    cpus > 0 && cpu_id < cpus && spec_bus_owner(cycle, cpus) == cpu_id
}

pub fn bus_slot_model(cycle: usize, cpu_id: usize, cpus: usize) -> (ret: bool)
    ensures ret == spec_bus_slot(cycle as nat, cpu_id as nat, cpus as nat),
{
    cpus > 0 && cpu_id < cpus && cycle % cpus == cpu_id
}

pub proof fn lemma_bus_slot_model_equiv(cycle: usize, cpu_id: usize, cpus: usize)
    ensures
        spec_bus_slot(cycle as nat, cpu_id as nat, cpus as nat) ==
            (cpus > 0 && cpu_id < cpus && cycle % cpus == cpu_id),
{
}

pub proof fn lemma_bus_slot_unique(cycle: nat, lhs_cpu: nat, rhs_cpu: nat, cpus: nat)
    requires
        spec_bus_slot(cycle, lhs_cpu, cpus),
        spec_bus_slot(cycle, rhs_cpu, cpus),
    ensures
        lhs_cpu == rhs_cpu,
{
}

} // verus!

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedMemory {
    bytes: Vec<u8>,
}

impl SharedMemory {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
        }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

#[derive(Clone, Debug)]
pub struct System {
    pub layout: ProcessorLayout,
    pub cpus: Vec<CpuState>,
    pub programs: Vec<Vec<Bundle>>,
    pub memory: SharedMemory,
    pub bus: Bus,
    pub cycle: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusAccessKind {
    Load,
    Store,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusReq {
    pub cpu_id: usize,
    pub slot: usize,
    pub opcode: Opcode,
    pub kind: BusAccessKind,
    pub address: usize,
    pub width_bytes: usize,
    pub value: u64,
    pub dst: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArbState {
    pub cpus: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bus {
    pub arb: ArbState,
    pub events: Vec<BusEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusEvent {
    pub cycle: u64,
    pub winner: usize,
    pub granted: Option<BusReq>,
}

impl System {
    pub fn new(
        layout: ProcessorLayout,
        programs: Vec<Vec<Bundle>>,
        latencies: LatencyTable,
    ) -> Result<Self, String> {
        if !layout.validate() {
            return Err("invalid processor layout; see docs/processor_layout_plan.md".to_string());
        }
        if programs.len() != layout.topology.cpus {
            return Err(format!(
                "topology declares {} CPU(s), but {} program(s) were provided",
                layout.topology.cpus,
                programs.len()
            ));
        }
        for (cpu_id, program) in programs.iter().enumerate() {
            if !program_layout_compatible_runtime(&layout, program) {
                return Err(format!(
                    "program for CPU {cpu_id} is incompatible with layout"
                ));
            }
            if let Some((bundle_idx, slot)) =
                first_bus_slot_conflict(program, cpu_id, layout.topology.cpus)
            {
                return Err(format!(
                    "program for CPU {cpu_id} issues memory op in bundle {bundle_idx} slot {slot}, \
                     but bus owner for cycle {bundle_idx} is CPU {}",
                    bus_owner(bundle_idx as u64, layout.topology.cpus)
                ));
            }
        }

        let cpus = (0..layout.topology.cpus)
            .map(|_| CpuState::new_for_layout(&layout, latencies.clone()))
            .collect();
        let memory = SharedMemory::new(layout.arch.memory_bytes);

        let cpus_count = layout.topology.cpus;

        Ok(Self {
            layout,
            cpus,
            programs,
            memory,
            bus: Bus::new(cpus_count),
            cycle: 0,
        })
    }

    pub fn from_program(program: Program, latencies: LatencyTable) -> Result<Self, String> {
        Self::new(program.layout, vec![program.bundles], latencies)
    }

    pub fn step_global(&mut self) -> bool {
        let mut any_progress = false;
        let mut requests = Vec::new();

        for cpu_id in 0..self.cpus.len() {
            self.cpus[cpu_id].cycle = self.cycle;
            self.cpus[cpu_id].memory.clone_from(&self.memory.bytes);

            if let Some(req) = self.next_memory_request(cpu_id) {
                requests.push(req);
            }
        }

        let winner = self.bus.owner(self.cycle);
        let grant = self.bus.arbitrate(self.cycle, &requests);

        for cpu_id in 0..self.cpus.len() {
            self.cpus[cpu_id].cycle = self.cycle;
            self.cpus[cpu_id].memory.clone_from(&self.memory.bytes);

            let request = requests.iter().find(|req| req.cpu_id == cpu_id);
            let stepped = if request.is_some() {
                let mut bundle = self.programs[cpu_id][self.cpus[cpu_id].pc].clone();
                bundle.set_slot(request.expect("request exists").slot, Syllable::nop());
                let one_bundle = vec![bundle];
                let old_pc = self.cpus[cpu_id].pc;
                self.cpus[cpu_id].pc = 0;
                let stepped = self.cpus[cpu_id].step(&self.layout, &one_bundle);
                if stepped {
                    self.cpus[cpu_id].pc = old_pc + 1;
                } else {
                    self.cpus[cpu_id].pc = old_pc;
                }
                stepped
            } else {
                self.cpus[cpu_id].step(&self.layout, &self.programs[cpu_id])
            };

            if stepped {
                self.memory.bytes.clone_from(&self.cpus[cpu_id].memory);
                any_progress = true;
            }
        }

        if let Some(req) = &grant {
            self.commit_bus_request(req);
            any_progress = true;
        }
        self.bus.events.push(BusEvent {
            cycle: self.cycle,
            winner,
            granted: grant,
        });

        if any_progress {
            self.cycle = self.cycle.wrapping_add(1);
            for cpu in &mut self.cpus {
                cpu.cycle = self.cycle;
            }
        }

        any_progress
    }

    pub fn run_until_quiescent(&mut self) {
        while self.step_global() {}
    }

    fn next_memory_request(&self, cpu_id: usize) -> Option<BusReq> {
        let cpu = &self.cpus[cpu_id];
        if cpu.halted || cpu.pc >= self.programs[cpu_id].len() {
            return None;
        }
        let bundle = &self.programs[cpu_id][cpu.pc];
        for (slot, syl) in bundle.syllables.iter().enumerate() {
            if !self.syllable_active(cpu, syl) {
                continue;
            }
            let Some((kind, width_bytes)) = memory_access(syl.opcode) else {
                continue;
            };
            if !memory_sources_ready(cpu, syl) {
                return None;
            }
            let address = cpu.read_src_gpr(syl.src[0]).wrapping_add(syl.imm as u64) as usize;
            let value = if kind == BusAccessKind::Store {
                mask_to_width(cpu.read_src_gpr(syl.src[1]), width_bytes)
            } else {
                0
            };
            return Some(BusReq {
                cpu_id,
                slot,
                opcode: syl.opcode,
                kind,
                address,
                width_bytes,
                value,
                dst: syl.dst,
            });
        }
        None
    }

    fn syllable_active(&self, cpu: &CpuState, syl: &Syllable) -> bool {
        let pred = cpu.read_pred(syl.predicate);
        if syl.pred_negated {
            !pred
        } else {
            pred
        }
    }

    fn commit_bus_request(&mut self, req: &BusReq) {
        match req.kind {
            BusAccessKind::Load => {
                let value = self.memory.load(req.address, req.width_bytes);
                let (_, latency) = self.cpus[req.cpu_id].cache.access_load(req.address);
                if let Some(dst) = req.dst {
                    self.cpus[req.cpu_id].write_gpr(dst, value);
                    if dst < self.cpus[req.cpu_id].scoreboard.len() {
                        self.cpus[req.cpu_id].scoreboard[dst] = ScoreboardEntry {
                            ready_cycle: self.cycle.wrapping_add(1).wrapping_add(latency as u64),
                        };
                    }
                }
            }
            BusAccessKind::Store => {
                for (cpu_id, cpu) in self.cpus.iter_mut().enumerate() {
                    cpu.cache
                        .store_commit_transition(req.address, cpu_id == req.cpu_id);
                }
                self.memory.store(req.address, req.width_bytes, req.value);
            }
        }
        self.cpus[req.cpu_id].memory.clone_from(&self.memory.bytes);
    }
}

impl Bus {
    pub fn new(cpus: usize) -> Self {
        Self {
            arb: ArbState { cpus },
            events: Vec::new(),
        }
    }

    pub fn owner(&self, cycle: u64) -> usize {
        bus_owner(cycle, self.arb.cpus)
    }

    pub fn arbitrate(&self, cycle: u64, requests: &[BusReq]) -> Option<BusReq> {
        let winner = self.owner(cycle);
        requests.iter().find(|req| req.cpu_id == winner).cloned()
    }
}

impl fmt::Display for Bus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "bus trace v1")?;
        for event in &self.events {
            write!(f, "{event}")?;
        }
        Ok(())
    }
}

impl fmt::Display for BusEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bus cycle={} winner=cpu{}", self.cycle, self.winner)?;
        if let Some(req) = &self.granted {
            write!(
                f,
                " grant=cpu{}:slot{}:{}@0x{:08x}/{}",
                req.cpu_id,
                req.slot,
                format_bus_kind(req.kind),
                req.address,
                req.width_bytes
            )?;
        } else {
            write!(f, " grant=none")?;
        }
        writeln!(f)
    }
}

impl SharedMemory {
    fn load(&self, address: usize, width_bytes: usize) -> u64 {
        if !memory_access_in_bounds(self.bytes.len(), address, width_bytes) {
            return 0;
        }
        let mut value = 0u64;
        for offset in 0..width_bytes {
            value |= (self.bytes[address + offset] as u64) << (offset * 8);
        }
        value
    }

    fn store(&mut self, address: usize, width_bytes: usize, value: u64) {
        if !memory_access_in_bounds(self.bytes.len(), address, width_bytes) {
            return;
        }
        for offset in 0..width_bytes {
            self.bytes[address + offset] = ((value >> (offset * 8)) & 0xff) as u8;
        }
    }
}

fn memory_access(opcode: Opcode) -> Option<(BusAccessKind, usize)> {
    match opcode {
        Opcode::LoadB => Some((BusAccessKind::Load, 1)),
        Opcode::LoadH => Some((BusAccessKind::Load, 2)),
        Opcode::LoadW => Some((BusAccessKind::Load, 4)),
        Opcode::LoadD | Opcode::AcqLoad => Some((BusAccessKind::Load, 8)),
        Opcode::StoreB => Some((BusAccessKind::Store, 1)),
        Opcode::StoreH => Some((BusAccessKind::Store, 2)),
        Opcode::StoreW => Some((BusAccessKind::Store, 4)),
        Opcode::StoreD | Opcode::RelStore => Some((BusAccessKind::Store, 8)),
        _ => None,
    }
}

fn memory_access_in_bounds(memory_len: usize, address: usize, width_bytes: usize) -> bool {
    width_bytes <= memory_len && address <= memory_len - width_bytes
}

fn mask_to_width(value: u64, width_bytes: usize) -> u64 {
    match width_bytes {
        1 => value & 0xff,
        2 => value & 0xffff,
        4 => value & 0xffff_ffff,
        _ => value,
    }
}

fn format_bus_kind(kind: BusAccessKind) -> &'static str {
    match kind {
        BusAccessKind::Load => "load",
        BusAccessKind::Store => "store",
    }
}

pub fn bus_owner(cycle: u64, cpus: usize) -> usize {
    if cpus == 0 {
        0
    } else {
        (cycle as usize) % cpus
    }
}

pub fn bus_slot(cycle: u64, cpu_id: usize, cpus: usize) -> bool {
    cpus > 0 && cpu_id < cpus && bus_owner(cycle, cpus) == cpu_id
}

fn memory_sources_ready(cpu: &CpuState, syl: &Syllable) -> bool {
    let needed_cycle = cpu.cycle.wrapping_add(1);
    for src in syl.src.iter().flatten() {
        if *src > 0
            && *src < cpu.scoreboard.len()
            && cpu.scoreboard[*src].ready_cycle > needed_cycle
        {
            return false;
        }
    }
    true
}

pub fn is_memory_opcode(opcode: Opcode) -> bool {
    memory_access(opcode).is_some()
}

pub fn first_bus_slot_conflict(
    program: &[Bundle],
    cpu_id: usize,
    cpus: usize,
) -> Option<(usize, usize)> {
    for (bundle_idx, bundle) in program.iter().enumerate() {
        for (slot, syl) in bundle.syllables.iter().enumerate() {
            if is_memory_opcode(syl.opcode) && !bus_slot(bundle_idx as u64, cpu_id, cpus) {
                return Some((bundle_idx, slot));
            }
        }
    }
    None
}

pub fn system_worst_case_load_latency(cpus: usize, bus_slot_cost: u32, cache_latency: u32) -> u32 {
    system_worst_case_load_latency_with_coherence(cpus, bus_slot_cost, cache_latency, 0)
}

pub fn system_worst_case_load_latency_with_coherence(
    cpus: usize,
    bus_slot_cost: u32,
    cache_latency: u32,
    coherence_drain: u32,
) -> u32 {
    let waiting_slots = cpus.saturating_sub(1) as u32;
    waiting_slots
        .saturating_mul(bus_slot_cost)
        .saturating_add(cache_latency)
        .saturating_add(coherence_drain)
}

pub fn coherence_drain(layout: &crate::layout::ProcessorLayout) -> u32 {
    if layout.topology.cpus <= 1 {
        0
    } else {
        // Stage 4D's MSI model serializes invalidation/upgrade on the same bus
        // transaction as the store. The only extra per-line coherence cost that can
        // affect a statically scheduled load is draining one dirty owner, bounded by
        // the configured L1D writeback latency. Stores still update SharedMemory at
        // commit time, so coherence never inserts a runtime stall.
        layout.cache.writeback_latency
    }
}

pub fn at_most_one_modified(caches: &[CacheState], address: usize) -> bool {
    at_most_one_modified_cache_states(caches, address)
}

pub fn at_most_one_modified_for_system(system: &System, address: usize) -> bool {
    // The verified bridge is over a slice of `CacheState`; collect the per-CPU
    // states here so this public system helper consumes that same checked path.
    let mut caches = Vec::new();
    for cpu in &system.cpus {
        caches.push(cpu.cache.clone());
    }
    at_most_one_modified(&caches, address)
}

/// Static upper bound on the number of cycles between a `RelStore` issuing on the
/// producer and the matching `AcqLoad` observing it on the consumer.
///
/// Formula: (cpus − 1) × bus_slot_cost + miss_latency + writeback_latency + coherence_drain
///
/// `bus_slot_cost` is 1 (the closed-form round-robin schedule grants each CPU one
/// slot per N-cycle window; the consumer waits at most cpus−1 slots for its turn).
pub fn worst_case_visibility(layout: &crate::layout::ProcessorLayout) -> u32 {
    let cpus = layout.topology.cpus;
    let bus_slot_cost = 1u32;
    let cache_cost = layout.cache.worst_case_load_latency();
    let coherence_drain = coherence_drain(layout);
    (cpus.saturating_sub(1) as u32)
        .saturating_mul(bus_slot_cost)
        .saturating_add(cache_cost)
        .saturating_add(coherence_drain)
}
