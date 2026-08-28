General pocket toolpaths
========================

The General Purpose workbench can generate a pocket toolpath inside a selected
closed vector. The cutter follows the compensated interior and removes the
area in deterministic passes. The current vector editor supplies circles; the
operation is designed to accept additional closed-vector generators as they
are added.

Create a pocket
---------------

1. Create or select a closed vector in the **2D View**.
2. In the **Toolpath Panel**, select the square **Create Pocket Toolpath**
   button.
3. Choose an Endmill, Ballnose, or Bullnose toolbit.
4. Select a path pattern, set the cutter parameters, and choose **Apply**.
   The editor remains open for quick setting changes; choose **Close** when
   finished.

The generated pocket appears in the 2D and 3D previews and is added to the
Toolpaths list. A pocket can be generated alongside profile operations on the
same vector.

Pocket controls
---------------

**Path pattern**
   Select ZigZag, Offset, ZigZag + Offset, Grid, or Line. Offset patterns use
   concentric compensated passes; scan patterns use clipped passes across the
   compensated circle. The patterns share the same deterministic depth-pass
   ordering.

**Cut mode**
   Select Climb or Conventional. This controls the direction of closed pocket
   rings. It does not change the selected vector or cutter compensation.

**Step over**
   Sets the distance between adjacent pocket passes. Smaller values leave less
   uncut material between passes but create more motion.

**Cut depth** and **Step down**
   Set the final depth below the material surface and the maximum material
   removed per pass. The final pass is shortened to finish exactly at the
   requested depth.

**Start height**
   Sets the configured starting Z reference for the operation. **Safe height**
   remains the rapid-clearance height above the selected Z-zero surface.

**Feed rate** and **Plunge rate**
   Override the selected toolbit's general defaults for this operation. The
   values are retained with the generated toolpath and are used when that
   toolpath is edited.

**Rest machining**
   Marks the operation as reserved for stock left by previous toolpaths. The
   flag is included in the operation and G-code metadata; stock-aware rest
   removal requires the future simulation/stock model.

Safety checks
-------------

Pocketing requires a closed vector and a valid profile-capable toolbit. The
tool radius is subtracted from the vector's interior before paths are created.
If the cutter cannot fit inside the vector, generation is rejected with a
visible warning and no G-code is emitted. Cut depth cannot exceed the
toolbit's cutting-edge height, and feed, plunge, step-down, step-over, and safe
height must be positive.

Generated pocket operations appear in the Toolpaths list with their source
vector. Click a row to select its vector, double-click to edit it, and use the
visibility checkbox to show or hide it in both previews.
