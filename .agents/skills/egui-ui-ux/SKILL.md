---
name: egui-ui-ux
description: Improve UI/UX quality in Rust egui applications through a design-first, screenshot-driven workflow. Use when creating, modifying, reviewing, or polishing egui screens, layouts, widgets, desktop interactions, visual hierarchy, spacing, styling, accessibility, or UI tests. Also use when translating a visual design or reference screenshot into egui, diagnosing a UI that feels awkward or inconsistent, or adding egui_kittest interaction or snapshot coverage.
---

# egui UI/UX

Produce deliberate desktop UI rather than a pile of locally-correct widgets. Separate UX decisions, implementation, and visual evaluation so the rendered result—not the code alone—determines whether the work is finished.

## Core workflow

For meaningful UI changes, follow this sequence:

1. Inspect the existing UI architecture, theme/tokens, reusable widgets, and nearby screens before editing.
2. State the screen's primary user goal and identify the primary action, secondary actions, information groups, and persistent navigation.
3. Choose an egui-compatible layout structure before writing widget code.
4. Reuse existing design tokens and shared widgets. Add reusable primitives when a pattern appears more than once or is visually important.
5. Implement the smallest coherent UI change.
6. Run relevant Rust checks and UI interaction tests.
7. Render or capture the affected UI when the repository provides a screenshot/snapshot path.
8. Inspect the rendered image as a critic. Do not infer appearance from source code.
9. Fix the largest visual or interaction problem first, then render again.
10. Stop when the UI is coherent and the remaining differences are intentional, not merely when it compiles.

Do not combine implementation and self-approval into one step. Treat the first render as evidence to evaluate.

## Before coding

For a new or substantially changed screen, briefly resolve:

- What is the user's main task on this screen?
- What should attract attention first?
- Which actions are primary, secondary, destructive, or contextual?
- Which information belongs together?
- What should remain visible while the user works?
- Is the screen a workspace, inspector, settings page, data table, dialog, dashboard, or navigation surface?

Prefer familiar desktop application patterns over web-marketing patterns.

## egui layout guidance

Use the highest-level primitive that expresses the intended structure.

Prefer:

- `TopBottomPanel` for toolbars, menubars, status areas, and persistent top/bottom chrome.
- `SidePanel` for navigation, inspectors, and persistent auxiliary controls.
- `CentralPanel` for the primary workspace.
- `ScrollArea` for content that can legitimately exceed the viewport.
- `Grid` for aligned property/value or form rows.
- `egui_extras::TableBuilder` for structured tabular data, especially with headers, resizing, and scrolling.
- `egui_extras::StripBuilder` when explicit row/column allocation is more important than normal child-driven layout.
- reusable custom widgets for repeated visual patterns.

Avoid long chains of nested `ui.horizontal` / `ui.vertical` calls when the desired geometry is actually a grid, table, split layout, or panel hierarchy.

Avoid hard-coded magic numbers scattered through screen code. Put repeated spacing, sizing, typography, radii, and color decisions into project-level tokens or shared style helpers.

## Desktop UI defaults

Favor:

- compact but readable information density;
- persistent navigation for persistent context;
- toolbars for frequent actions;
- inspectors/property grids for object details;
- selection, hover, focus, disabled, active, and destructive states that are visually distinguishable;
- context menus for contextual actions;
- keyboard shortcuts for frequent commands when appropriate;
- status/progress feedback for long-running actions;
- clear empty, loading, error, and disabled states;
- resizable panes/columns when users benefit from controlling workspace allocation.

Avoid by default:

- giant headings or hero sections;
- card-per-section layouts when simple grouping is clearer;
- excessive rounded containers;
- decorative gradients and ornamental effects;
- large vertical gaps that reduce useful workspace density;
- icon-only controls whose meaning is not obvious or discoverable;
- using color as the sole state indicator.

## Visual hierarchy

Use contrast intentionally. A screen should normally have one dominant content/action hierarchy, not many competing accents.

