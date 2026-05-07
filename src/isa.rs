/// Instruction Set Architecture for the VLIW processor.
///
/// Bundle width is declared by the runtime processor layout in [4, 8, 16, 32, 64, 128, 256].
/// Slot legality is declared by the runtime processor layout.
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

/// An opcode for the VLIW ISA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opcode {
    // Integer slot ops
    /// `dst = src0 + src1` with wrapping arithmetic.
    Add,
    /// `dst = src0 + imm` with wrapping arithmetic.
    AddImm,
    /// `dst = src0 - src1` with wrapping arithmetic.
    Sub,
    /// `dst = src0 - imm` with wrapping arithmetic.
    SubImm,
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
    /// Acquire-ordered 8-byte load. Semantically a `LoadD` with the bus total-order
    /// visibility guarantee: the value read reflects all `RelStore`s that committed
    /// before this CPU's bus slot.
    AcqLoad,
    /// Release-ordered 8-byte store. Semantically a `StoreD` with the bus total-order
    /// visibility guarantee: the written value is visible to all subsequent `AcqLoad`s
    /// on other CPUs within `worst_case_visibility(layout)` cycles.
    RelStore,
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
    /// Placeholder FP32 add over GPR bit patterns.
    FpAdd32,
    /// Placeholder FP32 subtract over GPR bit patterns.
    FpSub32,
    /// Placeholder FP32 multiply over GPR bit patterns.
    FpMul32,
    /// Placeholder FP32 divide over GPR bit patterns.
    FpDiv32,
    /// Placeholder FP32 compare over GPR bit patterns.
    FpCmp32,
    /// Placeholder FP32-to-FP64 conversion over GPR bit patterns.
    FpCvt32To64,
    /// Placeholder signed-int32-to-FP32 conversion over GPR bit patterns.
    FpCvtI32ToFp32,
    /// Placeholder FP32-to-signed-int32 conversion over GPR bit patterns.
    FpCvtFp32ToI32,
    /// Placeholder FP64 add over GPR bit patterns.
    FpAdd64,
    /// Placeholder FP64 subtract over GPR bit patterns.
    FpSub64,
    /// Placeholder FP64 multiply over GPR bit patterns.
    FpMul64,
    /// Placeholder FP64 divide over GPR bit patterns.
    FpDiv64,
    /// Placeholder FP64 compare over GPR bit patterns.
    FpCmp64,
    /// Placeholder FP64-to-FP32 conversion over GPR bit patterns.
    FpCvt64To32,
    /// Placeholder signed-int64-to-FP64 conversion over GPR bit patterns.
    FpCvtI64ToFp64,
    /// Placeholder FP64-to-signed-int64 conversion over GPR bit patterns.
    FpCvtFp64ToI64,
    /// Placeholder AES encrypt round over GPR bit patterns.
    AesEnc,
    /// Placeholder AES decrypt round over GPR bit patterns.
    AesDec,
    // Universal
    /// Do nothing.
    Nop,
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
            Opcode::Add
            | Opcode::AddImm
            | Opcode::Sub
            | Opcode::SubImm
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Shl
            | Opcode::Srl
            | Opcode::Sra
            | Opcode::Mov
            | Opcode::MovImm
            | Opcode::Mul
            | Opcode::MulH
            | Opcode::Lea
            | Opcode::LoadB
            | Opcode::LoadH
            | Opcode::LoadW
            | Opcode::LoadD
            | Opcode::AcqLoad
            | Opcode::FpAdd32
            | Opcode::FpSub32
            | Opcode::FpMul32
            | Opcode::FpDiv32
            | Opcode::FpCvt32To64
            | Opcode::FpCvtI32ToFp32
            | Opcode::FpCvtFp32ToI32
            | Opcode::FpAdd64
            | Opcode::FpSub64
            | Opcode::FpMul64
            | Opcode::FpDiv64
            | Opcode::FpCvt64To32
            | Opcode::FpCvtI64ToFp64
            | Opcode::FpCvtFp64ToI64
            | Opcode::AesEnc
            | Opcode::AesDec
            | Opcode::Call => true,
            Opcode::CmpEq
            | Opcode::CmpLt
            | Opcode::CmpUlt
            | Opcode::FpCmp32
            | Opcode::FpCmp64
            | Opcode::StoreB
            | Opcode::StoreH
            | Opcode::StoreW
            | Opcode::StoreD
            | Opcode::RelStore
            | Opcode::Prefetch
            | Opcode::Branch
            | Opcode::Jump
            | Opcode::Ret
            | Opcode::PAnd
            | Opcode::POr
            | Opcode::PXor
            | Opcode::PNot
            | Opcode::Nop => false,
        }
    }

    /// Returns true if this opcode writes a predicate register destination.
    /// Exhaustive match: adding a new opcode to the enum forces an update here.
    pub fn writes_pred(self) -> bool {
        match self {
            Opcode::CmpEq
            | Opcode::CmpLt
            | Opcode::CmpUlt
            | Opcode::FpCmp32
            | Opcode::FpCmp64
            | Opcode::PAnd
            | Opcode::POr
            | Opcode::PXor
            | Opcode::PNot => true,
            Opcode::Add
            | Opcode::AddImm
            | Opcode::Sub
            | Opcode::SubImm
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Shl
            | Opcode::Srl
            | Opcode::Sra
            | Opcode::Mov
            | Opcode::MovImm
            | Opcode::Mul
            | Opcode::MulH
            | Opcode::Lea
            | Opcode::LoadB
            | Opcode::LoadH
            | Opcode::LoadW
            | Opcode::LoadD
            | Opcode::AcqLoad
            | Opcode::FpAdd32
            | Opcode::FpSub32
            | Opcode::FpMul32
            | Opcode::FpDiv32
            | Opcode::FpCvt32To64
            | Opcode::FpCvtI32ToFp32
            | Opcode::FpCvtFp32ToI32
            | Opcode::FpAdd64
            | Opcode::FpSub64
            | Opcode::FpMul64
            | Opcode::FpDiv64
            | Opcode::FpCvt64To32
            | Opcode::FpCvtI64ToFp64
            | Opcode::FpCvtFp64ToI64
            | Opcode::AesEnc
            | Opcode::AesDec
            | Opcode::StoreB
            | Opcode::StoreH
            | Opcode::StoreW
            | Opcode::StoreD
            | Opcode::RelStore
            | Opcode::Prefetch
            | Opcode::Branch
            | Opcode::Jump
            | Opcode::Call
            | Opcode::Ret
            | Opcode::Nop => false,
        }
    }

    /// Returns true if this opcode reads predicate registers as ALU sources
    /// (as opposed to using a predicate only as a guard).
    /// Exhaustive match: adding a new opcode to the enum forces an update here.
    pub fn reads_pred_src(self) -> bool {
        match self {
            Opcode::PAnd | Opcode::POr | Opcode::PXor | Opcode::PNot => true,
            Opcode::Add
            | Opcode::AddImm
            | Opcode::Sub
            | Opcode::SubImm
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Shl
            | Opcode::Srl
            | Opcode::Sra
            | Opcode::Mov
            | Opcode::MovImm
            | Opcode::CmpEq
            | Opcode::CmpLt
            | Opcode::CmpUlt
            | Opcode::Mul
            | Opcode::MulH
            | Opcode::Lea
            | Opcode::LoadB
            | Opcode::LoadH
            | Opcode::LoadW
            | Opcode::LoadD
            | Opcode::AcqLoad
            | Opcode::FpAdd32
            | Opcode::FpSub32
            | Opcode::FpMul32
            | Opcode::FpDiv32
            | Opcode::FpCmp32
            | Opcode::FpCvt32To64
            | Opcode::FpCvtI32ToFp32
            | Opcode::FpCvtFp32ToI32
            | Opcode::FpAdd64
            | Opcode::FpSub64
            | Opcode::FpMul64
            | Opcode::FpDiv64
            | Opcode::FpCmp64
            | Opcode::FpCvt64To32
            | Opcode::FpCvtI64ToFp64
            | Opcode::FpCvtFp64ToI64
            | Opcode::AesEnc
            | Opcode::AesDec
            | Opcode::StoreB
            | Opcode::StoreH
            | Opcode::StoreW
            | Opcode::StoreD
            | Opcode::RelStore
            | Opcode::Prefetch
            | Opcode::Branch
            | Opcode::Jump
            | Opcode::Call
            | Opcode::Ret
            | Opcode::Nop => false,
        }
    }
}
