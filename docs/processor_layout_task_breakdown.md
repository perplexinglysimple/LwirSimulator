# Processor Layout Implementation Task Breakdown

Status: revised implementation breakdown.
Source plan: `docs/processor_layout_plan.md`.

This document records the verified rollout plan after reviewing the proposal
against the current codebase.

## Decisions

- Runtime-width bundles are a hard requirement for stage 0. The const-generic
  `Bundle<W>` / `CpuState<W>` API is removed as part of the first stage rather
  than deferred.
- Sparse layouts are valid. A layout does not need to include every hardware
  unit family as long as the loaded program only uses opcodes executable by the
  declared slot/unit configuration.
- Slot position remains architectural. Legality and dispatch depend on the
  concrete slot index and that slot's declared unit set.
- There is no explicit syllable-level unit selection syntax. If multiple units
  on a slot can execute the same opcode, dispatch chooses the first matching
  unit listed for that slot.
- Opcode placement/capability is separate from opcode side effects. Slot
  classes decide where an opcode may execute; functions like `writes_gpr`,
  `writes_pred`, and predicate-source reads remain opcode semantic facts.

## Verified Plan Corrections

- The current trusted-surface description in the source plan is inaccurate for
  this tree. `parse_program` is not marked `external_body`; `verify_program`
  and `LatencyTable::default` are.
- Stage 0 must include a full runtime-width data-model migration, not only a
  parser/header change.
- `layout_well_formed` should validate layout structure only. Program-specific
  compatibility is a separate check: every non-`nop` opcode used by the program
  must be executable by its scheduled slot.
- Cache miss latency cannot be treated as a semantics-preserving replacement
  for the current default load latency unless the verifier uses the new
  cache-configured worst-case latency. The current default load latency is 3,
  while the proposed miss latency example is 12.
- Stage 4 should be decomposed aggressively. Multi-CPU execution, shared
  memory, bus arbitration, synchronization opcodes, and MSI coherence should
  not land as one PR.

## Stage 0: Runtime-Width Program Model and `.processor` Header

Goal: replace `.width N` and all const-generic public program state with a
runtime-width processor layout.

Independent tasks:

- Add `src/layout.rs` with `ProcessorLayout`, `UnitKind`, `UnitDecl`,
  `SlotSpec`, placeholder `CacheConfig`, and placeholder `TopologyConfig`.
- Add `Program { layout: ProcessorLayout, bundles: Vec<Bundle> }`.
- Replace `Bundle<const W: usize>` with runtime `Bundle { syllables:
  Vec<Syllable> }`.
- Replace `Bundle::<W>::nop_bundle()` with `Bundle::nop_bundle(width)`.
- Replace `Bundle::width()` so it returns `syllables.len()`.
- Replace `CpuState<const W: usize>` with runtime `CpuState { width: usize,
  ... }`.
- Update `CpuState::new(latencies)` to `CpuState::new(width, latencies)` or
  `CpuState::new(layout, latencies)` depending on how much layout is embedded
  in the CPU state during stage 0.
- Remove CLI width dispatch in `src/main.rs` and `src/bin/lwir_verify.rs`.
- Extend the parser to require a leading `.processor { ... }` block.
- Reject legacy `.width N` with an error pointing to
  `docs/processor_layout_plan.md`.
- Parse `width`, `hardware`, and `layout slots`.
- Accept `cache { }` and `topology { cpus 1 }` as stage-0 placeholders.
- Add executable layout validation.
- Add Verus spec predicates for layout well-formedness.
- Add `program_layout_compatible(program, layout)` separately from
  layout well-formedness.
- Migrate examples and fixtures from `.width` to `.processor`.
- Update `docs/lwir_asm_format.md`, `docs/compiler_contract.md`, and README
  examples.
- Rewrite tests that currently instantiate `Bundle<W>`, `CpuState<W>`, or
  `parse_program::<W>`.

Acceptance:

- `cargo test --all-targets` passes.
- `cargo verus verify` passes.
- Both CLIs reject inputs without `.processor`.
- Legacy `.width` files are rejected with a clear migration hint.
- Existing migrated fixtures preserve bundle bodies and runtime behavior under
  the canonical layout.

## Stage 1: Layout-Driven Slot Legality and Dispatch

Goal: delete hardcoded static slot-class mapping while preserving current
semantics under the canonical layout.

Independent tasks:

- Add canonical layout construction for the current `I, I, M, X` model.
- Add opcode capability functions such as `opcode_primary_class`,
  `opcode_class_set`, or `unit_executes`.
- Keep opcode side-effect helpers independent of placement classes:
  `writes_gpr`, `writes_pred`, `reads_pred_src`, and call's implicit `r31`
  write.
- Replace runtime slot legality in `CpuState::bundle_is_legal` with
  `layout.slot_can_execute(slot, opcode)`.
- Replace verifier slot legality with layout-aware checks.
- Add deterministic first-matching-unit selection for execution dispatch.
- Replace the opcode dispatch match in `execute_syllable` with class/unit
  dispatch only where this does not obscure opcode semantics.
- Add a proof or checked test that the canonical layout accepts exactly the
  same scheduled opcodes as the old slot-class model.
