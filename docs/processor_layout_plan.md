# Processor Layout, Hardware Units, Cache, and Multi-CPU — Implementation Plan

Status: **stages 0–4D merged**. This document is preserved as the historical
plan; current behavior is described in `README.md`, `docs/compiler_contract.md`,
`docs/vliw_asm_format.md`, and `docs/adr/`. The staged breakdown lives in
`docs/processor_layout_task_breakdown.md`.

Where this plan and the implementation diverge, the implementation is
authoritative. Notable deltas worth knowing about while reading the plan:

- The `Fence` and `Cas` opcodes proposed in stage 4C were not added. Stage 4C
  ships `AcqLoad` and `RelStore` only; software polling loops bounded by
  `worst_case_visibility(layout)` replace fences and CAS. See ADR 0004 and
  `src/verifier.rs::check_cross_cpu_ordering` for the rejection rule.
- Stage 4B's bus arbitration is closed-form round-robin (`cycle % cpus`); the
  arbiter has no internal state. Memory ops on cycles a CPU does not own are
  *verifier-rejected programs*, not runtime stalls.
- Stage 4D's `at_most_one_modified` invariant is proved at the cache-transition
  level for two CPUs only. Generalizing to N CPUs is on the README's "next
  steps" list.
- `external_body` annotations were not eliminated as part of these stages.
  `verify_program` / `verify_program_for_cpu` and `LatencyTable::default`
  remain trusted; shrinking that surface is "next steps" work.

Prerequisite reading for understanding the original design intent:
`docs/compiler_contract.md`, `docs/vliw_asm_format.md`, `README.md`
§ "Verus annotations".

## 0. Goals and non-goals

### Goals

1. Replace the `.vliw` file header — which today is just a single `.width N` directive (`src/asm.rs:47`) — with a mandatory `.processor { ... }` block that declares:
   - bundle width,
   - a **processor layout** mapping slot positions to a *set of hardware units* installed on that slot (each unit advertises which opcode classes it handles),
   - the catalog of **hardware units** (built-in ones like `integer_alu`, `mem`, `ctrl`, `mul`; pluggable ones like `fp { variant fp64_fma }`, `aes { variant aes_ni }`),
   - cache geometry (line size, capacity, associativity, write policy),
   - multi-CPU topology and bus model.
2. Make that block the single source of truth driving the dispatch tables currently hardcoded in `src/isa.rs:100` (`spec_slot_class`) and `src/cpu/execute.rs:524`–`544` (the user-pointed line 542).
3. Run compiler / scheduling experiments on multiple layouts to study how optimization quality changes with ISA shape (the motivating use case).
4. Keep the verifier sound at every stage. Each stage's specs must be **cumulative**: stage *k+1* extends, never invalidates, the obligations proven in stage *k*.

### Explicit non-compatibility stance

There is **no backwards compatibility** with the existing `.width N` syntax. As of stage 0, every `.vliw` file in the repo is mechanically migrated to the new header form in the same PR that lands the parser. Old-form files are rejected with a parse error pointing at this document. The compiler back-end and any external tooling are expected to update; the format is intentionally a hard cutover so we do not maintain two parsers, two spec families, or two trusted surfaces.

### Non-goals (this plan only)

- No new compiler back-end work; the compiler keeps emitting VLIW and we extend the simulator + verifier to *receive* layout-shaped output.
- No micro-architectural pipeline modeling beyond what the cache/bus latency oracle requires (no branch predictor, no rename, no OoO). The simulator stays cycle-approximate.
- No deviation from Verus + Z3 as the verification stack.

### Cumulative-verification principle

> Every stage adds spec functions and lemmas to existing modules but **never weakens** an existing `ensures` clause.

### Trusted-surface principle (revised)

`#[verifier::external_body]` is treated as a **bug**, not a budget. The only pre-existing instances we inherit are `verify_program` (`src/verifier.rs:456`) and `LatencyTable::default` (`src/latency.rs:86`); both are already on the README's roadmap to be discharged. The assembly parser (`parse_program` in `src/asm.rs:12`) is **not** annotated — it's executable code currently outside the verified scope by virtue of not being called from any verified path with proof obligations. **No stage in this plan introduces a new `external_body` annotation.** Where I previously sketched one as a fallback (per-unit latency oracles, cache eviction picker, MSI lemma stub), the corresponding section now specifies the closed-form construction or proof obligation that lets us stay inside the proof body. If a stage finds it cannot meet that bar during implementation, the response is to redesign the data, not to add a hole — escalate before merging.

### Placement vs. semantics principle

A second discipline that the breakdown crystallized: **slot placement and opcode side-effects are independent axes** and the spec must not couple them.

