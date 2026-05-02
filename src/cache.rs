use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheConfig {
    pub line_bytes: usize,
    pub capacity_bytes: usize,
    pub associativity: usize,
    pub hit_latency: u32,
    pub miss_latency: u32,
    pub writeback_latency: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheOutcome {
    Hit,
    Miss,
    MissDirty,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheLine {
    pub tag: u64,
    pub valid: bool,
    pub dirty: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CacheState {
    pub config: CacheConfig,
    pub lines: Vec<CacheLine>,
}

impl CacheConfig {
    pub fn default_l1d() -> Self {
        CacheConfig {
            line_bytes: 64,
            capacity_bytes: 4096,
            associativity: 1,
            hit_latency: 1,
            miss_latency: 3,
            writeback_latency: 0,
        }
    }

    pub fn nlines(&self) -> usize {
        if self.line_bytes == 0 {
            0
        } else {
            self.capacity_bytes / self.line_bytes
        }
    }

    pub fn validate(&self) -> bool {
        self.line_bytes > 0
            && self.capacity_bytes >= self.line_bytes
            && self.capacity_bytes % self.line_bytes == 0
            && self.associativity == 1
            && self.hit_latency > 0
            && self.miss_latency > 0
    }

    pub fn worst_case_load_latency(&self) -> u32 {
        self.miss_latency.saturating_add(self.writeback_latency)
    }
}

impl CacheState {
    pub fn new(config: CacheConfig) -> Self {
        let nlines = config.nlines();
        let mut lines = Vec::new();
        let mut i = 0usize;
        while i < nlines
            invariant
                i <= nlines,
                lines.len() == i,
            decreases nlines - i,
        {
            lines.push(CacheLine { tag: 0, valid: false, dirty: false });
            i += 1;
        }
        CacheState { config, lines }
    }

    pub fn peek_outcome(&self, addr: usize) -> CacheOutcome {
        if self.lines.len() == 0 || self.config.line_bytes == 0 {
            return CacheOutcome::Miss;
        }
        let index = cache_index(addr, self.config);
        let tag = cache_tag(addr, self.config);
        if index < self.lines.len() {
            let line = self.lines[index];
            if line.valid && line.tag == tag {
                CacheOutcome::Hit
            } else if line.valid && line.dirty {
                CacheOutcome::MissDirty
            } else {
                CacheOutcome::Miss
            }
        } else {
            CacheOutcome::Miss
        }
    }

    pub fn access_load(&mut self, addr: usize) -> (CacheOutcome, u32) {
        self.access(addr, false)
    }

    pub fn access_store(&mut self, addr: usize) -> CacheOutcome {
        let (outcome, _) = self.access(addr, true);
        outcome
    }

    fn access(&mut self, addr: usize, make_dirty: bool) -> (CacheOutcome, u32) {
        let outcome = self.peek_outcome(addr);
        let latency = cache_outcome_latency(self.config, outcome);
        if self.lines.len() != 0 && self.config.line_bytes != 0 {
            let index = cache_index(addr, self.config);
            if index < self.lines.len() {
                self.lines.set(index, CacheLine {
                    tag: cache_tag(addr, self.config),
                    valid: true,
                    dirty: make_dirty,
                });
            }
        }
        (outcome, latency)
    }
}

pub fn cache_index(addr: usize, config: CacheConfig) -> usize {
    let nlines = config.nlines();
    if config.line_bytes == 0 || nlines == 0 {
        0
    } else {
        (addr / config.line_bytes) % nlines
    }
}

pub fn cache_tag(addr: usize, config: CacheConfig) -> u64 {
    let nlines = config.nlines();
    if config.line_bytes == 0 || nlines == 0 {
        0
    } else {
        (addr / config.line_bytes / nlines) as u64
    }
}

pub fn cache_outcome_latency(config: CacheConfig, outcome: CacheOutcome) -> u32 {
    match outcome {
        CacheOutcome::Hit => config.hit_latency,
        CacheOutcome::Miss => config.miss_latency,
        CacheOutcome::MissDirty => config.miss_latency.saturating_add(config.writeback_latency),
    }
}

} // verus!
