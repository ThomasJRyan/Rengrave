User workflows
==============

Inputs and workbenches
----------------------

The selected workbench determines which input catalog entries are valid:

=================  ====================  =====================================
Workbench          Input                 Primary operation
=================  ====================  =====================================
Text Engrave       CXF, TTF              constant-depth stroke engraving
Text V-carve       CXF, TTF              variable-depth text V-carve
Text Inlay         CXF, TTF              V-carve-style inlay geometry
Image Engrave      DXF, SVG, bitmap      constant-depth vector engraving
Image V-carve      DXF, SVG, bitmap      variable-depth vector V-carve
Image Inlay        DXF, SVG, bitmap      V-carve-style inlay geometry
=================  ====================  =====================================

Text inputs are converted to stroke segments. DXF and SVG inputs are parsed as
vector geometry. Bitmaps are thresholded, alpha-composited over white, traced
to vector paths, and then sent through the same downstream geometry path as
other image inputs.

The **File > New project** picker presents the six machining workbenches as
square icons rather than full-width text buttons, along with the **General
Purpose** layout-only workbench. Hover a machining icon to reveal the exact
workbench name; the icon remains clickable and the text and image groups keep
the same ordering as the table above.

General Purpose workbench
-------------------------

The General Purpose workbench is a layout foundation for the future shared
workbench. It contains three horizontal panels: an approximately 256-pixel
Tool Panel, a flexible Editor/Preview Panel, and a 15%-wide Toolpath Panel.
The Tool Panel has three switchable vertical tabs labelled **Job Setup**,
**Design**, and **Tab 3**. The Editor/Preview Panel has **2D View** and
**3D View** tabs. The **2D View** now shows a simple grid and
uses the same navigation controls as the existing preview for panning and
zooming: secondary drag pans the view, and the mouse wheel zooms around the
cursor. The white canvas rectangle is centered in the viewport and its
width and height follow the **Width (X)** and **Height (Y)** Job Size values.
The 2D View also includes top and left rulers. Their zero point is the center
of the canvas, and pointer crosshair lines track the cursor through the ruler
and canvas area. Rulers show integer coordinates at normal zoom levels and
use up to two decimal places when zoomed in closely.
The vertical ruler uses the workbench's screen-oriented convention: values
above the centered zero are negative, while values below zero are positive.
The **Centre Job (F7)** button, or the ``F7`` hotkey, restores a centered view
of the current job with a comfortable responsive margin; the same fitting is
applied automatically the first time the 2D View opens.

The **3D View** presents the current job stock as a read-only shaded solid.
It is generated from the same job dimensions as the 2D canvas, so changes to
width, height, or thickness appear automatically. Drag with the left or
middle mouse button to orbit, drag with the right mouse button to pan, and
use the mouse wheel to zoom. The General 3D View intentionally has no grid or
rulers. A labelled orientation gizmo in the upper-right corner follows the
camera and identifies the red X, green Y, and blue Z axes. Stock faces are
opaque; rear-facing and edge-on faces are omitted to keep direct elevation
views clean. Zoomed geometry is clipped to the Editor/Preview Panel.

The **Job Setup** tab currently presents controls for job size and units, Z
zero position, and XY datum offset. Job Type and Modeling Resolution are
temporarily hidden. Job dimensions and datum offsets are stored internally in
millimetres. The visible controls display those values in the selected
**inches** or **mm** unit, and changing units preserves the physical job
dimensions while updating the canvas and ruler labels. Dragging a numeric field
uses 0.01-inch increments in inches mode and 0.1-millimetre increments in mm
mode. When a numeric value differs from its default, a square reset button
appears immediately to the left of the field. Select it to restore that field;
the button hides again and both General Purpose views reflect the restored
geometry immediately. Its reserved slot keeps the input fixed in place, so
the button can appear or disappear during a numeric drag without interrupting
the gesture. These controls are UI
scaffolding only and do not yet change machining geometry, calculations, or
machine output. The setup form stays within the Tool Panel: fields use a
compact aligned column, groups fill the panel width, and horizontal rows
distribute their controls across the available group width. The XY datum
illustration stacks above its controls when the panel is
narrow.

