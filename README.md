# LwirSimulator

A cycle-approximate **VLIW** (Very Long Instruction Word) simulator written in
Rust and verified with [Verus](https://github.com/verus-lang/verus).

The simulator is the architectural reference for the LWIR ISA and the execution
model for compiler bring-up, scheduling experiments, and backend validation.

---

## Architecture overview

LWIR is a statically scheduled VLIW ISA. Bundle structure, slot classes, and
operation placement are explicit in the instruction stream rather than inferred
by dynamic issue hardware. The simulator keeps that model concrete and
compiler-facing: resource usage is visible, latency is modeled explicitly, and
architectural state evolves in bundle order.

The current implementation follows a conservative FVLIW-style design point that
keeps backend work tractable while still exercising the important VLIW problems:
packet legality, latency modeling, predicate handling, and memory/control slot
constraints.

| Property | Value |
|---|---|
| **Bundle width W** | Compile-time const, power-of-2 in **[4 … 256]** |
| **Slot classes** per 4-slot group | `I0`, `I1`, `M`, `X` (integer×2, memory, control/mul) |
| **GPRs** | 32 × 64-bit (`r0` = 0 hardwired) |
| **Predicate registers** | 16 × 1-bit (`p0` = true hardwired) |
| **Memory** | 64 KiB byte-addressed, little-endian |
| **Exception model** | Precise by slot order; no speculative loads in v1 |
| **Memory ordering** | Relaxed base + `LD.ACQ`/`ST.REL`/`FENCE` (ISA stubs present) |

### Bundle width

The bundle width `W` is a **Rust const generic**:

```rust
const W: usize = 4;   // 4, 8, 16, 32, 64, 128, or 256
let cpu = CpuState::<W>::new(latencies);
```

Slot classes cycle through `I, I, M, X` modulo 4 regardless of `W`.
A 16-wide bundle therefore has four groups of `(I0, I1, M, X)`.

This preserves the core VLIW rule that issue structure is architectural. A
bundle is not a bag of interchangeable ops; each slot maps to a specific class
of work.

---

## Configurable latencies

The machine model assigns an explicit latency to every opcode. The latency table
is configurable before CPU construction:

```rust
let mut latencies = LatencyTable::default();
latencies.set(Opcode::Mul,   5);   // model a slow multiplier
latencies.set(Opcode::LoadD, 10);  // model high-latency DRAM
let cpu = CpuState::<W>::new(latencies);
```

Default latencies (in cycles):

| Class | Latency |
|---|---|
| Integer ALU, LEA | 1 |
| Load (any width) | 3 |
| Store, Prefetch | 1 |
| Multiply (MUL/MULH) | 3 |
| Control (BR, J, CALL, RET) | 1 |
| Predicate ops | 1 |
| NOP | 0 |

This matches the intended use of the simulator as a cycle-aware reference for
compiler scheduling work. The model does not hide latency behind dynamic issue
or out-of-order execution.

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
lwir_simulator::cpu::print_cpu_state(&cpu);

// clone the full state for checkpointing
let checkpoint = cpu.clone();
```

Architectural state includes:
- `gprs: Vec<u64>` — all 32 general-purpose registers
- `preds: Vec<bool>` — all 16 predicate registers
- `pc: usize` — bundle-level program counter
- `cycle: u64` — cycle counter (includes stall modeling)
- `scoreboard: Vec<ScoreboardEntry>` — per-GPR ready cycle
- `memory: Vec<u8>` — 64 KiB flat address space
- `halted: bool` — set when `RET` is executed with `lr == 0`

---

## Getting started

### Prerequisites

- Rust (stable, edition 2021)
- [Verus](https://github.com/verus-lang/verus) checked out as a sibling repo at
  `../verus` relative to this project

### Build and run a text assembly program

```sh
cargo run --bin lwir_simulator -- examples/hello.lwir
```

The simulator reads `.width <n>` when present and dispatches to widths
`4, 8, 16, 32, 64, 128, 256`; files without a width directive default to `W=4`.

Expected output:

```
LWIR VLIW Simulator (W=4)
Program: examples/hello.lwir
Bundles: 3

=== LWIR Processor State (width=4) ===
  PC: 3  Cycle: 7  Halted: true
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
cargo run --bin lwir_simulator -- --trace examples/hello.lwir
```

The trace format is line-oriented and starts with `trace v1 width=<n>`. Each
event records the bundle index, cycle, issue/stall/illegal outcome, active
non-`nop` syllables, stalls, GPR writes, predicate writes, memory effects, and
branch/jump/call/return decisions, followed by a final `pc/cycle/halted` line.

### Text assembly format

`lwir_simulator` consumes the stable bundle-level text format documented in
[`docs/lwir_asm_format.md`](docs/lwir_asm_format.md). The format is intended for
early backend output, regression fixtures, and golden tests.

Current rules:
- the parser accepts either one bundle per non-empty line or brace-delimited bundle blocks
- line form uses `|` to separate multiple syllables in the same bundle
- block form uses one `slot: instruction` per line inside `{ ... }`
- labels end with `:` and may prefix a bundle line or a following bundle block
- optional `.width <n>` must match the compiled bundle width
- comments start with `#`
- the first token is the slot: `i0`, `i1`, `m`, `x`, or a numeric slot index
- branches use `branch pN, target` or `branch !pN, target`
- non-branch instructions may use an optional guard prefix like `[p1]` or `[!p2]`
- `movi` is accepted as an alias for `mov_imm`
- loads/stores accept either `dst, base, imm` / `base, src, imm` or bracketed memory syntax like `ldd r1, [r0 + 0x20]` and `std [r0 + 0x20], r1`

Example:

```text
start: i0 mov_imm r1, 6 | i1 mov_imm r2, 7
       x mul r3, r1, r2
       m store_d r0, r3, 0x100
       x ret
```

Block-style example:

```text
.width 4

start:
{
  I0: movi r1, 10
  I1: movi r2, 20
  M : nop
  X : nop
}
```

Supported operand shapes:
- `add/sub/and/or/xor/shl/srl/sra/mul/mulh dst, src0, src1`
- `mov dst, src0`
- `mov_imm dst, imm`
- `cmpeq/cmplt/cmpult pdst, src0, src1`
- `load_b/load_h/load_w/load_d dst, base, imm`
- `store_b/store_h/store_w/store_d base, src, imm`
- `lea dst, base, imm`
- `prefetch base, imm`
- `branch pred, target`
- `jump target`
- `call target`
- `ret`
- `pand/por/pxor pdst, psrc0, psrc1`
- `pnot pdst, psrc0`
- `nop`

### Check a program with the static verifier

`lwir_verify` checks a `.lwir` / `.lwirasm` file against the compiler/scheduler
contract in [`docs/compiler_contract.md`](docs/compiler_contract.md) without
executing the program. It reads `.width <n>` when present and supports widths
`4, 8, 16, 32, 64, 128, 256`; files without a width directive default to `W=4`.

```sh
cargo run --bin lwir_verify -- examples/clean_schedule.lwir
cargo run --bin lwir_verify -- examples/illegal_raw_same_bundle.lwir
```

Exit codes:
- `0` - clean program
- `1` - compiler-contract violation(s)
- `2` - usage error or parse failure

The verifier is deliberately conservative about predicate guards. It treats
every non-`nop` syllable as potentially active, so complementary predicated
writes in the same bundle may run in the simulator but still fail static
verification.

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
# from LwirSimulator/
cargo verus verify
```

### Run tests

```sh
cargo test --all-targets
```

GitHub Actions runs `cargo verus verify`, `cargo test --all-targets`, explicit
`lwir_simulator --trace` runs over the legal golden fixtures, explicit
`lwir_verify` runs over the legal and illegal golden fixtures, and coverage on
every push to `main` / `master` and on pull requests.

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

The current local baseline is approximately:
- total line coverage: `98%`
- `cpu` module line coverage: `97%`

---

## Project layout

```
src/
  main.rs      - CLI runner for text assembly programs
  lib.rs       - crate root
  asm.rs       - text assembly parser and loader
  isa.rs       - opcodes, slot classes, Syllable type
  bundle.rs    - Bundle<W> with Verus width invariant
  verifier.rs  - static compiler-contract verifier and proof boundary
  cpu.rs       - thin module wrapper for the CPU implementation
  bin/
    lwir_verify.rs - standalone verifier CLI
  cpu/
    types.rs   - architectural constants, CpuState, scoreboard types
    spec.rs    - spec helpers used by the verified execution contracts
    state.rs   - well-formedness, constructor, register/predicate accessors
    legality.rs - packet legality checks and scoreboard stall checks
    memory.rs  - verified load/store helpers
    execute.rs - writeback, opcode-family execution, step()
    printer.rs - human-readable state dump
    trace.rs   - deterministic execution trace mode
  latency.rs   - LatencyTable (configurable per-opcode cycles)

docs/
  compiler_contract.md - scheduler legality contract
  lwir_asm_format.md   - stable text assembly format

examples/
  *.lwir       - clean and intentionally illegal assembly examples
  fixtures/    - backend golden fixtures across widths 4, 8, and 16

tests/
  smoke.rs     - runtime simulator coverage
  verifier.rs  - static verifier and CLI coverage
```

---

## Verus annotations

Verus `spec` and `proof` constructs encode core architectural properties and
connect some executable checks to conservative specs:

- `is_valid_width(W)` — bundle width is a power-of-two in [4, 256]
- Loop invariants in `Bundle::nop_bundle` (length grows monotonically)
- Pre-conditions on `Bundle::set_slot` (slot index in range)
- CPU well-formedness facts for register, predicate, memory, and scoreboard state
- Conservative verifier predicates for slot legality and same-bundle GPR/predicate hazards
- Soundness lemmas showing conservative verifier success implies the matching
  active-pair runtime legality condition

Functions marked `#[verifier::external]` such as `print_cpu_state` compile and
run normally without entering the proof boundary. The main remaining trusted
surface is executable code marked `#[verifier::external_body]`, especially the
static verifier entry point and latency-table defaults.

---

## Current status

The project has moved past first simulator bring-up:

- [x] Runtime bundle legality checks for slot class, same-bundle RAW/WAW, predicate hazards, and call/return link-register hazards
- [x] Scoreboard stalls for read-before-ready GPR dependencies
- [x] Stable bundle-level text assembly format with examples
- [x] Standalone `lwir_verify` CLI for static compiler-contract checks
- [x] Deterministic trace mode for scheduler debugging (`lwir_simulator --trace`)
- [x] Backend-facing legal/illegal golden fixtures across widths `4`, `8`, and `16`
- [x] Verus specs and lemmas for key bundle/verifier legality properties
- [x] Runtime, parser, verifier, and CLI tests with CI coverage artifacts

## Next steps

The next useful work is to turn the simulator from a checked execution model
into a compiler bring-up harness:

1. **Synchronize the docs with the implementation.** Update
   `docs/compiler_contract.md` so its enforcement table describes the current
   static verifier instead of planned checks, including width dispatch,
   conservative predicate handling, call-as-`r31` writer behavior, and the
   distinction between static timing diagnostics and runtime stalls.
2. **Shrink the trusted verification surface.** Move more of `verify_program`
   and `LatencyTable::default` out of `external_body`, and add specs for the
   GPR ready-cycle diagnostics so timing checks have a formal postcondition like
   the slot and same-bundle hazard checks.
3. **Define ISA edge-case policy.** Decide and document behavior for
   out-of-range memory, misaligned accesses, overflow, trap/exception reporting,
   and the currently stubbed memory-ordering opcodes.
4. **Build compiler-debug presentation tools.** Add a bundle pretty-printer or
   disassembler, plus an `llvm-mca`-style throughput/stall summary after
   execution.
5. **Exercise real scheduling kernels.** Add DAXPY, FIR, reductions, and small
   control-heavy kernels to validate software pipelining and predicate-heavy
   schedules against both `lwir_verify` and the simulator.
