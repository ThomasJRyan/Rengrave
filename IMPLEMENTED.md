# R-Engrave Implementation Status

This document records what has actually been implemented in the Rust port and what is still missing for F-Engrave parity. The short version: most progress is still in `rengrave-core` and batch/CLI generation, but the `eframe/egui` UI now exposes a usable MVP workflow for loading inputs, editing common settings, calculating, previewing, and exporting output.

## Current Shape

- `f-engrave_source/` is kept untouched as the behavior and licensing reference.
- `crates/rengrave-core` contains the real porting work: settings parsing, geometry, importers, layout, toolpath generation, exports, and tests.
- `crates/rengrave-cli` wraps the core batch workflow and preserves the main F-Engrave flags: `-b`, `-g`, `-f`, `-d`, and `-t`, plus Rust-specific output flags.
- `crates/rengrave-ui` launches a desktop app, loads a document from editable paths, exposes common layout/tool/output controls, runs core generation, previews parsed G-code moves, and exports generated G-code/SVG/DXF. It is still not a complete F-Engrave replacement UI.

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
- Arc fitting modes `none`, `center`, and `radius` are present.
- Initial V-carve point generation for V-bit, ball, and flat cutters.
- Initial inlay/depth-limit/effective diameter handling.
- Initial v-carve roughing/multipass depth caps.
- Initial cleanup path generation and secondary cleanup G-code files.
- SVG and DXF export helpers from generated layout segments.

## Implemented UI Work

- `eframe/egui` app starts at 1280x800 with a top toolbar, left input/settings panel, central preview, right output/tool panel, and bottom status/log panel.
- The UI now has editable Settings/Input/Default-dir path fields and Load/Calculate actions, so settings files and font/image paths are reachable without CLI launch arguments.
- Common F-Engrave settings are exposed as working controls: mode, units, justification, origin, height/width/spacing, angle, text radius, flip/mirror, Add Box, safe/cut Z, stroke, feed/plunge, arc fitting, bit shape, V-bit/inlay/depth settings, cleanup diameter/step/V size, and bitmap size/long-path toggles.
- UI controls emit legacy `fengrave_set` overrides into `rengrave-core`; they do not write temporary settings files.
- The app can calculate through the same batch core path used by the CLI.
- It stores generated G-code/SVG/DXF payloads and can write them to user-editable paths.
- It previews linear G-code moves and now approximates center-format full-circle arcs for display.
- It has basic toggles for toolpath and bounds layers and simple zoom/view rotation controls.

## Why The UI Looks Bare

The UI is now an MVP rather than only a shell, but it still lacks several expected desktop workflow pieces. There are no native file pickers, font browser, persistent user preferences, Potrace option panel, progress/cancel flow, or F-Engrave-style menus. Controls cover common settings, but not every legacy knob. Long calculations still run synchronously on the UI thread.

## Tests And Validation In Place

- Core tests currently cover settings, CXF/TTF parsing, DXF entities, bitmap conversion, layout transforms, Add Box/Circle, G-code, SVG/DXF export, cleanup, v-carve options, and batch generation.
- UI tests cover default output paths, path-field parsing, control-to-legacy override emission, text-file write errors, linear preview parsing, and full-circle arc preview parsing.
- Recent validation has been run with:
  - `cargo test -p rengrave-core`
  - `cargo test -p rengrave-ui`
  - `cargo test --workspace`

## Major Work Remaining For Parity

- Golden fixtures from F-Engrave output. Current tests are focused unit/batch checks, not broad golden comparisons against F-Engrave-generated `.ngc`, `.svg`, and `.dxf` files.
- Full F-Engrave UI workflow: file open/save dialogs, font browser, complete settings panels, complete v-carve/cleanup parity controls, bitmap controls, config save/load, clipboard operations, and parity menus.
- Worker-thread calculation model with progress, cancellation, stale-state handling, and no UI freeze.
- Stronger V-carve parity. The current V-carve implementation is initial and not proven equivalent to F-Engrave’s full algorithm for complex glyphs and artwork.
- Full prismatic/inlay parity, including all F-Engrave edge cases around Add Box/Flip Normals, cleanup, depth limits, and output ordering.
- Multipass parity for ordinary engraving and v-carve workflows beyond the currently ported roughing/depth-cap behavior.
- More exact cleanup-path behavior and ordering compared with F-Engrave.
- More exact G-code formatting and comments behavior, including decisions around `no_comments` versus the plan’s recovery-comment compatibility contract.
- DXF export parity beyond line-entity output; F-Engrave behavior should be fixture-tested.
- Bitmap parity against Potrace/F-Engrave fixtures, including image thresholding and alpha behavior.
- TTF conversion parity against F-Engrave’s historical CXF conversion behavior.
- Platform-specific validation on Linux, Windows, and macOS.
- Release automation such as `cargo dist`.

## Recommended Next Checkpoints

1. Build a golden-fixture harness that can compare generated output against checked-in F-Engrave fixtures with tolerances.
2. Add native file pickers, font/image browsing, a broader settings editor, and persistent user preferences.
3. Expand parity tests for default text, multiline text, text-on-circle, flip/mirror/origin/justify, DXF imports, bitmap imports, Add Box/Circle, arc-fit modes, and settings round trips.
4. Audit V-carve and cleanup output against F-Engrave fixtures before adding more UI around those features.
5. Add progress/cancel worker execution for long calculations.
