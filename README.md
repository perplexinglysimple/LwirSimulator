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
cargo run -- examples/hello.lwir
```

Expected output:

```
LWIR VLIW Simulator (W=4)
Program: examples/hello.lwir
Bundles: 4

=== LWIR Processor State (width=4) ===
  PC: 4  Cycle: 4  Halted: true
  GPRs:
    r1  = 0x0000000000000006  (6)
    r2  = 0x0000000000000007  (7)
    r3  = 0x000000000000002a  (42)
  Predicate registers:
    p0 = true
==========================================
```

### Text assembly format

`main` currently consumes a simple text assembly file. This is a temporary
compiler-facing format until the project grows a more standard binary/program
container.

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

GitHub Actions runs both `cargo verus verify` and `cargo test --all-targets`
on every push to `main` and on pull requests.

### Measure code coverage

Install `cargo-llvm-cov` once:

```sh
cargo install cargo-llvm-cov --locked
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
  main.rs      — CLI runner for text assembly programs
  lib.rs       — crate root
  asm.rs       — text assembly parser and loader
  isa.rs       — opcodes, slot classes, Syllable type
  bundle.rs    — Bundle<W> with Verus width invariant
  cpu.rs       — thin module wrapper for the CPU implementation
  cpu/
    types.rs   — architectural constants, CpuState, scoreboard types
    spec.rs    — spec helpers used by the verified execution contracts
    state.rs   — well-formedness, constructor, register/predicate accessors
    legality.rs — packet legality checks and scoreboard stall checks
    memory.rs  — verified load/store helpers
    execute.rs — writeback, opcode-family execution, step()
    printer.rs — human-readable state dump
  latency.rs   — LatencyTable (configurable per-opcode cycles)
```

---

## Verus annotations

Verus `spec` and `proof` constructs encode and verify core architectural
properties:

- `is_valid_width(W)` — bundle width is a power-of-two in [4, 256]
- Loop invariants in `Bundle::nop_bundle` (length grows monotonically)
- Pre-conditions on `Bundle::set_slot` (slot index in range)

Functions marked `#[verifier::external]` such as `print_cpu_state` compile and
run normally without entering the proof boundary.

---

## Planned work

- [x] Hazard detection: enforce no same-bundle RAW/WAW at runtime
- [x] Stall insertion: hold `pc` when a consumer reads before `ready_cycle`
- [x] Broad runtime coverage with CI-published LCOV output
- [ ] Software-pipelining test kernels (DAXPY, FIR)
- [ ] `llvm-mca`-style throughput report after execution
- [ ] Disassembler / pretty-printer for bundles
- [ ] Program loader / external input format for compiler-generated bundles
- [ ] Deterministic trace mode for compiler and scheduler debugging

## Pre-Compiler TODO

The simulator is verified and usable, but it still needs several cleanup and
correctness passes before it is a strong compiler-development target.

- [x] Enforce same-bundle legality rules instead of executing illegal packets silently
- [x] Implement scoreboard-based stall behavior for read-before-ready hazards
- [x] Add negative tests for illegal bundles and latency-unsafe issue patterns
- [x] Expand tests from smoke coverage to opcode-by-opcode execution coverage
- [x] Add targeted tests for predication, control flow, loads/stores, and return semantics
- [x] Add CI coverage reporting and establish a high-coverage baseline
- [ ] Decide and document the intended behavior for out-of-range addresses and other ISA edge cases
- [ ] Reduce trusted verification surface further, especially remaining `external_body` items such as `LatencyTable::default`
- [ ] Add a deterministic trace or execution log mode for compiler debugging
- [ ] Add a bundle/program text format or loader so compiler output can run directly in the simulator
- [ ] Add a disassembler or pretty-printer suitable for golden tests and backend debugging
