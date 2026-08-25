R-Engrave architecture
======================

This page describes the architecture currently implemented under
``crates/``. The central rule is that machining behavior belongs to
``rengrave-core``; the desktop UI and command line interface are clients of
the same calculation pipeline.

Workspace shape
---------------

The Cargo workspace contains four crates:

.. list-table:: Crates
   :header-rows: 1
   :widths: 25 75

   * - Crate
     - Responsibility
   * - ``rengrave-core``
     - Settings compatibility, project files, input parsing, layout, toolpath
       algorithms, G-code, SVG, and DXF generation.
   * - ``rengrave-ui``
     - Native ``eframe``/``egui`` application, controls, file browsing,
       preferences, background calculation, and preview rendering.
   * - ``rengrave-cli``
     - ``clap`` command line entry point for GUI launch, batch output, direct
       SVG/DXF exports, and debug artifacts.
   * - ``rengrave-potrace``
     - Native bitmap-to-curve tracing. It is used by ``rengrave-core`` and has
       no dependency on the UI or CLI.

Dependency direction
~~~~~~~~~~~~~~~~~~~~

.. code-block:: text

       user actions                         batch flags
            |                                      |
            v                                      v
     +-------------+                        +-------------+
     | rengrave-ui |                        | rengrave-cli|
     +-------------+                        +-------------+
            |                                      |
            +------------------+-------------------+
                               v
                    +-----------------------+
                    |    rengrave-core     |
                    | model + calculations |
                    +-----+------------+----+
                          |            |
                          v            v
                 rengrave-potrace   output artifacts
                                  G-code / SVG / DXF

``rengrave-core`` has no ``egui`` or ``eframe`` dependency. This keeps the
calculation and compatibility contract usable from both entry points and
testable without a native window.

General workbench units
-----------------------

The General Purpose workbench keeps its job dimensions and XY datum offsets
in millimetres in ``GeneralJobSetup``. ``general_display_value`` and
``general_storage_value`` form the UI boundary: controls convert to the
selected inches or millimetres display unit for editing, then immediately
convert edits back to millimetres. This prevents unit changes from mutating
the physical project dimensions. ``general_drag_speed`` keeps numeric drag
sensitivity in display units: 0.01 per step for inches and 0.1 per step for
millimetres. Job Setup and vector parameter controls share this boundary.
The reusable ``resettable_value_input`` widget wraps an arbitrary EGUI editor
closure, so it preserves the normal ``DragValue`` or ``TextEdit`` builder
configuration. It always reserves a 22-pixel square reset slot with a 3-pixel
gap, while only painting and exposing the SVG button when the value differs
from its default. Keeping the allocation count, input bounds, and generated
EGUI identity stable prevents a visibility transition from interrupting an
active numeric drag. The widget marks the wrapped response changed after a
reset and lets the existing immediate-mode scene path refresh both viewports.
Comparisons and defaults are supplied by the caller so canonical millimetre
values and text values can use appropriate equality rules.

The 2D canvas and rulers use the selected display unit for screen coordinates
and labels. When the unit selection changes, the workbench inversely rescales
its view zoom by ``25.4`` so the same physical canvas remains the same size on
screen while ruler values change to the new unit. The conversion is currently
local to the layout-only General workbench; it should become part of the
shared project model when that workbench gains a functional backend.

The left ruler intentionally inverts its displayed value relative to the
screen-space Y value: ``general_y_ruler_label(y, step)`` delegates to the
common formatter with ``-y``. Tick positions, canvas geometry, and crosshair
placement remain unchanged, so only the vertical ruler's sign convention
changes: screen-above-zero is labelled negative and screen-below-zero is
labelled positive.
Authored General design points use the same convention at the view boundary:
``general_display_point_to_view_point`` negates display Y before calling the
shared preview transform, and ``general_view_point_to_storage_point`` negates
it again when converting pointer selection back to stored millimetres. This
keeps editing and hit testing aligned with the displayed position fields.