The **Design** tab organizes future editing controls into **Create Vectors**,
**Transform Objects**, and **Edit Objects** categories. Each category uses the
same compact five-column row of square tool buttons. **Create Vectors**
currently contains a circle-icon button labelled **Create Circle** when
hovered or read by assistive technology. **Edit Objects** contains an **Edit
Vector Parameters** pencil button. Edit Object controls are disabled until a
vector is selected. **Transform Objects** remains empty until its tools are
defined. Category frames conform to the Tool Panel's available content width
and do not extend into the Editor/Preview Panel. All sub-panels in both Design
and Job Setup use one uniform width.

Selecting **Create Circle** temporarily replaces the Design categories with a
contained circle settings view. A live circle preview starts at the center of
the job. **Center Point** edits its X and Y position, while **Radius** and
**Diameter** choose how the size field is displayed; changing that choice does
not change the physical circle. Values follow the Job Setup unit selection but
remain stored in millimetres. **Create** commits the circle and **Cancel** or
``Escape`` discards the preview. Either action returns to the Design
categories. Circle position and size fields use the same reset control. During
creation they return to the centered 20 mm diameter default; during editing
they return to the values present when the edit view was opened.

Committed circles appear automatically in both the 2D and read-only 3D views.
Click a circle with the left mouse button in the 2D View to select it; the
selected outline and center mark use the selection accent. Selection is
reserved for later CAM-operation assignment and does not alter geometry. When
circles overlap, selection uses the distance from the pointer to each vector
outline. Click near the path you want; a small screen-space buffer makes the
outline easier to target without requiring pixel-perfect input. Repeated clicks
on the same path keep selecting that path and do not cycle through enclosing
objects. Press ``Delete`` to remove the selected circle. Blank clicks clear
selection.

With a circle selected, choose **Edit Vector Parameters** or double-click the
circle path to reopen its parameter view. The current center and size populate
the controls, and both 2D and 3D views preview changes immediately. **Update**
applies the new values to the existing vector without changing its identity.
**Cancel** or ``Escape`` discards the draft values. Updating a vector does not
save the project automatically; use **File > Save** or **File > Save As** when
you want to write the changes to the ``.rgrv`` file.

The 3D View displays circles on the job surface but does not provide selection
or editing. Saving a General Purpose project to an ``.rgrv`` file stores its
created vectors in canonical millimetres. Opening that project restores the
circles and their stable object identities; selection and an unfinished circle
preview are temporary UI state and are not saved.

Layout and coordinate choices
-----------------------------

The layout stage scales and places the source before any toolpath-specific
operation. The important controls are:

* **YSCALE** controls text height. **XSCALE** is a percentage relative to the
  vertical scale.
* **LSPACE**, **CSPACE**, and **WSPACE** control line, character, and word
  spacing.
* **Justify** changes the horizontal alignment of each line.
* **TANGLE**, **flip**, and **mirror** transform the source geometry.
* **TRADIUS**, **outer**, and **upper** bend text onto a circle.
* **origin**, **xorigin**, and **yorigin** select and offset the job coordinate
  reference.
* A profile cut expands the reference envelope by margin, cutter radius, and
  optional chamfer width before the origin is resolved. This keeps a top-left
  or other explicit origin aligned to the complete job, not only the text.

|coordinates|

The coordinate convention is mathematical: X increases to the right and Y
increases upward in model space. SVG output inverts Y while writing screen-like
coordinates so the exported visual has the expected top-to-bottom orientation.

V-carve settings
----------------

The V-carve controls are derived from the legacy keys:

======================  ================================================
Control                 Effect
======================  ================================================
Bit shape               V-bit, ball, or flat cutter model
V-bit angle             Included angle used by the V-bit radius/depth model
V-bit diameter          Maximum cutter diameter for the effective envelope
Step length             Sampling distance along source segments
Depth limit             Optional negative Z limit
Allowance               Inlay allowance and effective tool envelope input
Drive corner angle      Threshold for a corner that should be driven directly
Step corner angle       Threshold for intermediate corner samples
Check scope             Test against all geometry or the current character
Finish stock            Roughing stock left for the final path
Max depth/pass          Per-pass roughing cap when multipass is active
======================  ================================================

Changing the V-bit angle changes the depth (the emitted ``Z`` coordinates) of
the V-carve. The XY centerline normally remains the same because the V-carve
solver follows the widest circle that fits at each source position. An XY-only
preview or comparison of XY coordinates therefore does not show the angle
change; inspect the generated G-code's Z values to compare the cuts.

