# egui UI/UX workflow

These rules apply to UI/UX work in Rust `egui` code. They supplement the rest of this repository's agent instructions; they do not replace or weaken existing architecture, testing, style, or product requirements.

## Design before implementation

For a meaningful UI change, identify the screen's primary user goal, primary action, secondary actions, information groups, and persistent navigation before editing code. Prefer familiar desktop application patterns over web/marketing patterns.

Choose the intended egui structure before composing individual widgets. Prefer `TopBottomPanel`, `SidePanel`, `CentralPanel`, `ScrollArea`, `Grid`, `egui_extras::TableBuilder`, and `egui_extras::StripBuilder` where they express the layout more directly than deeply nested `ui.horizontal()` / `ui.vertical()` blocks.

## Use the existing design vocabulary

Before adding a new spacing value, color, control size, text style, radius, or stroke, search for an existing project token, theme value, helper, or reusable widget. Avoid scattering magic visual constants through screen code.

If repeated UI decisions lack a shared abstraction, create the smallest useful token or reusable primitive rather than copying values. Do not perform a broad design-system refactor unless the task requires it.

## Separate implementation from visual review

Do not assume UI code looks correct because it compiles or is locally reasonable. After implementing a meaningful visual change, use the repository's available render/screenshot/snapshot workflow and inspect the actual result.

Review rendered UI in this order:

1. overall structure and task flow;
2. visual hierarchy;
3. alignment;
4. spacing and grouping;
5. information density;
6. interaction states;
7. labels/copy;
8. cosmetic polish.

Fix structural problems before cosmetic ones. If the repository cannot render UI in the current environment, say so explicitly rather than claiming visual correctness.

## Desktop UX defaults

Favor compact readable density, persistent navigation, toolbars, inspectors/property grids, tables for tabular data, clear selection/focus/disabled/error states, keyboard shortcuts for frequent commands, context menus for contextual actions, and visible progress/status feedback.

Avoid giant headings, hero layouts, unnecessary cards, excessive rounded containers, decorative gradients, excessive whitespace, and ambiguous icon-only actions unless the product intentionally calls for them.

## Interaction and accessibility

Treat keyboard behavior, accessibility labels, focus order, hit target size, tooltips, and non-color state cues as implementation requirements where relevant. Prefer native egui interaction semantics over custom pointer logic unless custom behavior is necessary.

## Testing and snapshots

Use normal tests for behavior/state correctness. Use `egui_kittest` image snapshots selectively where visual geometry, clipping, alignment, or regression detection matters.

On a snapshot failure, inspect the expected/new/diff images. Do not update a snapshot merely to make the test pass. Update the baseline only after confirming the visual change is intentional. Keep tolerances as strict as practical and follow existing repository/platform policy.

## Scope discipline

Preserve the application's existing visual language unless the task explicitly requests a redesign. Do not redesign unrelated screens while touching one surface. Prefer the smallest coherent improvement that solves the UX problem.

## Definition of done for meaningful UI changes

Before declaring the work finished, verify that:

- the main user task is obvious;
- the chosen layout matches the intended desktop workflow;
- spacing/alignment are internally consistent;
- repeated visual values use shared vocabulary when appropriate;
- relevant interaction states are handled;
- tests/checks pass;
- the rendered UI was inspected when rendering is available;
- any snapshot updates represent intentional changes.