- Add fixtures for composed slots, unknown units, width mismatch, and missing
  processor header.

Acceptance:

- Canonical-layout migrated fixtures behave as before.
- Sparse layouts are allowed when the program uses only supported opcodes.
- Sparse layouts reject programs that schedule unsupported opcodes.
- No explicit unit-selection syntax is introduced.

## Stage 2: Composable Hardware Units

Goal: add new hardware unit families without changing the slot composition
model.

Independent tasks:

- Add `src/hw/mod.rs` for executable hardware-unit helpers.
- Extend `UnitKind` with FP variants.
- Extend `UnitKind` with AES variants.
- Parse `fp { variant ... latency ... }`.
- Parse `aes { variant ... latency ... }`.
- Add FP opcodes and parser mnemonics.
- Add AES opcodes and parser mnemonics.
- Add placeholder executable semantics for FP and AES opcodes.
- Add Verus spec functions for unit classes, unit execution, and unit latency.
- Add executable/spec equivalence lemmas per unit family where feasible.
- Add `MissingHardwareUnit` or equivalent verifier diagnostics for
  program/layout incompatibility.
- Add legal FP, legal AES, and illegal missing-unit fixtures.

Acceptance:

- Slot composition remains position-dependent.
- If multiple units in one slot execute an opcode, the first listed unit wins.
- Layouts without FP/AES remain valid for programs that do not use FP/AES.

## Stage 3: Direct-Mapped L1 Cache Latency

Goal: make load/store latency depend on a concrete direct-mapped L1 cache.

Independent tasks:

- Parse concrete `cache { l1d { ... } }` config.
- Add `src/cache.rs` with `CacheLine`, `CacheState`, `CacheConfig`, and
  direct-mapped index/tag helpers.
- Add `cache: CacheState` to runtime CPU state.
- Initialize cache state from layout config.
- Implement load cache access and latency calculation.
- Implement store cache access and dirty-bit updates.
- Replace load latency table lookup with cache-derived latency.
- Keep non-load opcode latency lookup through `LatencyTable`.
- Update verifier timing to use cache-configured worst-case load latency.
- Add `cache_outcome` to memory trace events.
- Add fixtures for hit streaks and dirty eviction.

Acceptance:

- Static timing checks use a safe upper bound for the configured cache.
- Old schedules are not claimed valid unless they satisfy the new latency
  contract.
- Trace diffs are intentional and document cache outcomes.

## Stage 4A: System Shell and Shared Memory

Goal: introduce multi-CPU structure without coherence first.

Independent tasks:

- Add `src/system.rs` with `System`, `SharedMemory`, and per-CPU state vector.
- Move architectural memory ownership from `CpuState` to `System`.
- Add symmetric topology parsing for `topology { cpus N }`.
- Add per-CPU stepping under one global cycle counter.
- Keep one shared memory model with deterministic access ordering.
- Add two-CPU smoke tests without caches/coherence.

Acceptance:

- Single-CPU behavior matches stage 3.
- Two CPUs can run independent programs against shared memory.

## Stage 4B: Bus Arbitration

Goal: serialize memory requests through a deterministic shared bus.

Independent tasks:

- Add `Bus`, `BusReq`, and `ArbState`.
- Implement strict deterministic arbitration.
- Stall only the issuing memory slot when its request loses arbitration.
- Add bus events to trace output.
- Add property tests for deterministic request ordering.
- Add Verus spec functions for bus serialization.
- Prove or test first that at most one bus request commits per cycle.

Acceptance:

- Memory writes have a single total bus order.
- Arbitration is deterministic and independent of host iteration quirks.

## Stage 4C: Synchronization Opcodes

Goal: add explicit synchronization semantics on top of the serialized bus.

Independent tasks:

- Add `Fence`, `AcqLoad`, `RelStore`, and `Cas` opcodes.
- Add parser support for sync opcodes.
- Add `OpClass::Sync` or equivalent capability class.
- Map sync opcodes to memory/sync-capable units.
- Implement CAS as one serialized bus transaction.
- Implement fence as a local drain of prior memory requests.
- Add two-CPU CAS counter fixture.
- Add acquire/release ordering tests.

Acceptance:

- Sync opcodes require declared hardware capability.
- CAS behavior is atomic with respect to the bus order.

## Stage 4D: MSI Coherence

Goal: add coherent private caches after the bus and sync model is stable.

Independent tasks:

- Extend `CacheLine` with MSI state.
- Add invalidation on writes.
- Add read transitions to Shared.
- Add write transitions to Modified.
- Add dirty writeback behavior.
- Add two-CPU coherence fixtures.
- State the `at_most_one_modified` invariant.
- Prove the invariant for two CPUs first.
- Generalize to N CPUs only after the two-CPU proof is stable.

Acceptance:

- At most one cache holds a line in Modified state.
- Store visibility follows bus serialization.
- No new `external_body` is added for coherence.

## Cross-Stage Checklist

- Keep `cargo test --all-targets` green.
- Keep `cargo verus verify` green.
- Add no new `#[verifier::external_body]`.
- If a proof is intractable, simplify the data model instead of adding a
  trusted hole.
- Add or update an ADR for each merged stage.
- Preserve position-dependent scheduling semantics throughout.