- *Placement / capability* — "may opcode `op` execute at slot `s` of layout `l`?" — is decided by the layout's per-slot unit set (stage 0/1).
- *Semantics / side-effects* — "does executing `op` write a GPR? a predicate? read a predicate source? implicitly write `r31` (call)?" — is a property of the opcode alone and lives in `src/isa.rs:200` (`writes_gpr`), `:218` (`writes_pred`), `:236` (`reads_pred_src`).

These two functions families never get folded into each other. The bitmask `OpClass` in `src/layout.rs` describes *placement-relevant* membership only; it is not the source of truth for whether a load writes a GPR.

---

## 1. Stage map (read top-to-bottom)

| Stage | Theme                                  | Major artifacts                                                                  | New trusted surface |
|-------|----------------------------------------|----------------------------------------------------------------------------------|---------------------|
| 0     | Runtime-width data model + `.processor` header | `src/layout.rs`, runtime `Bundle`, runtime `CpuState`, `src/asm.rs` rewrite, fixture migration | 0          |
| 1     | Layout-driven slot legality and dispatch | refactor `src/isa.rs`, `src/cpu/execute.rs`, `src/verifier.rs`; canonical layout | 0                   |
| 2     | Composable hardware units (FP, AES)    | `src/hw/`, opcode-family extensions, per-slot unit composition                   | 0                   |
| 3     | Direct-mapped L1 cache latency         | `src/cache.rs`, `CpuState` cache field, deterministic eviction, verifier adopts cache worst-case | 0    |
| 4A    | System shell + shared memory           | `src/system.rs`, `SharedMemory`, per-CPU step under one global cycle             | 0                   |
| 4B    | Bus arbitration                        | `Bus`, `BusReq`, `ArbState`, deterministic round-robin                           | 0                   |
| 4C    | Synchronization opcodes                | `Fence`, `AcqLoad`, `RelStore`, `Cas`; new `OpClass::Sync`                       | 0                   |
| 4D    | MSI coherence                          | `CacheLine` MSI state, invalidation, `at_most_one_modified` invariant proof      | 0                   |

Each stage is independently shippable and gated by its own CI job. Do not start stage *k+1* until stage *k* is merged with green Verus and golden tests.

---

## 2. Stage 0 — Runtime-width data model and `.processor` header

The center of gravity for stage 0 is **removing the const-generic `<const W: usize>` parameter from `Bundle` (`src/bundle.rs:20`) and `CpuState` (`src/cpu/types.rs:21`)** and threading width through the runtime layout instead. Header parsing falls out as a consequence — the layout has to be parsed before bundles are built because the bundles' slot count is now a runtime value carried by the layout. This is not a parser-side stage with a data-model footnote; it's a data-model stage with a parser update.

### 2.1 Format

The header is a **mandatory** `.processor { ... }` block at the top of every `.vliw` file. A file without it is a parse error; a file with the legacy `.width N` is a parse error with a hint pointing at this document. Body is a small declarative DSL parsed by the same hand-rolled lexer in `src/asm.rs`. Canonical example for stage 0 (cache/topology blocks are placeholders until stages 3/4 fill them):

```text
.processor {
  width 4

  hardware {
    unit alu  = integer_alu          # built-in: { gpr_writer, compare, predicate_logic }
    unit mem  = memory               # built-in: { gpr_writer (loads), store }
    unit ctrl = control              # built-in: { control }
    unit mul  = multiplier           # built-in: { gpr_writer (mul/mulh) }
  }

  layout slots {
    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { ctrl, mul }                # this slot accepts control OR integer multiply
  }

  cache    { }                       # extended in stage 3
  topology { cpus 1 }                # extended in stage 4
}
```

The slot RHS is a **set of unit names**; the slot's accepted opcode-class set is the union of the classes advertised by those units. This is the mechanism by which a single slot symbol can accept multiple subsets — you compose units instead of widening a single class label.

Stage 0 ships only the four built-in units shown above (`integer_alu`, `memory`, `control`, `multiplier`). Stage 2 adds pluggable `fp` and `aes` units, which compose into slots the same way.

There is no implicit "default VLIW" layout. Stage 0's PR contains a one-shot migration of every existing fixture (`examples/*.vliw`, `examples/fixtures/legal/*.vliw`, `examples/fixtures/illegal/*.vliw`) to the explicit form above — the four-unit composition reproduces today's `I, I, M, X` mapping exactly.

### 2.2 New module: `src/layout.rs`

