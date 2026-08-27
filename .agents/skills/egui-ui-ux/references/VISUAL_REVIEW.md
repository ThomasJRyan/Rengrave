# Visual review rubric

Use this only after the affected UI has been rendered. Review the image, not the source code.

## 1. Structure

- Does the panel hierarchy fit the user's task?
- Is important content persistently visible when it should be?
- Is scrolling attached to the right region rather than the entire window by accident?
- Are fixed vs flexible regions allocated intentionally?

## 2. Hierarchy

- Can a user identify the screen purpose and primary action within a few seconds?
- Are too many controls competing for emphasis?
- Are destructive actions appropriately distinct but not dominant?

## 3. Alignment

- Check left edges, label/value columns, control baselines, table headers, button groups, and repeated rows.
- Prefer a small number of strong alignment lines over decorative boxes.

## 4. Spacing and grouping

- Related controls should be closer to each other than to unrelated controls.
- Repeated rows should use consistent rhythm.
- Empty space should communicate grouping or afford resizing; it should not appear accidental.

## 5. Density

- Desktop tools should use the viewport efficiently.
- Avoid both cramped controls and web-like expanses of whitespace.
- Tables, inspectors, and settings views should expose useful information without forcing unnecessary scrolling.

## 6. Interaction states

Check, when applicable:

- normal;
- hover;
- focus;
- selected;
- pressed/active;
- disabled;
- error/warning;
- destructive;
- loading/progress;
- empty state.

Do not rely on color alone to communicate important state.

## 7. Copy

- Use specific labels and concise action verbs.
- Avoid redundant headings and explanatory prose where control labels already make the action clear.
- Keep terminology consistent with the rest of the application.

## 8. Polish

Only after structure, hierarchy, alignment, spacing, density, states, and copy are sound, refine color, strokes, radii, icon sizing, and other cosmetic details.
