# Agent Instructions

## Change Discipline

- Every completed feature, bug fix, or general code change must be committed.
- Use one semantic commit per completed change. Use Conventional Commit subjects such as `feat: add profile alignment`, `fix: preserve top-left origin`, `docs: describe v-carve geometry`, `test: cover profile sizing`, or `chore: update documentation tooling`.
- Keep commits focused. Do not combine unrelated cleanup, feature work, and documentation changes in one commit unless the documentation is required by the code change.
- Before committing, inspect the diff, run the focused validation for the change, and run `git diff --check`.

## Performance and Parallelism

R-Engrave should generate G-Code as quickly as practical without changing the
quality, geometry, ordering, formatting, or compatibility of its output.

- Parallelize independent CPU work wherever it is safe and beneficial,
  including independent contours, toolpath operations, exports, parsing or
  preprocessing chunks, and other read-only calculations. Prefer the existing
  Rayon-based approach and keep parallel work in portable core code when
  possible.
- Preserve deterministic results. Collect parallel results in a defined order,
  use stable reductions where ordering affects floating-point or text output,
  and compare generated G-Code and companion outputs against existing golden
  or byte-identical results whenever the implementation changes.
- Do not trade machining quality for throughput. Preserve tolerances,
  candidate selection, path ordering semantics, depth calculations, units,
  cancellation behavior, and legacy F-Engrave compatibility unless a change is
  explicitly requested and documented.
- Avoid data races and oversubscription. Shared inputs must be immutable or
  synchronized, cancellation and progress callbacks must satisfy the required
  thread-safety bounds, and UI work must remain responsive rather than blocking
  the render thread. Prefer coarse independent work units over fine-grained
  tasks that add more scheduling overhead than useful computation.
- Measure performance on representative dense image and vector workloads in
  both normal and single-threaded configurations. Record meaningful timing or
  allocation evidence, verify that parallelism improves the target workload,
  and retain an appropriate way to limit worker threads such as
  ``RAYON_NUM_THREADS`` when users need CPU headroom.
- Add focused regression coverage for every parallelized algorithm, including
  cancellation, empty or degenerate inputs, deterministic output, and any
  quality-sensitive geometry or serialization behavior.

## Project Structure

This repository is a Rust port of F-Engrave. Keep `f-engrave_source/` intact as the upstream behavior and licensing reference. New Rust code lives under `crates/`:

- `crates/rengrave-core`: portable settings, geometry, parsers, toolpath logic, exporters, and core tests.
- `crates/rengrave-cli`: batch and command-line entry point.
- `crates/rengrave-ui`: `eframe/egui` desktop UI and egui harness tests.
- `docs/`: the self-contained Sphinx documentation project, including reStructuredText user/developer pages, diagrams, screenshots, custom stylesheet, `pyproject.toml`, `uv.lock`, `conf.py`, `Makefile`, and generated output.

Use `PLAN.md` as the current porting roadmap. Add fixtures under a future `fixtures/` or crate-local `tests/` directory when golden F-Engrave outputs are created.

## Documentation Requirements

Documentation is part of every feature, bug fix, and behavior-changing change.

- Write documentation in reStructuredText (`.rst`) under `docs/`.
- Update the user-facing documentation for every user-visible feature, setting, workflow, output change, and bug fix. Describe what the user sees, the controls involved, defaults, constraints, and expected output.
- Update developer documentation for every feature and algorithmic or compatibility change. Describe the relevant modules, data flow, invariants, formulas, tolerances, coordinate systems, units, and compatibility decisions so another developer can implement or debug the behavior.
- Every R-Engrave feature must have both a user explanation and a developer/algorithm explanation. Put algorithm notes in an appropriate `docs/developer/` or `docs/algorithms/` document and link them from the documentation index.
- Include diagrams and application screenshots wherever they clarify geometry, coordinate systems, toolpaths, UI behavior, or a user workflow. Store source and raster assets under `docs/` in a descriptive subdirectory, and keep screenshots representative of the current UI.
- Keep documentation accurate when behavior changes. Remove or rewrite stale instructions rather than leaving contradictory historical guidance.
- Use a structure such as:

  - `docs/index.rst`: documentation entry point and table of contents.
  - `docs/user/`: workflows, settings, importing, previewing, and exporting.
  - `docs/developer/`: architecture, compatibility, testing, and extension notes.
  - `docs/algorithms/`: geometry, layout, tracing, v-carve, cleanup, and toolpath mathematics.
  - `docs/_static/`: custom CSS and other documentation assets.
  - `docs/_images/`: diagrams and screenshots.
  - `docs/_build/`: generated HTML output; do not hand-edit generated files.

