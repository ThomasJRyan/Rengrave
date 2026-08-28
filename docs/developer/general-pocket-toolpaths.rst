General pocket toolpath architecture
====================================

Pocketing follows the General workbench CAM boundary used by profile cuts:

.. code-block:: text

   VectorDocument / DesignGeometry
        | profile_exteriors()
        v
   closed ProfileBoundary
        | tool-radius compensation + pattern generation
        v
   ProfileToolpathContour depth passes
        | deterministic pocket G-code
        v
   internal buffer + 2D/3D preview

``rengrave_core::general_pocket`` owns ``GeneralPocketOperation``,
``PocketParameters``, path/cut-mode enums, validation, pattern generation, and
G-code emission. It consumes ``DesignGeometry::profile_exteriors`` rather than
iterating display strokes. That preserves the future distinction between
pocketing a closed silhouette and engraving every available line.

Compensation and fit
--------------------

For a circular boundary with radius :math:`R` and cutter diameter :math:`D`,
the reachable pocket radius is:

.. math::

   R_{usable} = R - \frac{D}{2}

Generation fails when ``R_usable`` is not positive. This check is performed
before any contour or G-code is returned, so an oversized tool cannot silently
cut outside the selected vector. Future polygonal adapters should apply an
equivalent inward offset and reject empty results.

Depths are generated in stable order as
``min(pass_number * step_down, cut_depth)``. Preview Z values are negative
distances below the surface. The writer emits absolute metric motion, safe-Z
rapids, plunge feed, cutting feed, and a final spindle stop/program end.

Patterns
--------

The current circle adapter provides concentric rings for ``Offset`` and
``ZigZagOffset``. ``ZigZag`` and ``Grid`` provide clipped horizontal spans,
and ``Line`` provides one representative clipped span. Closed rings are
sampled with 128 points for preview and linear G-code output; conventional
cut mode reverses their point order. The enum and boundary adapter are the
extension points for polygon clipping and richer pattern implementations.

Rest machining is intentionally metadata-only at this stage. Its boolean flag
is preserved in the operation and emitted as a G-code comment, but no claim is
made that stock remaining from another path has been calculated until the
General workbench has a material/removal model.

UI integration
--------------

The UI stores pocket and profile operations behind ``GeneralToolpathOperation``
in the shared generated-toolpath list. A pocket session is temporary until
Generate succeeds; the selected tool snapshot and operation parameters are
then stored on the record. This allows multiple operations on one source
vector, double-click editing, shared visibility controls, and the same preview
pipeline for both operation kinds. Pocket output also updates the General
internal G-code buffer.
