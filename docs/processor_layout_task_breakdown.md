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
- Hardware never inserts a runtime stall. Every cycle of delay must be visible
  in the static schedule. The current `bundle_has_unready_gpr_sources` stall is
  a transitional fallback and will be removed once the verifier is tight enough
  for the multi-CPU model. Software polling loops are the *only* way to wait
  for a cross-CPU event, and the compiler is free to fill each loop iteration
  with independent work.
- Multi-CPU programs are verified as one whole-system static schedule, not as
  N independent per-CPU schedules. Cross-CPU contention (bus arbitration,
  cache invalidation) must be resolvable at compile time so the verifier can
  emit a tight worst-case bound for every cross-CPU dependency.

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

Goal: serialize memory requests through a deterministic shared bus whose
per-cycle winner is a pure function of `(cycle, cpu_id)` so the compiler can
schedule around contention without hardware stalls.

Independent tasks:

- Add `Bus`, `BusReq`, and `ArbState`.
- Implement strict deterministic arbitration as a closed-form schedule (e.g.
  round-robin by `cycle % cpus`) with no internal arbitration state that
  depends on prior request history.
- A memory op scheduled on a cycle the issuing CPU does not own is a
  *verifier-rejected program*, not a runtime stall. Add a `BusSlotConflict`
  diagnostic and reject any per-CPU bundle that issues a memory op on a cycle
  the bus does not award to that CPU.
- Add a `bus_slot(cycle, cpu_id) -> bool` runtime/spec helper and its
  equivalence lemma; the verifier consumes the spec form.
- Tighten `LatencyTable`/cache-derived load latency for the verifier to the
  whole-system worst case: `(cpus − 1) × bus_slot_cost + miss + writeback`.
  Loads outside the issuing CPU's bus slot are illegal, so per-load contention
  collapses to "wait until your next slot."
- Add bus events to trace output (winner, request, address, cycle).
- Add property tests for deterministic request ordering and for
  `BusSlotConflict` rejection of off-slot memory ops.
- Add Verus spec functions for the closed-form bus schedule and a lemma that
  at most one bus request commits per cycle.

Acceptance:

- Memory writes have a single total bus order that is computable from
  `(cycle, cpu_id)` alone.
- The simulator never needs to stall a memory op for arbitration: programs
  that would have stalled are rejected by the verifier at compile time.
- Bus arbitration is deterministic and independent of host iteration quirks.

## Stage 4C: Cross-CPU Ordering Primitives

Goal: add the minimum ordering opcodes needed for software polling loops.
Hardware does not retry, drain, or stall; all waiting is a compiler-visible
loop whose iteration count is bounded by the verifier.

Independent tasks:

- Add `AcqLoad` and `RelStore` opcodes. These are *one-shot* ordering hints,
  not retry primitives. Semantically they are an ordinary load/store with the
  guarantee that the value read/written respects the bus total order.
- Do **not** add a `Fence` opcode. The verifier's worst-case visibility bound
  subsumes it. If a real fence is later needed it lands in its own stage.
- Do **not** add a `Cas` opcode. Atomic retry is incompatible with the
  no-hardware-stall rule. If atomic compare-and-update is needed, it must be
  expressed as a software polling loop over `AcqLoad` + a producer-side
  `RelStore`, with the bus arbitration schedule guaranteeing no other CPU
  touches the address in the same cycle.
- Add parser support for the two opcodes.
- Add `OpClass::Sync` (or fold into `Memory`) and route to memory-capable units.
- Compute and expose `worst_case_visibility(layout) -> cycles`:
  `(cpus − 1) × bus_slot_cost + miss + writeback + coherence_drain`. This is
  the static upper bound on the gap between a `RelStore` issuing on the
  producer and the matching `AcqLoad` observing it on the consumer.
- Extend the verifier with a cross-CPU pass that recognizes flag-based
  handshakes: for every consumer loop that re-issues an `AcqLoad` on the same
  address until a guard predicate flips, prove the loop terminates within
  `worst_case_visibility(layout)` cycles assuming the matched producer
  `RelStore` issues by some statically known cycle. Reject loops without a
  matching producer or without a bounded iteration count.
- The verifier's existing per-CPU schedule check still runs, but each CPU's
  schedule is read from the whole-system program rather than in isolation.
- Add a two-CPU producer/consumer fixture using `RelStore` + `AcqLoad`
  polling, with the consumer's loop body filled with independent work.
- Add a fixture demonstrating that a polling loop the verifier cannot bound
  (e.g. no matching producer in any CPU's schedule) is rejected.

Acceptance:

- Sync opcodes require declared hardware capability.
- A program that uses `AcqLoad`/`RelStore` either passes verification with a
  bounded polling loop, or is rejected at compile time. There is no third
  option where the simulator stalls.
- Single-CPU programs that do not use these opcodes are unaffected.

## Stage 4D: MSI Coherence

Goal: add coherent private caches after the bus and ordering model are stable,
*without* introducing any new runtime stall path. Coherence overhead is a
constant the verifier folds into `worst_case_visibility`.

Independent tasks:

- Extend `CacheLine` with MSI state.
- Add invalidation on writes.
- Add read transitions to Shared.
- Add write transitions to Modified.
- Add dirty writeback behavior.
- Define `coherence_drain(layout) -> cycles` as a closed-form upper bound on
  the cost of any single invalidation/upgrade. Fold it into
  `worst_case_visibility` (Stage 4C) and into the verifier's load-latency
  bound (Stage 4B). A coherence event must never cause an in-flight
  instruction to stall: any program whose static schedule depends on a
  faster-than-worst-case coherence path is by construction still legal,
  because it scheduled for the worst case.
- Add two-CPU coherence fixtures.
- State the `at_most_one_modified` invariant.
- Prove the invariant for two CPUs first.
- Generalize to N CPUs only after the two-CPU proof is stable.

Acceptance:

- At most one cache holds a line in Modified state.
- Store visibility follows bus serialization.
- Coherence events do not introduce any new stall path; the verifier's
  worst-case bound from Stages 4B/4C remains tight.
- No new `external_body` is added for coherence.

## Cross-Stage Checklist

- Keep `cargo test --all-targets` green.
- Keep `cargo verus verify` green.
- Add no new `#[verifier::external_body]`.
- If a proof is intractable, simplify the data model instead of adding a
  trusted hole.
- Add or update an ADR for each merged stage.
- Preserve position-dependent scheduling semantics throughout.
