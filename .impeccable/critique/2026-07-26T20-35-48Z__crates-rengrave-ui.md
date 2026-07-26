---
target: crates/rengrave-ui
total_score: 27
p0_count: 0
p1_count: 3
timestamp: 2026-07-26T20-35-48Z
slug: crates-rengrave-ui
---
## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|---|---:|---|
| 1 | Visibility of System Status | 3 | Strong stale/output/warning state, but generation progress and preview-layer state compete across panels. |
| 2 | Match System / Real World | 3 | CNC terminology and machining units are appropriate; some legacy labels remain opaque to first-time users. |
| 3 | User Control and Freedom | 3 | Recalculate, cancel, reset, visibility toggles, and project persistence are good; the dense workflow makes undo/recovery less discoverable. |
| 4 | Consistency and Standards | 2 | Preview controls, layer labels, and settings controls are repeated in multiple surfaces with slightly different wording. |
| 5 | Error Prevention | 3 | Preflight warnings and stale-output detection are valuable; high-risk settings still allow invalid or surprising combinations without enough inline explanation. |
| 6 | Recognition Rather Than Recall | 2 | Tooltips help, but users must remember legacy terms, profile/cleanup distinctions, and three-button 3D navigation. |
| 7 | Flexibility and Efficiency | 3 | Strong power-user density and batch parity; keyboard shortcuts and repeatable view commands are not prominent. |
| 8 | Aesthetic and Minimalist Design | 2 | Restrained palette is disciplined, but the settings/sidebar surfaces are information-heavy and visually repetitive. |
| 9 | Error Recovery | 3 | Warnings, cached project output, and stale-state messaging support recovery; failure guidance could be more actionable. |
| 10 | Help and Documentation | 3 | Parameter help and documentation are unusually thorough, but crucial preview gestures are not visible at the point of use. |
| **Total** | | **27/40** | **Solid functional tool UI with a clear opportunity to reduce cognitive load and improve discoverability.** |

## Anti-Patterns Verdict

**LLM assessment:** This does not read as generic web UI or obvious AI-generated design. The dark graphite workbench, compact native controls, and semantic toolpath colors fit a CNC application. The main design smell is not visual slop; it is accumulated functional density. Repeated headings, checkboxes, and parameter rows make the interface feel like a long settings sheet rather than a staged machining workflow.

**Deterministic scan:** `detect.mjs --json crates/rengrave-ui` returned `[]`. No markup-oriented detector rules applied to this Rust/egui target. This is not evidence that the native UI has no issues; it means the bundled detector has no useful markup surface to inspect.

**Visual/browser evidence:** Browser inspection and live overlay injection were not applicable. This is a native Rust/egui desktop application with no running web surface or browser automation available in this session. Findings are based on source inspection, the committed design system, existing egui harness coverage, and the reported UI workflow.

## Overall Impression

R-Engrave has the right character: purposeful, compact, and machine-state focused. The single biggest opportunity is to make the workflow legible at a glance—what is being generated, what toolpath layers exist, what will be exported, and what action is safe next—without requiring users to scan several dense panels or remember legacy terminology.

## What's Working

- The palette has disciplined meaning: green for cuts/readiness, amber for travel/warnings, blue for cleanup, and pink for input overlay. That is appropriate for a machining inspection tool.
- Output confidence is treated as a first-class concern. Stale-output detection, preflight warnings, generation status, cached project G-code, and explicit export controls create useful reassurance before a job reaches a machine.
- Parameter help is broad and concrete. Descriptions such as profile margin, tab height, cleanup diameter ordering, and safe Z explain machining consequences rather than merely restating labels.

## Priority Issues

### [P1] The main workflow has too much simultaneous control density

**Why it matters:** Users must move between workbench settings, input preview, 3D preview layers, job details, export, and preflight. Important decisions compete with secondary controls, increasing the chance of overlooking stale output or an unsafe Z/feed combination.

**Fix:** Establish a stronger vertical workflow hierarchy: Inputs → Toolpath settings → Preview/layers → Preflight → Export. Keep the highest-value state visible and move lower-frequency diagnostics into collapsible sections. Use consistent section summaries so collapsed panels still communicate active settings.

**Suggested command:** `$impeccable layout crates/rengrave-ui`

