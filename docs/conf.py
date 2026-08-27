"""Sphinx configuration for the self-contained R-Engrave documentation."""


project = "R-Engrave"
copyright = "2026, R-Engrave contributors"
author = "R-Engrave contributors"
release = "0.1.0"

extensions = []
templates_path = ["_templates"]
exclude_patterns = ["_build", ".venv", "agent-docs"]
source_suffix = ".rst"
master_doc = "index"
language = "en"

html_theme = "alabaster"
html_title = "R-Engrave documentation"
html_static_path = ["_static"]
html_css_files = ["rengrave.css"]
