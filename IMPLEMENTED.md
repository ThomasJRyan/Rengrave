# R-Engrave Implementation Status

This document records what has actually been implemented in the Rust port and what is still missing for F-Engrave parity. The short version: most progress is still in `rengrave-core` and batch/CLI generation, but the `eframe/egui` UI now exposes a usable MVP workflow for loading inputs, editing common settings, calculating, previewing, and exporting output.

## Latest Checkpoint

- Cleanup companion G-code generation is now an explicit batch option, so the UI can request secondary cleanup files without pretending it is a CLI file-write operation.
- The UI stores secondary cleanup outputs from the core calculation, shows how many cleanup files are available, and can export them next to the selected primary G-code path using F-Engrave-style suffixes such as `_clean`.
- The bottom output area now has a Cleanup tab that shows generated companion G-code grouped by suffix before export.
- Stale-output detection now includes the secondary-output request flag, preventing cleanup files from silently falling out of sync with the visible preview/output.
- The input preview area is larger and has an explicit Refresh action for reloading changed source files from disk.
- R-Engrave now defaults to emitting recovery settings comments, honors legacy `no_comments 1` when loaded, and exposes that behavior in the UI as a Recovery comments toggle.
- Cleanup path selection is now exposed as explicit Straight/V-bit profile, X, Y, and loop checkboxes while still serializing to F-Engrave-compatible `clean_paths`.
- Preview axes are now a selectable view layer, and toolpath/bounds/axes layer flags initialize from and save back to legacy view settings.

## Current Shape

- `f-engrave_source/` is kept untouched as the behavior and licensing reference.
- `crates/rengrave-core` contains the real porting work: settings parsing, geometry, importers, layout, toolpath generation, exports, and tests.
- `crates/rengrave-cli` wraps the core batch workflow and preserves the main F-Engrave flags: `-b`, `-g`, `-f`, `-d`, and `-t`, plus Rust-specific output flags.
- `crates/rengrave-ui` launches a desktop app, loads a document through editable paths or an in-app browser, exposes common layout/tool/output controls, runs core generation, previews parsed G-code moves, and exports generated G-code/SVG/DXF. It is still not a complete F-Engrave replacement UI.

## Implemented Core And CLI Work

- Cargo workspace scaffold with separate core, CLI, and UI crates.
- Legacy settings parser and emitter for `(fengrave_set key value )` comments, including TCODE text reconstruction and emission.
- Default settings table covering current ported F-Engrave keys.
- Legacy boolean aliases such as `plotbox box` and `plotbox no_box` are handled.
- CLI/batch document loading from settings files, font/image inputs, default directories, and text overrides with `|` newline conversion.
- Recovery of stale image paths by basename through `NGC_DIR`, matching F-Engrave behavior for moved image files.
- CXF font parsing with line and arc support.
- TTF outline conversion through `ttf-parser`; the GPLv2-only F-Engrave helper is not copied.
- DXF import for lines, arcs, circles, LWPOLYLINE bulges, leaders, solids, ellipses, splines, weighted splines, and block inserts.
- Bitmap vectorization via Potrace sidecar for PBM/PNM/BMP and converted PNG/JPEG/TIFF/GIF inputs.
- Text layout with scaling, line spacing, character/word spacing, justification, origin handling, flip, mirror, rotation, text-on-circle, outside/inside and upper/lower circle modes.
- Add Box rectangular border support for engrave/v-carve cases.
- Add Circle support for text-on-circle engrave output, including full-circle `G2 I... J...` G-code and SVG circle output.
- Engrave G-code output with safe/depth moves, feeds, variables, units, preamble/postamble, and optional arc fitting.
- Recovery settings comments are emitted by default for compatibility and can be suppressed with legacy `no_comments 1`.
- Arc fitting modes `none`, `center`, and `radius` are present.
- Initial V-carve point generation for V-bit, ball, and flat cutters.
- Initial inlay/depth-limit/effective diameter handling.
- Initial v-carve roughing/multipass depth caps.
- Initial cleanup path generation and secondary cleanup G-code files.
- Batch generation can now request secondary cleanup outputs independently of writing the primary G-code to a CLI output path.
- SVG and DXF export helpers from generated layout segments.