### [P1] Preview navigation and layer semantics are not discoverable enough

**Why it matters:** The 3D preview now depends on left/middle orbit, right pan, wheel zoom, bounded negative pitch, reset, fit, and separate layer toggles. A first-time user can easily interpret a movement as a broken view or miss profile/cleanup output because the controls are not explained in the canvas itself.

**Fix:** Add a compact, dismissible “Navigation” affordance near the preview with the current mouse mappings and keyboard alternatives. Make the preview legend explicitly distinguish primary cuts, cleanup passes, profile paths, tabs, rapids, input overlay, and hidden layers. Keep the overlay available from a small help button after dismissal.

**Suggested command:** `$impeccable clarify crates/rengrave-ui`

### [P1] The preview layer model conflates secondary operations

**Why it matters:** The UI exposes “Show secondary moves” while the underlying output can contain cleanup, profile, chamfer, and other companion paths. Users inspecting a profile cut need to know which toolpath they are seeing and which cutter/output file it belongs to.

**Fix:** Promote secondary output identity into the layer model: show named rows such as “Cleanup 6.35 mm,” “Profile,” “Profile chamfer,” and “Tabs,” with tool diameter/depth where available. Keep the aggregate toggle as a convenience, not the primary explanation.

**Suggested command:** `$impeccable clarify crates/rengrave-ui`

### [P2] Legacy terminology and repeated labels increase recall burden

**Why it matters:** Labels like V-bit, V step, V-carve check scope, clean V, profile margin, and recovery comments are valid compatibility terms but not self-evident. Tooltips help only after users find the control and hover it.

**Fix:** Keep legacy keys in developer/compatibility documentation, but give the UI a plain-language label plus a compact legacy term in help text. Add short inline summaries for high-impact groups such as depth, cutter, cleanup, profile, and export.

**Suggested command:** `$impeccable clarify crates/rengrave-ui`

### [P2] High-risk preflight information is too passive

**Why it matters:** The preflight section reports state, Z, feed, warnings, and cautions, but users still have to interpret whether the generated output is safe to export. In CNC software, an overlooked stale state or warning is materially more consequential than ordinary UI friction.

**Fix:** Make the preflight state a compact, prioritized checklist: Output current, units, cutter/depth, warnings, companion files, and export path. Give each failed item a direct action (“Generate G-code”, “Review warnings”, “Choose output path”) and visually separate machine-risk items from convenience warnings.

**Suggested command:** `$impeccable harden crates/rengrave-ui`

## Persona Red Flags

**Alex — CNC power user:** The dense settings surface is efficient, but repeated navigation/layer controls and no prominent shortcut reference slow repeated inspection. Alex may know the desired operation but still has to scan for the correct profile/cleanup output and verify stale state manually.

**Jordan — first-time CNC user:** Jordan gets good tooltip coverage, but terms such as “V step,” “Clean V,” “profile chamfer,” and “recovery comments” require domain knowledge. The 3D canvas does not visibly teach the mouse mappings, and the aggregate “secondary moves” label does not explain what will actually be cut.

**Morgan — production operator:** Morgan benefits from preflight and explicit export paths, but needs stronger at-a-glance confirmation of units, current output, companion files, and warnings before copying or saving. The current information is present but distributed.

## Minor Observations

- The dark palette is appropriate, but muted labels should be checked at the smallest text sizes against the graphite panels, especially disabled controls and compact status rows.
- The settings/sidebar layout uses many horizontal rows with fixed label widths. Long translated or descriptive labels could clip or force awkward wrapping.
- “Show toolpath,” “Show secondary moves,” and “Show profile tabs” are understandable individually but would benefit from a single consistent layer legend and naming scheme.
- The native UI has no browser-style hover detector findings; egui harness screenshots are the right long-term visual regression surface.
- Project caching improves perceived performance, but the UI should expose whether displayed output is restored from cache or freshly generated in a stable, non-alarming way.

## Questions to Consider

- Should the right sidebar be reorganized around the user’s machining decision sequence rather than the current settings/component grouping?
- Would a named layer legend with tool/cutter identity reduce more risk than adding more preview color or geometry?
- What is the minimum preflight checklist you would require before allowing “Save to file” to feel unquestionably safe?
