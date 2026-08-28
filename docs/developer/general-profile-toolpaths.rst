General profile toolpath architecture
=====================================

The first native General CAM operation preserves the workbench's four-stage
boundary:

.. code-block:: text

   VectorDocument / DesignGeometry
        | profile_exteriors()
        v
   ProfileBoundary (closed exterior only)
        | cutter compensation + explicit flattening
        v
   ProfileToolpathContour (derived XYZ passes)
        | deterministic metric writer
        v
   internal G-code buffer + 2D/3D views

``DesignGeometry::profile_exteriors`` is deliberately not a stroke iterator.
Future SVG and compound vector adapters must return closed exterior
silhouettes appropriate for profiling, excluding open marks and interior
decorative strokes that belong to a future engraving operation. Authored
curves must remain authoritative; polygon flattening occurs only at this CAM
boundary.

Core model and compensation
---------------------------

``rengrave-core::general_profile`` owns ``GeneralProfileOperation``,
``ProfileParameters``, exact ``ProfileBoundary`` variants, derived XYZ
contours, validation, and G-code generation. The operation references a stable
``DesignObjectId`` and contains a frozen ``GeneralToolbit`` snapshot.
``ProfileParameters::feed_mm_min`` and ``plunge_mm_min`` are operation-local
values. The UI initializes them from the selected snapshot's library defaults,
then passes edited values to the writer. The tool snapshot remains useful for
identity, cutter geometry, and spindle direction without making library rate
edits retroactively alter an existing operation.
When a profile is generated, the selected toolbit's feed and plunge defaults
are updated from the operation values and saved through the toolbit library's
atomic JSON persistence. This makes a deliberate per-toolpath override the
remembered default for subsequent operations using that toolbit.

Toolpath records and visibility
--------------------------------

The UI stores each successful generation as a ``GeneralToolpathRecord`` with a
stable session-local identifier, the frozen ``GeneralProfileOperation``, its
derived contours, G-code buffer, and an ``enabled`` flag. New generations
append records; editing a row updates that record by identifier. Rendering
iterates enabled records in insertion order, so multiple operations may target
one source vector without overwriting one another. A row click updates the
selected source object, while a double-click creates a profile edit session
for the same record.

The generator uses ``general_toolpath_setup_group`` so its frame width is
derived from the available Toolpath Panel width rather than the narrower
design-panel maximum. Profile dimension and rate rows justify their controls
to the right and allocate value editors at 60 percent of the row's remaining
width. The generated-list width is captured before entering its scroll area;
rows reserve space for the visibility toggle and cap the clickable label so
long labels cannot expand past the panel edge.
Profile editor rows use a shared fixed label-column token and a common value
width calculation, with right-to-left value placement for a stable comparison
column. Toolpath visibility checkboxes expose semantic ``Show toolpath <id>``
labels while rendering without a text caption, preserving the available row
width.

For tool radius :math:`r` and additional offset :math:`a`, the signed exterior
offset is:

.. math::

   d = \begin{cases}
       +(r + a) & \text{outside} \\
       a        & \text{on line} \\
       -(r + a) & \text{inside}
       \end{cases}

Circles retain an exact center and radius through compensation and are
flattened to a deterministic 128-segment preview contour. G-code emits an
exact closed ``G2`` move. ``ClosedContour`` provides the extension point for
future polygonal exteriors and uses Clipper2 round-join polygon inflation.
Contours are normalized to positive winding before offsetting. Collapsed
inside paths, non-finite parameters, invalid depths, and unsupported cutters
return typed generation errors.

Depth and coordinate invariants
-------------------------------

Cut and pass depths are positive physical distances below the stock surface.
The intermediate preview stores them as negative Z values relative to the
surface. Pass depths are generated in stable order as
``min(pass_number * pass_depth, cut_depth)`` so the last pass lands exactly on
the requested depth.

The G-code writer uses millimetres and absolute positioning. With material
surface Z zero, the surface is ``Z0``. With machine-bed Z zero, the surface is
the configured stock thickness. The safe rapid plane is the surface plus safe
height, while each cut plane is the surface minus its physical pass depth.
Design X/Y coordinates are emitted unchanged. Only the UI render boundary
inverts Y to preserve the General workbench's negative-above screen
convention.

UI ownership and invalidation
-----------------------------

``RengraveApp`` owns a temporary ``ProfileToolSession``, the current generated
operation, derived preview contours, and the internal G-code string. The
Toolpath Panel replaces its tool list with a contained editor until Apply,
Close, or Escape. Apply commits the generated operation while retaining the
session, so repeated adjustments update the same record. Close exits the
session without changing the last applied output. The square SVG button and every parameter control expose
semantic labels for EGUI harness inspection.

Tool selection and generation readiness are separate checks. The picker lists
every Endmill, Ballnose, or Bullnose definition so a saved library entry is
never misreported as absent. The editor then validates required dimensions,
positive feed and plunge rates, and cutting-edge depth. It keeps **Apply**
disabled and exposes the first actionable correction while the definition is
incomplete.

Editing the source vector regenerates its active profile. Deleting that source
clears the operation, derived contours, and G-code buffer. Starting or opening
a project also clears temporary/generated profile state; project persistence
and file export for native General operations are intentionally separate
future work.

The 2D renderer converts each derived XY point through the same unit and
screen-Y boundary as design geometry. ``GeneralSceneObject::ProfileToolpath``
converts stored Y to scene Y and carries physical negative Z into the 3D
renderer. Both views therefore consume the intermediate contour rather than
re-parsing generated G-code.