## Implemented UI Work

- `eframe/egui` app starts at 1280x800 with a top File/Run/View menu row, toolbar, left input/settings panel, central preview, right output/tool panel, and bottom status/log panel.
- The UI now has editable Settings/Input/Default-dir path fields and Load/Calculate actions, so settings files and font/image paths are reachable without CLI launch arguments.
- Each path field has a Browse action that first tries a native file/folder/save dialog through `rfd`, then falls back to the in-app filesystem browser. The in-app browser can navigate parent/home directories, select settings/input files, select the default directory, and choose output files in the current directory.
- The left panel includes an input catalog that scans the current input/default directory for CXF, TTF, DXF, and bitmap files; selecting an entry updates the input path and starts background generation.
- The left panel also shows a cached input preview: CXF/TTF sample strokes, DXF line artwork, or a bitmap thumbnail, with decode/parser errors shown inline and a Refresh action for changed files.
- Current UI settings can be saved back out as reusable `fengrave_set` comments, including selected input/default directory, UI overrides, and TCODE text. Existing settings files are used as a base when present; new settings paths save from defaults.
- UI path preferences are persisted under the platform config directory and restored on the next launch when CLI arguments do not override them.
- Common F-Engrave settings are exposed as working controls: mode, units, justification, origin, height/width/spacing, angle, text radius, flip/mirror, Add Box, safe/cut Z, stroke, feed/plunge, arc fitting, bit shape, V-bit/inlay/depth settings, cleanup diameter/step/V size/path checkboxes/normal flip, and bitmap image-size/long-curve toggles.
- The right panel has a Bitmap section that reports Potrace detected/missing status, indicates when the selected input requires Potrace, and exposes Potrace turn policy, turd size, alpha max, and optimization tolerance.
- The right panel has an Advanced section for core-supported settings that were previously hidden: height calculation mode, G-code preamble/postamble, recovery comments, variable output, and extended TTF/CXF character conversion.
- UI controls emit legacy `fengrave_set` overrides into `rengrave-core`; they do not write temporary settings files.
- The app calculates through the same batch core path used by the CLI, now on a background worker so the preview and controls stay responsive.
- Calculation has an indeterminate progress indicator, a Cancel action, and stale-result handling. If inputs change while a worker is running, the old result is ignored instead of replacing the current state.
- After a successful calculation, the UI tracks the batch request that produced the displayed output and marks the output stale if text, input paths, or generation settings change before recalculation.
- It stores generated G-code/SVG/DXF payloads and can write them to user-editable paths.
- It stores generated secondary cleanup G-code payloads, displays the available cleanup-file count, and exports cleanup files beside the primary G-code path.
- File, Run, and View menus expose the same load/save/export/calculate/cancel/copy/Fit/layer actions as the panels and toolbar.
- The bottom panel has Status, G-code, Cleanup, SVG, and DXF tabs so generated text output can be inspected without exporting first.
- It previews cut moves, separately parses and draws rapid XY moves as a toggleable dashed layer, and approximates center-format full-circle arcs for display.
- It has basic toggles for toolpath, rapid, bounds, and axes layers, simple zoom/view rotation controls, and a bounds-aware Fit action that recenters generated geometry in the preview. Legacy toolpath, bounds, and axes layer flags round-trip through saved settings.

## Why The UI Looks Bare

The UI is now an MVP rather than only a shell, but it still lacks several expected desktop workflow pieces. Native dialogs are attempted with in-app fallback, but there are still no rich font/image preview editing controls, cooperative mid-algorithm cancellation, or full F-Engrave-style menu coverage. Controls cover common and several advanced settings, but not every legacy knob. Background jobs keep the UI responsive, but the current Cancel action ignores late worker results rather than interrupting every core loop immediately.

