use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use rengrave_core::batch::{BatchRequest, SecondaryGcode, prepare_batch_output};
use rengrave_ui::UiLaunchOptions;

#[derive(Debug, Parser)]
#[command(name = "rengrave")]
#[command(version)]
#[command(about = "Rust port of F-Engrave")]
struct Cli {
    #[arg(short = 'b', long = "batch", help = "Run batch mode without the GUI")]
    batch: bool,

    #[arg(
        short = 'g',
        long = "gcode_file",
        help = "F-Engrave G-code/settings file to read"
    )]
    gcode_file: Option<PathBuf>,

    #[arg(
        short = 'f',
        long = "fontdir",
        help = "Path to a font file, font directory, or image file"
    )]
    fontdir: Option<PathBuf>,

    #[arg(short = 'd', long = "defdir", help = "Default directory")]
    defdir: Option<PathBuf>,

    #[arg(
        short = 't',
        long = "text",
        help = "Text to engrave; use | for newlines"
    )]
    text: Option<String>,

    #[arg(
        short = 'o',
        long = "output",
        help = "Write batch output to this path instead of stdout"
    )]
    output: Option<PathBuf>,

    #[arg(long = "svg-output", help = "Write an SVG export to this path")]
    svg_output: Option<PathBuf>,

    #[arg(long = "dxf-output", help = "Write a DXF export to this path")]
    dxf_output: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.batch {
        return run_batch(cli);
    }

    rengrave_ui::run(UiLaunchOptions {
        gcode_file: cli.gcode_file,
        font_or_image: cli.fontdir,
        default_dir: cli.defdir,
        text: cli.text,
    })
    .map_err(|err| anyhow::anyhow!("{err}"))
}

fn run_batch(cli: Cli) -> anyhow::Result<()> {
    let request = BatchRequest {
        batch: cli.batch,
        gcode_file: cli.gcode_file,
        font_or_image: cli.fontdir,
        default_dir: cli.defdir,
        text: cli.text,
        output: cli.output,
        svg_output: cli.svg_output,
        dxf_output: cli.dxf_output,
        include_secondary: false,
        settings_overrides: Vec::new(),
    };
    let output = prepare_batch_output(&request)?;

    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    if let Some(path) = &request.output {
        fs::write(path, output.gcode)?;
        write_secondary_outputs(path, &output.secondary_gcode)?;
    } else {
        io::stdout().write_all(output.gcode.as_bytes())?;
    }
    if let (Some(path), Some(svg)) = (&request.svg_output, &output.svg) {
        fs::write(path, svg)?;
    }
    if let (Some(path), Some(dxf)) = (&request.dxf_output, &output.dxf) {
        fs::write(path, dxf)?;
    }

    Ok(())
}

fn write_secondary_outputs(path: &PathBuf, outputs: &[SecondaryGcode]) -> anyhow::Result<()> {
    for output in outputs {
        fs::write(secondary_output_path(path, &output.suffix), &output.gcode)?;
    }
    Ok(())
}

fn secondary_output_path(path: &PathBuf, suffix: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    let extension = path.extension().and_then(|value| value.to_str());
    let mut file_name = format!("{stem}_{suffix}");
    if let Some(extension) = extension {
        file_name.push('.');
        file_name.push_str(extension);
    }
    path.with_file_name(file_name)
}
