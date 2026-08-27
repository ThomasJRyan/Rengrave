R-Engrave documentation
========================

R-Engrave is a native Rust workbench for CNC engraving, V-carving, cleanup,
inlay, and profile operations. It is a parity-first port of F-Engrave 1.78:
generated output, legacy settings, and predictable machine motion take
priority over a new file format or a redesigned machining model.

This is the first documentation draft for the current workspace. It describes
implemented behavior, calls out compatibility boundaries, and marks UI details
that still need a native screenshot or a final product decision.

.. |workflow| image:: _images/workflow.svg
   :alt: R-Engrave input, calculation, preview, and export workflow
   :width: 100%

.. |coordinates| image:: _images/coordinate-system.svg
   :alt: Coordinate bounds, origin selection, and profile expansion
   :width: 100%

.. |vcarve| image:: _images/vcarve-circle.svg
   :alt: A V-bit tangent to a maximum inscribed circle in a carved region
   :width: 100%

.. |cleanup| image:: _images/cleanup-scanlines.svg
   :alt: Cleanup area offset followed by horizontal and vertical scanlines
   :width: 100%

.. |profile| image:: _images/profile-tabs.svg
   :alt: Profile perimeter, ramped tabs, and depth passes
   :width: 100%

.. |ui_placeholder| image:: _images/ui-overview-placeholder.svg
   :alt: Placeholder for a native R-Engrave desktop screenshot
   :width: 100%

User guide
----------

.. toctree::
   :maxdepth: 2
   :caption: User guide

   user/quickstart
   user/workflows
   user/reference
   user/troubleshooting
   user/toolbits
   user/profile-toolpaths

Developer guide
---------------

.. toctree::
   :maxdepth: 2
   :caption: Developer guide

   developer/architecture
   developer/algorithms
   developer/compatibility
   developer/testing
   developer/toolbits
   developer/general-profile-toolpaths

Current scope
-------------

The workspace currently contains four crates:

* ``rengrave-core`` contains settings, document loading, fonts, importers,
  layout, toolpath algorithms, G-code, SVG, and DXF output.
* ``rengrave-ui`` contains the native ``eframe``/``egui`` desktop workbench,
  input catalog, preview, calculation worker, preferences, and file browser.
* ``rengrave-cli`` provides GUI launch, F-Engrave-style batch flags, direct
  exports, and an agent-debug artifact mode.
* ``rengrave-potrace`` contains the native Rust bitmap tracing implementation.

The upstream ``f-engrave_source/`` directory remains the behavioral and
licensing reference. The port is not yet a complete replacement for every
F-Engrave feature or every F-Engrave UI path; use the compatibility and
troubleshooting pages to understand the current boundary.

Documentation conventions
-------------------------

``in`` and ``mm`` values follow the active job units. Negative Z values mean
cutting below the work surface, while positive safe-Z values clear the stock.
Code identifiers and legacy setting keys are written in a fixed-width font.
Statements marked *draft* describe an area that needs a future fixture,
screenshot, or compatibility decision rather than claiming a stronger
contract than the source currently supports.
