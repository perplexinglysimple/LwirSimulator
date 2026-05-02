use crate::isa::Opcode;

pub fn placeholder_gpr_result(opcode: Opcode, src0: u64, src1: u64) -> u64 {
    match opcode {
        Opcode::FpAdd32 => {
            let a = src0 as u32;
            let b = src1 as u32;
            a.wrapping_add(b) as u64
        }
        Opcode::FpMul32 => {
            let a = src0 as u32;
            let b = src1 as u32;
            a.wrapping_mul(b) as u64
        }
        Opcode::FpAdd64 => src0.wrapping_add(src1),
        Opcode::FpMul64 => src0.wrapping_mul(src1),
        Opcode::AesEnc => src0 ^ src1 ^ 0x63,
        Opcode::AesDec => src0 ^ src1 ^ 0x05,
        _ => 0,
    }
}
