User reference
==============

Command-line interface
----------------------

``rengrave`` launches the desktop UI unless ``--batch`` or
``--agent-debug-dir`` is supplied.

.. list-table:: CLI options
   :header-rows: 1
   :widths: 22 78

   * - Option
     - Meaning
   * - ``-b, --batch``
     - Run without the GUI. Write G-code to stdout unless ``--output`` is set.
   * - ``-g, --gcode_file PATH``
     - Read a legacy F-Engrave G-code/settings file, including its
       ``(fengrave_set key value )`` comments.
   * - ``-f, --fontdir PATH``
     - Select a CXF/TTF font, a font directory, or an image/vector input.
   * - ``-d, --defdir PATH``
     - Set the default directory used for input recovery and output names.
   * - ``-t, --text TEXT``
     - Override the engraving text. Use ``|`` for line breaks in a shell
       argument.
   * - ``-o, --output PATH``
     - Write primary G-code to this path instead of stdout.
   * - ``--svg-output PATH``
     - Write the SVG representation of the primary layout.
   * - ``--dxf-output PATH``
     - Write a DXF representation of the primary layout.
   * - ``--agent-debug-dir PATH``
     - Run calculation/export automation and write a manifest plus all
       available artifacts into the directory.

Example:

::

   cargo run -p rengrave-cli -- \
     --batch \
     -f assets/fonts/rengrave_demo.cxf \
     -t "Top line|Bottom line" \
     -o /tmp/job.ngc \
     --svg-output /tmp/job.svg \
     --dxf-output /tmp/job.dxf

Desktop panels
--------------

The desktop UI exposes the same core settings through compact panels:

* **Input** selects a font/image, filters the catalog, refreshes the input
  preview, and reports missing glyphs or bitmap mask statistics.
* **Geometry** contains units, scales, spacing, text circle, origin,
  justification, flip/mirror, box, and image-size behavior.
* **Tool / Cut** contains cut type, bit shape, depth, feed, plunge, accuracy,
  V-carve controls, inlay, multipass, cleanup, and profile operations.
* **Preview** controls cut, rapid, cleanup, tabs, bounds, axes, grid, and
  source-overlay layers. It also reports extents, lengths, move counts, and
  model-space cursor coordinates.
* **Export** chooses G-code, SVG, and DXF paths; saves projects/settings;
  exports all available artifacts; and shows preflight cautions.
* **Output** exposes status, G-code, cleanup G-code, SVG, and DXF tabs with
  copy actions.

Output state is explicit: *none*, *calculating*, *stale*, or *ready*. A stale
state names the changed area when possible. Copying or saving stale output is
allowed by the current UI but is called out as a preflight caution; recalculate
before machining.

Settings and project files
--------------------------

Legacy settings are recovered from comments such as:

::

   (fengrave_set units mm )
   (fengrave_set cut_type v-carve )
   (fengrave_set TCODE R-Engrave )

R-Engrave also supports the ``.rgrv`` project format. It is a versioned JSON
file containing application version, text, legacy settings, input path,
default directory, optional legacy settings path, selected workbench, and
primary export paths. The current project format version is ``1``.

For a portable project, keep referenced input files beside the project or use
stable absolute paths. A project stores paths; it does not embed fonts, images,
or generated G-code.

Units and precision
-------------------

The core keeps model geometry in the active job units. G-code coordinates use
four decimal places for inches and three decimal places for millimetres; feed
values use two decimal places for inches and one for millimetres. ``accuracy``
controls geometric path joining and simplification, not the machine's physical
repeatability.

Safety checklist
----------------

Before running any exported file:

* confirm the controller's units, work coordinate system, tool, stock, and
  Z-zero convention;
* inspect safe Z, cutting depth, feed, plunge, and any multipass depth caps;
* simulate or air-cut the job, including the profile and cleanup companion
  files;
* verify that a return-to-origin move is safe for the machine's travel limits;
* keep an emergency stop accessible and do not rely on preview colors as a
  substitute for controller verification.