The initial 2D frame and the **Centre Job (F7)** action calculate a fit zoom from
the displayed job width and height, the viewport size, and the fixed ruler
padding. They leave a 12 percent margin based on the smaller viewport
dimension, then clear pan and rotation so the job's center aligns with the
viewport's center. The button and ``F7`` key path share the same action.

The General 3D View consumes a ``GeneralScene`` built from the same canonical
millimetre job setup. Its object list is deliberately separate from the
legacy G-code preview layers: the first object is a shaded ``JobStock`` solid,
and future design geometry, toolpaths, finished-stock views, or animation
layers can be added without changing navigation. The renderer uses a small
CPU-side face list and egui painter polygons; it has no grid or ruler pass.
The upper-right orientation gizmo projects the three world-axis unit vectors
through the same yaw and pitch transform as the scene. Lettered endpoints
keep the axes distinguishable without relying on color alone.
Stock faces are sorted from farthest to nearest in camera space before they
are painted, preserving solid-face occlusion without a GPU depth buffer. The
renderer computes each face's outward normal in the same Z-scaled camera space
as projection and submits only camera-facing faces with a normalized alignment
above ``0.01``. ``GeneralSceneFace`` vertices must therefore remain wound
counter-clockwise when viewed from outside the solid. The renderer also culls
faces whose shortest screen-space dimension is narrower than their outline
before polygon tessellation. Visible faces are filled without a joined polygon
stroke, then outlined with finite line segments. Together these rules prevent
rear, nearly edge-on, or degenerate stroke geometry from extending beyond the
stock. The entire 3D pass uses the Editor/Preview rectangle as its painter clip,
so pan and zoom cannot draw over either side panel.

Core data flow
--------------

``batch::prepare_batch_output_with_cancel_and_progress`` is the integration
seam used by the UI and CLI. It returns a ``BatchOutput`` containing primary
G-code, warnings, optional companion G-code, and optional SVG/DXF strings.

.. code-block:: text

   BatchRequest
       |
       v
   project::load_document
       |  LegacySettings + text + resolved input + warnings
       v
   input loading / vectorization
       |  Font and strokes
       v
   layout::layout_text
       |  EngraveSegment + Bounds + optional circle border
       v
   operation selection
       |-- engrave: gcode::write_engrave_gcode
       |-- V-carve: vcarve + gcode::write_vcarve_gcode
       |-- cleanup: cleanup + gcode::write_cleanup_gcode
       `-- profile: profile + gcode::write_profile_gcode
       |
       +--> export::write_svg / write_dxf
       `--> render settings, motion, and postamble
                    |
                    v
              BatchOutput

Every stage checks the caller's cancellation function. Progress is reported
with the ``BatchProgress`` enum, including document loading, input parsing,
layout, toolpath calculation, export preparation, rendering, and completion.
Independent V-carve groups use Rayon where useful; results are collected in a
defined order so generated output remains deterministic.

Core module map
---------------

The public modules in ``crates/rengrave-core/src/lib.rs`` form a layered
pipeline rather than separate application modes.

* ``project.rs`` loads legacy settings and versioned ``.rgrv`` JSON project
  files, resolves input paths, and carries authored design geometry, cached
  output, and tool assignments.
* ``settings.rs`` owns the ordered legacy key/value representation, defaults,
  booleans, TCODE text settings, and compatibility serialization.
* ``font.rs``, ``dxf.rs``, and ``svg.rs`` normalize CXF, TTF, DXF, and SVG
  input into glyphs and strokes. ``bitmap.rs`` converts image input through
  the native tracer and the same DXF/stroke path.
* ``geometry.rs`` provides points and view-independent transforms. ``layout.rs``
  turns strokes and text into final coordinate-space ``EngraveSegment`` values,
  applying scale, spacing, justification, origins, rotation, circles, and
  optional boxes.
