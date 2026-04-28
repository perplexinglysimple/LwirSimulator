# Backend Golden Fixtures

These `.lwir` programs are stable scheduler fixtures for backend tests.

Legal fixtures must parse and pass `lwir_verify` with no diagnostics. Illegal
fixtures must parse and produce the expected verifier rule named in the file
header.

Widths covered here: `4`, `8`, and `16`.

CI runs the legal fixtures through both `lwir_verify` and
`lwir_simulator --trace`; illegal fixtures are checked with `lwir_verify` and
must report their expected rule tags.
