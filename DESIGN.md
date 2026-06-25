---
name: R-Engrave
description: Native CNC engraving workbench for reliable F-Engrave-compatible toolpaths.
colors:
  preview-bg: "#1c1e20"
  overlay-bg: "#121416d2"
  panel-bg: "#24272a"
  window-bg: "#2a2d30"
  grid-minor: "#2b3033"
  grid-major: "#383f43"
  divider: "#3a4246"
  hover-surface: "#3e484e"
  selection-blue: "#36738d"
  active-blue: "#5d7f8f"
  text-primary: "#d6dce0"
  text-muted: "#969ea4"
  separator-muted: "#788288"
  cut-green: "#5eb084"
  logo-green: "#2f9e63"
  logo-green-deep: "#1d5f3e"
  rapid-amber: "#be8e48"
  warning-amber: "#e1b054"
  cleanup-blue: "#76a4be"
  input-overlay-pink: "#e6a8dccd"
typography:
  title:
    fontFamily: "system-ui"
    fontSize: "16px"
    fontWeight: 600
    lineHeight: 1.25
  body:
    fontFamily: "system-ui"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "system-ui"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.25
  mono:
    fontFamily: "monospace"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.25
rounded:
  overlay: "4px"
  logo: "6px"
spacing:
  xs: "2px"
  sm: "4px"
  md: "8px"
  lg: "14px"
  panel: "24px"
components:
  button-standard:
    backgroundColor: "{colors.window-bg}"
    textColor: "{colors.text-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.overlay}"
    padding: "4px 10px"
    height: "22px"
  button-primary:
    backgroundColor: "{colors.selection-blue}"
    textColor: "{colors.text-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.overlay}"
    padding: "4px 10px"
    height: "22px"
  field-standard:
    backgroundColor: "{colors.preview-bg}"
    textColor: "{colors.text-primary}"
    typography: "{typography.label}"
    rounded: "{rounded.overlay}"
    height: "22px"
  preview-overlay:
    backgroundColor: "{colors.overlay-bg}"
    textColor: "{colors.text-primary}"
    typography: "{typography.mono}"
    rounded: "{rounded.overlay}"
    padding: "7px 8px"
---

# Design System: R-Engrave

## 1. Overview

**Creative North Star: "The Machinist's Bench"**

R-Engrave should feel like native functional software for a CNC workflow: dense, direct, and arranged for repeated use by someone checking dimensions before cutting material. The system is not trying to impress like a webpage. It should look like a workbench where every control has a job, every status indicator tells the user something useful, and every color earns its place in the toolpath review loop.

The current visual system is a restrained dark desktop interface with graphite panels, a deep preview canvas, compact form rows, and color reserved for machine state or toolpath semantics. Future UI work should strengthen this vocabulary rather than introducing marketing-page composition, decorative imagery, oversized headings, or novelty controls.

**Key Characteristics:**

- Compact desktop density with stable panel widths and fixed-height controls.
- Dark graphite surfaces that keep the toolpath preview visually dominant.
- Semantic color for cut, rapid, cleanup, warning, ready, stale, selection, and overlay states.
- Familiar egui-native controls: menus, sliders, checkboxes, drag values, combo boxes, and modal dialogs.
- Monospace readouts for numeric state, G-code statistics, scale, extents, and cursor coordinates.

## 2. Colors

The palette is restrained graphite with CNC-state accents. Neutrals carry almost every surface; green, amber, blue, and pink appear only when they communicate toolpath or machine state.

### Primary

- **Selection Steel Blue**: The selected-control and active-workflow accent. Use it for current selections, active controls, and focused command affordances.
- **Cut Green**: The primary toolpath color and ready-state signal. Use it for cutting moves, successful output state, and the R-Engrave logo field.

### Secondary

- **Rapid Amber**: Travel-move and warning-adjacent color. Use amber for rapid motion, stale output, warnings, and work-in-progress state, but keep labels explicit so the color is never the only cue.
- **Cleanup Blue**: Cleanup toolpath color. Use it only for cleanup layers and related legend/readout entries.

### Tertiary

- **Input Overlay Pink**: Input outline overlay color. Use it as a translucent inspection aid, not as general UI decoration.

### Neutral

- **Preview Charcoal**: The central preview canvas. It should remain the darkest large surface so grid, bounds, toolpaths, and overlays are readable.
- **Panel Graphite**: Primary panel and toolbar surface. Use it for sidebars, status strips, and persistent desktop chrome.
- **Window Graphite**: Raised window and modal surface. Use it for dialogs and contained controls that need to sit above panels.
- **Grid Graphite**: Minor and major grid lines. Keep grid lines low contrast; the toolpath must be louder than the measuring surface.
- **Readout Silver**: Primary text and overlay stroke color. Use it for labels, summaries, cursor coordinates, scale bars, and neutral output state.

### Named Rules

**The Accent Means State Rule.** Green, amber, blue, and pink are not decoration. They are reserved for toolpath layers, output status, warnings, selection, and overlays.

**The Preview First Rule.** The preview canvas must stay visually dominant. Panels support the work; they do not compete with toolpath geometry.

## 3. Typography

**Display Font:** system-ui
**Body Font:** system-ui
**Label/Mono Font:** monospace for numeric and code-like readouts

**Character:** Typography is native, compact, and utilitarian. R-Engrave should use the platform text stack for controls and a monospace face for machine-readable values, paths, G-code statistics, extents, coordinates, and scale labels.

### Hierarchy

