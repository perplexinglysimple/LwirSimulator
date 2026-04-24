# LWIR Assembly Format (Stable Bundle-Level IR)

This document defines the stable text format emitted by the LLVM backend for the LWIR simulator.

## 1. File structure

A file is plain text with optional comments and labels.

- Comments start with `#` and run to end-of-line.
- Optional width directive:
  - `.width <N>`
  - Must match simulator/parser width parameter (`parse_program::<N>`).
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
- Label syntax: `[A-Za-z_][A-Za-z0-9_]*`.
- Duplicate labels are illegal.
- Unknown label references are illegal.

## 4. Slots

Slots can be written as symbolic names or numeric indices.

- Symbolic: `i0`, `i1`, `m`, `x`.
- Numeric: `0..W-1`.

For wider bundles, numeric slots are recommended; architectural classes repeat every 4 slots (`I, I, M, X`).

## 5. Predication syntax

Most non-branch ops may use a guard token before opcode:

- `[pN]` execute when predicate true.
- `[!pN]` execute when predicate false.

Example:

```text
i0 [p1] movi r4, 1 | i1 [!p1] movi r4, 0
```

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

Backend recommendation: emit one canonical spelling per opcode for stability.

## 8. Legality expectations for backend output

Assembly syntax validity is necessary but not sufficient. Backend output must also satisfy the scheduler contract in `docs/compiler_contract.md`:

- legal width
- slot/opcode class legality
- no same-bundle RAW/WAW/predicate hazards
- readiness-aware scheduling for scoreboard timing

## 9. Minimal canonical example

```text
.width 4

entry:
{
  i0: movi r1, 6
  i1: movi r2, 7
  m : nop
  x : nop
}

{
  i0: nop
  i1: nop
  m : nop
  x : mul r3, r1, r2
}

{
  i0: nop
  i1: nop
  m : std [r0 + 0x100], r3
  x : ret
}
```
