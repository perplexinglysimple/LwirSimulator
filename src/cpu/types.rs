verus! {

pub const NUM_GPRS: usize = crate::layout::DEFAULT_NUM_GPRS;
pub const NUM_PREDS: usize = crate::layout::DEFAULT_NUM_PREDS;
pub const MEM_SIZE: usize = crate::layout::DEFAULT_MEM_SIZE;

/// Scoreboard: tracks when each GPR's in-flight write completes.
#[derive(Clone, Copy, Debug)]
pub struct ScoreboardEntry {
    pub ready_cycle: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryFaultKind {
    Load,
    Store,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryFault {
    pub kind: MemoryFaultKind,
    pub address: usize,
    pub width_bytes: usize,
    pub memory_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepResult {
    Issued,
    Stalled,
    Halted,
    Fault(MemoryFault),
}

/// Full architectural state — all fields pub so callers can snapshot freely.
#[derive(Clone, Debug)]
pub struct CpuState {
    pub width:      usize,
    pub num_gprs:   usize,
    pub num_preds:  usize,
    pub mem_size:   usize,
    pub gprs:       Vec<u64>,
    pub preds:      Vec<bool>,
    pub pc:         usize,
    pub cycle:      u64,
    pub scoreboard: Vec<ScoreboardEntry>,
    pub memory:     Vec<u8>,
    pub cache:      CacheState,
    pub halted:     bool,
    pub latencies:  LatencyTable,
}

} // verus!

impl MemoryFault {
    pub fn diagnostic(&self) -> String {
        format!(
            "error: {} at 0x{:x} (width={}) is out of bounds (memory size=0x{:x})",
            match self.kind {
                MemoryFaultKind::Load => "load",
                MemoryFaultKind::Store => "store",
            },
            self.address,
            self.width_bytes,
            self.memory_size
        )
    }
}