```rust
pub enum OpClass {
    GprWriter,
    Compare,
    Store,
    Control,
    PredicateLogic,
    FloatingPoint,        // populated in stage 2 — declared now so the bitmask width is stable
}

pub enum UnitKind {
    IntegerAlu,
    Memory,
    Control,
    Multiplier,
    Fp(FpVariant),        // stage 2; FpVariant is `enum { Fp32, Fp64, Fp64Fma }`
    Aes(AesVariant),      // stage 2
}

pub struct UnitDecl {
    pub name: SymbolId,           // interned at parse time
    pub kind: UnitKind,
}

pub struct SlotSpec {
    pub units: Vec<SymbolId>,     // names referencing UnitDecl entries
}

pub struct ProcessorLayout {
    pub width: usize,
    pub units: Vec<UnitDecl>,                 // header `hardware { ... }` block
    pub slots: Vec<SlotSpec>,                 // length == width
    pub cache: CacheConfig,                   // stage 3 extension; trivially valued today
    pub topology: TopologyConfig,             // stage 4 extension; { cpus: 1 } today
}
```

All fields are `Vec` / `usize` / closed enums — no `dyn`, no lifetimes, no trait objects. Verus specs that quantify over slots and units are direct `forall` over `Seq<...>`.

`OpClass` membership for each `UnitKind` is a **`spec fn`** (closed match), not data, so the verifier can reason about it by case-splitting:

```rust
spec fn unit_classes(k: UnitKind) -> Set<OpClass>;       // closed match
spec fn slot_accepts(layout: ProcessorLayout, slot: int, c: OpClass) -> bool {
    exists |i: int| 0 <= i < layout.slots[slot].units.len() &&
        unit_classes(layout.unit_kind_of(layout.slots[slot].units[i])).contains(c)
}
```

### 2.3 Data-model migration (the bulk of stage 0)

Independent tasks, in roughly the order they should be tackled:

- Replace `Bundle<const W: usize>` (`src/bundle.rs:20`) with runtime `Bundle { syllables: Vec<Syllable> }`. `Bundle::nop_bundle()` becomes `Bundle::nop_bundle(width: usize)`; `Bundle::width()` returns `syllables.len()`.
- Replace `CpuState<const W: usize>` (`src/cpu/types.rs:21`) with runtime `CpuState { width: usize, ... }`. Constructor signature shifts from `CpuState::<W>::new(latencies)` to `CpuState::new(layout, latencies)` (or `CpuState::new(width, latencies)` if the layout is owned at the `System` level by stage 4 — defer that decision but pick one for stage 0 and keep it).
- Add `pub struct Program { pub layout: ProcessorLayout, pub bundles: Vec<Bundle> }` in `src/lib.rs` or a new `src/program.rs`.
- Drop the const-generic dispatch in `src/main.rs:79` and `src/bin/vliw_verify.rs:67` — both binaries take `&Program` directly.
- Rewrite `tests/smoke.rs` and `tests/verifier.rs` to construct runtime `Bundle`s and pass the canonical layout. There is no `parse_program_w` shim.

### 2.4 Parser changes

- Extend `collect_lines` in `src/asm.rs:31` to recognize `.processor` and switch into a *block-scan* sub-mode that consumes balanced braces.
- New function `parse_processor_block(lines: &[&str]) -> Result<ProcessorLayout, AsmError>`.
- `parse_program::<W>(text)` becomes `parse_program(text: &str) -> Result<Program, AsmError>`.
- A bare `.width N` (the current syntax at `src/asm.rs:47`) is rejected with an error pointing at this document.

### 2.5 Verifier surface

- The parser remains executable code outside the verified scope (it is **not** `external_body` today and stage 0 does not annotate it). The two pre-existing `external_body` annotations — `verify_program` (`src/verifier.rs:456`) and `LatencyTable::default` (`src/latency.rs:86`) — are unchanged and discharged on their own roadmap.
- Two **separate** predicates live in the proof body. Conflating them was an error in earlier drafts.
  1. **Layout well-formedness** — `spec fn layout_well_formed(l: ProcessorLayout) -> bool`. Structural validity only:
     - `l.slots.len() == l.width`,
     - `l.width ∈ {4, 8, 16, 32, 64, 128, 256}` (carried over from the current `WIDTHS` invariant),
     - every slot's `units` is non-empty,
     - every name in `slots[i].units` resolves into `l.units`.
  2. **Program/layout compatibility** — `spec fn program_layout_compatible(p: Program) -> bool`. For every non-`nop` syllable in every bundle, the slot it occupies has at least one declared unit that executes the syllable's opcode (`unit_executes(...)` returns true). This is the check that distinguishes a sparse layout from a missing-hardware error.
- Each predicate has an executable mirror with a `lemma_*_check_matches_spec` discharge lemma; the parser and verifier call the executable forms. The spec/exec boundary is closed.

### 2.6 Tests / fixtures — migration

In the same PR:

