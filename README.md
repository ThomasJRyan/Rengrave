# R-Engrave

R-Engrave is a Rust rewrite of F-Engrave focused on CNC engraving and v-carving workflows. It aims to preserve F-Engrave's file compatibility, settings behavior, and generated toolpaths while building a maintainable Rust codebase with a native desktop UI.

The project is still in active development. Current work is parity-first: matching F-Engrave output is more important than optimization or redesign.

## What It Does

R-Engrave generates CNC toolpaths for text, image, and vector engraving workflows. The intended feature set includes:

- Text engraving and text v-carving from CXF and TTF fonts.
- Image engraving and image v-carving from bitmap-like inputs.
- DXF/SVG import and G-code, SVG, and DXF export.
- F-Engrave-compatible settings comments and batch-style command-line use.
- Preview, unit conversion, origin/justification controls, cleanup paths, multipass cutting, and arc fitting.

## How It Works

The workspace is split into focused Rust crates:

- `crates/rengrave-core`: settings, geometry, parsers, toolpath generation, and exporters.
- `crates/rengrave-ui`: the `eframe/egui` desktop interface.
- `crates/rengrave-cli`: command-line entry point for batch and GUI launch workflows.
- `crates/rengrave-potrace`: native Rust bitmap tracing work intended to replace or supplement external Potrace use.

The UI and CLI call into the same core APIs so interactive and batch output can be tested against the same behavior.

## Development

Common commands:

```sh
cargo build --workspace
cargo test --workspace
cargo run -p rengrave-cli
```

Use F-Engrave-generated fixtures as the compatibility baseline when changing toolpath or export behavior.

## Credits

R-Engrave is based on the behavior and workflows of F-Engrave by Scorch Works. F-Engrave remains the primary reference for compatibility and expected CNC output.

Bitmap tracing work is based on Potrace by Peter Selinger. R-Engrave includes an in-progress Rust implementation intended to provide native tracing behavior while preserving the output quality expected from Potrace-based workflows.

## License

R-Engrave is licensed under the GNU General Public License, version 3 or later. See `LICENSE`.

## AI Disclaimer

This project has been generated and modified with assistance from AI coding agents. All generated code and documentation should be reviewed, tested, and validated before use on CNC hardware.
