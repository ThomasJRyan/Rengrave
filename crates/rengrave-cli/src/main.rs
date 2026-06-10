use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use rengrave_core::batch::{BatchRequest, prepare_batch_output};
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
    };
    let output = prepare_batch_output(&request)?;

    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    if let Some(path) = &request.output {
        fs::write(path, output.gcode)?;
    } else {
        io::stdout().write_all(output.gcode.as_bytes())?;
    }

    Ok(())
}
