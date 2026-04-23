# LwirSimulator

A cycle-approximate simulator for a **VLIW** (Very Long Instruction Word) processor,
written in Rust and formally verified with [Verus](https://github.com/verus-lang/verus).

The primary purpose of this project is to provide a ground-truth execution
environment while you develop and test a compiler that targets the LWIR ISA.

---

## Architecture overview

The ISA is modeled after the *FVLIW-64/4* design described in the background
documents with the following deliberate simplifications that keep the simulator
easy to target from a compiler:

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

---

## Configurable latencies

Every opcode has a per-instance latency that can be overridden before
constructing the CPU:

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

---

## Reading processor state

`CpuState` is a plain Rust struct — all fields are `pub`.  You can snapshot
or inspect it at any time:

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

The state includes:
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
- [Verus](https://github.com/verus-lang/verus) source at `/home/george/repos/verus`
  (the `Cargo.toml` references it via `path =`)

### Build and run the hello-world demo

```sh
cargo run
```

Expected output:

```
LWIR VLIW Simulator — hello world (W=4)

Running 4 bundles…

=== LWIR Processor State (width=4) ===
  PC: 4  Cycle: 4  Halted: true
  GPRs:
    r1  = 0x0000000000000006  (6)
    r2  = 0x0000000000000007  (7)
    r3  = 0x000000000000002a  (42)
  Predicate registers:
    p0 = true
==========================================

All assertions passed — 6 × 7 = 42 ✓
```

### Verify with Verus

First, build `cargo-verus` from the Verus source tree:

```sh
cd /home/george/repos/verus/source
source ../tools/activate
./tools/get-z3.sh
vargo build
```

Then verify this project:

```sh
# from LwirSimulator/
cargo verus verify
```

---

## Project layout

```
src/
  main.rs      — hello-world demo program
  lib.rs       — crate root
  isa.rs       — opcodes, slot classes, Syllable type
  bundle.rs    — Bundle<W> with Verus width invariant
  cpu.rs       — CpuState<W>, execution engine, state printer
  latency.rs   — LatencyTable (configurable per-opcode cycles)
```

---

## Verus annotations

Verus `spec` and `proof` constructs are used to state and verify:

- `is_valid_width(W)` — bundle width is a power-of-two in [4, 256]
- Loop invariants in `Bundle::nop_bundle` (length grows monotonically)
- Pre-conditions on `Bundle::set_slot` (slot index in range)

Functions marked `#[verifier::external]` (like `print_cpu_state`) are
excluded from verification but still compile and run normally.

---

## Planned work

- [ ] Hazard detection: enforce no same-bundle RAW/WAW at runtime
- [ ] Stall insertion: hold `pc` when a consumer reads before `ready_cycle`
- [ ] Software-pipelining test kernels (DAXPY, FIR)
- [ ] `llvm-mca`-style throughput report after execution
- [ ] Disassembler / pretty-printer for bundles
- [ ] Integration test harness for the compiler-under-development