## Documentation Build

Build the documentation from `docs/` with Sphinx and `uv`. The documentation
environment owns its Python dependencies and generated output:

```sh
cd docs
uv sync
make html
```

Use `make serve` to build and serve the site locally, or `make linkcheck` to
check links. Verify the generated HTML contains the expected sections, links,
images, and stylesheet reference. Keep custom styling in `docs/_static/rengrave.css`;
do not embed one-off styling in generated HTML. Do not use the old standalone
`rst2html` workflow.

## Build and Development Commands

- `cargo fmt --all`: format every workspace crate.
- `cargo run -p rengrave-ui`: launch the desktop UI shell for manual verification.
- `cargo run -p rengrave-cli -- -b -t "Text"`: exercise batch-mode compatibility output.
- `make -C docs html`: build the Sphinx documentation site.
- `make -C docs serve`: build and serve the site at `http://127.0.0.1:8000`.
- `make -C docs linkcheck`: check documentation links.

## Testing Requirements

Every feature, bug fix, and general code change must include unit-test coverage appropriate to the changed behavior. Do not consider a change complete without tests.

- Add focused Rust unit tests beside the code they verify.
- Add focused integration or golden-output tests when the change affects generated G-code, settings compatibility, geometry, parsers, exporters, or cross-crate behavior.
- Add egui harness tests for every UI behavior that can be exercised through the harness, including controls, default values, rendering, interaction, stale/recalculation state, visible warnings, and relevant responsive/layout behavior.
- For UI changes, include a screenshot or harness assertion when visual behavior is part of the contract.
- Use tolerances for floating-point geometry and strict byte/text comparisons where formatting or compatibility is intentional.
- Run focused tests relevant to the changed code. Do not run the full workspace suite by default; run `cargo test --workspace` only when the user explicitly requests it or when the change is sufficiently cross-cutting that focused tests cannot provide truthful coverage, and record that reason.
- Before committing, run `cargo fmt --all --check` and `git diff --check` in addition to the focused tests.

Typical focused commands are:

```sh
cargo test -p rengrave-core profile::tests::profile_dimensions_and_ratio_resize_the_profile_envelope
cargo test -p rengrave-core --test golden simple_cxf_text_matches_checked_golden_outputs
cargo test -p rengrave-ui --lib ui_controls_emit_core_overrides
cargo test -p rengrave-ui --lib kittest_renders_compact_gcode_status_strip
```

Use the actual test filter that matches the changed behavior; the examples above are illustrative and should be updated as tests evolve.

## Rust Style and Compatibility

- Use standard Rust formatting from `rustfmt`; do not hand-align large blocks.
- Prefer clear module boundaries over broad utility files.
- Name crates and modules with `snake_case` paths, Rust types with `PascalCase`, and functions/variables with `snake_case`.
- Keep compatibility-specific names close to F-Engrave terminology when they map to legacy settings such as `fengrave_set`, `TCODE`, or `v_bit_angle`.
- Prefer structured parsers and existing local helpers over ad hoc string manipulation.
- Preserve generated G-code and settings compatibility as the primary contract. Document intentional compatibility differences.
- Keep `f-engrave_source/` unchanged unless the task explicitly concerns the upstream reference or licensing material.

## Licensing

F-Engrave is GPLv3-or-later, while `f-engrave_source/TTF2CXF_STREAM/` is GPLv2-only. Do not copy the helper into the Rust binary; reimplement TTF behavior in Rust and preserve the applicable license boundaries.

## Review and Delivery

Before reporting completion, include the semantic commit created, the focused tests run, documentation files updated, and any documentation build or manual UI verification performed. Pull requests should describe compatibility impact, list tests and documentation builds, link related issues, and include screenshots for UI changes.
