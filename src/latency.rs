/// Configurable per-opcode execution latency table.
use crate::isa::Opcode;
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

// ---------------------------------------------------------------------------
// Spec functions
// ---------------------------------------------------------------------------

/// Index of the first entry in `s[start..]` whose opcode matches `op`, or -1.
pub open spec fn spec_find(s: Seq<(Opcode, u32)>, op: Opcode, start: int) -> int
    decreases s.len() - start
{
    if start >= s.len() {
        -1int
    } else if s[start].0 == op {
        start
    } else {
        spec_find(s, op, start + 1)
    }
}

/// The logical result of a table lookup: first match's latency, or 1 as default.
pub open spec fn spec_get(s: Seq<(Opcode, u32)>, op: Opcode) -> u32 {
    let idx = spec_find(s, op, 0);
    if idx >= 0 { s[idx].1 } else { 1u32 }
}

// ---------------------------------------------------------------------------
// Lemmas
// ---------------------------------------------------------------------------

/// `spec_find` returns an index in `[start, s.len())` or -1.
proof fn lemma_find_bounds(s: Seq<(Opcode, u32)>, op: Opcode, start: int)
    requires 0 <= start <= s.len()
    ensures -1 <= spec_find(s, op, start) < s.len()
    decreases s.len() - start
{
    if start < s.len() && s[start].0 != op {
        lemma_find_bounds(s, op, start + 1);
    }
}

/// When the first match is at index `i`, `spec_find` from `start` returns `i`.
proof fn lemma_find_at(s: Seq<(Opcode, u32)>, op: Opcode, start: int, i: int)
    requires
        0 <= start <= i < s.len(),
        s[i].0 == op,
        forall|j: int| start <= j < i ==> s[j].0 != op,
    ensures spec_find(s, op, start) == i,
    decreases i - start,
{
    if start < i {
        lemma_find_at(s, op, start + 1, i);
    }
}

/// When no entry in `s[start..]` matches `op`, `spec_find` returns -1.
proof fn lemma_find_none(s: Seq<(Opcode, u32)>, op: Opcode, start: int)
    requires
        0 <= start <= s.len(),
        forall|j: int| start <= j < s.len() ==> s[j].0 != op,
    ensures spec_find(s, op, start) == -1,
    decreases s.len() - start,
{
    if start < s.len() {
        lemma_find_none(s, op, start + 1);
    }
}

// ---------------------------------------------------------------------------
// LatencyTable
// ---------------------------------------------------------------------------

/// Cycle latency for each opcode.
#[derive(Clone, Debug)]
pub struct LatencyTable {
    pub entries: Vec<(Opcode, u32)>,
}

