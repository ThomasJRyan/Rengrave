# Golden Fixtures

This directory holds checked inputs and expected outputs for generated-output
tests.

- `inputs/` contains small source files used by the harness.
- `expected/` contains currently checked R-Engrave regression outputs.

The harness in `../golden.rs` is intentionally shaped for F-Engrave parity
fixtures: add reference-generated `.ngc`, `.svg`, or `.dxf` outputs here and
compare them from the integration test. In this environment the bundled
F-Engrave script cannot currently run because Python imports `pyclipper` before
batch mode starts, so the first fixture pins current Rust output until
F-Engrave reference outputs can be generated.
