SHELL := /bin/sh

UV ?= uv
SPHINXBUILD ?= $(UV) run sphinx-build
SOURCEDIR := .
BUILDDIR := _build
PORT ?= 8000

.PHONY: help html serve clean linkcheck

help:
	@printf '%s\n' \
	  'R-Engrave documentation targets:' \
	  '  make html       Build the HTML site in docs/_build/html' \
	  '  make serve      Build and serve the site at http://127.0.0.1:8000' \
	  '  make linkcheck   Check documentation links' \
	  '  make clean      Remove generated documentation output'

html:
	$(SPHINXBUILD) -b html $(SOURCEDIR) $(BUILDDIR)/html

serve: html
	$(UV) run python -m http.server $(PORT) --bind 127.0.0.1 --directory $(BUILDDIR)/html

linkcheck:
	$(SPHINXBUILD) -b linkcheck $(SOURCEDIR) $(BUILDDIR)/linkcheck

clean:
	rm -rf $(BUILDDIR)/html $(BUILDDIR)/linkcheck $(BUILDDIR)/doctrees