impl LatencyTable {
    /// Default latency table matching the FVLIW-64/4 model.
    #[verifier::external_body]
    pub fn default() -> (ret: Self)
        ensures
            spec_get(ret.entries@, Opcode::Add)      == 1u32,
            spec_get(ret.entries@, Opcode::Sub)      == 1u32,
            spec_get(ret.entries@, Opcode::And)      == 1u32,
            spec_get(ret.entries@, Opcode::Or)       == 1u32,
            spec_get(ret.entries@, Opcode::Xor)      == 1u32,
            spec_get(ret.entries@, Opcode::Shl)      == 1u32,
            spec_get(ret.entries@, Opcode::Srl)      == 1u32,
            spec_get(ret.entries@, Opcode::Sra)      == 1u32,
            spec_get(ret.entries@, Opcode::Mov)      == 1u32,
            spec_get(ret.entries@, Opcode::MovImm)   == 1u32,
            spec_get(ret.entries@, Opcode::LoadB)    == 3u32,
            spec_get(ret.entries@, Opcode::LoadH)    == 3u32,
            spec_get(ret.entries@, Opcode::LoadW)    == 3u32,
            spec_get(ret.entries@, Opcode::LoadD)    == 3u32,
            spec_get(ret.entries@, Opcode::Mul)      == 3u32,
            spec_get(ret.entries@, Opcode::MulH)     == 3u32,
            spec_get(ret.entries@, Opcode::Nop)      == 0u32,
    {
        LatencyTable {
            entries: vec![
                (Opcode::Add,      1u32),
                (Opcode::Sub,      1u32),
                (Opcode::And,      1u32),
                (Opcode::Or,       1u32),
                (Opcode::Xor,      1u32),
                (Opcode::Shl,      1u32),
                (Opcode::Srl,      1u32),
                (Opcode::Sra,      1u32),
                (Opcode::Mov,      1u32),
                (Opcode::MovImm,   1u32),
                (Opcode::CmpEq,    1u32),
                (Opcode::CmpLt,    1u32),
                (Opcode::CmpUlt,   1u32),
                (Opcode::LoadB,    3u32),
                (Opcode::LoadH,    3u32),
                (Opcode::LoadW,    3u32),
                (Opcode::LoadD,    3u32),
                (Opcode::StoreB,   1u32),
                (Opcode::StoreH,   1u32),
                (Opcode::StoreW,   1u32),
                (Opcode::StoreD,   1u32),
                (Opcode::Lea,      1u32),
                (Opcode::Prefetch, 1u32),
                (Opcode::Mul,      3u32),
                (Opcode::MulH,     3u32),
                (Opcode::Branch,   1u32),
                (Opcode::Jump,     1u32),
                (Opcode::Call,     1u32),
                (Opcode::Ret,      1u32),
                (Opcode::PAnd,     1u32),
                (Opcode::POr,      1u32),
                (Opcode::PXor,     1u32),
                (Opcode::PNot,     1u32),
                (Opcode::FpAdd32,  4u32),
                (Opcode::FpMul32,  4u32),
                (Opcode::FpAdd64,  6u32),
                (Opcode::FpMul64,  6u32),
                (Opcode::AesEnc,   4u32),
                (Opcode::AesDec,   4u32),
                (Opcode::Nop,      0u32),
            ],
        }
    }

    /// Look up latency for `op`.
    ///
    /// Postconditions (strong):
    ///   - If the first occurrence of `op` in the table is at index `i` with latency `v`,
    ///     then the result is `v`.
    ///   - If `op` is not in the table at all, the result is 1.
    ///   - The result always equals `spec_get(entries, op)`.
    pub fn get(&self, op: Opcode) -> (ret: u32)
        ensures ret == spec_get(self.entries@, op),
    {
        let mut i: usize = 0;
        while i < self.entries.len()
            invariant
                i <= self.entries.len(),
                forall|j: int| 0 <= j < i ==> self.entries@[j].0 != op,
            decreases self.entries.len() - i,
        {
            if self.entries[i].0 == op {
                proof {
                    lemma_find_at(self.entries@, op, 0int, i as int);
                }
                return self.entries[i].1;
            }
            i += 1;
        }
        proof {
            lemma_find_none(self.entries@, op, 0int);
        }
        1u32
    }

    /// Override the latency of `op` to `cycles`.
    ///
    /// Postcondition: after the call, `spec_get(entries, op) == cycles`.
    /// All other entries are either preserved or shadowed (first-match semantics).
    pub fn set(&mut self, op: Opcode, cycles: u32)
        ensures spec_get(self.entries@, op) == cycles,
    {
        let mut i: usize = 0;
        while i < self.entries.len()
            invariant
                i <= self.entries.len(),
                forall|j: int| 0 <= j < i ==> self.entries@[j].0 != op,
            decreases self.entries.len() - i,
        {
            if self.entries[i].0 == op {
                self.entries.set(i, (op, cycles));
                proof {
                    lemma_find_at(self.entries@, op, 0int, i as int);
                }
                return;
            }
            i += 1;
        }
        // Not found: append a new entry.
        let ghost old_len: int = self.entries@.len() as int;
        self.entries.push((op, cycles));
        proof {
            // All entries before old_len don't match op.
            lemma_find_at(self.entries@, op, 0int, old_len);
        }
    }
}

} // verus!
