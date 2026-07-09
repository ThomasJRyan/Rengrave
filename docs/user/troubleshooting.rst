Troubleshooting
===============

No toolpath was generated
-------------------------

The batch pipeline can still emit settings-only G-code when it cannot produce
a toolpath. Check:

* the input path exists and has a supported extension;
* a CXF/TTF font contains the requested characters;
* a bitmap produces a useful trace mask;
* a DXF or SVG contains supported geometry;
* the selected workbench matches the input kind; and
* the requested depth, diameter, and profile settings are usable.

The UI lists missing glyphs and bitmap mask statistics in the input preview.
The CLI writes warnings to stderr and the agent-debug manifest records them in
``result.warnings``.

The preview is empty or clipped
-------------------------------

Use **Fit** or double-click the central preview. Turn on bounds, axes, and grid
to distinguish a geometry problem from a view problem. Check the model-space
X/Y readout and confirm that the selected origin, mirror, flip, or rotation is
intentional.

If a profile is enabled, remember that the profile envelope can be larger than
the text or artwork. The origin is calculated against that expanded envelope
so the profile stays in the requested alignment.

The output is stale
-------------------

Change detection covers text, controls, input paths, cleanup requests, and the
requested export set. Press **Recalculate** after editing a setting, then wait
for the status to return to ready. Do not machine a file while the UI says
calculating or stale.

V-carve is too dense or too shallow
-----------------------------------

The V-carve sampler follows the source with ``v_step_len``. A smaller value
captures more detail but increases work and output size. A larger value is
faster but can miss narrow transitions. Check the V-bit angle, effective
diameter, depth limit, accuracy, and rough-stock settings together; the
effective envelope and depth model are coupled.

Cleanup has no visible paths
----------------------------

Cleanup requires closed source loops. Open strokes can be engraved, but they do
not define an enclosed region for offset or scanline cleanup. Enable at least
one cleanup selection and check the selected bit's diameter. A cleanup file is
secondary output and is only written when the request asks for it.

Profile tabs cut through or leave too much stock
------------------------------------------------

Tab height is measured above the final profile depth. Tab width determines the
flat portion; R-Engrave ramps at 45 degrees and caps each ramp at half the tab
width. Increase width for a longer flat, increase height to leave more stock,
or reduce both for a less visible tab. Confirm that the endmill can safely
enter and leave the profile path.

The native UI does not start
----------------------------

The UI requires a desktop compositor supported by ``winit``. In headless or
remote environments, use the CLI batch path or ``--agent-debug-dir``. The
agent-debug SVG is deterministic and is the preferred artifact for automated
visual inspection when a native window cannot be captured.
