# ADR 0004: Stage 4D MSI Coherence

## Status

Accepted for Stage 4D.

## Context

Stage 4D adds private-cache coherence after the shared bus and acquire/release
ordering model. The scheduler contract still forbids runtime stalls: any
coherence overhead must be represented as a verifier-visible worst-case bound.

## Decision

- Cache lines carry an explicit MSI state: `Invalid`, `Shared`, or `Modified`.
- A load transitions the local line to `Shared`.
- A store invalidates matching peer lines, then transitions the writer's line to
  `Modified`.
- The two-CPU invariant is stated and proved at the cache-transition level:
  after a bus-serialized store commit, at most one cache line for the addressed
  line is `Modified`.
- `coherence_drain(layout)` is a closed-form bound:
  - `0` for single-CPU layouts.
  - `layout.cache.writeback_latency` for multi-CPU layouts.

The drain bound represents the maximum cost of draining one dirty owner during
a serialized invalidation/upgrade. It is folded into `worst_case_visibility`
and into verifier load timing. The simulator does not stall an in-flight memory
instruction for coherence.

## Current Model Limits

Stores currently update `SharedMemory` at bus commit time. That makes memory
non-stale relative to a `Modified` cache line, so a remote load does not need to
snoop a dirty owner or force an `M -> S` downgrade for correctness in this
stage. If the cache later becomes truly write-back with stale shared memory,
remote loads must add dirty-owner writeback/downgrade behavior.

The invariant proof is intentionally two-CPU first. Generalizing the invariant
to N CPUs remains open and should happen only after the two-CPU proof remains
stable under subsequent coherence changes.

## Consequences

- Store visibility follows the bus commit order.
- At most one cache can hold the addressed line in `Modified` after a
  two-CPU store commit.
- The Stage 4D implementation adds no new `external_body`.
- Documentation debt remains for earlier processor-layout stages, which were
  merged without ADRs.
