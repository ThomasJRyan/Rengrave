# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust port of F-Engrave. Keep `f-engrave_source/` intact as the upstream behavior and licensing reference. New Rust code lives under `crates/`:

- `crates/rengrave-core`: portable settings, geometry, parsers, toolpath logic, and tests.
- `crates/rengrave-cli`: batch/command-line entry point.
- `crates/rengrave-ui`: `eframe/egui` desktop UI.

Use `PLAN.md` as the current porting roadmap. Add fixtures under a future `fixtures/` or crate-local `tests/` directory when golden F-Engrave outputs are created.

## Build, Test, and Development Commands

- `cargo fmt --all`: format every workspace crate.
- `cargo test -p rengrave-core`: run focused core unit tests.
- `cargo test --workspace`: run all Rust tests after broader changes.
- `cargo run -p rengrave-cli -- -b -t "Text"`: exercise batch-mode compatibility output.
- `cargo run -p rengrave-ui`: launch the desktop UI shell.

## Coding Style & Naming Conventions

Use standard Rust formatting from `rustfmt`; do not hand-align large blocks. Prefer clear module boundaries over broad utility files. Name crates and modules with `snake_case` paths, Rust types with `PascalCase`, and functions/variables with `snake_case`. Keep compatibility-specific names close to F-Engrave terminology when they map to legacy settings such as `fengrave_set`, `TCODE`, or `v_bit_angle`.

## Testing Guidelines

Write focused unit tests beside the code they verify. Use golden-output tests for F-Engrave parity once fixtures exist, with tolerances for floating-point coordinates and stricter byte checks where formatting is intentional. Agents should run focused tests relevant to the changed code unless the user explicitly requests the full suite.

## Commit & Pull Request Guidelines

There is no existing commit history yet. Use Conventional Commit messages such as `feat: add settings parser`, `fix: preserve TCODE newlines`, or `test: add default text fixture`. Create one commit per completed feature, bug fix, or general change. PRs should describe the compatibility impact, list tests run, link related issues, and include screenshots for UI changes.

## Licensing & Compatibility Notes

F-Engrave is GPLv3-or-later, while `f-engrave_source/TTF2CXF_STREAM/` is GPLv2-only. Do not copy the helper into the Rust binary; reimplement TTF behavior in Rust. Treat generated G-code/settings compatibility as the primary contract.