## Tests And Validation In Place

- Core tests currently cover settings, CXF/TTF parsing, DXF entities, bitmap conversion, layout transforms, Add Box/Circle, G-code, SVG/DXF export, cleanup, v-carve options, and batch generation.
- A crate-level golden-output harness now exists under `crates/rengrave-core/tests/golden.rs` with a minimal CXF fixture, checked G-code/SVG regression outputs, and numeric-tolerant G-code comparison helpers. These first expected files pin current R-Engrave output; they are not yet F-Engrave-generated parity fixtures.
- F-Engrave fixture generation was rechecked on 2026-06-10 with `python f-engrave_source/f-engrave.py -b -f crates/rengrave-core/tests/fixtures/inputs/simple.cxf -t AB`; it still fails before batch mode because `pyclipper` is missing.
- UI tests cover default and secondary output paths, cleanup companion preview formatting, cleanup path checkbox serialization, view-layer settings serialization, native save-dialog filename helpers, settings save serialization, path-field parsing, in-app browser directory behavior, input catalog scanning, input preview loading, preference persistence, worker stale-result detection, output stale-state detection, control-to-legacy override emission, bitmap/Potrace control mapping, advanced setting mapping, preview fitting, output preview truncation, text-file write errors, cut/rapid preview parsing, and full-circle arc preview parsing.
- Recent validation has been run with:
  - `cargo test -p rengrave-core`
  - `cargo test -p rengrave-ui`
  - `cargo test --workspace`

## Major Work Remaining For Parity

- Golden fixtures from F-Engrave output. The harness and first R-Engrave regression fixture now exist, but broad comparisons against F-Engrave-generated `.ngc`, `.svg`, and `.dxf` files are still missing because local F-Engrave batch execution currently lacks `pyclipper`.
- Full F-Engrave UI workflow: richer font/image preview editing controls, complete settings panels, complete v-carve/cleanup parity controls, full config parity, and parity menus.
- Deeper cooperative cancellation/progress inside long core algorithms. The UI has a background worker, indeterminate progress, Cancel, and stale-state handling, but core routines do not yet report detailed progress or stop mid-loop.
- Stronger V-carve parity. The current V-carve implementation is initial and not proven equivalent to F-Engrave’s full algorithm for complex glyphs and artwork.
- Full prismatic/inlay parity, including all F-Engrave edge cases around Add Box/Flip Normals, cleanup, depth limits, and output ordering.
- Multipass parity for ordinary engraving and v-carve workflows beyond the currently ported roughing/depth-cap behavior.
- More exact cleanup-path behavior and ordering compared with F-Engrave.
- More exact G-code formatting and comment wording compared with F-Engrave fixture output.
- DXF export parity beyond line-entity output; F-Engrave behavior should be fixture-tested.
- Bitmap parity against Potrace/F-Engrave fixtures, including image thresholding, alpha behavior, option edge cases, and fixture-proven DXF/toolpath output.
- TTF conversion parity against F-Engrave’s historical CXF conversion behavior.
- Platform-specific validation on Linux, Windows, and macOS.
- Release automation such as `cargo dist`.

## Recommended Next Checkpoints

1. Generate actual F-Engrave reference fixtures once `pyclipper` is available, then wire them into the existing tolerance-ready golden harness.
2. Enrich font/image preview controls and add more complete F-Engrave-style setting panels.
3. Expand parity tests for default text, multiline text, text-on-circle, flip/mirror/origin/justify, DXF imports, bitmap imports, Add Box/Circle, arc-fit modes, and settings round trips.
4. Audit V-carve and cleanup output against F-Engrave fixtures before adding more UI around those features.
5. Add detailed progress reporting and cooperative cancellation checks inside expensive core import, cleanup, and v-carve routines.
