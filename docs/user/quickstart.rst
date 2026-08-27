Quick start
===========

.. |workflow| image:: ../_images/workflow.svg
   :alt: R-Engrave input, calculation, preview, and export workflow
   :width: 100%
.. |ui_placeholder| image:: ../_images/ui-overview-placeholder.svg
   :alt: Placeholder for a native R-Engrave desktop screenshot
   :width: 100%

The safest first job is a small text engraving on scrap stock. Confirm the
preview, inspect the generated file, and simulate or dry-run the G-code in the
controller that will run it before cutting material.

The workbench flow
------------------

Startup screen
~~~~~~~~~~~~~~

When R-Engrave is opened without a command-line input, it starts on the
project screen rather than opening a machining workbench. The screen contains
the top navigation bar, a project-action pane, and the R-Engrave logo. No tool
settings, input catalogs, or toolpath controls are shown until a project is
created or opened.

Choose one of the following actions:

* **New Project** opens the workbench picker. Select General Purpose, Text
  Engrave, Text V-carve, Text Inlay, Image Engrave, Image V-carve, or Image
  Inlay. General Purpose currently provides the shared three-panel layout;
  it does not create or configure tools.
* **Open Project** opens an existing ``.rgrv`` project file and then enters
  its saved workbench.
* **Recent Project** shows successfully opened or saved projects remembered
  by R-Engrave. Unavailable files remain visible but disabled so their paths
  can be located again with **Open Project**.

Recent projects are stored in the per-user R-Engrave UI preferences, not in
the project file. At most ten entries are retained, with the newest entry
first. Choosing **Clear Recent Projects** removes the list.

The normal workbench flow is:

|workflow|

The normal loop is:

#. Choose a workbench: Text Engrave, Text V-carve, Text Inlay, Image Engrave,
   Image V-carve, or Image Inlay.
#. Select a font or image input. CXF and TTF are text inputs; DXF, SVG, and
   bitmap files are image/vector inputs.
#. Set units, origin, size, cutter, depth, feed, and any cleanup or profile
   operation.
#. Press **Calculate** or allow the debounced automatic calculation to run.
#. Review the preview layers, bounds, model coordinates, warnings, and output
   statistics.
#. Export G-code and, when useful, SVG, DXF, or secondary cleanup G-code.
#. Verify the exported file in a simulator and on the machine's controller.

Choosing a new project
----------------------

Choose **File > New project** to open the workbench picker. Each workbench is
represented by a square icon grouped under text or image generation. Hover an
icon to see its workbench name, then click the icon to start that project type.
The picker also includes the layout-only General Purpose workbench. The six
machining choices are Text Engrave, Text V-carve, Text Inlay, Image Engrave,
Image V-carve, and Image Inlay.

Launching the desktop application
----------------------------------

From the repository root:

::

   cargo run -p rengrave-ui

The CLI also launches the same UI when no batch flag is supplied:

::

   cargo run -p rengrave-cli -- \
     -f assets/fonts/rengrave_demo.cxf \
     -t "R-Engrave"

The default window is sized for a 1280 by 800 desktop workbench. The central
preview is the visual source of truth for the current generated toolpath. The
left side is organized around input and geometry, the right side around tool,
preview, and export controls. The top status row reports the current workflow
state and, after calculation completes, the elapsed G-code generation time.
The bottom area reports output and machine-facing status.

Native UI screenshot placeholder
--------------------------------

The application could not be captured in the current build environment because
the host has no compositor. Replace the placeholder below with a screenshot at
1280 x 800 or wider showing the input catalog, central preview, right-hand
controls, and bottom output/status strip. The screenshot should show a simple
text job with cut, rapid, bounds, axes, and grid layers visible.

|ui_placeholder|

.. note::

   Screenshot placeholder only. It is intentionally not presented as a live
   application capture. The deterministic CLI/SVG path is available through
   ``--agent-debug-dir`` and is suitable for automated visual review.

First text engraving
--------------------

#. Choose **Text Engrave**.
#. Select a CXF or TTF font in the input catalog, or browse to one directly.
#. Enter the text. Use the UI's multiline editor for line breaks.
#. In **Geometry**, choose the unit system, height, horizontal scale,
   justification, rotation, origin, and optional box.
#. In **Tool / Cut**, set the cutting depth, safe height, feed, plunge feed,
   and stroke thickness.
#. Calculate and inspect the generated paths. Green cut moves should remain
   inside the expected bounds; amber travel moves should be at safe Z.
#. Export the G-code only after the output status reads ready and no warning
   describes stale controls or missing input.

First V-carve
-------------

V-carving changes the geometry contract: the tool follows a center-drive path,
and its Z depth varies with the maximum cutter radius that fits at each sample.
Start with a closed glyph or vector region, a known V-bit angle, a conservative
step length, and a shallow depth limit. The **Max depth/pass** controls are
needed when rough stock and multipass V-carving are enabled.

First bitmap job
----------------

#. Choose an Image Engrave, Image V-carve, or Image Inlay workbench.
#. Browse to a bitmap and inspect both the original thumbnail and the trace
   mask in **Input preview**.
#. Check the black-pixel count and coverage percentage. A nearly empty or
   nearly full mask usually means the threshold, alpha treatment, or source
   image needs attention.
#. Run the calculation and check the source overlay against the generated
   path. Bitmap tracing uses the native Rust Potrace-style implementation.

Batch smoke test
----------------

The following command exercises calculation and all three primary export
formats without opening a window:

::

   cargo run -p rengrave-cli -- \
     --agent-debug-dir /tmp/rengrave-debug \
     -f assets/fonts/rengrave_demo.cxf \
     -t "R-Engrave"

The directory contains ``debug.json``, ``output.ngc``, ``output.svg``,
``output.dxf``, and any requested secondary cleanup files. The manifest records
warnings, line counts, requested inputs, and the artifact paths.
