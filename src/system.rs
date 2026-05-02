use crate::bundle::Bundle;
use crate::cpu::CpuState;
use crate::latency::LatencyTable;
use crate::layout::{program_layout_compatible_runtime, ProcessorLayout};
use crate::program::Program;

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
    pub cycle: u64,
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
        }

        let cpus = (0..layout.topology.cpus)
            .map(|_| CpuState::new_for_layout(&layout, latencies.clone()))
            .collect();
        let memory = SharedMemory::new(layout.arch.memory_bytes);

        Ok(Self {
            layout,
            cpus,
            programs,
            memory,
            cycle: 0,
        })
    }

    pub fn from_program(program: Program, latencies: LatencyTable) -> Result<Self, String> {
        Self::new(program.layout, vec![program.bundles], latencies)
    }

    pub fn step_global(&mut self) -> bool {
        let mut any_progress = false;

        for cpu_id in 0..self.cpus.len() {
            self.cpus[cpu_id].cycle = self.cycle;
            self.cpus[cpu_id].memory.clone_from(&self.memory.bytes);

            if self.cpus[cpu_id].step(&self.layout, &self.programs[cpu_id]) {
                self.memory.bytes.clone_from(&self.cpus[cpu_id].memory);
                any_progress = true;
            }
        }

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
}
