use crate::isa::Opcode;

pub fn placeholder_gpr_result(opcode: Opcode, src0: u64, src1: u64) -> u64 {
    match opcode {
        Opcode::FpAdd32 => {
            let a = src0 as u32;
            let b = src1 as u32;
            a.wrapping_add(b) as u64
        }
        Opcode::FpSub32 => {
            let a = src0 as u32;
            let b = src1 as u32;
            a.wrapping_sub(b) as u64
        }
        Opcode::FpMul32 => {
            let a = src0 as u32;
            let b = src1 as u32;
            a.wrapping_mul(b) as u64
        }
        Opcode::FpDiv32 => {
            let a = src0 as u32;
            let b = src1 as u32;
            if b == 0 {
                0
            } else {
                (a / b) as u64
            }
        }
        Opcode::FpCvt32To64 => src0 as u32 as u64,
        Opcode::FpCvtI32ToFp32 => src0 as u32 as u64,
        Opcode::FpCvtFp32ToI32 => src0 as u32 as i32 as u64,
        Opcode::FpAdd64 => src0.wrapping_add(src1),
        Opcode::FpSub64 => src0.wrapping_sub(src1),
        Opcode::FpMul64 => src0.wrapping_mul(src1),
        Opcode::FpDiv64 => {
            if src1 == 0 {
                0
            } else {
                src0 / src1
            }
        }
        Opcode::FpCvt64To32 => (src0 as u32) as u64,
        Opcode::FpCvtI64ToFp64 => src0,
        Opcode::FpCvtFp64ToI64 => src0,
        Opcode::AesEnc => src0 ^ src1 ^ 0x63,
        Opcode::AesDec => src0 ^ src1 ^ 0x05,
        _ => 0,
    }
}