Check:

- primary actions are visually stronger than secondary actions;
- section headings are distinct without becoming oversized;
- labels and values align consistently;
- related controls share spacing and containment;
- destructive actions are differentiated without dominating the screen;
- disabled state remains readable but clearly inactive;
- long labels and translated text have room to grow;
- dense information uses alignment before decoration.

## Design system behavior

Before introducing a new visual constant, search for an existing token, theme value, helper, or reusable widget.

When the project lacks a design system and the change is substantial, prefer establishing a small one instead of duplicating values. Useful categories include:

- spacing scale;
- common control heights;
- icon sizes;
- panel widths/minimums;
- text styles;
- semantic colors;
- strokes and corner radii.

Do not refactor the entire UI merely to introduce tokens. Extract the smallest useful shared vocabulary and expand it as repetition becomes real.

## Screenshot-driven review

If a screenshot, snapshot, or rendered image can be produced, inspect it after implementation.

Evaluate in this order:

1. **Structure** — Is the overall panel/layout model correct for the task?
2. **Hierarchy** — Is it immediately obvious where to look and what to do?
3. **Alignment** — Do columns, labels, controls, and baselines line up?
4. **Spacing** — Are related things close and unrelated things separated?
5. **Density** — Is useful information visible without feeling cramped?
6. **States** — Are selection, hover, focus, disabled, error, and destructive states clear?
7. **Copy** — Are labels concise, specific, and consistent?
8. **Polish** — Only after the above, consider subtle stylistic refinement.

Do not spend a refinement pass polishing colors or radii while structural or alignment problems remain.

When a reference screenshot exists, match its design principles and geometry rather than blindly copying branding or pixel values.

## egui_kittest

Use `egui_kittest` when the project already uses it or when the task benefits from repeatable interaction/snapshot coverage.

Prefer normal behavioral assertions for interaction and state correctness. Use image snapshots selectively for surfaces where geometry, clipping, alignment, or visual regression matters.

For snapshot failures:

- inspect the expected, new, and diff images;
- determine whether the difference is intended;
- fix accidental differences before updating the baseline;
- never update snapshots merely to make a failing test green;
- keep comparison tolerances as strict as practical.

If the test environment produces platform-specific rendering differences, follow the repository's existing tolerance and platform policy rather than inventing a new one casually.

## Accessibility and interaction

Treat accessibility metadata and keyboard behavior as part of UX, not post-polish.

Ensure controls have meaningful labels when their visible content is ambiguous. Preserve logical tab/focus order. Avoid interaction targets that are unnecessarily tiny. Provide tooltips for unfamiliar icon-only actions. Prefer native egui interaction semantics over hand-built pointer handling unless custom behavior is truly required.

## Change discipline

For a requested UI improvement, do not opportunistically redesign unrelated screens.

Preserve existing product conventions unless the task explicitly calls for a broader redesign. When existing conventions conflict with good UX, call out the conflict and make the smallest coherent improvement.

Do not introduce a new dependency for a single trivial styling problem. A dependency is justified when it meaningfully simplifies a recurring layout, widget, testing, or interaction need.

## Completion criteria

A UI task is complete only when applicable checks pass and the implementation satisfies all of the following:

- the primary user task is clear;
- layout primitives fit the intended desktop structure;
- spacing and alignment are internally consistent;
- repeated visual decisions use shared project vocabulary where appropriate;
- common interaction states are handled;
- the UI has been visually inspected when rendering is available;
- snapshot baselines were changed only for intentional visual changes;
- no unrelated redesign was bundled into the change.

## Repository integration

If the repository contains UI-specific instructions in `AGENTS.md`, `CONTRIBUTING.md`, design documentation, or local references, treat those as project-specific constraints and combine them with this workflow. Do not assume this skill overrides repository instructions.

For a recommended drop-in `AGENTS.md` policy that works with this skill, read `references/AGENTS_FRAGMENT.md`.
