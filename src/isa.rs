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
    Integer,   // ADD, SUB, AND, OR, XOR, SHL, SRL, SRA, CMP
    Memory,    // LD, ST, LEA, PREFETCH
    Control,   // MUL, MULH, BR, J, CALL, RET, predicate ops
}

/// An opcode for the LWIR ISA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opcode {
    // Integer slot ops
    Add,
    Sub,
    And,
    Or,
    Xor,
    Shl,
    Srl,
    Sra,
    Mov,
    MovImm,
    CmpEq,
    CmpLt,
    CmpUlt,
    // Memory slot ops
    LoadB,
    LoadH,
    LoadW,
    LoadD,
    StoreB,
    StoreH,
    StoreW,
    StoreD,
    Lea,
    Prefetch,
    // Control/multiply slot ops
    Mul,
    MulH,
    Branch,
    Jump,
    Call,
    Ret,
    PAnd,
    POr,
    PXor,
    PNot,
    // Universal
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
    pub opcode: Opcode,
    /// Destination register index (0..32), or None for stores/branches.
    pub dst: Option<usize>,
    /// Source register indices.
    pub src: [Option<usize>; 2],
    /// Immediate value (sign-extended 64-bit).
    pub imm: i64,
    /// Predicate register index (0 = p0 = always true).
    pub predicate: usize,
    /// If true, execute when predicate is false (negated predicate).
    pub pred_negated: bool,
}

impl Syllable {
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
