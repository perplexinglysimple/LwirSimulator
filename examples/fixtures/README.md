# Backend Golden Fixtures

These `.lwir` programs are stable scheduler fixtures for backend tests.

Legal fixtures must parse and pass `lwir_verify` with no diagnostics. Illegal
fixtures must parse and produce the expected verifier rule named in the file
header.

Widths covered here: `4`, `8`, and `16`.
