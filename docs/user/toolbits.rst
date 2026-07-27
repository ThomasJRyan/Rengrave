Toolbit library
===============

The **Settings > Toolbit library...** window stores named cutters for reuse
between jobs. The library starts empty so R-Engrave never guesses a cutter
dimension. Choose **New**, enter the physical geometry, and save the library
when the tool is valid.

Geometry is stored in millimetres. Numeric job controls are displayed and
applied in the active job units. A tool may also store feed and plunge defaults;
operation settings such as stock, tabs, cleanup step-over, and depth strategy
remain job-specific.

Tool types
----------

Straight endmills and V-bits can be selected by supported operations. V-bits
require an included angle. Bullnose tools may be stored and edited for future
operations, but are not offered to operations whose core geometry does not
support them. Unknown type names are retained in the JSON library for forward
compatibility and are not silently used for cutting.

Assigning a tool
----------------

The Tool / Cut, Cleanup, and Profile panels expose selectors for eligible
library tools. Selecting one copies its snapshot into the current job and marks
the generated output stale. **Use custom values for this job** remains the
legacy fallback: edit the numeric fields directly when a job is unusual or a
tool is not in the library. Editing a library entry later does not change a
saved job.

Cleanup tools are an ordered list. Add the largest cutter first and continue
from largest to smallest; the residual-only cleanup pipeline then avoids
retracing material already removed by a larger cutter. The existing ``+`` and
``-`` controls change the list without changing operation-specific cleanup
settings.

Persistence and projects
------------------------

The library is saved as ``toolbits.json`` in R-Engrave's platform configuration
directory: ``$XDG_CONFIG_HOME`` or ``~/.config`` on Linux, Application Support
on macOS, and ``%APPDATA%`` on Windows. Projects store a named tool assignment
and a frozen snapshot in ``toolbit_assignments``. Older ``.rgrv`` files omit
that field and continue to load using their existing legacy numeric settings.