- Rewrite all existing files under `examples/` (`hello.vliw`, `clean_schedule.vliw`, `mul_latency.vliw`, `predication.vliw`, `illegal_raw_same_bundle.vliw`, `illegal_wrong_slot.vliw`) and `examples/fixtures/{legal,illegal}/*.vliw` to the new header form.
- Regenerate goldens. Diffs are reviewed by hand to confirm only the header changed (bundle bodies are untouched and trace output past the header is byte-identical).
- Add new fixtures:
  - `examples/fixtures/legal/w4_composed_slot.vliw` — slot 3 hosts both `ctrl` and `mul` (exercising composition).
  - `examples/fixtures/illegal/w4_no_processor_header.vliw` — bare `.width 4`, must be rejected with a clear pointer to this doc.
  - `examples/fixtures/illegal/w4_unknown_unit.vliw` — slot references a unit name not declared in `hardware { ... }`.
  - `examples/fixtures/illegal/w4_layout_width_mismatch.vliw` — slot count ≠ `width`.

### 2.7 Acceptance criteria

- `cargo verus verify` green.
- `cargo test --all-targets` green including new fixtures.
- `vliw_simulator --trace` over migrated goldens differs from old goldens **only** in the header echo line (or wherever the trace surfaces the layout); the cycle/event stream is byte-identical.
- `vliw_simulator` and `vliw_verify` reject any input lacking the `.processor` block.

---

## 3. Stage 1 — Layout-driven slot legality and dispatch

This stage replaces the hardcoded 3-class `SlotClass` (`src/isa.rs:13`) and its dispatch with a layout-driven scheme. **Per the placement-vs-semantics principle stated in § 0**, we are only refactoring the *placement* axis; opcode side-effect helpers are left alone.

### 3.1 Note on classification arity

The current code has three placement classes: `Integer`, `Memory`, `Control` (`src/isa.rs:13`). The new design has six placement-relevant classes: `GprWriter`, `Compare`, `Store`, `Control`, `PredicateLogic`, `FloatingPoint` (the last is populated in stage 2). This is a **deliberate refinement**, not a renaming — the new classes let a layout express "this slot can do compares but not stores," which the old 3-class model could not. The migration lemma below shows the canonical layout reproduces the old coarse mapping; finer layouts are new expressive territory.

### 3.2 What gets replaced

| Site | Current code | Replacement |
|------|--------------|-------------|
| `src/isa.rs:100` (`spec_slot_class`) | hardcoded match on `Opcode` returning 3-variant `SlotClass` | `spec fn opcode_class_set(op: Opcode) -> Set<OpClass>` plus `spec fn slot_can_execute(layout, slot, op)` |
| `src/isa.rs:120` (`Opcode::slot_class`) | runtime version of above | `layout.slot_can_execute(slot, op)` |
| `src/cpu/execute.rs:524`–`544` (the `match syl.opcode { ... }` dispatch — the line you selected at 542) | direct match on opcode that decides *both* placement and execution-family | **only the slot-legality decision** moves to the layout; the per-family branches (`exec_gpr_writer` / `exec_compare` / `exec_store` / `exec_control` / `exec_predicate_logic`) stay because they encode opcode *semantics*, not placement |
| `src/verifier.rs` `check_slot_legality` | uses static `slot_class` | uses layout-injected `slot_can_execute` |

What stays untouched, per the placement-vs-semantics principle in § 0:

| Site | Rationale |
|------|-----------|
| `src/isa.rs:200` (`writes_gpr`) | semantic side-effect; not derivable from placement |
| `src/isa.rs:218` (`writes_pred`) | semantic side-effect; predicate compares write predicates regardless of which slot they occupy |
| `src/isa.rs:236` (`reads_pred_src`) | semantic side-effect on the predicate source operand |
| The `Call` opcode's implicit `r31` write | semantic; layout doesn't know about it |

### 3.3 Spec/proof obligations

- Add lemma `lemma_canonical_layout_matches_legacy_slot_class`: for every `Opcode`, the canonical four-unit layout used by the migrated fixtures (`{alu, alu, mem, ctrl+mul}`) decides slot legality identically to today's `Opcode::slot_class()` against the corresponding `I/I/M/X` slot positions. This is the load-bearing lemma for the migration: it proves the rewritten fixtures preserve runtime semantics. There is no implicit "default layout" data anywhere — the lemma is stated against an explicit `ProcessorLayout` constant declared in `src/layout.rs::canonical`.
- The `program_layout_compatible` predicate from stage 0 § 2.5 now does real work: for each non-`nop` syllable, the slot it occupies must have at least one declared unit that executes the opcode (`unit_executes(...)`). A sparse layout that omits FP units is fine for FP-free programs; it rejects FP programs with `MissingHardwareUnit`.
- Update `verifier.rs:288`–`382` `verus!` block: spec functions take `&ProcessorLayout` as an extra argument. The conservative-ness property (a fully-active spec implies the runtime predicate) is preserved by structural induction on `OpClass` instead of `Opcode`.

### 3.4 Risk and mitigation

The opcode enum is intentionally not opaque to the verifier — the *enumeration discipline* of `writes_gpr` / `writes_pred` / `reads_pred_src` is what gives Verus its leverage. Because the placement-vs-semantics principle in § 0 forbids re-deriving these from `OpClass`, that discipline is **preserved by construction** in this stage; the risk that motivated an earlier draft's "thin shim" mitigation is no longer present.

