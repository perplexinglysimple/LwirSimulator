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
pub enum MsiState {
    Invalid,
    Shared,
    Modified,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheLine {
    pub tag: u64,
    pub valid: bool,
    pub dirty: bool,
    pub msi: MsiState,
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
            lines.push(CacheLine {
                tag: 0,
                valid: false,
                dirty: false,
                msi: MsiState::Invalid,
            });
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
        self.access(addr, MsiState::Shared)
    }

    pub fn access_store(&mut self, addr: usize) -> CacheOutcome {
        let (outcome, _) = self.access(addr, MsiState::Modified);
        outcome
    }

    pub fn store_commit_transition(&mut self, addr: usize, is_writer: bool) -> (ret: CacheOutcome)
        ensures
            self.config == old(self).config,
            self.lines.len() == old(self).lines.len(),
            old(self).lines.len() != 0 && old(self).config.line_bytes != 0
                && spec_cache_index(addr, old(self).config) < old(self).lines.len() ==>
                self.lines[spec_cache_index(addr, old(self).config) as int] ==
                    spec_store_commit_line(
                        old(self).lines[spec_cache_index(addr, old(self).config) as int],
                        is_writer,
                        addr,
                        old(self).config,
                    ),
    {
        let outcome = self.peek_outcome(addr);
        if self.lines.len() != 0 && self.config.line_bytes != 0 {
            let index = cache_index(addr, self.config);
            if index < self.lines.len() {
                let line = self.lines[index];
                let new_line = store_commit_line_exec_model(line, is_writer, addr, self.config);
                self.lines.set(index, new_line);
            }
        }
        outcome
    }

    pub fn invalidate(&mut self, addr: usize) {
        if self.lines.len() != 0 && self.config.line_bytes != 0 {
            let index = cache_index(addr, self.config);
            let tag = cache_tag(addr, self.config);
            if index < self.lines.len() {
                let line = self.lines[index];
                if line.valid && line.tag == tag {
                    self.lines.set(index, CacheLine {
                        tag: line.tag,
                        valid: false,
                        dirty: false,
                        msi: MsiState::Invalid,
                    });
                }
            }
        }
    }

    fn access(&mut self, addr: usize, new_state: MsiState) -> (CacheOutcome, u32) {
        let outcome = self.peek_outcome(addr);
        let latency = cache_outcome_latency(self.config, outcome);
        if self.lines.len() != 0 && self.config.line_bytes != 0 {
            let index = cache_index(addr, self.config);
            if index < self.lines.len() {
                let dirty = match new_state {
                    MsiState::Modified => true,
                    MsiState::Invalid | MsiState::Shared => false,
                };
                let valid = match new_state {
                    MsiState::Invalid => false,
                    MsiState::Shared | MsiState::Modified => true,
                };
                self.lines.set(index, CacheLine {
                    tag: cache_tag(addr, self.config),
                    valid,
                    dirty,
                    msi: new_state,
                });
            }
        }
        (outcome, latency)
    }
}

pub fn cache_line_matches_addr(line: CacheLine, addr: usize, config: CacheConfig) -> (ret: bool)
    ensures
        ret == (line.valid && line.tag == spec_cache_tag(addr, config)),
{
    line.valid && line.tag == cache_tag(addr, config)
}

pub fn at_most_one_modified_two_lines(
    lhs: CacheLine,
    rhs: CacheLine,
    addr: usize,
    config: CacheConfig,
) -> (ret: bool)
    ensures
        ret == spec_at_most_one_modified(seq![lhs, rhs], addr, config),
{
    let lhs_modified = lhs.valid && lhs.tag == cache_tag(addr, config) && match lhs.msi {
        MsiState::Modified => true,
        MsiState::Invalid | MsiState::Shared => false,
    };
    let rhs_modified = rhs.valid && rhs.tag == cache_tag(addr, config) && match rhs.msi {
        MsiState::Modified => true,
        MsiState::Invalid | MsiState::Shared => false,
    };
    !(lhs_modified && rhs_modified)
}

