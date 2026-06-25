---
target: crates/rengrave-ui
total_score: 25
p0_count: 0
p1_count: 2
timestamp: 2026-06-24T02-11-05Z
slug: crates-rengrave-ui
---
## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|---|---:|---|
| 1 | Visibility of System Status | 3 | Status text, stale summaries, spinner, preview stats, and bottom strip exist, but warnings are count-only and export readiness is dispersed. |
| 2 | Match System / Real World | 2 | CNC vocabulary is present, but labels such as `Turd size`, `Alpha max`, `Drive corner`, and `Step corner` lack local framing. |
| 3 | User Control and Freedom | 3 | Fit/reset/cancel/save/load are present; no obvious undo, section revert, or explicit stale-output discard model. |
| 4 | Consistency and Standards | 3 | Strong egui-native controls and reusable rows; layer controls appear in toolbar, sidebar, and menu, which weakens authority. |
| 5 | Error Prevention | 2 | High-risk machine values use raw numeric controls with little visible guarding around units, sign conventions, feed, depth, paths, or stale output. |
| 6 | Recognition Rather Than Recall | 2 | Many controls are terse compatibility labels; advanced settings assume F-Engrave memory. |
| 7 | Flexibility and Efficiency | 2 | Menus and dense panels help, but no visible shortcuts, presets, recent projects, or command palette. |
| 8 | Aesthetic and Minimalist Design | 3 | The graphite workbench vocabulary fits; cognitive density is still high. |
| 9 | Error Recovery | 2 | Errors and warnings surface as text/counts, but no consolidated recovery panel or next action. |
| 10 | Help and Documentation | 1 | Sparse hover help; obscure or high-risk machining settings are not explained inline. |
| **Total** | | **25/40** | **Functional and credible, not yet confidence-building.** |

## Anti-Patterns Verdict

**LLM assessment:** R-Engrave passes the visual product slop test. It does not look like a generated web app: no hero layout, no gradient text, no decorative cards, no glass, no oversized marketing type. The graphite theme in `preferences.rs`, semantic preview colors in `preview.rs`, and compact egui controls match the “Machinist’s Bench” direction in `DESIGN.md`.

The risk is operational, not cosmetic. The UI exposes a dense settings machine more than it guides a CNC job. A fluent CNC user can probably trust the preview, but a cautious operator will hesitate around stale output, warnings, export readiness, and unexplained compatibility terms.

**Deterministic scan:** `detect.mjs --json crates/rengrave-ui` returned `[]` with exit code 0. No rules, file locations, or counts were reported. This agrees with the visual review: there are no detectable web-pattern anti-patterns in the target.

**Visual overlays:** Skipped. The target is a native `eframe/egui` desktop app, not a browser-rendered DOM route, so a browser overlay would be invented evidence.

## Overall Impression

The current UI is credible shop software with a strong preview surface. The single biggest opportunity is to turn output generation into a stronger confidence workflow: current state, warnings, machine-critical settings, extents, and save target need to converge before the user exports G-code.

## What's Working

- The preview system is the best product surface: cut, rapid, cleanup, bounds, axes, grid, source overlay, scale bar, and cursor readout are all concrete inspection aids.
- The UI respects the new design system: flat graphite panels, compact controls, monospace readouts, and no decorative web grammar.
- Async calculation and stale-output detection are real behaviors, not placeholders; the app already has the state model needed to support a stronger export preflight.

## Priority Issues

**[P1] No single export preflight moment**

Why it matters: CNC output is high consequence. Users need one decisive read before copying or saving machine code.

Fix: Add an export readiness block above the export buttons: current/stale, units, extents, safe Z, cut depth or depth limit, feed/plunge, warnings, and output path. Make save/copy visibly cautioned or disabled when output is stale.

Suggested command: `$impeccable harden crates/rengrave-ui`

**[P1] Warnings are not actionable**

Why it matters: “Warnings: 2” tells the operator there may be a problem but gives no recovery path. That creates anxiety exactly where the UI should build trust.

Fix: Add a collapsible warning/details panel near status or export. Show exact messages, affected input/settings when known, and direct actions such as Recalculate, Open input, or inspect missing characters.

Suggested command: `$impeccable clarify crates/rengrave-ui`

**[P2] Legacy setting density overwhelms recognition**

Why it matters: Compatibility names are important, but raw labels make users rely on memory. That punishes occasional users and F-Engrave migrators who need confirmation, not mystery.

Fix: Keep compatibility labels where needed, but add short tooltips or secondary text for obscure/high-risk controls: `Turd size`, `Alpha max`, `Opt tolerance`, `Drive corner`, `Step corner`, `V flop`, and `Height calc`.

Suggested command: `$impeccable clarify crates/rengrave-ui`

**[P2] Layer controls are duplicated across three places**

Why it matters: Toolbar toggles, right-sidebar checkboxes, and View menu layer items all work, but together they make users ask which surface is authoritative.

Fix: Make the right sidebar the canonical layer legend/control. Keep the canvas toolbar focused on view manipulation: Fit, Reset, Zoom, View rotation. Keep View menu as secondary access.

Suggested command: `$impeccable distill crates/rengrave-ui`

**[P2] Primary workflow is not staged enough**

Why it matters: The left panel lists Input, Text, Catalog, Layout, Cut, V-carve, Multipass, Cleanup, and Advanced. That is accurate, but it does not guide the job lifecycle: choose input, set geometry/tool, inspect preview, export.

Fix: Add subtle grouping or step state without turning the UI into onboarding: Input, Geometry, Tool/Cut, Preview, Export. Keep collapsible headers, but order and label them around the machining job.

Suggested command: `$impeccable layout crates/rengrave-ui`

## Persona Red Flags

**Cautious CNC Operator:** Wants to load input, inspect the job, and save G-code. Red flags: warning counts are not expandable from the main surface; export buttons do not present a final machine-state checklist; stale state is split across toolbar and bottom status.

**F-Engrave Migrator:** Wants to reproduce known F-Engrave output. Red flags: compatibility is implied by legacy labels and settings count, but there is no clear “loaded from settings / changed in UI / output current” audit trail.

**Occasional Maker:** Wants to type text, choose a font, and make a simple engraving. Red flags: the first settings surface quickly exposes Origin, Width %, Line space, Box gap, Safe Z, Accuracy, and more without a default-safe path or short explanations.

## Minor Observations

- `path_row()` right-aligns path text; useful for filenames, but it can hide parent-directory context during output verification.
- The catalog has filtering structure in code, but the visible panel relies mainly on search and implicit compatibility filtering.
- The preview overlay is strong, but it does not include input-outline status when the pink source overlay is visible.
- The green Ready dot in the bottom bar means calculation idle, not necessarily current, saved, or warning-free.
- “Auto” in the toolbar is terse for a setting that materially affects stale-output confidence.

## Questions to Consider

- What would make an operator willing to send this file to a CNC machine without opening the G-code elsewhere?
- Should “Ready” mean idle, current, warning-free, or saved? Right now it mostly means idle.
- Which settings are compatibility necessities, and which deserve modern grouped controls that write the same legacy keys underneath?
- Could the preview become the command center, with export readiness attached to the canvas instead of buried in the right sidebar?
- If the user changes one high-risk value, what exactly should the UI do to make stale state impossible to miss?
