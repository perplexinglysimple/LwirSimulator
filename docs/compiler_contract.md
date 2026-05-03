# VLIW Compiler/Scheduler Contract

This document defines the **minimum legality contract** that the compiler must
satisfy before the simulator will accept and execute a `.vliw` program.

The intent is to make compiler bring-up unambiguous: if a bundle stream
violates any rule below, it is outside contract and may be rejected by the
simulator (`bundle_is_legal`), by the static `vliw_verify` CLI, or by the
multi-CPU `verifier::verify_system` entry point.

## Scope

- Contract applies at the **scheduled bundle level** (after instruction
  selection and bundling).
- Slot legality is decided by the program's `.processor { ... }` header — each
  slot's accepted opcode set is the union of opcodes its declared hardware
  units can execute. The historical `I, I, M, X` four-slot pattern is one
  concrete layout, not a hard rule. See `docs/vliw_asm_format.md` and
  `docs/processor_layout_plan.md`.

## Required compiler guarantees

### 1) Bundle width is valid

The emitted program must target a valid bundle width:

- any integer in `1 ..= 256`

**Why:** simulator data structures and issuance rules assume widths in this range.

**Enforcement mapping:**
- `.processor { width N }` is required and must use a supported value.
- `Bundle::nop_bundle(width)` carries `is_valid_width` as a precondition.
- `ProcessorLayout::validate` rejects malformed layouts before simulation or
  static verification.

### 2) Each slot contains only opcodes its declared units can execute

For every active syllable in slot `s`, at least one unit listed in
`layout.slots[s].units` must execute the opcode (per
`unit_kind_executes`). `nop` is legal in any slot.

A multi-unit slot like `{ ctrl, mul }` accepts any opcode that *either* unit
can execute; dispatch picks the first listed unit that matches.

**Enforcement mapping:**
- Runtime check: `CpuState::bundle_is_legal` calls `layout.slot_can_execute`.
- Static verifier rule: `slot-opcode-legality`.

### 3) No same-bundle RAW hazards

Within one bundle, a later active syllable must not read a GPR written by an
earlier active syllable in the same bundle.

Formally, for active `i < j`, if `slot i` writes `rD (rD != r0)`, including
`call`'s implicit write to `r31`, then `slot j` must not use `rD` as
`src0/src1` (and `ret` must not read an updated link register in the same
bundle).

**Enforcement mapping:**
- Runtime check: `CpuState::bundle_is_legal` pairwise hazard scan (active
  syllables only).
- Static verifier rule: `same-bundle-gpr-raw`. Conservative — guard predicates
  are not evaluated. Every non-`nop` syllable is treated as unconditionally
  active, so the check applies to all pairs regardless of guards. Programs
  with complementary predicated writes to the same destination
  (e.g., `[p1] mov rD, ... | [!p1] mov rD, ...`) are accepted by the simulator
  but rejected by the static verifier.

### 4) No same-bundle WAW hazards

Within one bundle, two active syllables must not both write the same
architectural destination in the same cycle.

- GPR WAW forbidden (`rD`, excluding architecturally hardwired `r0`).
- Predicate WAW forbidden (`pD`, excluding `p0` constant-true convention).

**Enforcement mapping:**
- Runtime check: `CpuState::bundle_is_legal` rejects same-destination writes
  (active syllables only).
- Static verifier rules: `same-bundle-gpr-waw`, `same-bundle-pred-hazard`.
  Conservative — same-destination writes are rejected for all syllable pairs
  regardless of guard predicates. See rule 3 note.

### 5) No same-bundle predicate hazards

Within one bundle, a later active syllable must not read a predicate written
by an earlier active syllable, and must not co-write that predicate.

Includes:
- predicate logic source reads (`p_and`, `p_or`, `p_xor`, `p_not`)
- branch predicate operand reads

**Enforcement mapping:**
- Runtime check: `CpuState::bundle_is_legal` checks predicate RAW/WAW (active
  syllables only).