pub fn cache_state_modified_for_addr(cache: &CacheState, addr: usize) -> (ret: bool)
    ensures
        ret == spec_cache_state_modified_for_addr(cache, addr),
{
    if cache.lines.len() == 0 || cache.config.line_bytes == 0 {
        return false;
    }
    let index = cache_index(addr, cache.config);
    if index >= cache.lines.len() {
        return false;
    }
    let line = cache.lines[index];
    line.valid && line.tag == cache_tag(addr, cache.config) && match line.msi {
        MsiState::Modified => true,
        MsiState::Invalid | MsiState::Shared => false,
    }
}

pub fn at_most_one_modified_cache_states(caches: &[CacheState], addr: usize) -> (ret: bool)
    ensures
        ret == spec_at_most_one_modified_cache_states(caches@, addr),
{
    let mut modified_index: Option<usize> = None;
    let mut i = 0usize;
    while i < caches.len()
        invariant
            i <= caches.len(),
            match modified_index {
                None => forall|j: int| 0 <= j < i ==> !spec_cache_state_modified_for_addr(&caches@[j], addr),
                Some(idx) =>
                    idx < i &&
                    spec_cache_state_modified_for_addr(&caches@[idx as int], addr) &&
                    forall|j: int| 0 <= j < i && j != idx ==> !spec_cache_state_modified_for_addr(&caches@[j], addr),
            },
        decreases caches.len() - i,
    {
        let modified = cache_state_modified_for_addr(&caches[i], addr);
        if modified {
            match modified_index {
                Some(idx) => {
                    let _ = idx;
                    assert(idx < i);
                    assert(spec_cache_state_modified_for_addr(&caches@[idx as int], addr));
                    assert(spec_cache_state_modified_for_addr(&caches@[i as int], addr));
                    assert(idx != i);
                    assert(!spec_at_most_one_modified_cache_states(caches@, addr));
                    return false;
                }
                None => {
                    modified_index = Some(i);
                }
            }
        }
        i += 1;
    }
    assert(i == caches.len());
    assert(spec_at_most_one_modified_cache_states(caches@, addr));
    true
}

pub fn store_commit_line_exec_model(
    line: CacheLine,
    is_writer: bool,
    addr: usize,
    config: CacheConfig,
) -> (ret: CacheLine)
    ensures
        ret == spec_store_commit_line(line, is_writer, addr, config),
{
    if is_writer {
        CacheLine {
            tag: cache_tag(addr, config),
            valid: true,
            dirty: true,
            msi: MsiState::Modified,
        }
    } else if cache_line_matches_addr(line, addr, config) {
        CacheLine {
            tag: line.tag,
            valid: false,
            dirty: false,
            msi: MsiState::Invalid,
        }
    } else {
        line
    }
}

pub open spec fn spec_line_modified_for_addr(
    line: CacheLine,
    addr: usize,
    config: CacheConfig,
) -> bool {
    line.valid && line.tag == spec_cache_tag(addr, config) && match line.msi {
        MsiState::Modified => true,
        MsiState::Invalid | MsiState::Shared => false,
    }
}

pub open spec fn spec_at_most_one_modified_two(
    lhs: CacheLine,
    rhs: CacheLine,
    addr: usize,
    config: CacheConfig,
) -> bool {
    !(spec_line_modified_for_addr(lhs, addr, config)
        && spec_line_modified_for_addr(rhs, addr, config))
}

pub open spec fn spec_line_invalid(line: CacheLine) -> bool {
    match line.msi {
        MsiState::Invalid => true,
        MsiState::Shared | MsiState::Modified => false,
    }
}

pub open spec fn spec_at_most_one_modified(
    lines: Seq<CacheLine>,
    addr: usize,
    config: CacheConfig,
) -> bool {
    forall|i: int, j: int|
        0 <= i < lines.len()
            && 0 <= j < lines.len()
            && spec_line_modified_for_addr(lines[i], addr, config)
            && spec_line_modified_for_addr(lines[j], addr, config) ==> i == j
}

pub open spec fn spec_cache_state_modified_for_addr(cache: &CacheState, addr: usize) -> bool {
    if cache.lines.len() == 0 || cache.config.line_bytes == 0 {
        false
    } else {
        let index = spec_cache_index(addr, cache.config);
        if index >= cache.lines.len() {
            false
        } else {
            spec_line_modified_for_addr(cache.lines[index as int], addr, cache.config)
        }
    }
}

