/// Instruction Set Architecture for the LWIR VLIW processor.
///
/// Bundle width W is a compile-time const in [4, 8, 16, 32, 64, 128, 256].
/// Slots cycle through slot classes: I (integer), M (memory), X (control/mul).
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

/// Slot class determines which functional unit executes an operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotClass {
    /// Integer ALU slot for arithmetic, logic, moves, and integer compares.
    Integer,
    /// Memory/address slot for loads, stores, address formation, and cache hints.
    Memory,
    /// Control/multiply slot for control flow, predicate logic, and long-latency multiply work.
    Control,
}

/// An opcode for the LWIR ISA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opcode {
    // Integer slot ops
    /// `dst = src0 + src1` with wrapping arithmetic.
    Add,
    /// `dst = src0 - src1` with wrapping arithmetic.
    Sub,
    /// `dst = src0 & src1`.
    And,
    /// `dst = src0 | src1`.
    Or,
    /// `dst = src0 ^ src1`.
    Xor,
    /// Logical left shift by the low 6 bits of `src1`.
    Shl,
    /// Logical right shift by the low 6 bits of `src1`.
    Srl,
    /// Arithmetic right shift by the low 6 bits of `src1`.
    Sra,
    /// Copy `src0` into `dst`.
    Mov,
    /// Copy the immediate into `dst`.
    MovImm,
    /// Compare `src0 == src1` and write the boolean result to a predicate register.
    CmpEq,
    /// Signed compare `src0 < src1` and write the boolean result to a predicate register.
    CmpLt,
    /// Unsigned compare `src0 < src1` and write the boolean result to a predicate register.
    CmpUlt,
    // Memory slot ops
    /// Load one byte from memory into `dst`.
    LoadB,
    /// Load two bytes from memory into `dst`.
    LoadH,
    /// Load four bytes from memory into `dst`.
    LoadW,
    /// Load eight bytes from memory into `dst`.
    LoadD,
    /// Store the low byte of `src1` to memory at `src0 + imm`.
    StoreB,
    /// Store the low 16 bits of `src1` to memory at `src0 + imm`.
    StoreH,
    /// Store the low 32 bits of `src1` to memory at `src0 + imm`.
    StoreW,
    /// Store all 64 bits of `src1` to memory at `src0 + imm`.
    StoreD,
    /// Compute an effective address: `dst = src0 + imm`.
    Lea,
    /// Non-binding cache hint with no architectural state change.
    Prefetch,
    // Control/multiply slot ops
    /// Low 64 bits of `src0 * src1`.
    Mul,
    /// High 64 bits of the 128-bit product `src0 * src1`.
    MulH,
    /// Predicated branch to `imm`.
    Branch,
    /// Unconditional jump to `imm`.
    Jump,
    /// Save the current bundle PC to the link register and jump to `imm`.
    Call,
    /// Return to the link register, or halt if the link register is zero.
    Ret,
    /// Predicate-register AND.
    PAnd,
    /// Predicate-register OR.
    POr,
    /// Predicate-register XOR.
    PXor,
    /// Predicate-register NOT.
    PNot,
    // Universal
    /// Do nothing.
    Nop,
}

/// Spec: maps each opcode to its slot class.
pub open spec fn spec_slot_class(op: Opcode) -> SlotClass {
    match op {
        Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
        | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
        | Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt | Opcode::Nop
            => SlotClass::Integer,
        Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
        | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
        | Opcode::Lea | Opcode::Prefetch
            => SlotClass::Memory,
        Opcode::Mul | Opcode::MulH | Opcode::Branch | Opcode::Jump
        | Opcode::Call | Opcode::Ret | Opcode::PAnd | Opcode::POr
        | Opcode::PXor | Opcode::PNot
            => SlotClass::Control,
    }
}