The remaining risk is a different one: a layout that decides `slot_can_execute(s, op) == true` while the dispatch in `execute_syllable` has no semantic branch for `op`. Mitigation: `lemma_classify_total` requires that for every opcode the canonical layout ever accepts at any slot, the dispatch has a branch — and we statically check this at compile time by a totality match. The match must be exhaustive on `Opcode`; this is the same property that keeps `writes_gpr` honest today.

### 3.5 Acceptance criteria

- All lemmas proven, no new `external_body`.
- Side-effect helpers `writes_gpr`, `writes_pred`, `reads_pred_src` are byte-identical before and after the diff (they are not in the refactor scope).
- Diff is mechanical: a single PR of <1000 lines should suffice. If it grows beyond that, split per-module.

---

## 4. Stage 2 — Composable hardware units (FP, AES, …)

### 4.1 Header extension

Stage 2 adds two new built-in unit families to the `hardware { ... }` block introduced in stage 0. Composition is the same as before — slots are sets of unit names — and the same slot can host an FP unit alongside an integer unit, alongside AES, etc. Example:

```text
hardware {
  unit alu  = integer_alu
  unit mem  = memory
  unit ctrl = control
  unit mul  = multiplier
  unit fp0  = fp  { variant fp64_fma  latency 4 }
  unit aes0 = aes { variant aes_ni    latency 6 }
}

layout slots {
  0 = { alu, fp0 }                  # int ALU OR fp64-fma on this slot
  1 = { alu, aes0 }                 # int ALU OR AES on this slot
  2 = { mem }
  3 = { ctrl, mul }
}
```

Unknown variants (`fp { variant nonsense }`, `aes { variant ... }`) are parse errors.

### 4.2 New module tree: `src/hw/`

```
src/hw/
  mod.rs          // HardwareUnit registry, HardwareConfig
  fp.rs           // fp32 / fp64 / fp64_fma variants
  aes.rs          // aes_ni / aes_armv8 variants
```

`HardwareUnit` is the closed enum already declared in `src/layout.rs::UnitKind` (stage 0). All operations on it are `spec fn` closed matches — Verus reasons exhaustively, no trait objects, no `dyn`:

```rust
spec fn unit_classes(k: UnitKind) -> Set<OpClass>;          // closed match
spec fn unit_executes(k: UnitKind, op: Opcode) -> bool;     // closed match
spec fn unit_latency(k: UnitKind, op: Opcode) -> u32;       // closed match — total
```

Crucially `unit_latency` is a **spec function**, not an `external_body`-annotated trait method. Closing the enum means the verifier exhaustively checks every variant × every opcode case at proof time, which is the whole point of the closed design. The cost is that adding a new hardware family requires editing the enum and re-discharging the totality lemma — that cost is the feature.

### 4.3 Slot capability and dispatch

The opcode's "intrinsic class set" is read from the same closed `OpClass` lookup added in stage 1. An opcode `op` is legal at slot `s` of layout `l` iff:

```rust
spec fn slot_can_execute(l: ProcessorLayout, s: int, op: Opcode) -> bool {
    exists |i: int| 0 <= i < l.slots[s].units.len() &&
        unit_executes(l.unit_kind_of(l.slots[s].units[i]), op)
}
```

When a slot lists multiple units that can both execute the same opcode (e.g. two integer ALUs), the dispatch picks the one declared first. This is deterministic and provable; we don't model contention between units on a single slot in stage 2.

### 4.4 Opcode enum extension

Add `Fadd`, `Fsub`, `Fmul`, `Fdiv`, `Fmadd`, `Fcvt`, `AesEnc`, `AesDec`, `AesKeygen` to `Opcode` (`src/isa.rs:24`). Update the layout-driven classifier from stage 1 to map them to `OpClass::FloatingPoint` and the AES variants to a new `OpClass` (one option: reuse `GprWriter` since they write a GPR — but per the placement-vs-semantics principle in § 0, the GPR-write fact lives in `writes_gpr`, not in the placement bitmask).

### 4.5 Feature gating

If a program uses an FP opcode but no slot in the layout hosts an FP unit, `verify_program` (`src/verifier.rs`) emits a new diagnostic `MissingHardwareUnit`. Spec:

```rust
spec fn program_uses_only_declared_hardware(p: Program, l: ProcessorLayout) -> bool;
```

`verify_program` `ensures` this on success. The check is structural and lives in the proof body — same shape as today's slot-class legality check, with `slot_can_execute` substituted for the static class predicate.