* ``vcarve.rs`` calculates depth-aware V-bit points and ordering.
  ``cleanup.rs`` calculates offset, boolean, and scanline cleanup paths.
  ``profile.rs`` creates profile envelopes, depth passes, chamfers, and tabs.
* ``gcode.rs`` converts each operation into machine motion, including units,
  safe-Z moves, plunge/cut feeds, optional arc fitting, tabs, preamble, and
  postamble.
* ``export.rs`` serializes the shared layout segments to SVG or DXF. The
  exports do not reconstruct geometry from the UI or from G-code.
* ``toolbit.rs`` stores persistent tool definitions and role assignments;
  ``external.rs`` contains input-format classification helpers.
* ``batch.rs`` composes all of these modules and is the stable application
  boundary for calculation, progress, cancellation, and output collection.

Shared model boundary
---------------------

The authoritative geometry is model-space ``Point`` and
``layout::EngraveSegment`` data. Bounds, loop identifiers, toolpath points,
preview geometry, SVG/DXF output, and G-code are derived from this coordinate
space. A UI pan, zoom, or viewport rotation is view state and must not alter
exported coordinates.

The project boundary is separate from the calculation boundary:

* ``RengraveProjectFile`` persists versioned settings, text, input paths,
  workbench information, authored ``VectorDocument`` geometry, toolbit
  snapshots, output paths, and matching output caches. The additive
  ``design_document`` field uses a default so older version-1 files load with
  an empty design document.
* ``BatchRequest`` describes one calculation. It is assembled from a project,
  legacy settings file, CLI arguments, or UI controls.
* ``BatchOutput`` is transient generated output. The UI may cache it in a
  project only when it still represents the saved request.

Desktop UI lifecycle
--------------------

``rengrave-ui/src/lib.rs`` owns ``RengraveApp`` and composes the private UI
modules:

* ``controls.rs`` maps widgets to legacy settings and detects stale output.
* ``preview.rs`` parses primary and secondary G-code into preview layers and
  performs model-to-screen transforms.
* ``input_preview.rs`` shows source vectors or bitmap information before and
  alongside the generated toolpath.
* ``catalog.rs`` scans usable fonts and input files; ``browser.rs`` handles
  project, input, directory, and output selections.
* ``preferences.rs`` persists small ``key=value`` UI preferences; ``widgets.rs``
  provides shared form rows and parameter help.

The application lifecycle has two explicit screens:

* ``Startup`` renders only the menu bar, the 20%-wide project-action pane,
  and the 80%-wide logo pane. It does not start a calculation or expose
  workbench controls.
* ``Workbench`` renders the existing input, settings, preview, output, and
  status panels. It is entered by selecting a new workbench or successfully
  loading a project.

Normal desktop launches enter ``Startup``. Explicit CLI input arguments retain
the direct workbench launch path for batch-oriented workflows. The startup
logo is embedded from ``assets/logo/logo-full.png`` at compile time so the
installed application does not depend on the developer's absolute checkout
path.

Recent project paths are UI preference state. Successful project loads and
saves move a path to the front of a bounded ten-entry list, remove duplicates,
and persist the list as repeated ``recent_project=`` records. Missing paths
are retained and disabled in the recent-project window rather than being
silently discarded. Preference writes use a temporary sibling file followed by
rename so a partially written recent-project list is not treated as valid UI
state.

The UI starts a calculation on a background thread, sends progress through an
``mpsc`` channel, and uses an ``Arc<AtomicBool>`` cancellation flag. Results
are applied only for the current calculation id. This keeps the egui render
thread responsive and prevents an older result from replacing newer controls.

