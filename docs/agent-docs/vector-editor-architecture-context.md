# Vector editor architecture context

Date captured: 2026-08-17

This document records the architectural guidance discussed after reviewing
`/tmp/chatgpt_resp.md` against the current R-Engrave repository. It is a
planning context document, not an implementation specification.

## Architectural direction

The proposal is directionally sound: R-Engrave should grow toward a native,
CNC-oriented vector design workbench while retaining its existing
F-Engrave-compatible calculation and export behavior.

The central boundary is:

```text
editable design geometry
        -> CAM operation geometry
        -> toolpath segments
        -> G-code, preview, and simulation
```

`PreviewSegment` must remain a derived UI preview representation. It is a
start/end line segment suitable for displaying generated motion, but it is
not an editable design model. The current `layout::EngraveSegment` is a more
meaningful core representation, but it is also too close to machining output
to serve as the long-term authored design model.

## Proposed native model

Add a `VectorDocument`-style model in `rengrave-core`. It should preserve
authored geometry and editing identity, including:

- layers or objects;
- contours and open/closed state;
- stable node, segment, contour, and object IDs;
- lines, arcs/DXF bulges, quadratic curves, and cubic curves;
- holes, winding, grouping, and ordering;
- document-space bounds and transforms; and
- enough metadata to distinguish authored geometry from derived geometry.

Use Kurbo for curve mathematics and conversion, but do not make Kurbo types
the persisted public model. A R-Engrave-specific representation preserves
freedom to support CNC-specific semantics and schema evolution.

Prefer a stable-ID path/segment model over a model made exclusively of
Bezier nodes. Nodes can represent endpoints while segments retain their own
line, arc, quadratic, or cubic data. This avoids unnecessarily approximating
DXF arcs and makes CNC-relevant geometry explicit.

## Existing pipeline compatibility

Do not replace the current legacy batch pipeline immediately. The current
workspace has a clear core/UI split:

- `rengrave-core` owns settings compatibility, parsing, layout, toolpath
  algorithms, and exports;
- `rengrave-ui` owns the native egui application, controls, background
  calculation, and preview rendering; and
- `rengrave-cli` calls the same core calculation boundary.

SVG, DXF, font, and bitmap inputs should first gain adapters into the native
design model. Existing legacy settings and batch generation should remain
available behind an adapter until native design-to-CAM output has equivalent
focused and golden coverage.

Legacy concepts should remain for actual import and migration needs, but they
should not define the new native workbench.

## Geometry and CAM separation

The editor modifies design geometry. A separate CAM preparation layer decides:

- which contours participate in an operation;
- engraving, pocket, profile, cleanup, or V-carve semantics;
- tool, depth, tabs, ordering, and passes;
- flattening tolerance for operations that require polygons; and
- the intermediate toolpath representation consumed by exporters and
  preview/simulation.

Curves must not be permanently flattened merely because a renderer or
polygon boolean engine needs line approximations. Keep authored curves and
make flattening an explicit, tested boundary. Continue using the existing
`clipper2` integration initially; investigate another polygon engine only if
specific limitations are demonstrated.

## UI architecture

Keep egui/eframe. Extract reusable canvas concerns from the current preview
implementation into a `Canvas2D`/view-transform abstraction shared by the
design editor and toolpath preview. It should provide model/screen conversion,
zoom-at-cursor, pan, fit-to-bounds, and coordinate readouts without allowing
view state to alter exported model coordinates.

A future editor can be organized approximately as:

```text
rengrave-ui/
  vector_editor.rs
  vector_editor/
    interaction.rs
    rendering.rs
    selection.rs
    snapping.rs
    commands.rs
```

Selection, hit testing, snapping, and interaction state should be explicit
subsystems rather than scattered pointer-event conditionals. Selection should
support combinations of objects, contours, segments, nodes, and handles as
the feature set grows.

## Undo/redo and persistence

Use document-level edit commands for mutations such as moving geometry,
inserting/deleting nodes, splitting/joining paths, and transforms. The UI
should translate gestures into validated commands; commands should not own
egui pointer state.

Extend the versioned `.rgrv` project format additively. Use defaults and
explicit migration handling for the native design document, CAM operations,
and future cached intermediate toolpaths. Project-local tool assignments
must remain frozen snapshots rather than silently following later library
changes.

## Recommended implementation sequence

1. Define and test the native `VectorDocument` model.
2. Add stable IDs, contours, segments, bounds, transforms, and persistence.
3. Add SVG/DXF/font adapters without changing existing output.
4. Extract the reusable canvas/view-transform layer.
5. Add selection and hit testing.
6. Add move and transform commands with undo/redo.
7. Add curve handles, arcs, node insertion/deletion, and snapping.
8. Add CAM-operation assignment and an intermediate toolpath model.
9. Add offset, booleans, trim, join, and related geometry operations.
10. Add the Design workbench and connect it to the existing toolpath preview.

The first useful vertical slice should import one SVG, preserve its curves,
allow contour selection and movement, support undo/redo, save/reopen the
project, and prove unchanged geometry still produces the existing output.

## Validation boundaries

Each native geometry change should test geometry semantics directly, not only
rendering or generated G-code. Validate the intermediate geometry/toolpath
model before parsing G-code. Preserve deterministic ordering, tolerances,
units, cancellation, and compatibility output. UI changes need egui harness
interaction and populated visual evidence where layout or pointer behavior is
part of the contract.

The intended product remains a purpose-driven native desktop workbench, not a
web-style application and not an immediate attempt to clone all of VCarve.