Floating-point semantics are specced via `spec fn` *uninterpreted functions* (e.g. `spec fn fadd(a: u64, b: u64) -> u64`) in stage 2 — the goal is to prove the *plumbing* (latency, slot dispatch, scoreboard, hazard freedom) is correct without committing to IEEE-754 bit-exactness yet. **Uninterpreted spec functions are not `external_body`** — Verus simply has no axioms about them, so any proof that depends on their values must thread the assumption explicitly. Bit-exactness is its own follow-up.

### 4.6 Tests / fixtures

- `examples/fixtures/legal/w4_fp_dot_product.vliw`: small DAXPY-style kernel using `fp64_fma`.
- `examples/fixtures/legal/w4_fp_int_shared_slot.vliw`: a slot hosting `{ alu, fp0 }`, exercising mixed dispatch in adjacent bundles.
- `examples/fixtures/legal/w4_aes_round.vliw`: one round of AES via the `aes_ni` unit.
- `examples/fixtures/illegal/w4_fp_without_hw.vliw`: program using `fadd` against a header that omits the FP unit; verifier must reject with `MissingHardwareUnit`.

### 4.7 No new trusted surface

`unit_classes`, `unit_executes`, `unit_latency` are all `spec fn`s with closed matches — fully internal to the proof body. The corresponding executable functions in `src/hw/` are written as parallel matches whose equivalence with the spec is proved by a single lemma per family (`lemma_fp_unit_exec_matches_spec`, `lemma_aes_unit_exec_matches_spec`). No `external_body`.

---

## 5. Stage 3 — Cache-aware load latency

### 5.1 Header extension

```text
cache {
  l1d {
    line_bytes  64
    capacity    4096        # bytes — e.g. 4 KiB direct-mapped for first cut
    associativity 1
    write_policy write_back
    miss_latency 12         # cycles on dirty eviction
    hit_latency  1
  }
}
```

Stage 3 ships **L1 only and direct-mapped** to keep the spec tractable. Set-associative L1 and L2 are explicit follow-ups.

### 5.2 Data model

`CpuState` (`src/cpu/types.rs:1`) gains a field:

```rust
pub cache: CacheState,
```

`CacheState` is fully concrete (no Vecs of Options, no trait objects):

```rust
pub struct CacheLine { pub tag: u64, pub valid: bool, pub dirty: bool }
pub struct CacheState { pub lines: Vec<CacheLine> }   // length == capacity / line_bytes
```

### 5.3 Latency replacement

The `LatencyTable` (`src/latency.rs:154`) lookup for load opcodes is replaced by `cache.access(addr, kind, cfg) -> u32`. Other opcodes still go through the static table. The integration point is `writeback()` in `src/cpu/execute.rs:9`–`34`, which already takes a `lat` parameter — the call sites compute `lat` from the cache for loads and from the table for everything else.

Stores go through the cache too (write-back): they update the line's `dirty` bit; their *visible* latency stays as today (1 cycle), but evicting a dirty line during a future load contributes the `miss_latency` to that load.

### 5.4 Spec/proof obligations

The verifier's existing per-bundle scoreboard rule (`compiler_contract.md` rule 6) becomes parameterized over a *latency oracle* expressed as a `spec fn` — no oracle trait, no `external_body`:

```rust
spec fn cache_latency_upper_bound(cfg: CacheConfig, op: Opcode) -> u32;        // pure
spec fn cache_access_latency(state: CacheState, cfg: CacheConfig,
                             addr: u64, op: Opcode) -> u32;                    // pure
spec fn writeback_ready_at(state: CpuState, op: Opcode, addr: Option<u64>) -> u64;
```

For direct-mapped L1, the eviction picker is closed-form: `line_index(addr, cfg) = (addr / cfg.line_bytes) % cfg.nlines`. That's a single `spec fn` of arithmetic — no `external_body`. The executable mirror in `src/cache.rs` is one line of code; equivalence is one lemma. Set-associative caches in a follow-up will require an LRU pseudo-state, but the pseudo-state is a `Seq<u8>` encoding access order — still pure, still inside the proof body.

#### What "cumulative" means here — and what it does not

Earlier drafts of this plan claimed stage 3 was "semantics-preserving" because the cache "only replaces a constant with a bounded value." That claim is **wrong** and is corrected here. The current default load latency in `LatencyTable::default` is **3 cycles** (`src/latency.rs:99-102`); the proposed direct-mapped cache miss latency is **larger** (the `12` in the example, or whatever the user picks). That makes some loads *slower* in the new model, which means schedules that were legal under the static-3 timing contract may become illegal under the cache contract — the scoreboard ready-cycle bookkeeping moves and a reader that was just-in-time under the old model is now too early.

Stage 3 therefore does **not** claim to preserve old programs unconditionally. Instead:

