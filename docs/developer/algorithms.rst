Algorithms and mathematics
==========================

Coordinate spaces and layout
----------------------------

The input path begins in font or vector coordinates. Layout converts it into
real job units, after which toolpath generation operates in the same space.
The transform order is conceptually:

.. math::

   p_1 = (x \cdot s_x, y \cdot s_y)

   p_2 = R(\theta) p_1

   p_3 = (-p_{2x}, p_{2y}) \quad\text{if mirror}

   p_4 = (p_{2x}, -p_{2y}) \quad\text{if flip}

   p_{final} = p_4 - z + o

``s_x`` is derived from Y scale and the XSCALE percentage, ``R`` is a 2D
rotation, ``z`` is the selected origin point in the reference bounds, and
``o`` is the explicit X/Y origin offset.

Text layout derives line height from either the complete font or the glyphs
used by the current string. Character advance, character spacing, word
spacing, and line spacing are applied before per-line justification. Missing
characters are retained as a warning list rather than silently substituted.

|coordinates|

Profile-aware origin bounds are important. With a profile cut enabled, the
reference bounds expand by:

.. math::

   e = \max(m, 0) + \frac{d_e}{2} + w_c

where ``m`` is profile margin, ``d_e`` is endmill diameter, and the chamfer
width is:

.. math::

   w_c = h_c \tan\left(\frac{\alpha}{2}\right)

for chamfer depth ``h_c`` and included angle ``alpha``. An explicit profile
width, height, or aspect ratio replaces the corresponding expanded dimension;
alignment then chooses the profile rectangle's anchor.

V-carve maximum-circle walk
---------------------------

V-carve generation samples each source segment at approximately
``v_step_len`` spacing. A spatial partition grid limits which segments must be
tested for each sample. At an outline position ``q``, the solver searches for
the largest radius ``r`` such that a circle centered at ``q`` can be driven in
the selected direction without crossing the relevant geometry.

|vcarve|

For a V-bit with included angle ``beta``, the half-angle relationship between
cut width and depth is:

.. math::

   z(r) = -\frac{r}{\tan(\beta / 2)}

For an inlay, the inlay depth offset is added to the V-bit depth. Ball and flat
bits use different depth models: a ball cutter follows the circular segment of
its radius, while a flat bit clamps to half its diameter.

Corner handling is intentionally explicit. A corner below the drive threshold
gets a radius-zero drive point. A corner above the step threshold receives
intermediate angular samples. The angular increment is derived from step
length and maximum radius and is clamped to a minimum of two degrees. The
``v_check_all`` mode controls whether the partition query checks all geometry
or the current character/loop.

The generated points are reordered by loop and simplified during G-code
emission. V-carve simplification uses a three-dimensional Douglas-Peucker pass
over X/Y/Z samples so a change in depth cannot be simplified away as if it were
only a planar deviation.

Roughing and multipass
~~~~~~~~~~~~~~~~~~~~~~

For a point radius ``r``, final depth is ``z(r)``. If rough stock ``s`` is
positive, a rough pass is capped at ``z(r) + s`` until the final pass. If
``max_cut`` is configured, successive caps are emitted until the rough target
is reached. This separates stock removal from the final cutter-contact path.

Cleanup offsets and scanlines
-----------------------------

Cleanup first collects closed segment loops. Open paths are intentionally
ignored because they do not delimit an area. Clipper2 performs scaled integer
offset/boolean operations with a scale factor of 10,000; the result is reduced
back to floating-point model coordinates and simplified at the requested
accuracy.

For a straight cleanup bit, the region is offset by:

.. math::

   \Delta_s = -\left(\frac{d_s}{2} + r_v\right)

where ``d_s`` is the cleanup diameter and ``r_v`` is the effective radius of
the primary V-carve tool. V-bit cleanup builds an inner and full-depth reach
and takes their even-odd difference.

|cleanup|

Scanline spacing is ``s = d \times p``. A horizontal scanline intersects each
closed loop, sorts and deduplicates X intersections, pairs them into interior
spans, and trims each span by half the cleanup diameter. Vertical scanlines do
the same with Y. Nearest-endpoint ordering reduces travel without changing
cutting geometry. ``v_flop`` selects the opposite pairing parity for legacy
orientation behavior.

Profile paths, corners, and tabs
--------------------------------

The profile path surrounds the source or fitted profile envelope. Its straight
offset is endmill radius plus chamfer width. Rounded corners are sampled with
an angular step constrained by the requested accuracy and a four-to-sixty-four
step bound, preventing pathological point counts.

Depth passes are evenly spaced between zero and the negative target depth:

.. math::

   z_i = -\frac{|z_{target}|}{n} i, \qquad i = 1, \ldots, n

The final pass is forced to the exact target to avoid accumulated rounding.

|profile|

Tabs are intervals along perimeter distance. Let ``d`` be full cutting depth,
``t`` tab depth, and ``L`` the ramp run. With the current 45-degree policy:

.. math::

   L = \min\left(\frac{|t-d|}{\tan 45^\circ},
                 \frac{\text{tab width}}{2}\right)

Z is linearly interpolated through the entry ramp, held at tab depth, then
interpolated back to cutting depth. The path remains continuous in XY.

G-code and arc fitting
----------------------

Path segments are grouped by loop and joined when the gap is no greater than
``accuracy``. The writer emits ``G90``, ``G20`` or ``G21``, preamble, feed, and
per-path safe-Z/rapid/plunge sequences. ``PLUNGE = 0`` means use the cutting
feed for the plunge.

Arc fitting recognizes eligible line runs and emits either no arcs, center
offset arcs using ``I/J`` (with ``G91.1``), or radius-format arcs using ``R``.
Arc fitting changes serialization, not the source geometry. The shared trailer
retracts, emits the postamble, and optionally returns to ``X0 Y0``.

Performance notes for dense bitmap inputs
------------------------------------------

The V-carve solver is intentionally sequential across each ordered loop, but
each sampled point performs a spatial-grid query. This is the dominant cost for
large bitmap contours. The query keeps the hot path allocation-free, compares
squared center distances instead of taking a square root, caches each segment's
axis-aligned bounds, and stops once an exact zero-radius result is reached.
These changes preserve the candidate ordering and generated output while
reducing work substantially on dense image paths.

The UI input-outline overlay is a separate display concern. It is simplified
with a small model-space tolerance after the calculation returns; the
simplification affects only the pink inspection layer, never the primary or
secondary toolpaths. Keep this distinction explicit when optimizing preview
rendering: output geometry must remain source-faithful, while an inspection
overlay may be reduced to the screen's useful resolution.
