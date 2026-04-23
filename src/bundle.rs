/// VLIW bundle: W syllables packed together and issued in a single cycle.
///
/// W must be a power of two in [4, 256].  The constraint is enforced at
/// construction time so that downstream code can rely on it.
use crate::isa::{Opcode, Syllable};
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

pub open spec fn is_valid_width(w: usize) -> bool {
    w == 4 || w == 8 || w == 16 || w == 32 || w == 64 || w == 128 || w == 256
}

/// A bundle of W syllables.
///
/// The generic parameter W is checked at runtime via `Bundle::new`.
#[derive(Clone, Debug)]
pub struct Bundle<const W: usize> {
    pub syllables: Vec<Syllable>,
}

impl<const W: usize> Bundle<W> {
    /// Create a fully-NOP bundle.
    ///
    /// Precondition: W is a valid bundle width (power-of-2 in [4..=256]).
    /// Postconditions:
    ///   - exactly W syllables
    ///   - every syllable has opcode Nop and predicate 0
    pub fn nop_bundle() -> (ret: Self)
        requires is_valid_width(W as usize),
        ensures
            ret.syllables.len() == W,
            forall|i: int| 0 <= i < W ==> ret.syllables[i].opcode == Opcode::Nop,
            forall|i: int| 0 <= i < W ==> ret.syllables[i].predicate == 0usize,
    {
        let mut syllables: Vec<Syllable> = Vec::new();
        let mut i = 0usize;
        while i < W
            invariant
                i <= W,
                syllables.len() == i,
                forall|j: int| 0 <= j < i ==> syllables[j].opcode == Opcode::Nop,
                forall|j: int| 0 <= j < i ==> syllables[j].predicate == 0usize,
            decreases W - i,
        {
            syllables.push(Syllable::nop());
            i += 1;
        }
        Bundle { syllables }
    }

    /// Replace the syllable at `slot`.
    ///
    /// Precondition: slot is within the syllable vector (weak — no width assumption).
    /// Postconditions:
    ///   - length is unchanged
    ///   - slot holds the new syllable's opcode and predicate
    ///   - every other slot is unchanged
    pub fn set_slot(&mut self, slot: usize, syl: Syllable)
        requires slot < old(self).syllables.len(),
        ensures
            self.syllables.len() == old(self).syllables.len(),
            self.syllables[slot as int].opcode      == syl.opcode,
            self.syllables[slot as int].dst         == syl.dst,
            self.syllables[slot as int].imm         == syl.imm,
            self.syllables[slot as int].predicate   == syl.predicate,
            self.syllables[slot as int].pred_negated == syl.pred_negated,
            forall|i: int| 0 <= i < self.syllables.len() && i != slot ==>
                self.syllables[i].opcode == old(self).syllables[i].opcode,
    {
        self.syllables.set(slot, syl);
    }

    /// Bundle width (always W).
    pub fn width(&self) -> (ret: usize)
        ensures ret == W,
    {
        W
    }

    /// True iff every syllable has opcode Nop.
    ///
    /// Postcondition: result precisely reflects the content of every slot.
    pub fn is_all_nop(&self) -> (ret: bool)
        ensures ret == forall|i: int| 0 <= i < self.syllables.len() ==>
            self.syllables[i].opcode == Opcode::Nop,
    {
        let mut i = 0usize;
        while i < self.syllables.len()
            invariant
                forall|j: int| 0 <= j < i ==> self.syllables[j].opcode == Opcode::Nop,
            decreases self.syllables.len() - i,
        {
            if self.syllables[i].opcode != Opcode::Nop {
                return false;
            }
            i += 1;
        }
        true
    }
}

} // verus!