At every sampled outline position, R-Engrave searches for the largest circle
that can fit without crossing the relevant boundary. The circle radius becomes
the cutter contact width and is converted to a Z value. The result is then
simplified in 3D before G-code emission.

Preview and inspection
----------------------

The central preview is a 3D toolpath view. Cutting, rapid, cleanup, and tab
moves are projected from their X/Y/Z coordinates, with the Z axis shown in
magenta so plunges, retracts, V-carve depth, and profile-tab heights are
visible. Use the mouse wheel to zoom, drag with the left or middle button to
orbit, and drag with the right button to pan. This matches the common CAD
navigation convention used by the 3D preview; the same mapping is shown in the
preview canvas. The **Reset view**
control in the upper-right returns to a centred top view. Orbit pitch moves only
from the top view through a direct side view using the -90° direction, while
horizontal orbit remains continuous; the workpiece cannot flip upside down. The upper-left readout
reports the X, Y, and Z extents of the visible toolpath. **Fit preview** also
re-centres the complete toolpath.

The 3D view is a toolpath inspection aid, not a stock-removal simulation. It
shows commanded motion and depth, but it does not model cutter diameter,
material removal, acceleration, or machine kinematics.

Cleanup operations
------------------

Cleanup is emitted as secondary G-code. It is selected independently for a
straight bit and a V-bit. Each bit can request a profile pass, X scanlines, Y
scanlines, and repeated loop offsets. For straight-bit cleanup, enter one or
more comma-separated diameters in **Clean diameters**, largest first, such as
``6.35,3.175,1.5``. R-Engrave emits one cleanup file per diameter and assigns
only residual toolpaths to each smaller tool, so a smaller endmill does not
retrace material already covered by a larger one. **Clean dia** remains the
fallback when the list is empty or invalid.

The preview identifies companion paths by operation: **Cleanup**, **Profile**,
and **Profile chamfer**. Use **Show cleanup and profile paths** to toggle these
secondary operations together; profile tabs remain a separate layer.

Cleanup files are standalone operations: they write the configured feed rate
before the first plunge and use the configured plunge rate when it differs.
They cut to the primary V-carve's calculated maximum depth, including any
roughing passes and inlay allowance, rather than the constant-depth engraving
``Cut Z`` value.

The straight-bit area is an offset of the source region by the sum of the
cleanup radius and the primary cutter radius. V-bit cleanup constructs two
offset regions and takes their difference to retain the area that the V-bit
cannot reach at full depth.

|cleanup|

For scanline cleanup, the spacing is:

.. math::

   s = d \times p

where ``d`` is the cleanup tool diameter and ``p`` is the step-over fraction.
Horizontal and vertical intersections are paired using an even-odd rule, then
ordered by nearest endpoint to reduce unnecessary rapids.

Profile cuts, chamfers, and tabs
--------------------------------

Enable **Profile** when the job should cut around its outside envelope. The
profile path is built from the text/vector bounds, expanded by:

.. math::

   e = m + r_c + w_{chamfer}

where ``m`` is profile margin, ``r_c`` is the endmill radius, and ``w_chamfer``
is zero unless a V-bit chamfer is enabled. A profile can specify width, height,
aspect ratio, alignment, corner radius, depth passes, and tabs.

Tabs are not vertical teleports. The emitted path ramps from cutting depth to
tab depth and back at 45 degrees, capped at half the tab width. This preserves
continuous XY motion while reducing the chance of marking the stock with a
sharp Z transition.

|profile|

Output and return-to-origin
---------------------------

G-code starts with absolute positioning, the selected ``G20`` or ``G21`` unit
command, the configured preamble, and a feed rate. Each path rapids to safe Z,
rapids to its first point, plunges, and emits cutting moves. Independent
V-carve paths are automatically rearranged when that reduces the connecting
rapid distance; each path keeps its generated direction. This does not change
the cut depths or geometry. Arc fitting can emit linearized paths, center-offset
``I/J`` arcs, or radius-format ``R`` arcs.

The shared trailer retracts to safe Z, writes the postamble, and, when
``return_to_origin`` is enabled, emits ``G0 X0 Y0``. The default is enabled in
the current settings table.
