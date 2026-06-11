# R-Engrave Implementation Status

This document records what has actually been implemented in the Rust port and what is still missing for F-Engrave parity. The short version: most progress is still in `rengrave-core` and batch/CLI generation, but the `eframe/egui` UI now exposes a usable MVP workflow for loading inputs, editing common settings, calculating, previewing, and exporting output.

## Latest Checkpoint

- The central preview now draws a lower-right model-space scale bar using the active inch/mm units, making zoom level and drawing size easier to read at a glance.
- The golden/regression harness now includes end-to-end batch coverage for flat-text Add Box output and `arc_fit` modes, including no-arc, center-offset `I/J`, and radius-format `R` G-code.
- Small arc and closed-square CXF fixtures were added under `crates/rengrave-core/tests/fixtures/inputs/` so arc fitting, V-carve, and cleanup companion output are represented in the crate-level integration fixture set.
- The golden/regression harness now includes an end-to-end V-carve batch case for closed CXF text with secondary cleanup G-code requested.
- The central preview now draws a compact overlay summarizing visible cut/rapid/cleanup layer counts and the current model X/Y ranges, making generated output easier to inspect without switching panels.
- The rapid layer toggle and independent viewport rotation now persist in UI preferences, so inspection state survives relaunches alongside cleanup/grid layer preferences.
- Cleanup companion G-code is now parsed back into preview geometry and shown as a separate Cleanup layer in the central canvas, with move/length readouts and a persisted layer toggle.
- Vector input previews now draw the fitted geometry bounds and origin axes behind the source strokes, making CXF/TTF/DXF source previews easier to read at first glance.
- The golden/regression harness now includes end-to-end Rust batch coverage for multiline text, text-on-circle with Add Circle SVG/G-code output, transform/settings comment round trips, and DXF image input with SVG/DXF export payloads.
- A simple DXF fixture was added under `crates/rengrave-core/tests/fixtures/inputs/` so DXF import is represented in the integration fixture set instead of only lower-level unit tests.
- Cleanup companion G-code generation is now an explicit batch option, so the UI can request secondary cleanup files without pretending it is a CLI file-write operation.
- The UI stores secondary cleanup outputs from the core calculation, shows how many cleanup files are available, and can export them next to the selected primary G-code path using F-Engrave-style suffixes such as `_clean`.
- The bottom output area now has a Cleanup tab that shows generated companion G-code grouped by suffix before export.
- Stale-output detection now includes the secondary-output request flag, preventing cleanup files from silently falling out of sync with the visible preview/output.
- The input preview area is larger and has an explicit Refresh action for reloading changed source files from disk.
- Font input preview now has an optional persisted Sample field, so CXF/TTF glyph coverage and outlines can be inspected independently of the engraving text while leaving generated output unchanged.
- R-Engrave now defaults to emitting recovery settings comments, honors legacy `no_comments 1` when loaded, and exposes that behavior in the UI as a Recovery comments toggle.
- Cleanup path selection is now exposed as explicit Straight/V-bit profile, X, Y, and loop checkboxes while still serializing to F-Engrave-compatible `clean_paths`.
- Vector input previews now report stroke length, extents, and coordinate ranges, making selected font/DXF geometry easier to verify before calculation.
- Preview axes are now a selectable view layer, and toolpath/bounds/axes layer flags initialize from and save back to legacy view settings.
- CXF/TTF input previews now use the current engraving text sample instead of always drawing a hard-coded `R-Engrave` sample; DXF and bitmap previews remain cached by input path.
- The generated-output preview now approximates radius-format `G2`/`G3 ... R...` arcs instead of drawing them as straight chords when Radius arc fitting is selected.
- The central preview now supports cursor-centered mouse-wheel/pinch zoom and double-click fit in addition to toolbar/menu controls.
- The central preview now shows live model-space X/Y coordinates under the pointer using the same pan/zoom/rotation transform as the drawn toolpath.
- The Output panel and File menu now include a Use default dir action that resets G-code/SVG/DXF export paths to the current default directory with standard filenames.
- The File menu can now choose G-code, SVG, and DXF output paths, matching the Output panel browse actions.
- Manual edits to settings/input/default-dir and output path fields now persist to UI preferences without requiring a browse/export action first.
- Loaded settings now surface the resolved font/image path back to the UI Input field, including relative font files resolved through `fontdir` and moved image files recovered through `NGC_DIR`.
- Launching the UI with an explicit `-g/--gcode_file` settings file no longer lets an old remembered input path silently override that settings file unless `-f/--fontdir` is also supplied.
- A small bundled CXF demo font is used for an unconfigured first UI launch, so the default `F-Engrave` text generates visible toolpaths instead of starting in settings-only output.
- Settings can now be saved through an explicit Save As workflow, using a native save dialog or the in-app browser fallback, so first-run users no longer need to type a settings path manually.
- The Input Catalog now has CXF/TTF/DXF/Bitmap filters with per-type counts, making font, artwork, and bitmap workflows easier to isolate in populated directories.
- Browse/native/in-app selections now follow through: selecting a settings file loads it immediately, and selecting an input file starts calculation immediately.
- The top toolbar now includes a compact job summary row showing source type/file, active mode/bit/units, output freshness, available artifacts, warning count, and Potrace readiness when relevant.
- Background calculation cancellation now passes through to the core batch path, which checks for cancellation at document, layout, v-carve, cleanup, and output-rendering stage boundaries.
- V-carve multipass controls are now explicit in the Tool panel: Finish stock enables/disables Max depth/pass following F-Engrave's policy, and the panel reports whether multipass is active or misconfigured.
- The bottom output tabs now have a Copy tab action, so status logs, G-code, cleanup companion G-code, SVG, and DXF payloads can be copied directly from the visible tab.
- Settings-only fallback output no longer describes the port as an unimplemented scaffold; it now says no toolpath was generated and points users at the warning state.
- Background calculation now reports visible phase status for queued, document preparation, toolpath generation, and output finalization instead of only showing an undifferentiated spinner.
- Background calculation progress now comes from the core batch pipeline, with real stage labels for document load, font/DXF/bitmap input, layout, exports, V-carve, cleanup, and G-code rendering.
- V-carve point generation now has an internal cancellation hook, so UI Cancel can stop during long V-carve sampling instead of waiting for the whole V-carve stage to finish.
- Cleanup generation now has internal cancellation checks across closed-path collection, offset loops, X/Y scanlines, path ordering, and point emission, so UI Cancel can stop during long cleanup calculations.
- Bitmap vectorization now has cancellation checks during image-to-PBM conversion and while Potrace is running; canceling kills and waits for the Potrace sidecar instead of blocking on process completion.
- CXF and TTF font loading now have cancellable parser paths used by batch generation; CXF checks during line parsing and arc expansion, while TTF checks before parsing and during the glyph codepoint walk.
- DXF import now has cancellable parser paths used by batch generation; checks cover code/value grouping, section and block discovery, entity walking, block insert recursion, arc/bulge/polyline expansion, ellipse/spline sampling, and bitmap-vectorized DXF parsing.
- The central preview now renders a model-space precision grid that pans, zooms, and rotates with the toolpath, with a Grid layer toggle in the Preview panel and View menu.
- UI smoke coverage now checks the fixed-panel/preview layout at 1280x800 and 1920x1080, including fitted preview rotations at 0, 45, and 90 degrees with non-overlapping modeled regions.
- The UI now exposes additional F-Engrave compatibility settings for V-carve corner thresholds, V-carve check scope, thickness/V-area view flags, and plot-during-V-carve, so these legacy values load and save even before full algorithm parity.
- The Preview panel now reports total cut length and rapid length from the parsed generated G-code, alongside move counts and bounds.
- Stale-output indicators in the toolbar and bottom status bar now offer a direct Recalculate action when the worker is idle.
- Stale-output and active-calculation indicators now name the changed areas, such as text, controls, input file, cleanup request, or export set, instead of only saying output is stale.
- The Output panel and File menu now include Export all available, which writes generated G-code, SVG, DXF, and cleanup companion files in one action.
- The Layout panel and File menu now include a defaults action that resets controls and view layers from the core F-Engrave-compatible default settings table without changing the current input/text paths.
- The Preview panel now reports generated model extents and X/Y ranges from the combined cut and rapid bounds.
- Bitmap input previews now show both the original thumbnail and the black/white trace mask produced by the same luma threshold and alpha-over-white treatment used before Potrace.
- Bitmap input previews now report black/white trace-mask pixel counts and black coverage percentage so tracing problems are visible before running generation.
- CXF/TTF input previews now warn when the current sample text contains characters missing from the selected font, with duplicate missing glyphs collapsed in the readout.

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
- Document loading exposes the resolved input path that generation will use, so frontends can display the effective font/image source instead of only the path the user typed.
- CXF font parsing with line and arc support.
- TTF outline conversion through `ttf-parser`; the GPLv2-only F-Engrave helper is not copied.
- DXF import for lines, arcs, circles, LWPOLYLINE bulges, leaders, solids, ellipses, splines, weighted splines, and block inserts, with cancellable batch parsing.
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
- Settings-only fallback G-code is still emitted for missing/invalid inputs so recovery comments remain available, but the warning/comment wording now reflects the actual no-toolpath condition rather than an old scaffold state.
- SVG and DXF export helpers from generated layout segments.

