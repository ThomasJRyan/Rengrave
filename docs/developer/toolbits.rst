General toolbit schema
======================

``rengrave_core::general_toolbit`` owns the General-workbench toolbit model.
It is intentionally additive and does not replace ``rengrave_core::toolbit``
or the legacy machining-workbench project assignment bridge. The General
model uses a closed ``GeneralToolbitKind`` enum for the supported FreeCAD
reference catalog and stores dimensions in millimetres.

Each ``GeneralToolbit`` contains a stable ID, label, tool number, spindle
direction, material, common cutter geometry, feeds, and optional parameters
for V-bit angle/tip diameter, bullnose corner radius, drill point angle,
chamfer angle, and slitting-saw thickness. ``validate`` reports missing or
out-of-range values without preventing an in-progress editor from being
saved. Probe definitions may have zero flutes; other cutter kinds require at
least one flute.

``GeneralToolbitLibrary`` serializes as versioned JSON. The UI loads the
platform-local ``general_toolbits.json`` at startup, treats a missing file as
an empty library, and saves edits through a sibling temporary file followed by
a rename. Import merges entries and changes colliding IDs. Export writes the
same versioned structure, so a library can be transferred between R-Engrave
installations without depending on FreeCAD's native files.

The UI bundles eight attributed SVG diagrams from FreeCAD's CAM tool-shape
directory under ``crates/rengrave-ui/assets/toolbits/freecad``. They are parsed
through the existing SVG stroke parser and scaled into the reference pane;
the diagrams are explanatory only and are not used as machining geometry.
The floating manager must be rendered from the General-workbench branch before
that branch returns from ``eframe::App::ui``; its harness regression selects
``ToolView::GeneralPurpose`` so this branch boundary remains covered.

The model is operation-ready but deliberately not wired into current G-code
generation in this change. Future General CAM operations should consume a
selected toolbit snapshot at operation creation time, preserving the same
snapshot-vs-library-edit boundary used by the legacy compatibility path.

Legacy machining compatibility
------------------------------

The existing ``rengrave_core::toolbit`` model remains responsible for legacy
machining selectors and project assignments. Its ``Toolbit::apply_to_settings``
conversion and ``RengraveProjectFile::toolbit_assignments`` snapshot behavior
are unchanged by the General manager. Keeping these models separate prevents
a General library edit from changing an existing machining project or its
generated G-code.