``ToolView::GeneralPurpose`` is the layout-only workbench foundation. It is
persisted using the ``general-purpose`` workbench identifier but is not part
of ``ToolView::ALL`` and therefore does not receive a machining-tool icon.
Its renderer owns the selected Tool Panel tab and the selected 2D/3D
Editor/Preview tab. The renderer
allocates approximately 256 pixels to the Tool Panel, 15% to the Toolpath
Panel, and the remaining width to the Editor/Preview Panel, leaving the panels
otherwise unimplemented until future requirements define their contents. The
2D Editor/Preview tab reuses the preview module's grid renderer, zoom-anchor
transform, and ``ViewTransform`` state. Its painter is clipped to the center
panel rect so transformed grid endpoints cannot paint into neighboring panels.
The canvas is rendered from the local ``GeneralJobSetup`` width and height,
centered at the viewport origin, and scaled by the General Purpose transform.
Top and left rulers derive their tick positions from the same transform and
canvas-centered coordinate system. When the pointer is over the canvas, the
renderer draws clipped horizontal and vertical crosshair lines through the
corresponding ruler positions.
Ruler and grid spacing use a denser variant of the preview's readable step;
labels are integer-rounded while the step is at least one model unit and are
formatted to a maximum of two decimal places for fractional steps.
The
first Tool Panel tab now owns a local ``GeneralJobSetup`` presentation state
for job type, dimensions, units, Z zero, XY offset, and modeling resolution.
The job-type and modeling-resolution state is retained for later work, but
their render groups are currently disabled by the
``GENERAL_JOB_TYPE_VISIBLE`` and
``GENERAL_MODELING_RESOLUTION_VISIBLE`` visibility constants.
This state is intentionally not included in ``BatchRequest``, project
persistence, geometry, or export until the general workbench contract defines
its backend behavior. The presentation form is width-bounded by the Tool
Panel's content area; fixed-size inputs are kept compact and the datum block
uses a vertical arrangement. Horizontal setup rows use explicit inner-width
spacing to align values at the right edge without allowing native egui
controls to expand beyond the fixed-width panel boundary.

The second Tool Panel tab is the Design presentation surface. Its
``show_general_design_tools`` renderer defines three fixed categories:
Create Vectors, Transform Objects, and Edit Objects. Every category delegates
to the same bounded five-slot row helper, which calculates a square button size
from the bounded content width and fixed inter-button gaps. Empty grid cells
reserve the same row geometry, so tools added later remain aligned across
categories. The Design scroll area preserves the full width allocated by the
Tool Panel; it does not apply a second inner-width cap before group frame
margins are calculated. This matches Job Setup and keeps category frame bounds
inside the panel separator. The shared group helper treats its width limit as
an outer bound and subtracts the frame's measured border and horizontal margins
before assigning the inner content width. Horizontal setup rows consume that
derived inner width rather than imposing a competing fixed width, so every
Job Setup and Design category resolves to the same outer width.

Design tool artwork is stored as SVG under
``crates/rengrave-ui/assets/tool-icons``. The circle, edit-parameters, and reset
assets are parsed by the portable core SVG parser, cached in ``OnceLock``
values, and projected into the egui button painter as line segments. This keeps
SVG as the source of truth without parsing on each frame or adding a separate
rasterization pipeline. Each icon-only button supplies an accessible label and
matching hover text. Edit-category buttons are rendered through a disabled UI
scope whenever no stable object ID is selected.

The circle create and edit flows replace the category list with the same
reusable temporary settings shell. The shell owns the common title, contained
content column, equal-width confirmation and **Cancel** actions, and ``Escape``
cancellation. ``CircleToolSession`` combines a ``CircleDraft`` with either
``Create`` or ``Edit(DesignObjectId)`` mode. Mode selects the title and the
**Create** or **Update** confirmation label. Draft changes feed the same scene
path as committed objects, so both viewports update in the same frame. During
editing, the committed target is suppressed from rendering and replaced by the
draft preview; cancellation reveals the unchanged committed object again.