## Implemented UI Work

- `eframe/egui` app starts at 1280x800 with a top File/Run/View menu row, toolbar, left input/settings panel, central preview, right output/tool panel, and bottom status/log panel.
- The UI now has editable Settings/Input/Default-dir path fields and Load/Calculate actions, so settings files and font/image paths are reachable without CLI launch arguments.
- Loading a settings file updates the visible Input path from the resolved document source when the settings file supplies `fontdir`/`fontfile` or `imagefile`/`NGC_DIR`; explicit input paths still act as overrides.
- On a clean first run with no remembered settings or input path, the UI selects `assets/fonts/rengrave_demo.cxf` to produce an immediate previewable default job. Remembered settings files and explicit CLI paths still take precedence.
- Each path field has a Browse action that first tries a native file/folder/save dialog through `rfd`, then falls back to the in-app filesystem browser. The in-app browser can navigate parent/home directories, select settings/input files, select the default directory, and choose output files in the current directory; settings/input selections now immediately load or calculate.
- The left panel includes an input catalog that scans the current input/default directory for CXF, TTF, DXF, and bitmap files, with per-type filters/counts; selecting an entry updates the input path and starts background generation.
- The left panel also shows a cached input preview: CXF/TTF strokes from the current engraving text or a separate preview Sample with missing-character warnings, DXF line artwork with fitted bounds/origin axes plus stroke length/extents/range readouts, or bitmap original/trace-mask thumbnails with black/white coverage, with decode/parser errors shown inline and a Refresh action for changed files.
- Current UI settings can be saved back out as reusable `fengrave_set` comments, including selected input/default directory, UI overrides, and TCODE text. Existing settings files are used as a base when present; new settings paths save from defaults, and Save As can choose that path without manually editing the Settings field.
- UI path preferences are persisted under the platform config directory, updated from manual path edits as well as browse/export actions, and restored on the next launch when CLI arguments do not override them.
- Common F-Engrave settings are exposed as working controls: mode, units, justification, origin, height/width/spacing, angle, text radius, flip/mirror, Add Box, safe/cut Z, stroke, feed/plunge, arc fitting, bit shape, V-bit/inlay/depth settings, V-carve corner/check-scope compatibility values, explicit V-carve multipass finish-stock/max-depth controls, cleanup diameter/step/V size/path checkboxes/normal flip, and bitmap image-size/long-curve toggles. Controls and view layers can be reset back to the shared core defaults.
- The right panel has a Bitmap section that reports Potrace detected/missing status, indicates when the selected input requires Potrace, and exposes Potrace turn policy, turd size, alpha max, and optimization tolerance.
- The right panel has an Advanced section for settings that were previously hidden: height calculation mode, G-code preamble/postamble, recovery comments, variable output, extended TTF/CXF character conversion, and compatibility view/plot flags.
- UI controls emit legacy `fengrave_set` overrides into `rengrave-core`; they do not write temporary settings files.
- The app calculates through the same batch core path used by the CLI, now on a background worker so the preview and controls stay responsive.
- Calculation has an indeterminate progress indicator, core-reported stage messages, a Cancel action, and stale-result handling. If inputs change while a worker is running, the old result is ignored instead of replacing the current state.
- After a successful calculation, the UI tracks the batch request that produced the displayed output, marks the output stale if text, input paths, or generation settings change before recalculation, names the changed areas in the stale indicator, and exposes direct Recalculate buttons from stale indicators.
- It stores generated G-code/SVG/DXF payloads and can write them to user-editable paths individually or with one Export all available action; output paths can also be reset to the current default directory.
- It stores generated secondary cleanup G-code payloads, displays the available cleanup-file count, previews cleanup cut moves as a selectable central-canvas layer, and exports cleanup files beside the primary G-code path.
- UI Cancel now sets a worker flag consumed by the core batch generator at stage boundaries, inside font/DXF parsing, inside V-carve/cleanup loops, and through bitmap vectorization, so canceled jobs can stop during long font, DXF, V-carve, cleanup, and Potrace-backed bitmap calculations instead of only being ignored after completion.
- File, Run, and View menus expose the same load/save/export-all/export/calculate/cancel/copy/Fit/layer actions as the panels and toolbar, including output path selection for G-code, SVG, and DXF.
- The toolbar includes a compact job summary row so source, mode/tool/units, output state, artifacts, warnings, and bitmap tracing readiness are visible without switching panels.
- The bottom panel has Status, G-code, Cleanup, SVG, and DXF tabs so generated text output can be inspected without exporting first.
- The active bottom tab can be copied to the clipboard, including cleanup, SVG, and DXF payloads in addition to primary G-code.
- It previews cut moves, separately parses and draws rapid XY moves as a toggleable dashed layer, parses generated cleanup companion G-code into a separate overlay, and approximates center-format and radius-format arcs for display.
- It has basic toggles for toolpath, rapid, cleanup, bounds, axes, and grid layers, generated move counts, cut/rapid/cleanup path length readouts, a compact in-canvas layer/range overlay, a model-space scale bar, extents/range readouts, cursor-centered wheel/pinch zoom, live model-coordinate readout, simple zoom/view rotation controls, double-click/menu Fit actions, and bounds-aware recentering of generated geometry in the preview. Legacy toolpath, bounds, and axes layer flags round-trip through saved settings; the R-Engrave-only rapid/cleanup/grid layer preferences and viewport inspection rotation persist in UI state.

