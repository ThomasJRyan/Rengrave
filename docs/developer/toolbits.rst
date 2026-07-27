Toolbit schema and compatibility
================================

``rengrave_core::toolbit`` owns the persistent model. ``Toolbit`` stores a
stable string ID, display name, string type, canonical ``diameter_mm``,
optional V-bit angle or bullnose corner radius, and optional feed/plunge values
in millimetres per minute. The type is deliberately a string rather than a
closed enum: a future reader can preserve an unknown type without rejecting
the whole JSON library.

``ToolbitLibrary`` serializes to JSON and resolves the platform path through
``default_library_path``. Missing files are treated by the UI as an empty
library; malformed files produce a visible load error when explicitly loaded.
The core model validates known geometry and provides ``eligible`` filtering by
``ToolRole``. Bullnose is intentionally excluded from current core operations.

The compatibility seam is ``Toolbit::apply_to_settings``. It converts
millimetres to the active legacy units, writes the existing keys such as
``v_bit_dia``, ``v_bit_angle``, ``profile_endmill_dia``, ``FEED``, and
``PLUNGE``, and leaves operation-specific keys untouched. This keeps CLI and
legacy generation unchanged. The UI applies a project assignment's frozen
snapshot before calculating; saving the same project preserves the snapshot
even if the library entry is later edited.

``RengraveProjectFile::toolbit_assignments`` is ``serde(default)``. Therefore
minimal and older version-1 projects deserialize with no assignments and use
their existing ``LegacySettings`` values. Unsupported imported assignments
produce a warning and do not invent a replacement cutter.

