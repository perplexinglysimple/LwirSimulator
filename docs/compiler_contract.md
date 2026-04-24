# LWIR Compiler/Scheduler Contract

This document defines the **minimum legality contract** that the compiler must satisfy before the simulator will accept and execute a `.lwir` / `.lwirasm` program.

The intent is to make compiler bring-up unambiguous: if a bundle stream violates any rule below, it is outside contract and may be rejected (halted) by the simulator or by an upcoming offline verifier.

## Scope

- Contract applies at the **scheduled bundle level** (after instruction selection and bundling).
- The simulator model is a repeating 4-slot class pattern by slot index:
  - `0 -> I0 (Integer)`
  - `1 -> I1 (Integer)`
  - `2 -> M  (Memory)`
  - `3 -> X  (Control)`
  - then repeats every 4 slots for wider bundles.

## Required compiler guarantees

### 1) Bundle width is valid

The emitted program must target a valid bundle width from this set:

- `4, 8, 16, 32, 64, 128, 256`

**Why:** simulator data structures and issuance rules assume these widths.

**Enforcement mapping:**
- Existing check: source-level `.width` must match parser instantiation width (`parse_program::<W>`). If mismatched, parse fails.
- Existing invariant: `Bundle<W>` is only intended for valid widths via `is_valid_width` and `nop_bundle` precondition.
- Planned verifier check: reject programs that declare unsupported width before simulator instantiation.

### 2) Each slot contains only legal opcodes for that slot class

For every active syllable in slot `s`, `opcode.slot_class()` must equal the architectural class for slot `s`.

- Integer-only classes in `I*` slots.
- Memory-only classes in `M*` slots.
- Control/predicate/multiply classes in `X*` slots.
- `nop` is legal anywhere (treated as Integer class but semantically inert; scheduler should still place legal opcodes per slot class).

**Enforcement mapping:**
- Existing simulator check: `CpuState::bundle_is_legal` rejects active slot/opcode class mismatches.
- Planned verifier check: static pass over all bundles.

### 3) No same-bundle RAW hazards

Within one bundle, a later active syllable must not read a GPR written by an earlier active syllable in the same bundle.

Formally, for active `i < j`, if `slot i` writes `rD (rD != r0)`, then `slot j` must not use `rD` as `src0/src1` (and `ret` must not read updated link register in same bundle).

**Enforcement mapping:**
- Existing simulator check: `CpuState::bundle_is_legal` pairwise hazard scan.
- Planned verifier check: same pairwise static rule.

### 4) No same-bundle WAW hazards

Within one bundle, two active syllables must not both write the same architectural destination in the same cycle.

- GPR WAW forbidden (`rD`, excluding architecturally hardwired `r0`).
- Predicate WAW forbidden (`pD`, excluding `p0` constant-true convention).

**Enforcement mapping:**
- Existing simulator check: `CpuState::bundle_is_legal` rejects same-destination writes.
- Planned verifier check: bundle-local destination uniqueness by register class.

### 5) No same-bundle predicate hazards

Within one bundle, a later active syllable must not read a predicate written by an earlier active syllable, and must not co-write that predicate.

Includes:
- predicate logic source reads (`p_and`, `p_or`, `p_xor`, `p_not`)
- branch predicate operand reads

**Enforcement mapping:**
- Existing simulator check: `CpuState::bundle_is_legal` checks predicate RAW/WAW.
- Planned verifier check: explicit predicate def-use graph per bundle.

### 6) GPR reads only occur after producer ready cycle

Cross-bundle scoreboarding rule:

For every active GPR read in current bundle at cycle `C`, source register `rS` must satisfy:

- `scoreboard[rS].ready_cycle <= C + 1`

If not ready, the bundle is not executable this cycle (simulator stalls one cycle).

**Compiler-side contract requirement:** scheduled code must avoid persistent unready dependencies that would deadlock or violate intended throughput; legal schedules should naturally satisfy readiness at issue time.

**Enforcement mapping:**
- Existing simulator dynamic check: `CpuState::bundle_has_unready_gpr_sources`.
- Planned verifier check: timing-aware schedule validator using configured latencies.

### 7) Control ops obey X-slot restrictions

`branch`, `jump`, `call`, `ret` are only legal in X-class slots.

**Enforcement mapping:**
- Existing simulator check: covered by generic slot-class legality in `bundle_is_legal`.
- Planned verifier check: dedicated diagnostic for control op placement.

### 8) Memory ops obey M-slot restrictions

`load*`, `store*`, `lea`, `prefetch` are only legal in M-class slots.

**Enforcement mapping:**
- Existing simulator check: covered by generic slot-class legality in `bundle_is_legal`.
- Planned verifier check: dedicated diagnostic for memory op placement.

---

## Rule-to-check matrix

| Contract rule | Existing simulator check | Planned verifier check |
|---|---|---|
| Valid bundle width | Parser `.width` agreement and bundle construction invariants | Front-end width gate |
| Slot opcode legality | `bundle_is_legal` slot-class match | Static per-slot class validator |
| Same-bundle GPR RAW | `bundle_is_legal` | Static bundle hazard pass |
| Same-bundle GPR WAW | `bundle_is_legal` | Static bundle hazard pass |
| Same-bundle predicate hazards | `bundle_is_legal` | Static predicate hazard pass |
| GPR ready-cycle rule | `bundle_has_unready_gpr_sources` (dynamic stall) | Latency-aware schedule proof/check |
| Control-in-X restriction | `bundle_is_legal` via slot class | Dedicated control-placement diagnostic |
| Memory-in-M restriction | `bundle_is_legal` via slot class | Dedicated memory-placement diagnostic |

## Practical definition of a legal `.lwir` program

A program is legal for simulator acceptance if:

1. It parses successfully for the chosen width `W`.
2. Every bundle satisfies slot-class and same-bundle hazard rules.
3. At runtime, when a bundle is issued, each active GPR source is scoreboard-ready (otherwise the machine may stall until ready).

That is the compiler/simulator contract for Milestone 1 bring-up.
