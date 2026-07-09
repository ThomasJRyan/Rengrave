Compatibility and extension notes
=================================

F-Engrave settings
-------------------

The compatibility layer reads the latest ``fengrave_set`` entry for a key and
accepts legacy boolean spellings including ``1``, ``true``, ``yes``, ``on``, and
``box``. ``no_box`` and equivalent false forms are preserved as false. Text is
encoded through one or more ``TCODE`` entries and reconstructed when a legacy
file is loaded.

New settings should be added in four places:

#. the default table in ``settings.rs``;
#. the typed control/options mapping that consumes the key;
#. legacy serialization or project round-trip coverage; and
#. user and developer documentation describing the visible behavior.

Do not rename legacy keys casually. A Rust-friendly field may map to a
compatibility-specific key such as ``v_bit_angle``, ``profile_cut``,
``clean_paths``, or ``return_to_origin``. Keep the legacy spelling at the
boundary and use a typed field internally.

Project format
--------------

``.rgrv`` is versioned JSON. New fields should have serde defaults so older
projects remain readable. Increase ``RENGRAVE_PROJECT_FORMAT_VERSION`` only
when an older project can no longer be interpreted safely, and add a migration
or an explicit unsupported-version error. Paths are not embedded assets.

Generated output contract
-------------------------

Generated G-code is the primary compatibility contract. Changes to motion
ordering, coordinate formatting, comments, unit commands, arc syntax, safe-Z
behavior, or the trailer require a focused fixture update and an explanation
of the intentional difference.

The output writer currently preserves these important behaviors:

* absolute coordinates and explicit unit commands;
* configurable preamble and postamble split on ``|``;
* optional parameter variables for safe Z and engraving depth;
* safe-Z retract before every new path;
* center-offset or radius-format arc emission;
* secondary cleanup suffixes such as ``_clean`` and ``_v_clean``;
* profile and profile-chamfer comments and companion files; and
* optional final ``G0 X0 Y0`` return-to-origin motion.

Licensing boundaries
--------------------

F-Engrave is GPLv3-or-later. ``f-engrave_source/TTF2CXF_STREAM/`` is GPLv2-only
and must not be copied into the Rust binary. Reimplement compatible behavior in
Rust and retain the applicable license notices. Native bitmap tracing lives in
``rengrave-potrace`` rather than depending on the old sidecar workflow.

Adding an operation
-------------------

An operation should accept typed options derived from ``LegacySettings``,
operate in model units, return an explicit geometry type, support cancellation
when it can be long-running, and be wired through ``batch.rs`` before it is
exposed in the UI. Prefer one shared representation for UI preview and export.

The usual implementation sequence is:

#. add or extend the core option type and legacy mapping;
#. implement the geometry with numeric tolerances and cancellation checks;
#. add focused unit tests for formulas and edge cases;
#. add a batch or golden test for generated output;
#. parse the result into the UI preview if it is user-visible;
#. update both user and developer RST pages; and
#. run focused tests, formatting, documentation build, and ``git diff --check``.
