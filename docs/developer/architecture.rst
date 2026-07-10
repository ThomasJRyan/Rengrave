Architecture
============

Design constraints
------------------

R-Engrave is deliberately split so the UI is not the source of machining
truth. ``rengrave-core`` owns settings, geometry, parsing, toolpath generation,
and exports. Both ``rengrave-ui`` and ``rengrave-cli`` call that same core
pipeline. ``f-engrave_source/`` is retained as a reference and should remain
unchanged unless a task explicitly concerns upstream material or licensing.

::

   rengrave-cli ───────┐
                       ├──> rengrave-core ──> G-code / SVG / DXF
   rengrave-ui ────────┘          │
                                  ├──> preview geometry
                                  ├──> warnings / progress
                                  └──> legacy settings recovery

The core has no dependency on ``eframe`` or ``egui``. The UI owns presentation,
preferences, the input catalog, file dialogs, background calculation, and
preview parsing. The CLI owns argument translation and file writes.

Repository map
--------------

.. list-table:: Repository map
   :header-rows: 1
   :widths: 30 70

   * - Path
     - Responsibility
   * - ``core/src/project.rs``
     - document/project loading and path resolution
   * - ``core/src/settings.rs``
     - legacy keys, booleans, TCODE, defaults, emission
   * - ``core/src/font.rs``
     - CXF/TTF font parsing and glyph strokes
   * - ``core/src/dxf.rs``
     - DXF geometry and font-like vector input
   * - ``core/src/svg.rs``
     - SVG vector input
   * - ``core/src/bitmap.rs``
     - bitmap thresholding and native tracing handoff
   * - ``core/src/layout.rs``
     - text placement, transforms, bounds, origins
   * - ``core/src/vcarve.rs``
     - maximum-circle V-carve sampling and ordering
   * - ``core/src/cleanup.rs``
     - offsets, boolean regions, scanlines, ordering
   * - ``core/src/profile.rs``
     - profile envelope, corners, depth passes, tabs
   * - ``core/src/gcode.rs``
     - motion emission, arcs, trailers, simplification
   * - ``core/src/export.rs``
     - SVG and DXF serialization
   * - ``core/src/batch.rs``
     - shared staged pipeline and cancellation
   * - ``ui/src/lib.rs``
     - app state, worker lifecycle, panels, export UI
   * - ``ui/src/preview.rs``
     - parsed G-code preview layers and transforms
   * - ``ui/src/controls.rs``
     - UI-to-legacy settings mapping
   * - ``ui/src/widgets.rs``
     - Shared parameter rows and the centralized tooltip copy catalog
   * - ``cli/src/main.rs``
     - clap options and artifact writing

Batch pipeline
--------------

``prepare_batch_output_with_cancel_and_progress`` is the integration seam for
both UI and CLI. It reports explicit stages such as document loading, font/DXF/
SVG loading, bitmap vectorization, layout, V-carve, cleanup, profile emission,
and rendering. Cancellation is checked at stage and inner-loop boundaries.

|workflow|

The pipeline is intentionally staged:

#. ``load_document`` reads defaults or legacy comments, applies path/text
   overrides, normalizes bitmap settings, and resolves the actual input.
#. The input is parsed or vectorized into stroke segments.
#. ``layout_text`` applies scale, spacing, text-on-circle, transforms, box,
   profile-aware origin bounds, and explicit X/Y origin offsets.
#. The selected operation generates primary segments or depth-aware points.
#. Optional cleanup and profile operations are generated as secondary or
   companion operations.
#. G-code is emitted with units, preamble, safe-Z/plunge/cut motion, optional
   arcs, postamble, and return-to-origin.
#. SVG and DXF are rendered from the shared primary layout representation.

Data ownership and invariants
-----------------------------

``Point`` and ``EngraveSegment`` represent model-space geometry. Segments carry
``loop_id`` so path ordering and cleanup can recover loop boundaries. Bounds
are axis-aligned in model coordinates. Toolpath point types add a cutter radius
or Z-dependent state without mutating the source geometry.

The most important invariant is shared coordinates: input preview, generated
toolpath preview, SVG/DXF export, and G-code must be derived from the same
settings and transformed geometry. If a UI-only transform is introduced, it
must be clearly marked as view state and must not change exported coordinates.

UI lifecycle
------------

The UI loads preferences and a ``DocumentRequest`` at startup, derives
``UiControls`` from legacy settings, then launches a background calculation
worker. The worker returns ``BatchOutput``; the UI parses primary and secondary
G-code into preview layers and derives extents, move counts, cut/rapid lengths,
arc counts, and warnings. Manual settings edits persist through UI preferences
and stale-output detection names the changed request areas.

Parameter help is centralized in ``rengrave-ui/src/widgets.rs``. Numeric,
combo, text, and path rows attach the matching explanation to both the label
and interactive control; parameter checkboxes use the same catalog, while
cleanup-path checkboxes add index-specific descriptions because labels such as
``X`` and ``Profile`` occur for both cutter types. Keep tooltip copy brief and
describe the effect on layout, tool motion, output, or compatibility rather
than repeating the visible label.

The native preview applies pan, zoom, model rotation, and viewport rotation at
draw time. Cursor coordinates are converted back through the same view
transform so the readout remains in model space.
