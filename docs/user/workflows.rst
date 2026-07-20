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

The **File > New project** picker presents these six workbenches as square
icons rather than full-width text buttons. Hover an icon to reveal the exact
workbench name; the icon remains clickable and the text and image groups keep
the same ordering as the table above.

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