Authored General-workbench geometry begins in
``rengrave-core::design::VectorDocument`` rather than ``PreviewSegment`` or
legacy ``EngraveSegment`` output. ``DesignObjectId`` provides stable identity,
and ``DesignGeometry::Circle`` stores a center and radius in canonical
millimetres. Core validation rejects non-finite centers and non-positive
radii. Core hit testing measures the shortest distance from the pointer to each
vector path and selects the nearest path within the supplied tolerance. For a
circle this is ``abs(distance(pointer, center) - radius)``. Newest-first visual
stacking breaks exact distance ties. ``remove_object`` deletes by stable
identity without reusing IDs. ``update_circle`` validates replacement
parameters and updates geometry in place, preserving object identity and
ordering. This remains a focused first model slice and is not yet attached to
CAM operations.

The 2D renderer converts authored millimetres at the UI unit boundary, projects
circle centers through the existing ``ViewTransform``, and derives screen
radii from the same zoom. Per-object interaction regions expose accessible
``Circle N`` labels. A primary click converts the pointer into canonical model
units and asks the core for the nearest path. The tolerance is derived from a
six-pixel screen-space buffer divided by the current zoom, so selection remains
usable without becoming less precise when zoomed in. Selection has no click
history or cycling state. Blank primary clicks clear selection and ``Delete``
removes the selected object only while the 2D editor is active and no temporary
settings tool is open. A double-click resolves the same path hit and starts an
edit session for that stable ID. The General 3D scene receives matching
``DesignCircle`` objects and draws CPU-side 72-segment outlines on the stock
surface after opaque stock faces. Draft objects use a preview color. The 3D
path remains display-only and has no selection handlers.

The UI copies ``VectorDocument`` into ``RengraveProjectFile`` during save and
restores it before rendering a loaded General Purpose project. Selection and
temporary tool state are deliberately cleared on load and new-project reset.
The project field is additive and defaults empty, so the existing format
version remains compatible with older ``.rgrv`` files. An **Update** action
only mutates the in-memory document and does not call the project writer;
manual **Save** or **Save As** serializes the updated parameters later.

.. code-block:: text

   controls / project / file browser
                    |
                    v
              BatchRequest
                    |
              background thread
                    |
       progress + BatchOutput messages
                    |
                    v
   G-code parser -> preview layers -> canvas + status
                    |
                    +--> save primary and companion G-code
                    +--> save SVG / DXF
                    `--> persist matching project cache

The preview parses generated G-code into distinct toolpath, rapid, cleanup,
and tab layers. It reports extents, lengths, move counts, arcs, warnings, and
the current model-space cursor position.

CLI lifecycle
-------------

``rengrave-cli/src/main.rs`` parses options into a ``BatchRequest``. With
``--batch`` it writes primary G-code (or stdout), companion outputs, and any
requested SVG/DXF files. Without batch mode it calls ``rengrave-ui::run``.
``--agent-debug-dir`` runs the same core calculation and writes a manifest plus
deterministic G-code, SVG, DXF, and secondary-output artifacts for inspection.

Compatibility and extension rules
----------------------------------

``f-engrave_source/`` is the upstream behavior and licensing reference and is
not part of the Rust dependency graph. Legacy settings and generated G-code
are compatibility boundaries: changes to geometry, ordering, depth, units, or
formatting should be validated against focused tests and golden fixtures.

New machining behavior normally belongs in ``rengrave-core`` first, with the
UI exposing controls only after the core settings and pipeline behavior exist.
The CLI should continue to call the shared batch seam rather than duplicating
toolpath logic.

Validation map
--------------

* Core unit tests sit beside the relevant module; cross-format and output
  comparisons are under ``crates/rengrave-core/tests``.
* UI behavior and preview parsing are tested in ``rengrave-ui/src/lib.rs`` with
  egui harness tests where rendering or interaction is part of the contract.
* CLI behavior is exercised through the shared core request and artifact
  writers.

Useful focused commands are ``cargo test -p rengrave-core``,
``cargo test -p rengrave-ui --lib``, and
``cargo run -p rengrave-cli -- -b -t "Text"``. These validate source-level
behavior; they do not by themselves prove physical CNC motion or native GPU
behavior.
