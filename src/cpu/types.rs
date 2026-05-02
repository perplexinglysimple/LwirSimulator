verus! {

/// Number of architectural general-purpose registers in the simulated ISA.
/// This sizes the `gprs` file, the scoreboard, and all GPR index checks.
pub const NUM_GPRS:  usize = 32;
/// Number of architectural predicate registers.
/// This sizes the `preds` file and defines the valid predicate index range.
pub const NUM_PREDS: usize = 16;
/// Size of the flat byte-addressed memory in bytes.
/// Load/store helpers and bounds checks use this as the architectural memory limit.
pub const MEM_SIZE:  usize = 65536;

/// Scoreboard: tracks when each GPR's in-flight write completes.
#[derive(Clone, Copy, Debug)]
pub struct ScoreboardEntry {
    pub ready_cycle: u64,
}

/// Full architectural state — all fields pub so callers can snapshot freely.
#[derive(Clone, Debug)]
pub struct CpuState {
    pub width:      usize,
    pub gprs:       Vec<u64>,
    pub preds:      Vec<bool>,
    pub pc:         usize,
    pub cycle:      u64,
    pub scoreboard: Vec<ScoreboardEntry>,
    pub memory:     Vec<u8>,
    pub halted:     bool,
    pub latencies:  LatencyTable,
}

} // verus!
