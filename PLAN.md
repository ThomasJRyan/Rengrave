# R-Engrave Port Plan

R-Engrave is a parity-first Rust port of F-Engrave 1.78. The initial contract is file compatibility and generated-output compatibility, not cleanup-driven redesign. Keep `f-engrave_source/` untouched as the behavioral reference until golden fixtures prove the Rust implementation matches it.

## Workspace

- `crates/rengrave-core`: portable model, settings compatibility, parsers, transforms, geometry, toolpath planning, and exporters.
- `crates/rengrave-cli`: command-line compatibility with F-Engrave batch workflows.
- `crates/rengrave-ui`: `eframe/egui` desktop application.

The core crate must not depend on UI crates. The CLI and UI should call the same core APIs so batch and interactive generation cannot drift.

## Compatibility Contract

- Treat generated output as the primary contract.
- Read legacy `.ngc` settings comments written as `(fengrave_set key value )`.
- Emit compatible settings comments so F-Engrave recovery files remain useful.
- Preserve F-Engrave CLI flags `-b`, `-g`, `-f`, `-d`, and `-t`; add an explicit output path flag for R-Engrave.
- Preserve behavior for text, DXF, bitmap, SVG/DXF/G-code export, v-carve, cleanup, arc fitting, prismatic/inlay, multipass, units, origins, justification, flip, mirror, and text-on-circle.

## Implementation Direction

- Use `eframe/egui` for the desktop UI.
- Keep geometry in model space and apply pan, zoom, model rotation, and viewport rotation before drawing with egui's `Painter`. `emath::Rot2` is suitable for preview rotation.
- Use `image` for bitmap decoding, `ttf-parser` for TTF outlines, `clipper2` for cleanup offset/boolean geometry, `serde` for settings, and `rayon` or worker threads for long calculations.
- Keep Potrace as a detected or bundled sidecar for bitmap vectorization in v1. Missing Potrace must be reported clearly without disabling text and DXF workflows.
- Do not copy the GPLv2-only `ttf2cxf_stream` helper into the Rust binary. Reimplement TTF conversion behavior in Rust.

## GUI Direction

- Compact utilitarian desktop layout: menu/toolbar on top, left inspector, center precision preview, right output/tool panel, and bottom status/log/input area.
- Default to system theme with R-Engrave light/dark variants using graphite, light neutral, steel, amber, and signal-green accents.
- Preview supports pan, zoom, fit, selectable layers, model rotation from engraving settings, and independent viewport rotation for inspection.
- Long calculations run off the UI thread with progress, cancellation, stale-state indicators, and no UI freeze.

## Golden Fixtures

Before optimizing or changing behavior, create fixtures from F-Engrave for:

- Default text and multiline text.
- Text-on-circle, flip, mirror, origin, and justification.
- CXF and TTF fonts.
- DXF bulges, blocks, ellipses, splines, and image-origin behavior.
- Bitmap import through Potrace.
- Engrave, v-carve, prismatic/inlay, multipass, and cleanup paths.
- Arc-fit modes `none`, `center`, and `radius`.
- Inch/mm scaling and settings round trips.

Use numeric tolerances for floating-point output and byte checks where formatting is intentionally preserved.