1. The static `LatencyTable` lookup for **load opcodes** is removed; loads route through the cache oracle exclusively.
2. The verifier's scoreboard rule (`compiler_contract.md` rule 6) is parameterized over the **cache-configured worst-case** load latency for the active layout, not the old constant 3.
3. Existing fixtures whose schedules assumed `lat=3` for loads are re-checked against the new contract. Schedules that fail are either (a) re-emitted by the back-end, or (b) re-tagged as `examples/fixtures/illegal/` if the layout's miss latency is higher than the original schedule allowed for. Either response is intentional and recorded in the stage-3 ADR.
4. The bounding lemma `lemma_cache_access_latency_le_miss_latency` lives in `src/cache.rs` — it is what makes the *worst-case-driven* verifier sound, not what makes old programs forward-compatible.

What is genuinely cumulative across stages 0→3 is the **proof structure**, not the schedule legality: every spec function added in stages 0–2 keeps holding, and the ready-cycle invariant has the same shape — only its constant is now a function of the cache config.

### 5.5 Tests / fixtures

- `examples/fixtures/legal/w4_cache_hit_streak.vliw`: warm-cache streaming load, asserts trace shows `hit_latency` after the first miss.
- `examples/fixtures/legal/w4_cache_dirty_eviction.vliw`: store, evict-by-mismatched-tag, load, assert `miss_latency` plus eviction cost.
- A trace-format extension: each `MemoryEffect` in `src/cpu/trace.rs` gains a `cache_outcome: { Hit, Miss, MissDirty }` field. Goldens regenerate.

---

## 6. Stage 4 — Multi-CPU, shared bus, sync, and coherence

Stage 4 is decomposed into four PR-sized substages — 4A, 4B, 4C, 4D — each with its own merge bar. Earlier drafts shipped them as one stage; that was too coarse given the verification effort involved. The substages **must land in order**: 4B's bus arbiter assumes 4A's `System` shell, 4C's sync opcodes need the bus, 4D's MSI invariant needs sync semantics to be meaningful.

The header extension is fully introduced in 4A and refined as later substages need:

```text
topology {
  cpus 2
  bus  shared { arbitration round_robin  contention_latency 2 }   # block accepted in 4A, fields used by 4B
}
```

Per-CPU asymmetric layouts (different `.processor` blocks per CPU id) are an experiment *enabled* by stage 4 but **out of scope for stage 4 itself**. Symmetric topology only.

### 6A. Stage 4A — System shell and shared memory

Goal: introduce multi-CPU structure without bus serialization or coherence.

New module `src/system.rs`:

```rust
pub struct SharedMemory { pub bytes: Vec<u8> }
pub struct System {
    pub cpus:   Vec<CpuState>,
    pub memory: SharedMemory,
    pub layout: SystemLayout,           // per-CPU ProcessorLayout + topology
    pub cycle:  u64,                    // global cycle, monotone
}
```

Architectural memory ownership moves from `CpuState` (`src/cpu/types.rs:1`) to `System`. `topology { cpus N }` is parsed; the bus block is *accepted but unused* in 4A. Per-CPU stepping advances all CPUs by one global cycle with a deterministic, fixed access ordering (e.g. CPU id ascending) — no arbitration yet.

Acceptance: single-CPU behavior matches stage 3; two CPUs running independent programs produce deterministic interleaved trace output.

### 6B. Stage 4B — Bus arbitration

Goal: serialize memory requests through a deterministic shared bus.

Add `Bus`, `BusReq`, `ArbState`. Memory ops enqueue requests; the arbiter picks one per cycle; losers stall *only* their issuing memory slot (not the entire CPU). Round-robin is a closed function:

```rust
fn pick(arb: ArbState, cpus: &[CpuState]) -> usize    // increment counter mod cpus.len(), skip empties
```

Verus discharges termination and the index-bound by induction on the skip count.

**Required spec/proof:** `lemma_bus_serializes` — at most one `BusReq` is dequeued per cycle, so memory writes have a single total order. **No `external_body`.**

Acceptance: memory writes have a single total bus order; arbitration is deterministic and independent of host iteration quirks.

### 6C. Stage 4C — Synchronization opcodes

Goal: add explicit synchronization semantics on top of the serialized bus.

New opcodes: `Fence` (full barrier), `AcqLoad`, `RelStore` (acquire/release semantics), `Cas` (compare-and-swap as a single serialized bus transaction). New `OpClass::Sync` for capability declaration; sync opcodes route to memory- or sync-capable units (no per-CPU pragma in this stage). `Fence` drains the issuing CPU's prior in-flight memory requests.

**Required spec/proof:**
- `lemma_acquire_release_happens_before` — a `RelStore` in CPU A is observed by an `AcqLoad` of the same address in CPU B only if the bus serialization places the store before the load.
- `lemma_fence_drains_pipeline` — after a `Fence` on CPU A, all of A's prior memory ops are visible to all CPUs.

Acceptance: sync opcodes require declared hardware capability in the layout; CAS is atomic w.r.t. the bus order; acquire/release ordering tests pass.

