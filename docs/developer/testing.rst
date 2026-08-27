Testing and documentation workflow
==================================

Test layers
-----------

Use the smallest test that proves the changed contract:

* core unit tests cover geometry formulas, settings parsing, path ordering,
  cancellation, and option defaults;
* core integration/golden tests cover document loading, output text, SVG/DXF
  payloads, legacy round trips, and cross-module behavior;
* UI library tests cover controls, stale/recalculation state, preview layers,
  startup layout and actions, recent-project ordering, layout at representative
  desktop sizes, and egui harness interaction;
* CLI checks cover argument translation, artifact paths, and debug manifests.

Typical commands:

::

   cargo fmt --all --check
   cargo test -p rengrave-core --test golden
   cargo test -p rengrave-ui --lib
   cargo run -p rengrave-cli -- \
     --agent-debug-dir /tmp/rengrave-debug \
     -f assets/fonts/rengrave_demo.cxf -t "R-Engrave"
   git diff --check

Do not run the full workspace suite by default for a narrow documentation or
algorithm change. Use ``cargo test --workspace`` when a change crosses crate
boundaries or when focused tests cannot provide truthful coverage, and record
that reason in the change summary.

Geometry tolerances
-------------------

Use explicit floating-point tolerances for geometric equality, path joining,
closed-loop detection, and simplification. Use strict byte/text comparisons
where output formatting is intentionally part of compatibility. Test both
normal and degenerate inputs: empty paths, tiny segments, missing glyphs,
open loops, zero/negative dimensions, narrow tabs, and unsupported project
versions.

Documentation build
-------------------

Source documentation and its Python environment live entirely under
``docs/``. Use ``uv`` through the documentation Makefile; the generated site
is built into ``docs/_build/`` and is ignored by Git:

::

   cd docs
   uv sync
   make html
   make serve

The generated HTML is a multi-page Sphinx site. Open
``docs/_build/html/index.html`` or use ``make serve`` for local browser
inspection. Edit the RST or source stylesheet, then rebuild it. ``make
linkcheck`` runs Sphinx's link checker.

Manual review checklist
-----------------------

Before committing a feature or behavior change:

* read the changed user workflow from a fresh user's point of view;
* read the developer page and confirm names, units, formulas, and invariants
  match the code;
* verify image alt text and captions explain why a diagram exists;
* include a screenshot when a UI behavior is part of the contract, or leave a
  specific placeholder with the required viewport and state;
* inspect the built HTML in a browser or with a local HTML renderer; and
* commit one focused semantic change after validation.