- Static verifier rule: `same-bundle-pred-hazard`. Conservative — guard
  predicates are not evaluated. See rule 3 note.

### 6) GPR reads only occur after producer ready cycle

Cross-bundle scoreboarding rule:

For every active GPR read in current bundle at cycle `C`, source register `rS`
must satisfy:

- `scoreboard[rS].ready_cycle <= C + 1`

Producer ready cycles use:

- the cache-derived worst-case load latency for `LoadB/H/W/D` and `AcqLoad`:
  `(cpus − 1) × 1 + cache.miss_latency + cache.writeback_latency
  + coherence_drain(layout)`,
- the `LatencyTable` lookup for every other GPR-writing opcode.

If a runtime bundle is issued before the producer is ready, the simulator
stalls one cycle and re-issues; the static verifier emits `gpr-ready-cycle`
because the schedule is not stall-free under the configured worst case.

**Compiler-side contract requirement:** scheduled code must avoid persistent
unready dependencies that would deadlock or violate intended throughput; legal
schedules should naturally satisfy readiness at issue time.

**Enforcement mapping:**
- Runtime dynamic check: `CpuState::bundle_has_unready_gpr_sources`.
- Static verifier rule: `gpr-ready-cycle` (uses the worst-case load latency
  derived from the layout's cache and topology).

### 7) Memory ops respect bus ownership (multi-CPU only)

In a layout with `topology { cpus N }`, cycle `c` is owned by CPU
`c % N`. A memory opcode (any load, store, `acqload`, `relstore`) scheduled by
CPU `k` on a cycle whose owner is not `k` is rejected.

This rule exists so the bus arbiter never has to stall a memory op for
contention — losers are not delayed, they are illegal.

**Enforcement mapping:**
- Runtime check: `System::new` rejects programs whose first off-slot memory op
  is detected by `first_bus_slot_conflict`.
- Static verifier rule: `bus-slot-conflict`.

### 8) Cross-CPU polling loops must have a matching producer

A backward branch over a body containing `acqload` must have at least one
other CPU that issues a `relstore` somewhere in its program. Without a
producer, the consumer's polling loop has no statically bounded termination
and is rejected.

**Enforcement mapping:**
- Static verifier rule: `unbounded-polling-loop`. Implemented by
  `verify_system` (multi-CPU). The single-CPU `vliw_verify` CLI does not run
  this pass.

---

## Rule-to-check matrix

| Contract rule | Simulator check | Static verifier rule |
|---|---|---|
| Valid bundle width | `.processor` parse + bundle invariants | parse failure (exit 2) |
| Slot opcode legality | `bundle_is_legal` via `layout.slot_can_execute` | `slot-opcode-legality` |
| Same-bundle GPR RAW | `bundle_is_legal` | `same-bundle-gpr-raw` |
| Same-bundle GPR WAW | `bundle_is_legal` | `same-bundle-gpr-waw` |
| Same-bundle predicate hazards | `bundle_is_legal` | `same-bundle-pred-hazard` |
| GPR ready-cycle rule | `bundle_has_unready_gpr_sources` (dynamic stall) | `gpr-ready-cycle` (cache-/coherence-aware bound) |
| Bus ownership for memory ops | `System::new` rejection | `bus-slot-conflict` |
| Bounded `acqload` polling | (system rejection at `verify_system`) | `unbounded-polling-loop` |

## Practical definition of a legal `.vliw` program

A program is legal for simulator acceptance if:

1. It parses successfully with a valid `.processor { ... }` layout.
2. Every bundle satisfies slot legality and same-bundle hazard rules under the
   declared layout.
3. The schedule is stall-free under the layout's worst-case load latency.
4. (Multi-CPU only.) Every memory op is on its CPU's bus slot, and every
   `acqload` polling loop has a matching producer `relstore`.
5. At runtime, when a bundle is issued, each active GPR source is
   scoreboard-ready (otherwise the machine stalls until ready, and the
   schedule was outside the static contract from rule 6).

That is the compiler/simulator contract through Stage 4D bring-up.