- **Display**: Not part of the product vocabulary. Do not introduce hero-scale type into the desktop UI.
- **Headline** (semibold, 16px, 1.25): Use only for panel headings, modal titles, and major workbench labels.
- **Title** (semibold, 13-14px, 1.25): Use for section headers such as Layers, Statistics, Export, and Advanced.
- **Body** (regular, 13px, 1.35): Use for ordinary labels, menu items, warnings, and explanatory text.
- **Label** (regular, 13px, 1.25): Use for form rows, checkboxes, combo boxes, buttons, and tool toggles.
- **Mono** (regular, 11-13px, 1.25): Use for generated G-code summaries, cursor coordinates, scale labels, line counts, arc counts, paths, and dimension readouts.

### Named Rules

**The No Hero Type Rule.** R-Engrave is not a webpage. Large display headings, promotional type scale, and landing-page hierarchy are prohibited in the desktop workbench.

**The Numbers Are Instruments Rule.** Numeric machining state belongs in monospace text so values align mentally with measurement, code, and machine output.

## 4. Elevation

R-Engrave uses tonal layering and borders, not shadows. Depth comes from panel placement, darker preview surfaces, window fill differences, separators, modal layering, and thin strokes around overlays. This keeps the interface flat, stable, and precise.

### Named Rules

**The Flat Workbench Rule.** Surfaces are flat at rest. Do not add soft drop shadows, glass effects, or decorative elevation to make controls look more designed.

**The Stroke Shows Containment Rule.** Use a 1px stroke for overlay panels, scale bars, logo marks, and modal containment when the boundary matters.

## 5. Components

### Buttons

- **Shape:** Slightly curved desktop controls (4px radius by approximation; egui default shape unless explicitly overridden).
- **Primary:** Selection Steel Blue background with Readout Silver text for focused command emphasis. Use sparingly; not every command is primary.
- **Hover / Focus:** Hover should move through the existing Hover Surface or active widget color. Focus must remain visible and should not rely only on color.
- **Secondary / Ghost / Tertiary:** Standard egui buttons are acceptable for ordinary commands such as Fit, Reset view, Browse, Recalculate, Cancel, and Save.

### Chips

- **Style:** Toggle-style layer controls are compact text toggles, not decorative pills.
- **State:** Selected layer toggles must remain legible alongside the actual preview layer colors. Never encode layer availability by color alone.

### Cards / Containers

- **Corner Style:** Avoid card framing as a layout default. Use panels, separators, scroll areas, and overlays instead.
- **Background:** Sidebars use Panel Graphite; dialogs and windows use Window Graphite; preview overlays use translucent Overlay Black.
- **Shadow Strategy:** No shadows. See the Flat Workbench Rule.
- **Border:** Use 1px strokes only when a floating overlay, scale bar, or logo mark needs clear containment.
- **Internal Padding:** Compact desktop margins, typically 4-8px in controls and 8-14px in overlays or strips.

### Inputs / Fields

- **Style:** Fixed-height rows with right-aligned values where comparison matters. Path rows reserve width for the text field and Browse command.
- **Focus:** Use egui focus treatment or the Selection Steel Blue state. Do not invent custom web-form focus styling.
- **Error / Disabled:** Disabled controls should remain visible but lower emphasis. Errors and warnings use Warning Amber with explicit text.

### Navigation

- **Style:** Native menu bar first: File, Run, View, and debug-only menus. Keep commands grouped by workflow, not marketing categories.
- **Active State:** The toolbar states the workbench mode, active tool view, calculation state, and stale output summary.
- **Mobile Treatment:** None. This is a desktop application; responsive work is about resizable panels and avoiding clipped controls, not phone layouts.

### Preview Canvas

- **Style:** Full central surface with grid, axes, bounds, toolpath layers, scale bar, cursor readout, and layer overlay.
- **Layer Colors:** Cut Green for cuts, Rapid Amber for travel, Cleanup Blue for cleanup, Input Overlay Pink for source outlines, muted graphite for bounds and grid.
- **Interaction:** Pan, zoom, fit, reset, view rotation, and layer toggles are core controls. Keep them close to the canvas.

### Status Strips

- **Style:** Compact horizontal readouts with separators and monospace values.
- **State:** Ready uses Cut Green, working or stale uses Warning Amber, neutral state uses Readout Silver. State text must accompany every colored dot or label.

## 6. Do's and Don'ts

### Do:

- **Do** keep the interface compact, direct, and arranged around the CNC workflow: input, settings, preview, output, status, and logs.
- **Do** reserve accent colors for toolpath layers, output state, warnings, selection, and overlays.
- **Do** keep the preview canvas central and visually dominant.
- **Do** use monospace readouts for machine-like values: coordinates, dimensions, line counts, arc counts, scale, paths, and G-code summaries.
- **Do** keep every state label explicit; never rely on color alone for critical machining or output state.
- **Do** preserve familiar egui desktop affordances: menus, checkboxes, sliders, combo boxes, drag values, scroll areas, and modals.

### Don't:

- **Don't** make R-Engrave feel like a webpage or marketing surface.
- **Don't** use hero layouts, decorative imagery, oversized typography, promotional copy, empty visual flourish, or controls arranged for visual novelty instead of task flow.
- **Don't** add cards as decorative section wrappers. Use panels, separators, and the preview canvas structure.
- **Don't** introduce gradient text, glass effects, soft oversized shadows, diagonal stripe backgrounds, or side-stripe card accents.
- **Don't** invent mobile-first or landing-page responsive patterns. This is resizable desktop software.
- **Don't** use color as the only indicator for warnings, stale output, readiness, active layers, or disabled functionality.