### 6D. Stage 4D — MSI coherence

Goal: add coherent private caches on top of the bus and sync model.

`CacheLine` (introduced in stage 3) gains an MSI state field. Writes invalidate matching lines in other CPUs' caches via a serialized invalidation transaction; reads transition to `Shared`; writes transition to `Modified`; dirty writeback on eviction or on snoop.

**Required spec/proof:** `lemma_msi_invariant` — at most one cache holds a line in `Modified` state at any cycle. Proved as a global invariant maintained by `System::step`, established by induction on cycle count. Approach: ship the 2-CPU specialization first, then generalize to N. The proof is large but mechanical once each transition's local effect is stated as a lemma.

Acceptance: at most one cache holds a line in Modified at any cycle (proven, not tested); store visibility follows bus serialization; **no new `external_body`** anywhere in the substage.

### 6.5 Trace and tooling (touches 4A and 4D)

- `TraceLog` (`src/cpu/trace.rs`) gains a `per_cpu: Vec<TraceEvent>` per global cycle (4A).
- `vliw_simulator --trace` accepts `--cpu N` to filter, defaulting to interleaved per-cycle output for human reading (4A).
- Golden fixture "two CPUs incrementing a shared counter via CAS" lands in 4C; an MSI-coherence-stress fixture lands in 4D.

### 6.6 Risk

Stage 4 is by far the largest verification undertaking. The risk is **proof-engineering effort**, not soundness — there is no `external_body` escape hatch in this plan. Mitigations, in priority order:

1. Build the executable multi-CPU model first under `proptest` with randomized bus interleavings, *before* attempting Verus discharge. Property tests find shape bugs cheaply; we only invest in proof effort once the executable model is stable.
2. The 4A/4B/4C/4D split itself is the primary mitigation: each substage has a small, discharged proof obligation. If 4D's general N-CPU MSI proves intractable, ship 4D in two PRs (2-CPU first, then N) — never on stubs.
3. If a specific lemma resists Verus, redesign the data (e.g. simplify arbitration to strict rotation, drop optional MSI states until they're needed) — never weaken the trusted surface.

The merge bar for every substage is unchanged from prior stages: `cargo verus verify` green and zero new `external_body`.

---

## 7. Cumulative-verification checklist

After each stage merges, the following must hold:

- [ ] `cargo verus verify` passes with no new warnings.
- [ ] **Zero new `#[verifier::external_body]` annotations.** If a stage's PR adds one, it does not merge — period. The pre-existing annotations (parser, `LatencyTable::default`) are tracked separately on the README's roadmap and discharged on their own schedule.
- [ ] All prior-stage fixtures still pass under `--trace` (or, for stage 3+, intentionally regenerated with a documented diff that audits to "header echo only" or "cache-outcome field added").
- [ ] A short ADR (`docs/adr/NNNN-stage-X.md`) records the design choice and any new closed-enum / spec-fn the verifier now depends on, with the discharge lemmas linked.
- [ ] CI gates: stage `k`'s job depends on stage `k-1` passing; no skipping.

---

## 8. Open questions for review

1. Should `OpClass` be a bitmask (an opcode can belong to multiple classes — e.g. M-slot loads are both `GprWriter` and a memory op) or a primary class plus a side-effect set? Bitmask is more expressive; primary-class is friendlier to Verus triggers. Recommendation: **bitmask, with a "primary class" derivation function that the dispatch in `execute.rs` uses.**
2. When a slot lists multiple units that *both* execute the same opcode (e.g. two integer ALUs, or `{ alu, fp0 }` where `fp0` adds an integer side-channel), does deterministic "first-declared wins" cover the experiments you care about, or do you want a syllable-level pragma to force a specific unit (`i0 fadd@fp0 r1, r2, r3`)? Stage 2 currently assumes first-declared-wins. The pragma is a small grammar add if needed.
3. Should `.processor` headers be allowed *per-program* (today's model after stage 0) or be promoted to a shared `.proc` file referenced by `include`? The compiler-experiment use case argues for shared files because you'll want to vary the program but keep the layout. Recommendation: **defer to stage 1; keep inline-only for stage 0.**
4. For stage 4, is `Sequential Consistency` worth shipping as an alternative model behind a header flag? Probably yes for ground truth in property tests; not on the critical path.

---

## 9. Out-of-scope follow-ups (deliberately deferred)

- IEEE-754 bit-exact FP semantics in Verus.
- Set-associative caches; L2; coherent prefetchers.
- Asymmetric multiprocessing (different `ProcessorLayout` per CPU).
- Branch prediction, register renaming, OoO issue.
- Memory-mapped I/O regions.
- DMA engines / accelerator fabrics.
- A formal compiler back-end retarget. This plan only updates the *simulator* and *verifier*; back-end work to actually *emit* layout-specific schedules is its own project.