## Why The UI Looks Bare

The UI is now an MVP rather than only a shell, but it still lacks several expected desktop workflow pieces. The central canvas has real pan/zoom/rotation, layer toggles, cut/rapid/bounds/axes drawing, live coordinates, and a model-space grid; font previews have an independent sample field, but image preview editing is still basic and full F-Engrave-style menu coverage is not complete. Controls cover common and several advanced settings, but not every legacy knob. Background jobs keep the UI responsive, report real core stages, and now cancel at batch stage boundaries, inside font/DXF/V-carve/cleanup loops, and during bitmap vectorization.

## Tests And Validation In Place

- Core tests currently cover settings, CXF/TTF parsing, DXF entities, bitmap conversion, layout transforms, Add Box/Circle, G-code, SVG/DXF export, cleanup, v-carve options, batch generation, cancellation stage boundaries, CXF/TTF/DXF parser cancellation, V-carve sampling cancellation, cleanup scanline cancellation, bitmap conversion/vectorization cancellation, and settings-only fallback output.
- A crate-level golden-output harness now exists under `crates/rengrave-core/tests/golden.rs` with minimal CXF, arc CXF, closed-square CXF, and DXF fixtures, checked G-code/SVG regression outputs, numeric-tolerant G-code comparison helpers, and end-to-end Rust batch cases for multiline text, text-on-circle Add Circle output, flat-text Add Box output, arc-fit formats, V-carve with secondary cleanup output, transform/settings round trips, and DXF SVG/DXF export generation. These tests pin current R-Engrave output; they are not yet F-Engrave-generated parity fixtures.
- F-Engrave fixture generation was rechecked on 2026-06-10 with `python f-engrave_source/f-engrave.py -b -f crates/rengrave-core/tests/fixtures/inputs/simple.cxf -t AB`; it still fails before batch mode because `pyclipper` is missing.
- UI tests cover default and secondary output paths, default-directory export path reset helpers, export-all availability, cleanup companion preview formatting and parsed cleanup overlay geometry, active-tab clipboard payload selection, demo-font first-run generation, core progress status labels, toolbar job-summary formatting, V-carve multipass state formatting, cleanup path checkbox serialization, default control mapping, view-layer settings serialization, native save-dialog filename helpers, settings Save As follow-up policy, browse-selection follow-up policy, settings save serialization, path-field parsing, loaded-document input-path display policy, explicit settings launch behavior, in-app browser directory behavior, input catalog scanning/filtering, input preview loading, vector input preview readouts/fit math/origin axes, font missing-character preview warnings, bitmap trace-mask preview thresholding and coverage stats, font text-sample and custom-sample selection, preference persistence including rapid/cleanup/grid layers and clamped viewport rotation, worker stale-result detection, output stale-state detection, stale-reason summaries, recalculation availability, control-to-legacy override emission, bitmap/Potrace control mapping, advanced setting mapping, preview fitting, cleanup-inclusive bounds, preview overlay summaries, preview scale-bar sizing/labels, grid spacing/persistence, 1280x800/1920x1080 layout smoke checks, generated extents and path-length formatting, cursor zooming, screen-to-model coordinate conversion, output preview truncation, text-file write errors, cut/rapid preview parsing, and center/radius arc preview parsing.
- Recent validation has been run with:
  - `cargo test -p rengrave-core`
  - `cargo test -p rengrave-ui`
  - `cargo test --workspace`

