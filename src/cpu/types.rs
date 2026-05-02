verus! {

pub const NUM_GPRS: usize = crate::layout::DEFAULT_NUM_GPRS;
pub const NUM_PREDS: usize = crate::layout::DEFAULT_NUM_PREDS;
pub const MEM_SIZE: usize = crate::layout::DEFAULT_MEM_SIZE;

/// Scoreboard: tracks when each GPR's in-flight write completes.
#[derive(Clone, Copy, Debug)]
pub struct ScoreboardEntry {
    pub ready_cycle: u64,
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
