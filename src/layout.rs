use crate::bundle::Bundle;
use crate::cache::CacheConfig;
use crate::isa::Opcode;
use builtin::*;
use builtin_macros::*;
use vstd::prelude::*;

verus! {

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpClass {
    GprWriter,
    Compare,
    Store,
    Control,
    PredicateLogic,
    FloatingPoint,
    Aes,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpVariant {
    Fp32,
    Fp64,
    Fp64Fma,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AesVariant {
    AesNi,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnitKind {
    IntegerAlu,
    Memory,
    Control,
    Multiplier,
    Fp(FpVariant),
    Aes(AesVariant),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitDecl {
    pub name: String,
    pub kind: UnitKind,
    pub latency: Option<u32>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotSpec {
    pub units: Vec<String>,
}

pub const DEFAULT_NUM_GPRS: usize = 32;
pub const DEFAULT_NUM_PREDS: usize = 16;
pub const DEFAULT_MEM_SIZE: usize = 65536;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArchConfig {
    pub gprs: usize,
    pub preds: usize,
    pub memory_bytes: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TopologyConfig {
    pub cpus: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProcessorLayout {
    pub width: usize,
    pub units: Vec<UnitDecl>,
    pub slots: Vec<SlotSpec>,
    pub arch: ArchConfig,
    pub cache: CacheConfig,
    pub topology: TopologyConfig,
}

impl ProcessorLayout {
    pub fn validate(&self) -> (ret: bool)
        ensures ret == (layout_well_formed(self) && topology_supported(self) && arch_supported(self)),
    {
        if !is_valid_width_runtime(self.width) {
            return false;
        }
        if self.slots.len() != self.width {
            return false;
        }
        if !topology_supported_runtime(self) { return false; }
        if !arch_supported_runtime(self.arch) { return false; }

        let mut slot = 0usize;
        while slot < self.slots.len()
            invariant
                slot <= self.slots.len(),
                self.slots.len() == self.width,
                crate::bundle::is_valid_width(self.width),
                topology_supported(self),
                arch_supported(self),
                forall|i: int| 0 <= i < slot ==> self.slots[i].units.len() > 0,
                forall|i: int, j: int|
                    0 <= i < slot && 0 <= j < self.slots[i].units.len() ==>
                        unit_name_exists(self, self.slots[i].units[j]@),
            decreases self.slots.len() - slot,
        {
            if self.slots[slot].units.len() == 0 {
                return false;
            }
            let mut unit = 0usize;
            while unit < self.slots[slot].units.len()
                invariant
                    slot < self.slots.len(),
                    unit <= self.slots[slot as int].units.len(),
                    forall|j: int| 0 <= j < unit ==> unit_name_exists(self, self.slots[slot as int].units[j]@),
                decreases self.slots[slot as int].units.len() - unit,
            {
                if !self.unit_name_exists_runtime(&self.slots[slot].units[unit]) {
                    return false;
                }
                unit += 1;
            }
            slot += 1;
        }

        true
    }

    pub fn unit_name_exists_runtime(&self, name: &String) -> (ret: bool)
        ensures ret == unit_name_exists(self, name@),
    {
        let mut i = 0usize;
        while i < self.units.len()
            invariant
                i <= self.units.len(),
                forall|j: int| 0 <= j < i ==> self.units[j].name@ != name@,
            decreases self.units.len() - i,
        {
            if self.units[i].name == *name {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn slot_can_execute(&self, slot: usize, opcode: Opcode) -> (ret: bool)
        ensures ret == layout_slot_accepts_opcode(self, slot as int, opcode),
    {
        if opcode == Opcode::Nop {
            return true;
        }
        if slot >= self.slots.len() {
            return false;
        }

        let mut unit_ref = 0usize;
        while unit_ref < self.slots[slot].units.len()
            invariant
                slot < self.slots.len(),
                opcode != Opcode::Nop,
                unit_ref <= self.slots[slot as int].units.len(),
                forall|j: int, k: int|
                    0 <= j < unit_ref && 0 <= k < self.units.len() ==>
                        !(self.slots[slot as int].units[j]@ == self.units[k].name@
                            && unit_kind_executes(self.units[k].kind, opcode)),
            decreases self.slots[slot as int].units.len() - unit_ref,
        {
            let mut unit = 0usize;
            while unit < self.units.len()
                invariant
                    slot < self.slots.len(),
                    opcode != Opcode::Nop,
                    unit_ref < self.slots[slot as int].units.len(),
                    unit <= self.units.len(),
                    forall|j: int, k: int|
                        0 <= j < unit_ref && 0 <= k < self.units.len() ==>
                            !(self.slots[slot as int].units[j]@ == self.units[k].name@
                                && unit_kind_executes(self.units[k].kind, opcode)),
                    forall|k: int| 0 <= k < unit ==>
                        !(self.slots[slot as int].units[unit_ref as int]@ == self.units[k].name@
                            && unit_kind_executes(self.units[k].kind, opcode)),
                decreases self.units.len() - unit,
            {
                if self.slots[slot].units[unit_ref] == self.units[unit].name
                    && unit_kind_executes_runtime(self.units[unit].kind, opcode)
                {
                    return true;
                }
                unit += 1;
            }
            unit_ref += 1;
        }
        false
    }

}

pub fn default_arch_config() -> ArchConfig {
    ArchConfig {
        gprs: DEFAULT_NUM_GPRS,
        preds: DEFAULT_NUM_PREDS,
        memory_bytes: DEFAULT_MEM_SIZE,
    }
}

pub fn topology_supported_runtime(layout: &ProcessorLayout) -> (ret: bool)
    ensures ret == topology_supported(layout),
{
    layout.topology.cpus >= 1
}

pub fn arch_supported_runtime(arch: ArchConfig) -> (ret: bool)
    ensures ret == arch_supported_config(arch),
{
    arch.gprs >= 32 && arch.preds >= 1 && arch.memory_bytes >= 8
}

pub fn unit_kind_executes_runtime(kind: UnitKind, opcode: Opcode) -> (ret: bool)
    ensures ret == unit_kind_executes(kind, opcode),
{
    match kind {
        UnitKind::IntegerAlu => match opcode {
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt => true,
            _ => false,
        },
        UnitKind::Memory => match opcode {
            Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Lea | Opcode::Prefetch => true,
            _ => false,
        },
        UnitKind::Control => match opcode {
            Opcode::Branch | Opcode::Jump | Opcode::Call | Opcode::Ret
            | Opcode::PAnd | Opcode::POr
            | Opcode::PXor | Opcode::PNot => true,
            _ => false,
        },
        UnitKind::Multiplier => match opcode {
            Opcode::Mul | Opcode::MulH => true,
            _ => false,
        },
        UnitKind::Fp(_) => match opcode {
            Opcode::FpAdd32 | Opcode::FpMul32 | Opcode::FpAdd64 | Opcode::FpMul64 => true,
            _ => false,
        },
        UnitKind::Aes(_) => match opcode {
            Opcode::AesEnc | Opcode::AesDec => true,
            _ => false,
        },
    }
}

pub fn unit_kind_has_class_runtime(kind: UnitKind, class: OpClass) -> (ret: bool)
    ensures ret == unit_kind_has_class(kind, class),
{
    match kind {
        UnitKind::IntegerAlu => match class {
            OpClass::GprWriter | OpClass::Compare | OpClass::PredicateLogic => true,
            _ => false,
        },
        UnitKind::Memory => match class {
            OpClass::GprWriter | OpClass::Store => true,
            _ => false,
        },
        UnitKind::Control => match class {
            OpClass::Control | OpClass::PredicateLogic => true,
            _ => false,
        },
        UnitKind::Multiplier => match class {
            OpClass::GprWriter => true,
            _ => false,
        },
        UnitKind::Fp(_) => match class {
            OpClass::FloatingPoint => true,
            _ => false,
        },
        UnitKind::Aes(_) => match class {
            OpClass::Aes => true,
            _ => false,
        },
    }
}

pub fn unit_kind_default_latency_runtime(kind: UnitKind) -> (ret: u32)
    ensures ret == unit_kind_default_latency(kind),
{
    match kind {
        UnitKind::IntegerAlu => 1u32,
        UnitKind::Memory => 3u32,
        UnitKind::Control => 1u32,
        UnitKind::Multiplier => 3u32,
        UnitKind::Fp(FpVariant::Fp32) => 4u32,
        UnitKind::Fp(FpVariant::Fp64) => 6u32,
        UnitKind::Fp(FpVariant::Fp64Fma) => 6u32,
        UnitKind::Aes(AesVariant::AesNi) => 4u32,
    }
}

pub fn is_valid_width_runtime(width: usize) -> (ret: bool)
    ensures ret == crate::bundle::is_valid_width(width),
{
    width == 4 || width == 8 || width == 16 || width == 32 || width == 64 || width == 128 || width == 256
}

pub open spec fn unit_name_exists(layout: &ProcessorLayout, name: Seq<char>) -> bool {
    exists|i: int| 0 <= i < layout.units.len() && layout.units[i].name@ == name
}

pub open spec fn layout_well_formed(layout: &ProcessorLayout) -> bool {
    &&& crate::bundle::is_valid_width(layout.width)
    &&& layout.slots.len() == layout.width
    &&& forall|i: int| 0 <= i < layout.slots.len() ==> layout.slots[i].units.len() > 0
    &&& forall|i: int, j: int|
        0 <= i < layout.slots.len() && 0 <= j < layout.slots[i].units.len() ==>
            unit_name_exists(layout, layout.slots[i].units[j]@)
}

pub open spec fn topology_supported(layout: &ProcessorLayout) -> bool {
    layout.topology.cpus >= 1
}

pub open spec fn arch_supported(layout: &ProcessorLayout) -> bool {
    arch_supported_config(layout.arch)
}

pub open spec fn arch_supported_config(arch: ArchConfig) -> bool {
    arch.gprs >= 32 && arch.preds >= 1 && arch.memory_bytes >= 8
}

pub open spec fn unit_kind_has_class(kind: UnitKind, class: OpClass) -> bool {
    match kind {
        UnitKind::IntegerAlu => match class {
            OpClass::GprWriter | OpClass::Compare | OpClass::PredicateLogic => true,
            _ => false,
        },
        UnitKind::Memory => match class {
            OpClass::GprWriter | OpClass::Store => true,
            _ => false,
        },
        UnitKind::Control => match class {
            OpClass::Control | OpClass::PredicateLogic => true,
            _ => false,
        },
        UnitKind::Multiplier => match class {
            OpClass::GprWriter => true,
            _ => false,
        },
        UnitKind::Fp(_) => match class {
            OpClass::FloatingPoint => true,
            _ => false,
        },
        UnitKind::Aes(_) => match class {
            OpClass::Aes => true,
            _ => false,
        },
    }
}

pub open spec fn unit_kind_default_latency(kind: UnitKind) -> u32 {
    match kind {
        UnitKind::IntegerAlu => 1u32,
        UnitKind::Memory => 3u32,
        UnitKind::Control => 1u32,
        UnitKind::Multiplier => 3u32,
        UnitKind::Fp(FpVariant::Fp32) => 4u32,
        UnitKind::Fp(FpVariant::Fp64) => 6u32,
        UnitKind::Fp(FpVariant::Fp64Fma) => 6u32,
        UnitKind::Aes(AesVariant::AesNi) => 4u32,
    }
}

pub open spec fn unit_decl_latency(unit: UnitDecl) -> u32 {
    match unit.latency {
        Some(latency) => latency,
        None => unit_kind_default_latency(unit.kind),
    }
}

pub open spec fn unit_kind_executes(kind: UnitKind, opcode: Opcode) -> bool {
    match kind {
        UnitKind::IntegerAlu => match opcode {
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt => true,
            _ => false,
        },
        UnitKind::Memory => match opcode {
            Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Lea | Opcode::Prefetch => true,
            _ => false,
        },
        UnitKind::Control => match opcode {
            Opcode::Branch | Opcode::Jump | Opcode::Call | Opcode::Ret
            | Opcode::PAnd | Opcode::POr
            | Opcode::PXor | Opcode::PNot => true,
            _ => false,
        },
        UnitKind::Multiplier => match opcode {
            Opcode::Mul | Opcode::MulH => true,
            _ => false,
        },
        UnitKind::Fp(_) => match opcode {
            Opcode::FpAdd32 | Opcode::FpMul32 | Opcode::FpAdd64 | Opcode::FpMul64 => true,
            _ => false,
        },
        UnitKind::Aes(_) => match opcode {
            Opcode::AesEnc | Opcode::AesDec => true,
            _ => false,
        },
    }
}

pub open spec fn layout_slot_accepts_opcode(layout: &ProcessorLayout, slot: int, opcode: Opcode) -> bool {
    opcode == Opcode::Nop ||
    exists|j: int, k: int|
        0 <= slot < layout.slots.len() &&
        0 <= j < layout.slots[slot].units.len() &&
        0 <= k < layout.units.len() &&
        layout.slots[slot].units[j]@ == layout.units[k].name@ &&
        unit_kind_executes(layout.units[k].kind, opcode)
}

pub open spec fn canonical_slot_accepts_legacy_units(slot: int, opcode: Opcode) -> bool {
    opcode == Opcode::Nop ||
    if slot % 4 == 0 || slot % 4 == 1 {
        unit_kind_executes(UnitKind::IntegerAlu, opcode)
    } else if slot % 4 == 2 {
        unit_kind_executes(UnitKind::Memory, opcode)
    } else {
        unit_kind_executes(UnitKind::Control, opcode) ||
        unit_kind_executes(UnitKind::Multiplier, opcode)
    }
}

pub open spec fn legacy_slot_accepts_opcode(slot: int, opcode: Opcode) -> bool {
    opcode == Opcode::Nop ||
    if slot % 4 == 0 || slot % 4 == 1 {
        match opcode {
            Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
            | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mov | Opcode::MovImm
            | Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt => true,
            _ => false,
        }
    } else if slot % 4 == 2 {
        match opcode {
            Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD
            | Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD
            | Opcode::Lea | Opcode::Prefetch => true,
            _ => false,
        }
    } else {
        match opcode {
            Opcode::Mul | Opcode::MulH | Opcode::Branch | Opcode::Jump
            | Opcode::Call | Opcode::Ret | Opcode::PAnd | Opcode::POr
            | Opcode::PXor | Opcode::PNot => true,
            _ => false,
        }
    }
}

pub proof fn lemma_canonical_layout_matches_legacy_slot_class()
    ensures
        forall|slot: int, opcode: Opcode| 0 <= slot ==>
            canonical_slot_accepts_legacy_units(slot, opcode) ==
            legacy_slot_accepts_opcode(slot, opcode),
{
    assert forall|slot: int, opcode: Opcode| 0 <= slot implies
        canonical_slot_accepts_legacy_units(slot, opcode) ==
        legacy_slot_accepts_opcode(slot, opcode)
    by {
        match opcode {
            Opcode::Add => {}
            Opcode::Sub => {}
            Opcode::And => {}
            Opcode::Or => {}
            Opcode::Xor => {}
            Opcode::Shl => {}
            Opcode::Srl => {}
            Opcode::Sra => {}
            Opcode::Mov => {}
            Opcode::MovImm => {}
            Opcode::CmpEq => {}
            Opcode::CmpLt => {}
            Opcode::CmpUlt => {}
            Opcode::LoadB => {}
            Opcode::LoadH => {}
            Opcode::LoadW => {}
            Opcode::LoadD => {}
            Opcode::StoreB => {}
            Opcode::StoreH => {}
            Opcode::StoreW => {}
            Opcode::StoreD => {}
            Opcode::Lea => {}
            Opcode::Prefetch => {}
            Opcode::Mul => {}
            Opcode::MulH => {}
            Opcode::Branch => {}
            Opcode::Jump => {}
            Opcode::Call => {}
            Opcode::Ret => {}
            Opcode::PAnd => {}
            Opcode::POr => {}
            Opcode::PXor => {}
            Opcode::PNot => {}
            Opcode::FpAdd32 => {}
            Opcode::FpMul32 => {}
            Opcode::FpAdd64 => {}
            Opcode::FpMul64 => {}
            Opcode::AesEnc => {}
            Opcode::AesDec => {}
            Opcode::Nop => {}
        }
    }
}

pub open spec fn program_layout_compatible(layout: &ProcessorLayout, bundles: &Vec<Bundle>) -> bool {
    forall|b: int, s: int|
        0 <= b < bundles.len() && 0 <= s < bundles[b].syllables.len() ==>
            layout_slot_accepts_opcode(layout, s, bundles[b].syllables[s].opcode)
}

pub fn program_layout_compatible_runtime(layout: &ProcessorLayout, bundles: &Vec<Bundle>) -> (ret: bool)
    ensures ret == program_layout_compatible(layout, bundles),
{
    let mut bundle = 0usize;
    while bundle < bundles.len()
        invariant
            bundle <= bundles.len(),
            forall|b: int, s: int|
                0 <= b < bundle && 0 <= s < bundles[b].syllables.len() ==>
                    layout_slot_accepts_opcode(layout, s, bundles[b].syllables[s].opcode),
        decreases bundles.len() - bundle,
    {
        let mut slot = 0usize;
        while slot < bundles[bundle].syllables.len()
            invariant
                bundle < bundles.len(),
                slot <= bundles[bundle as int].syllables.len(),
                forall|b: int, s: int|
                    0 <= b < bundle && 0 <= s < bundles[b].syllables.len() ==>
                        layout_slot_accepts_opcode(layout, s, bundles[b].syllables[s].opcode),
                forall|s: int| 0 <= s < slot ==>
                    layout_slot_accepts_opcode(
                        layout,
                        s,
                        bundles[bundle as int].syllables[s].opcode,
                    ),
            decreases bundles[bundle as int].syllables.len() - slot,
        {
            if !layout.slot_can_execute(slot, bundles[bundle].syllables[slot].opcode) {
                return false;
            }
            slot += 1;
        }
        bundle += 1;
    }
    true
}

} // verus!

pub fn canonical_layout(width: usize) -> ProcessorLayout {
    let units = vec![
        UnitDecl {
            name: "alu".to_string(),
            kind: UnitKind::IntegerAlu,
            latency: None,
        },
        UnitDecl {
            name: "mem".to_string(),
            kind: UnitKind::Memory,
            latency: None,
        },
        UnitDecl {
            name: "ctrl".to_string(),
            kind: UnitKind::Control,
            latency: None,
        },
        UnitDecl {
            name: "mul".to_string(),
            kind: UnitKind::Multiplier,
            latency: None,
        },
    ];
    let mut slots = Vec::with_capacity(width);
    for slot in 0..width {
        let units = match slot % 4 {
            0 | 1 => vec!["alu".to_string()],
            2 => vec!["mem".to_string()],
            _ => vec!["ctrl".to_string(), "mul".to_string()],
        };
        slots.push(SlotSpec { units });
    }
    ProcessorLayout {
        width,
        units,
        slots,
        arch: default_arch_config(),
        cache: CacheConfig::default_l1d(),
        topology: TopologyConfig { cpus: 1 },
    }
}
