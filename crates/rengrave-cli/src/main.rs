use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use rengrave_core::batch::{
    BatchOutput, BatchRequest, SecondaryGcode, prepare_batch_output, secondary_output_path,
};
use rengrave_ui::UiLaunchOptions;
use serde_json::{Value, json};

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

    #[arg(
        long = "agent-debug-dir",
        help = "Run calculation/export automation and write debug artifacts to this directory"
    )]
    agent_debug_dir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.agent_debug_dir.is_some() {
        return run_agent_debug(cli);
    }

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
    let request = batch_request_from_cli(
        &cli,
        cli.batch,
        cli.output.clone(),
        cli.svg_output.clone(),
        cli.dxf_output.clone(),
        false,
    );
    let output = prepare_batch_output(&request)?;

    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    write_batch_artifacts(&request, &output)?;

    Ok(())
}

fn run_agent_debug(cli: Cli) -> anyhow::Result<()> {
    let debug_dir = cli
        .agent_debug_dir
        .clone()
        .expect("agent_debug_dir checked by caller");
    fs::create_dir_all(&debug_dir)?;
    let gcode_path = debug_dir.join("output.ngc");
    let svg_path = debug_dir.join("output.svg");
    let dxf_path = debug_dir.join("output.dxf");
    let request = batch_request_from_cli(
        &cli,
        true,
        Some(gcode_path.clone()),
        Some(svg_path.clone()),
        Some(dxf_path.clone()),
        true,
    );
    let output = prepare_batch_output(&request)?;

    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    let secondary_paths = write_batch_artifacts(&request, &output)?;
    let manifest = agent_debug_manifest(
        &request,
        &output,
        &gcode_path,
        &svg_path,
        &dxf_path,
        &secondary_paths,
    );
    let manifest_path = debug_dir.join("debug.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    println!("{}", manifest_path.display());

    Ok(())
}

fn batch_request_from_cli(
    cli: &Cli,
    batch: bool,
    output: Option<PathBuf>,
    svg_output: Option<PathBuf>,
    dxf_output: Option<PathBuf>,
    include_secondary: bool,
) -> BatchRequest {
    BatchRequest {
        batch,
        gcode_file: cli.gcode_file.clone(),
        font_or_image: cli.fontdir.clone(),
        default_dir: cli.defdir.clone(),
        text: cli.text.clone(),
        output,
        svg_output,
        dxf_output,
        include_secondary,
        settings_overrides: Vec::new(),
    }
}

fn write_batch_artifacts(
    request: &BatchRequest,
    output: &BatchOutput,
) -> anyhow::Result<Vec<PathBuf>> {
    let secondary_paths = if let Some(path) = &request.output {
        fs::write(path, &output.gcode)?;
        write_secondary_outputs(path, &output.secondary_gcode)?
    } else {
        io::stdout().write_all(output.gcode.as_bytes())?;
        Vec::new()
    };
    if let (Some(path), Some(svg)) = (&request.svg_output, &output.svg) {
        fs::write(path, svg)?;
    }
    if let (Some(path), Some(dxf)) = (&request.dxf_output, &output.dxf) {
        fs::write(path, dxf)?;
    }
    Ok(secondary_paths)
}

fn agent_debug_manifest(
    request: &BatchRequest,
    output: &BatchOutput,
    gcode_path: &PathBuf,
    svg_path: &PathBuf,
    dxf_path: &PathBuf,
    secondary_paths: &[PathBuf],
) -> Value {
    json!({
        "schema": "rengrave-agent-debug-v1",
        "actions": [
            "calculate",
            "export-gcode",
            "export-svg",
            "export-dxf",
            "export-secondary-cleanup"
        ],
        "inputs": {
            "settings": request.gcode_file.as_ref().map(path_string),
            "font_or_image": request.font_or_image.as_ref().map(path_string),
            "default_dir": request.default_dir.as_ref().map(path_string),
            "text": request.text.as_deref(),
        },
        "artifacts": {
            "gcode": path_string(gcode_path),
            "svg": output.svg.as_ref().map(|_| path_string(svg_path)),
            "dxf": output.dxf.as_ref().map(|_| path_string(dxf_path)),
            "secondary_gcode": secondary_paths.iter().map(path_string).collect::<Vec<_>>(),
        },
        "result": {
            "warnings": &output.warnings,
            "gcode_lines": output.gcode.lines().count(),
            "secondary_gcode_count": output.secondary_gcode.len(),
            "has_svg": output.svg.is_some(),
            "has_dxf": output.dxf.is_some(),
        },
        "notes": {
            "purpose": "Local automation artifact for agents to inspect generation without driving the native GUI.",
            "visual_review": "Use the SVG artifact as the deterministic visual review target; native window screenshots still require the host windowing system."
        }
    })
}

fn path_string(path: &PathBuf) -> String {
    path.display().to_string()
}

fn write_secondary_outputs(
    path: &PathBuf,
    outputs: &[SecondaryGcode],
) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for output in outputs {
        let output_path = secondary_output_path(path, &output.suffix);
        fs::write(&output_path, &output.gcode)?;
        paths.push(output_path);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_output_path_preserves_extension() {
        assert_eq!(
            secondary_output_path(&PathBuf::from("/tmp/output.ngc"), "vclean"),
            PathBuf::from("/tmp/output_vclean.ngc")
        );
        assert_eq!(
            secondary_output_path(&PathBuf::from("/tmp/output"), "clean"),
            PathBuf::from("/tmp/output_clean")
        );
    }

    #[test]
    fn agent_debug_manifest_lists_actions_and_artifacts() {
        let request = BatchRequest {
            batch: true,
            text: Some("Debug".to_owned()),
            output: Some(PathBuf::from("/tmp/debug/output.ngc")),
            svg_output: Some(PathBuf::from("/tmp/debug/output.svg")),
            dxf_output: Some(PathBuf::from("/tmp/debug/output.dxf")),
            include_secondary: true,
            ..BatchRequest::default()
        };
        let output = BatchOutput {
            gcode: "G90\nG1 X0\n".to_owned(),
            warnings: vec!["sample warning".to_owned()],
            secondary_gcode: vec![SecondaryGcode {
                suffix: "clean".to_owned(),
                gcode: "G90\n".to_owned(),
            }],
            svg: Some("<svg/>".to_owned()),
            dxf: Some("0\nEOF\n".to_owned()),
        };

        let manifest = agent_debug_manifest(
            &request,
            &output,
            &PathBuf::from("/tmp/debug/output.ngc"),
            &PathBuf::from("/tmp/debug/output.svg"),
            &PathBuf::from("/tmp/debug/output.dxf"),
            &[PathBuf::from("/tmp/debug/output_clean.ngc")],
        );

        assert_eq!(manifest["schema"], "rengrave-agent-debug-v1");
        assert_eq!(manifest["result"]["gcode_lines"], 2);
        assert_eq!(manifest["result"]["secondary_gcode_count"], 1);
        assert_eq!(manifest["artifacts"]["svg"], "/tmp/debug/output.svg");
        assert_eq!(manifest["actions"][0], "calculate");
    }
}
