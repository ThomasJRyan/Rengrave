# Golden Fixtures

This directory holds checked inputs and expected outputs for generated-output
tests.

- `inputs/` contains small source files used by the harness.
- `expected/` contains currently checked R-Engrave regression outputs.

The harness in `../golden.rs` is intentionally shaped for F-Engrave parity
fixtures: add reference-generated `.ngc`, `.svg`, or `.dxf` outputs here and
compare them from the integration test. G-code comparisons use numeric
tolerance so equivalent floating-point formatting can be accepted while keeping
non-numeric lines exact.

Current local blocker: on 2026-06-10, running
`python f-engrave_source/f-engrave.py -b -f crates/rengrave-core/tests/fixtures/inputs/simple.cxf -t AB`
fails before batch mode with `ModuleNotFoundError: No module named 'pyclipper'`.
Until F-Engrave reference outputs can be generated in an environment with that
dependency, the first fixture pins current Rust output only.
