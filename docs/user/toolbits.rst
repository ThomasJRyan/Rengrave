General toolbit library
=======================

The **General > Settings > Toolbit Library...** window manages the cutters
available to the General workbench. It is separate from the legacy machining
workbench's operation selectors. The library is intended to supply tool
definitions to General-workbench CAM operations as those operations are added;
opening the manager does not change the current G-code operation settings.

The manager uses a two-pane layout:

* the left pane searches and selects saved toolbits, creates tools, duplicates
  or deletes them, and imports or exports JSON libraries;
* the right pane edits identity, spindle, material, geometry, feeds, and
  shape-specific parameters, with a bundled FreeCAD reference drawing.

Use **New from preset** to start from the common FreeCAD-style catalog:
endmills, ballnose and bullnose cutters, V-bits, chamfers, drills, slitting
saws, and probes. **New** creates a blank definition. A toolbit is marked
**Needs attention** until its required dimensions and shape parameters are
valid. Delete requires confirmation.

Units and persistence
---------------------

Dimensions are stored canonically in millimetres. The editor can display
dimensions in metric or imperial units; feed and plunge remain expressed as
millimetres per minute. Changes are saved automatically to
``general_toolbits.json`` in R-Engrave's platform configuration directory:
``$XDG_CONFIG_HOME`` or ``~/.config`` on Linux, Application Support on macOS,
and ``%APPDATA%`` on Windows.

Use **Export JSON** to create a portable copy and **Import JSON** to merge a
copy into the current General library. Imported IDs are made unique when
needed, so importing a library cannot silently replace an existing toolbit.
The JSON format is R-Engrave's interchange format; FreeCAD ``.fctb`` and
``.fctl`` files are not imported.
