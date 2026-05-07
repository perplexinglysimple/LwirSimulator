# VLIW Assembly Format (Stable Bundle-Level IR)

This document defines the stable text format emitted by the LLVM backend for the VLIW simulator.

## 1. File structure

A file is plain text with optional comments and labels.

- Comments start with `#` and run to end-of-line.
- Mandatory processor header:
  - `.processor { ... }`
  - Must declare `width`, hardware units, `layout slots`, cache config, and `topology { cpus 1 }`.
  - May declare memory capacity with `memory { size 0x10000 }`; otherwise the default is 64 KiB.
  - Legacy `.width <N>` files are rejected.
- Program is a sequence of **bundles**.

## 2. Bundle encodings

Two equivalent encodings are supported.

### A) Inline bundle form

One line = one bundle, with slot syllables separated by `|`.

```text
i0 movi r1, 6 | i1 movi r2, 7 | m nop | x nop
```

### B) Block bundle form

A `{ ... }` block = one bundle. Each line inside names one slot.

```text
{
  i0: movi r1, 6
  i1: movi r2, 7
  m : nop
  x : nop
}
```

## 3. Labels

Labels map to bundle indices (not byte offsets).

## Processor Memory

The processor has a byte-addressed little-endian memory. The default size is
64 KiB. A header may override it with either `memory { size N }` or the legacy
`arch { memory N }` spelling.

```text
memory { size 0x10000 }
```

## Processor Cache

`cache { }` remains accepted and uses the default direct-mapped L1D configuration:
64-byte lines, 4 KiB capacity, hit latency 1, miss latency 3, and no dirty
writeback penalty.

Concrete direct-mapped L1D config is also supported:

```text
cache {
  l1d {
    line_bytes 64
    capacity 4096
    associativity 1
    write_policy write_back
    hit_latency 1
    miss_latency 12
    writeback_latency 12
  }
}
```

Load latency is derived from the cache. Stores update the cache line and dirty
bit, while their visible opcode latency still comes from the non-load latency
table.

```text
start:
{
  i0: movi r1, 1
  i1: nop
  m : nop
  x : jump done
}

...

done: x ret
```

Rules:
- Label syntax: `[A-Za-z_.$][A-Za-z0-9_.$]*`.
- Label references are accepted for control targets (`br`/`branch`,
  `jmp`/`jump`, and `call`); numeric bundle targets are also accepted.
- Duplicate labels are illegal.
- Unknown label references are illegal.

## 4. Slots

Slots can be written as symbolic names or numeric indices.

- Explicit aliases: declare `alias <name> = <slot>` inside `layout slots`.
- Built-in compatibility aliases: `i0 = 0`, `i1 = 1`, `m = 2`, `x = 3`.
- Numeric: `0..W-1`.

Aliases name slot indices, not hardware unit classes. After an alias resolves
to a numeric slot, the slot's declared unit set controls whether the opcode may
issue there.

For wider bundles, numeric slots are recommended. The canonical stage-0 layout repeats every 4 slots (`I, I, M, X`) by assigning `alu`, `alu`, `mem`, and `ctrl, mul` units.

## 5. Predication syntax

Most non-branch ops may use a guard token before opcode:

- `[pN]` execute when predicate true.
- `[!pN]` execute when predicate false.

Example:

```text
i0 [p1] movi r4, 1 | i1 [!p1] movi r4, 0
```

The runtime accepts complementary predicated writes to the same destination because exactly one slot is active per cycle. The static verifier (`vliw_verify`) is deliberately conservative: it does not evaluate guard predicates and treats all non-nop syllables as unconditionally active. The pattern above would be flagged as a same-bundle WAW. Compilers targeting `vliw_verify` must avoid same-destination writes within a bundle regardless of guard complementarity.

Branch is special and carries predicate as an operand:

```text
x branch p1 target
x branch !p1 target
```

Guard syntax (`[p1]`) is not used with `branch`.

## 6. Operand syntax

- GPRs: `r0`, `r1`, ...
- Predicates: `p0`, `p1`, ...
- Immediates: decimal (`42`, `-8`) or hex (`0x100`, `-0x20`)

Load-like forms (`load*`, `lea`):
- `op dst, base, imm`
- `op dst, [base + imm]`

Store-like forms (`store*`):
- `op base, src, imm`
- `op [base + imm], src`

## 7. Opcode spellings

The parser accepts canonical mnemonics and aliases.

Examples:
- `movi`, `movimm`, `mov_imm`
- `ldd` / `load_d`
- `std` / `store_d`
- `br` / `branch`
- `jmp` / `jump`
- `acqload` (acquire-ordered 8-byte load), `relstore` (release-ordered
  8-byte store)
- `fpadd32`, `fpmul32`, `fpadd64`, `fpmul64` (placeholder FP semantics; require
  a slot with an `fp { variant ... }` unit)
- `aesenc`, `aesdec` (placeholder AES round; require a slot with an
  `aes { variant aes_ni }` unit)

Backend recommendation: emit one canonical spelling per opcode for stability.

## 8. Legality expectations for backend output

Assembly syntax validity is necessary but not sufficient. Backend output must
also satisfy the scheduler contract in `docs/compiler_contract.md`:

- legal width
- slot/opcode legality against the declared layout's per-slot units
- no same-bundle RAW/WAW/predicate hazards
- readiness-aware scheduling under the cache- and topology-derived worst-case
  load latency
- (multi-CPU) memory ops scheduled on the issuing CPU's bus slot
- (multi-CPU) every `acqload` polling loop has a matching producer `relstore`
  on some other CPU

## 9. Minimal canonical example

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

entry:
{
  I0: movi r1, 6
  I1: movi r2, 7
  M : nop
  X : nop
}

{
  I0: nop
  I1: nop
  M : nop
  X : mul r3, r1, r2
}

{
  I0: nop
  I1: nop
  M : std [r0 + 0x100], r3
  X : ret
}
```