## Major Work Remaining For Parity

- Golden fixtures from F-Engrave output. The harness and first R-Engrave regression fixture now exist, but broad comparisons against F-Engrave-generated `.ngc`, `.svg`, and `.dxf` files are still missing because local F-Engrave batch execution currently lacks `pyclipper`.
- Full F-Engrave UI workflow: richer font/image preview editing controls, complete settings panels, complete v-carve/cleanup parity controls, full config parity, and parity menus.
- Deeper cooperative cancellation inside remaining lower-level blocking operations. The UI has a background worker, indeterminate progress, core stage reporting, Cancel, stale-state handling, CXF/TTF/DXF parser cancellation, V-carve/cleanup inner-loop cancellation, and bitmap vectorization cancellation; native file reads and some third-party library calls are still not interruptible mid-call.
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
2. Continue enriching font/image preview controls and add more complete F-Engrave-style setting panels.
3. Continue expanding parity tests for bitmap imports, additional Add Box edge cases, TTF conversion, cleanup ordering, and V-carve/inlay edge cases; multiline text, text-on-circle, basic Add Box, arc-fit formats, basic V-carve with cleanup companion output, transform/settings round trips, and basic DXF import now have integration coverage.
4. Audit V-carve and cleanup output against F-Engrave fixtures before adding more UI around those features.
5. Replace the deterministic UI smoke checks with screenshot-based native UI checks if/when a reliable headless renderer is added.
