# VliwSimulator

A cycle-approximate **VLIW** (Very Long Instruction Word) simulator written in
Rust and verified with [Verus](https://github.com/verus-lang/verus).

The simulator is the architectural reference for the VLIW ISA and the execution
model for compiler bring-up, scheduling experiments, and backend validation.

---

## Architecture overview

This project models a statically scheduled VLIW ISA. Bundle structure, slot
classes, and operation placement are explicit in the instruction stream rather
than inferred by dynamic issue hardware. The simulator keeps that model
concrete and compiler-facing: resource usage is visible, latency is modeled
explicitly, and architectural state evolves in bundle order.

The processor shape — bundle width, the catalog of hardware units, the
per-slot unit set, the L1D cache, the architectural register/memory sizes, and
the multi-CPU topology — is declared by a mandatory `.processor { ... }` header
on every program. The header is the single source of truth for slot legality,
load latency, bus arbitration, and coherence; nothing about the machine shape
is hard-coded into the simulator.

| Property | Value |
|---|---|
| **Bundle width W** | Runtime, integer in **[1 … 256]**, declared by `.processor { width N }` |
| **Slot composition** | Per-slot set of hardware units, declared in `layout slots { ... }` |
| **Built-in unit kinds** | `integer_alu`, `memory`, `control`, `multiplier`, `fp { variant ... }`, `aes { variant aes_ni }` |
| **GPRs** | 32 × 64-bit by default (`r0` = 0 hardwired); configurable via `arch { gprs N }` |
| **Predicate registers** | 16 × 1-bit by default (`p0` = true hardwired); configurable via `arch { preds N }` |
| **Memory** | 64 KiB byte-addressed little-endian by default; configurable via `arch { memory N }` |
| **L1D cache** | Direct-mapped, write-back, MSI-coherent across CPUs |
| **Topology** | `topology { cpus N }`; symmetric layout per CPU |
| **Bus** | Shared, deterministic round-robin: cycle `c` is owned by CPU `c % cpus` |
| **Memory ordering** | Bus total order; `acqload` / `relstore` are concrete ordering opcodes |

### Layout-driven slot legality

The canonical 4-slot pattern (`I, I, M, X`) is one specific layout, not a hard
rule. A slot's accepted opcode set is the union of opcodes its declared units
can execute. A slot may host multiple units (e.g. `{ ctrl, mul }`), in which
case dispatch picks the first listed unit that executes the opcode. Programs
that schedule opcodes onto slots whose declared units cannot execute them are
rejected at parse / verify time.

---

## Configurable latencies

The machine model assigns an explicit latency to every opcode. Non-load
latencies come from a `LatencyTable` that callers can mutate before CPU
construction:

```rust
let mut latencies = LatencyTable::default();
latencies.set(Opcode::Mul, 5);   // model a slow multiplier
let cpu = CpuState::new_for_layout(&layout, latencies);
```

**Load latency is no longer a single table entry.** Loads route through the L1D
cache: the runtime returns `hit_latency` on a hit and `miss_latency` (plus
`writeback_latency` on a dirty miss) on a miss. The verifier uses a closed-form
worst-case bound:

```
worst_case_load_latency = (cpus − 1) × 1
                        + miss_latency
                        + writeback_latency
                        + coherence_drain
```

`coherence_drain` is `0` for single-CPU layouts and `cache.writeback_latency`
for multi-CPU layouts (the closed-form upper bound on a single MSI
invalidation/upgrade — see ADR 0004).

Default non-load latencies (in cycles):

| Class | Latency |
|---|---|
| Integer ALU, LEA | 1 |
| Store, Prefetch | 1 |
| Multiply (MUL/MULH) | 3 |
| Control (BR, J, CALL, RET) | 1 |
| Predicate ops | 1 |
| FP placeholder | 4–6 by variant |
| AES placeholder | 4 |
| NOP | 0 |

The default L1D config is `line_bytes 64`, `capacity 4096`, `hit_latency 1`,
`miss_latency 3`, `writeback_latency 0`. A concrete `cache { l1d { ... } }`
block in the header overrides these.

---

## Reading processor state

`CpuState` is a plain Rust struct with public fields. State inspection,
checkpointing, and direct test assertions are straightforward:

```rust
// read a GPR
let val = cpu.read_gpr(3);

// read a predicate
let cond = cpu.read_pred(1);

// dump everything
vliw_simulator::cpu::print_cpu_state(&cpu);

// clone the full state for checkpointing
let checkpoint = cpu.clone();
```

Architectural state includes:
- `width: usize` — runtime bundle width
- `gprs: Vec<u64>`, `num_gprs: usize` — general-purpose register file
- `preds: Vec<bool>`, `num_preds: usize` — predicate register file
- `pc: usize` — bundle-level program counter
- `cycle: u64` — cycle counter (includes stall modeling)
- `scoreboard: Vec<ScoreboardEntry>` — per-GPR ready cycle
- `memory: Vec<u8>`, `mem_size: usize` — local view of architectural memory
- `cache: CacheState` — direct-mapped L1D with MSI line state
- `halted: bool` — set when `RET` is executed with `lr == 0`
- `latencies: LatencyTable` — non-load opcode latencies

Multi-CPU programs run inside `vliw_simulator::system::System`, which owns the
shared memory, the per-CPU `CpuState` vector, and the bus.

---

## Getting started

### Prerequisites

- Rust (stable, edition 2021)
- [Verus](https://github.com/verus-lang/verus) checked out as a sibling repo at
  `../verus` relative to this project

### Build and run a text assembly program

```sh
cargo run --bin vliw_simulator -- examples/hello.vliw
```

The simulator requires a leading `.processor { ... }` header. The CLI accepts
one program file and rejects layouts whose `topology { cpus N }` is not 1.
Multi-CPU programs run from Rust against `System::new(layout, programs, ...)`.

Expected output:

```
VLIW Simulator (W=4)
Program: examples/hello.vliw
Bundles: 5

=== VLIW Processor State (width=4) ===
  PC: 5  Cycle: 5  Halted: true
  GPRs:
    r1  = 0x0000000000000006  (6)
    r2  = 0x0000000000000007  (7)
    r3  = 0x000000000000002a  (42)
  Predicate registers:
    p0 = true
==========================================
```

### Trace execution

Use `--trace` to emit a deterministic scheduler-debug log instead of the final
state dump:

```sh
cargo run --bin vliw_simulator -- --trace examples/hello.vliw
```

The trace format is line-oriented and starts with `trace v1 width=<n>`. Each
event records the bundle index, cycle, issue/stall/illegal outcome, active
non-`nop` syllables, stalls, GPR writes, predicate writes, memory effects
(including cache outcome), and branch/jump/call/return decisions, followed by a
final `pc/cycle/halted` line.

### JSON final state

Use `--json` to emit a machine-readable final architectural state:

```sh
cargo run --bin vliw_simulator -- --json examples/hello.vliw
```

`--trace=json` is accepted as an alias. The JSON output uses
`"format": "vliw-sim-final-state-v1"` and includes `halted`, `pc`, `cycle`, all
GPRs including zero-valued registers, all predicates including false
predicates, and a `memory_writes` array for executed stores.

### Query dumps

Use `--dump-reg`, `--dump-mem`, or `--dump-all-regs` for line-oriented final
state queries:

```sh
cargo run --bin vliw_simulator -- --dump-reg r1 --dump-mem 0x100:4 examples/hello.vliw
```

Register dumps print the requested value even when it is zero. Memory dumps use
`addr:width`, where `addr` may be decimal or hex and `width` is 1, 2, 4, or 8
bytes. Dump output is intended for simple compiler-harness assertions without
parsing the execution trace.

### Text assembly format

`vliw_simulator` consumes the stable bundle-level text format documented in
[`docs/vliw_asm_format.md`](docs/vliw_asm_format.md). The format is intended
for early backend output, regression fixtures, and golden tests.

Current rules:
- a mandatory `.processor { ... }` header declares width, units, slot layout,
  cache, and topology
- the parser accepts either one bundle per non-empty line or brace-delimited
  bundle blocks
- line form uses `|` to separate multiple syllables in the same bundle
- block form uses one `slot: instruction` per line inside `{ ... }`
- labels end with `:` and may prefix a bundle line or a following bundle block
- comments start with `#`
- the first token of each syllable is the slot: `i0`, `i1`, `m`, `x`, or a
  numeric slot index
- `layout slots` may declare explicit slot aliases with `alias <name> = <slot>`
- branches use `branch pN, target` or `branch !pN, target`
- non-branch instructions may use an optional guard prefix like `[p1]` or `[!p2]`
- `movi` is accepted as an alias for `mov_imm`
- loads/stores accept either `dst, base, imm` / `base, src, imm` or bracketed
  memory syntax like `ldd r1, [r0 + 0x20]` and `std [r0 + 0x20], r1`

Minimal example:

```text
.processor {
  width 4

  hardware {
    unit alu = integer_alu
    unit mem = memory
    unit ctrl = control
    unit mul = multiplier
  }

  layout slots {
    alias I0 = 0
    alias I1 = 1
    alias M = 2
    alias X = 3

    0 = { alu }
    1 = { alu }
    2 = { mem }
    3 = { ctrl, mul }
  }

  cache { }
  topology { cpus 1 }
}

start: I0 mov_imm r1, 6 | I1 mov_imm r2, 7
       X mul r3, r1, r2
       I0 nop
       I0 nop
       M store_d r0, r3, 0x100 | X ret
```

The first bundle can also be written in block style:

```text
start:
{
  I0: movi r1, 6
  I1: movi r2, 7
  M : nop
  X : nop
}
```

Here `I0`, `I1`, `M`, and `X` are slot aliases declared in the header. They map
to numeric slot indices, not directly to hardware units: `I0` selects slot 0,
`I1` selects slot 1, `M` selects slot 2, and `X` selects slot 3. The selected
slot's declared unit set then determines whether the opcode is legal. For
compatibility, the parser still has built-in aliases `i0 = 0`, `i1 = 1`,
`m = 2`, and `x = 3`, but new examples should declare aliases explicitly when
using symbolic slot names.

Supported operand shapes, grouped by hardware unit kind:

- `integer_alu`
  - `add/sub/and/or/xor/shl/srl/sra dst, src0, src1`
  - `mov dst, src0`
  - `mov_imm dst, imm`
  - `cmpeq/cmplt/cmpult pdst, src0, src1`
- `memory`
  - `load_b/load_h/load_w/load_d dst, base, imm`
  - `store_b/store_h/store_w/store_d base, src, imm`
  - `acqload dst, base, imm` (acquire-ordered 8-byte load)
  - `relstore base, src, imm` (release-ordered 8-byte store)
  - `lea dst, base, imm`
  - `prefetch base, imm`
- `control`
  - `branch pred, target`
  - `jump target`
  - `call target`
  - `ret`
  - `pand/por/pxor pdst, psrc0, psrc1`
  - `pnot pdst, psrc0`
- `multiplier`
  - `mul/mulh dst, src0, src1`
- `fp { variant ... }`
  - `fpadd32/fpmul32/fpadd64/fpmul64 dst, src0, src1` (placeholder semantics)
- `aes { variant aes_ni }`
  - `aesenc/aesdec dst, src0, src1` (placeholder semantics)
- any slot
  - `nop`

### Check a program with the static verifier

`vliw_verify` checks a `.vliw` file against the compiler/scheduler contract in
[`docs/compiler_contract.md`](docs/compiler_contract.md) without executing the
program. It uses the mandatory `.processor { ... }` header for layout, cache,
and topology.

```sh
cargo run --bin vliw_verify -- examples/clean_schedule.vliw
cargo run --bin vliw_verify -- examples/illegal_raw_same_bundle.vliw
```

Exit codes:
- `0` — clean program
- `1` — compiler-contract violation(s)
- `2` — usage error or parse failure

The verifier emits diagnostics tagged with one of:

- `slot-opcode-legality` — opcode not executable by the slot's declared units
- `same-bundle-gpr-raw` — later syllable reads a GPR an earlier syllable writes
- `same-bundle-gpr-waw` — two syllables write the same GPR in the same bundle
- `same-bundle-pred-hazard` — predicate RAW or WAW within a bundle
- `gpr-ready-cycle` — a GPR source is not ready at issue under the configured
  layout's worst-case load latency
- `bus-slot-conflict` — a memory op is scheduled on a cycle the issuing CPU
  does not own (multi-CPU only)
- `unbounded-polling-loop` — a backward branch over an `acqload` has no
  matching `relstore` on any other CPU (multi-CPU only)

The verifier is deliberately conservative about predicate guards. It treats
every non-`nop` syllable as potentially active, so complementary predicated
writes in the same bundle may run in the simulator but still fail static
verification.

The `vliw_verify` CLI runs single-CPU verification. The whole-system check
that adds the cross-CPU polling-loop pass is exposed as
`verifier::verify_system` for callers that drive multi-CPU programs from Rust.

### Verify with Verus

Build `cargo-verus` from the Verus source tree:

```sh
cd ../verus/source
source ../tools/activate
./tools/get-z3.sh
vargo build
```

Verify this project:

```sh
# from VliwSimulator/
cargo verus verify
```

### Run tests

```sh
cargo test --all-targets
```

GitHub Actions runs `cargo verus verify`, `cargo test --all-targets`, explicit
example checks through `scripts/check_examples.sh`, and coverage on every push
to `main` / `master` and on pull requests. The example check verifies and
simulates every top-level positive example and every single-CPU legal fixture,
asserts expected verifier failures for illegal examples, and runs the multi-CPU
coherence fixture pair through the Rust system tests.

### Measure code coverage

Install `cargo-llvm-cov` once:

```sh
cargo install cargo-llvm-cov --locked --force
```

Generate a local coverage summary and LCOV report:

```sh
cargo llvm-cov --workspace --all-targets --lcov --output-path lcov.info
```

This writes `lcov.info` at the repo root. GitHub Actions also runs the same
coverage command and uploads the LCOV file as a build artifact.

---

## Project layout

```
src/
  main.rs       - CLI runner for text assembly programs
  lib.rs        - crate root
  asm.rs        - text assembly parser and `.processor` header loader
  isa.rs        - Opcode enum, Syllable type, side-effect helpers
  bundle.rs     - runtime-width Bundle with Verus width invariant
  layout.rs     - ProcessorLayout, UnitKind/UnitDecl, slot legality, arch/topology
  program.rs    - Program = layout + bundles
  cache.rs     - direct-mapped L1D cache, MSI line state, coherence specs
  system.rs    - System: shared memory, per-CPU CpuStates, bus arbitration,
                 worst-case visibility / coherence drain
  hw/mod.rs     - hardware-unit helper hooks
  latency.rs    - LatencyTable for non-load opcode cycles
  verifier.rs   - static compiler-contract verifier and proof boundary
  cpu.rs        - thin module wrapper for the CPU implementation
  cpu/
    types.rs    - architectural constants, CpuState, scoreboard types
    spec.rs     - spec helpers used by the verified execution contracts
    state.rs    - well-formedness, constructors (incl. new_for_layout)
    legality.rs - packet legality checks and scoreboard stall checks
    memory.rs   - verified load/store helpers
    execute.rs  - writeback, opcode-family execution, step()
    printer.rs  - human-readable state dump
    trace.rs    - deterministic execution trace mode
  bin/
    vliw_verify.rs - standalone verifier CLI

docs/
  compiler_contract.md             - scheduler legality contract
  vliw_asm_format.md               - stable text assembly format
  processor_layout_plan.md         - layout/cache/multi-CPU rollout plan
  processor_layout_task_breakdown.md - staged implementation breakdown
  adr/                             - per-stage architecture decision records

examples/
  *.vliw                  - clean and intentionally illegal assembly examples
  fixtures/legal/*.vliw   - backend golden fixtures across widths 4, 8, and 16
  fixtures/illegal/*.vliw - rule-tagged failure fixtures

tests/
  smoke.rs    - runtime simulator coverage
  verifier.rs - static verifier and CLI coverage
```

---

## Verus annotations

Verus `spec` and `proof` constructs encode core architectural properties and
connect executable checks to conservative specs:

- `is_valid_width(W)` — bundle width is an integer in [1, 256]
- Loop invariants in `Bundle::nop_bundle` (length grows monotonically)
- Pre-conditions on `Bundle::set_slot` (slot index in range)
- CPU well-formedness facts for register, predicate, memory, scoreboard, and
  cache state (`CpuState::wf`)
- Layout well-formedness (`layout_well_formed`, `arch_supported`,
  `topology_supported`) and program/layout compatibility
  (`program_layout_compatible`)
- Conservative verifier predicates for slot legality and same-bundle GPR /
  predicate hazards, plus soundness lemmas (`lemma_*_implies_active_pair_ok`)
  showing conservative success implies the matching active-pair runtime
  legality condition
- Closed-form bus schedule (`spec_bus_owner`, `spec_bus_slot`,
  `lemma_bus_slot_unique`) and at-most-one-modified MSI invariant
  (`spec_at_most_one_modified_cache_states`,
  `lemma_two_cpu_store_commit_preserves_msi_invariant`)

Functions marked `#[verifier::external]` such as `print_cpu_state` compile and
run normally without entering the proof boundary. The main remaining trusted
surface is executable code marked `#[verifier::external_body]` —
`verify_program` / `verify_program_for_cpu` and `LatencyTable::default`.

---

## Next steps

The next useful work is to turn the simulator from a checked execution model
into a compiler bring-up harness:

1. **Generalize the MSI invariant to N CPUs.** The two-CPU proof in
   `lemma_two_cpu_store_commit_preserves_msi_invariant` is intentionally
   first-cut; generalizing to arbitrary `topology { cpus N }` is the next ADR.
2. **Shrink the trusted verification surface.** Move more of `verify_program`
   and `LatencyTable::default` out of `external_body`, and add specs for the
   GPR ready-cycle diagnostics so timing checks have a formal postcondition
   like the slot and same-bundle hazard checks.
3. **Define ISA edge-case policy.** Decide and document behavior for
   out-of-range memory, misaligned accesses, overflow, trap/exception
   reporting, and FP/AES placeholder semantics.
4. **Build compiler-debug presentation tools.** Add a bundle pretty-printer or
   disassembler, plus an `llvm-mca`-style throughput/stall summary after
   execution.
5. **Exercise real scheduling kernels.** Add DAXPY, FIR, reductions, and small
   control-heavy kernels to validate software pipelining and predicate-heavy
   schedules against both `vliw_verify` and the simulator.
6. **Expose multi-CPU verification in a CLI.** `verify_system` runs the
   cross-CPU polling-loop pass; today it has no `vliw_verify`-style entry
   point.