pub open spec fn spec_at_most_one_modified_cache_states(
    caches: Seq<CacheState>,
    addr: usize,
) -> bool {
    forall|i: int, j: int|
        0 <= i < caches.len()
            && 0 <= j < caches.len()
            && spec_cache_state_modified_for_addr(&caches[i], addr)
            && spec_cache_state_modified_for_addr(&caches[j], addr) ==> i == j
}

pub proof fn lemma_invalidated_peer_preserves_two_cpu_msi_invariant(
    writer_line: CacheLine,
    peer_line: CacheLine,
    addr: usize,
    config: CacheConfig,
)
    requires
        !spec_line_modified_for_addr(peer_line, addr, config),
    ensures
        spec_at_most_one_modified_two(writer_line, peer_line, addr, config),
{
}

pub open spec fn spec_store_commit_line(
    line: CacheLine,
    is_writer: bool,
    addr: usize,
    config: CacheConfig,
) -> CacheLine {
    if is_writer {
        CacheLine {
            tag: spec_cache_tag(addr, config),
            valid: true,
            dirty: true,
            msi: MsiState::Modified,
        }
    } else if line.valid && line.tag == spec_cache_tag(addr, config) {
        CacheLine {
            tag: line.tag,
            valid: false,
            dirty: false,
            msi: MsiState::Invalid,
        }
    } else {
        line
    }
}

pub proof fn lemma_two_cpu_store_commit_preserves_msi_invariant(
    cpu0_line_before: CacheLine,
    cpu1_line_before: CacheLine,
    writer_cpu: nat,
    addr: usize,
    config: CacheConfig,
)
    requires
        writer_cpu < 2,
    ensures
        spec_at_most_one_modified(
            seq![
                spec_store_commit_line(cpu0_line_before, writer_cpu == 0, addr, config),
                spec_store_commit_line(cpu1_line_before, writer_cpu == 1, addr, config),
            ],
            addr,
            config,
        ),
{
    if writer_cpu == 0 {
        assert(!spec_line_modified_for_addr(
            spec_store_commit_line(cpu1_line_before, false, addr, config),
            addr,
            config,
        ));
        lemma_invalidated_peer_preserves_two_cpu_msi_invariant(
            spec_store_commit_line(cpu0_line_before, true, addr, config),
            spec_store_commit_line(cpu1_line_before, false, addr, config),
            addr,
            config,
        );
    } else {
        assert(writer_cpu == 1);
        assert(!spec_line_modified_for_addr(
            spec_store_commit_line(cpu0_line_before, false, addr, config),
            addr,
            config,
        ));
        lemma_invalidated_peer_preserves_two_cpu_msi_invariant(
            spec_store_commit_line(cpu1_line_before, true, addr, config),
            spec_store_commit_line(cpu0_line_before, false, addr, config),
            addr,
            config,
        );
    }
}

pub open spec fn spec_cache_tag(addr: usize, config: CacheConfig) -> u64 {
    let nlines = if config.line_bytes == 0 {
        0
    } else {
        config.capacity_bytes / config.line_bytes
    };
    if config.line_bytes == 0 || nlines == 0 {
        0
    } else {
        (addr / config.line_bytes / nlines) as u64
    }
}

pub open spec fn spec_cache_index(addr: usize, config: CacheConfig) -> usize {
    let nlines = if config.line_bytes == 0 {
        0
    } else {
        config.capacity_bytes / config.line_bytes
    };
    if config.line_bytes == 0 || nlines == 0 {
        0
    } else {
        (addr / config.line_bytes) % nlines
    }
}

pub fn cache_index(addr: usize, config: CacheConfig) -> (ret: usize)
    ensures
        ret == spec_cache_index(addr, config),
{
    let nlines = if config.line_bytes == 0 {
        0
    } else {
        config.capacity_bytes / config.line_bytes
    };
    if config.line_bytes == 0 || nlines == 0 {
        0
    } else {
        (addr / config.line_bytes) % nlines
    }
}

pub fn cache_tag(addr: usize, config: CacheConfig) -> (ret: u64)
    ensures
        ret == spec_cache_tag(addr, config),
{
    let nlines = if config.line_bytes == 0 {
        0
    } else {
        config.capacity_bytes / config.line_bytes
    };
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