impl Opcode {
    /// Default slot class for this opcode.
    /// Postcondition: result exactly matches the spec for every opcode.
    pub fn slot_class(self) -> (ret: SlotClass)
        ensures ret == spec_slot_class(self),
    {
        match self {
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt => SlotClass::Integer,

            Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Lea | Opcode::Prefetch => SlotClass::Memory,

            Opcode::Mul | Opcode::MulH | Opcode::Branch | Opcode::Jump
            | Opcode::Call | Opcode::Ret | Opcode::PAnd | Opcode::POr
            | Opcode::PXor | Opcode::PNot => SlotClass::Control,

            Opcode::Nop => SlotClass::Integer,
        }
    }
}

/// A single syllable (one slot's worth of instruction) in a bundle.
#[derive(Clone, Debug)]
pub struct Syllable {
    /// Operation to execute in this slot.
    pub opcode: Opcode,
    /// Destination architectural register.
    /// For GPR-writing ops this names a general-purpose register.
    /// For compare/predicate ops this names a predicate register.
    /// `None` is used for instructions with no register destination.
    pub dst: Option<usize>,
    /// Up to two source operands.
    /// `src[0]` is typically the left operand or base register.
    /// `src[1]` is typically the right operand or store-data register.
    pub src: [Option<usize>; 2],
    /// Immediate payload used for literals, displacements, and branch/jump targets.
    pub imm: i64,
    /// Guard predicate index.
    /// `0` means the always-true architectural predicate `p0`.
    pub predicate: usize,
    /// If set, execute when the guard predicate is false instead of true.
    pub pred_negated: bool,
}

impl Syllable {
    /// Construct a canonical no-op syllable.
    /// Postconditions: every field of the NOP syllable is exactly specified.
    pub fn nop() -> (ret: Self)
        ensures
            ret.opcode == Opcode::Nop,
            ret.dst   == Option::<usize>::None,
            ret.src[0] == Option::<usize>::None,
            ret.src[1] == Option::<usize>::None,
            ret.imm == 0i64,
            ret.predicate == 0usize,
            ret.pred_negated == false,
    {
        Syllable {
            opcode: Opcode::Nop,
            dst: None,
            src: [None, None],
            imm: 0,
            predicate: 0,
            pred_negated: false,
        }
    }
}

/// Attach a spec to the derived PartialEq for Opcode so Verus can reason
/// about `==` comparisons in exec code (e.g. latency table lookups).
pub assume_specification[<Opcode as core::cmp::PartialEq>::eq]
    (_0: &Opcode, _1: &Opcode) -> (ret: bool)
    ensures ret == (*_0 == *_1),
;

} // verus!

impl Opcode {
    /// Returns true if this opcode writes a GPR destination, including implicit writes.
    /// Exhaustive match: adding a new opcode to the enum forces an update here.
    pub fn writes_gpr(self) -> bool {
        match self {
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::Mul | Opcode::MulH | Opcode::Lea
            | Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::Call => true,
            Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Prefetch
            | Opcode::Branch | Opcode::Jump | Opcode::Ret
            | Opcode::PAnd | Opcode::POr | Opcode::PXor | Opcode::PNot
            | Opcode::Nop => false,
        }
    }

    /// Returns true if this opcode writes a predicate register destination.
    /// Exhaustive match: adding a new opcode to the enum forces an update here.
    pub fn writes_pred(self) -> bool {
        match self {
            Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt
            | Opcode::PAnd | Opcode::POr | Opcode::PXor | Opcode::PNot => true,
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::Mul | Opcode::MulH | Opcode::Lea
            | Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Prefetch
            | Opcode::Branch | Opcode::Jump | Opcode::Call | Opcode::Ret
            | Opcode::Nop => false,
        }
    }

    /// Returns true if this opcode reads predicate registers as ALU sources
    /// (as opposed to using a predicate only as a guard).
    /// Exhaustive match: adding a new opcode to the enum forces an update here.
    pub fn reads_pred_src(self) -> bool {
        match self {
            Opcode::PAnd | Opcode::POr | Opcode::PXor | Opcode::PNot => true,
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt
            | Opcode::Mul | Opcode::MulH | Opcode::Lea
            | Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Prefetch
            | Opcode::Branch | Opcode::Jump | Opcode::Call | Opcode::Ret
            | Opcode::Nop => false,
        }
    }
}
