use std::path::PathBuf;

use crate::dxf::read_dxf_font;
use crate::font::{read_cxf, read_ttf};
use crate::gcode::{GcodeOptions, write_engrave_gcode};
use crate::layout::{LayoutSettings, layout_text};
use crate::project::{DocumentError, DocumentRequest, load_document};
use crate::project::{InputKind, resolve_input_kind};
use crate::settings::LegacySettings;
use crate::{FENGRAVE_VERSION, RENGRAVE_VERSION};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchRequest {
    pub batch: bool,
    pub gcode_file: Option<PathBuf>,
    pub font_or_image: Option<PathBuf>,
    pub default_dir: Option<PathBuf>,
    pub text: Option<String>,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOutput {
    pub gcode: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error(transparent)]
    Document(#[from] DocumentError),
}

pub fn prepare_batch_output(request: &BatchRequest) -> Result<BatchOutput, BatchError> {
    let document = load_document(&DocumentRequest {
        gcode_file: request.gcode_file.clone(),
        font_or_image: request.font_or_image.clone(),
        default_dir: request.default_dir.clone(),
        text: request.text.clone(),
    })?;

    let mut warnings = document.warnings;
    let generated_gcode = generate_engrave_gcode(&document.settings, &document.text, &mut warnings);
    let gcode = if let Some(gcode_lines) = generated_gcode {
        render_gcode(&document.settings, &document.text, &gcode_lines)
    } else {
        warnings.push(
            "toolpath generation is not available for this input yet; output contains compatible settings comments only"
                .to_owned(),
        );
        render_settings_only_gcode(&document.settings, &document.text)
    };

    Ok(BatchOutput { gcode, warnings })
}

fn generate_engrave_gcode(
    settings: &LegacySettings,
    text: &str,
    warnings: &mut Vec<String>,
) -> Option<Vec<String>> {
    if settings.get_last("cut_type") == Some("v-carve") {
        warnings.push("v-carve generation is not ported yet".to_owned());
        return None;
    }

    if settings.get_last("input_type") == Some("image") {
        return generate_dxf_engrave_gcode(settings, warnings);
    }

    generate_text_engrave_gcode(settings, text, warnings)
}

fn generate_text_engrave_gcode(
    settings: &LegacySettings,
    text: &str,
    warnings: &mut Vec<String>,
) -> Option<Vec<String>> {
    let input = resolve_input_kind(settings);
    let segarc = settings
        .get_last("segarc")
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);
    let font = match input {
        InputKind::CxfFont(path) => match read_cxf(&path, segarc) {
            Ok(font) => font,
            Err(err) => {
                warnings.push(err.to_string());
                return None;
            }
        },
        InputKind::TtfFont(path) => {
            let extended_chars = settings
                .get_last("ext_char")
                .map(|value| matches!(value, "1" | "true" | "True"))
                .unwrap_or(false);
            match read_ttf(&path, segarc, extended_chars) {
                Ok(font) => font,
                Err(err) => {
                    warnings.push(err.to_string());
                    return None;
                }
            }
        }
        _ => {
            warnings.push(
                "only CXF and TTF text fonts are currently generated in Rust batch mode".to_owned(),
            );
            return None;
        }
    };

    let layout_settings = LayoutSettings::from_legacy(settings);
    let layout = layout_text(&font, text, &layout_settings);
    if !layout.missing_chars.is_empty() {
        let missing: String = layout.missing_chars.iter().collect();
        warnings.push(format!("characters not found in font file: {missing}"));
    }
    if layout.segments.is_empty() {
        warnings.push("no engraveable text segments were generated".to_owned());
        return None;
    }

    Some(write_engrave_gcode(
        &layout.segments,
        &GcodeOptions::from_legacy(settings),
    ))
}

fn generate_dxf_engrave_gcode(
    settings: &LegacySettings,
    warnings: &mut Vec<String>,
) -> Option<Vec<String>> {
    let InputKind::Image(path) = resolve_input_kind(settings) else {
        warnings.push("image input path is missing".to_owned());
        return None;
    };

    let is_dxf = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("dxf"))
        .unwrap_or(false);
    if !is_dxf {
        warnings.push("bitmap vectorization through Potrace is not ported yet".to_owned());
        return None;
    }

    let segarc = settings
        .get_last("segarc")
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);
    let font = match read_dxf_font(&path, segarc) {
        Ok(font) => font,
        Err(err) => {
            warnings.push(err.to_string());
            return None;
        }
    };
    let layout = layout_text(&font, "F", &LayoutSettings::from_legacy(settings));
    if layout.segments.is_empty() {
        warnings.push("DXF contained no engraveable segments".to_owned());
        return None;
    }

    Some(write_engrave_gcode(
        &layout.segments,
        &GcodeOptions::from_legacy(settings),
    ))
}

fn render_settings_only_gcode(settings: &LegacySettings, text: &str) -> String {
    let mut lines = render_settings_header(settings, text);
    lines.push("( R-Engrave scaffold: toolpath generation is not implemented yet )".to_owned());
    lines.push(String::new());
    lines.join("\n")
}

fn render_gcode(settings: &LegacySettings, text: &str, gcode_lines: &[String]) -> String {
    let mut lines = render_settings_header(settings, text);
    lines.extend(gcode_lines.iter().cloned());
    lines.push(String::new());
    lines.join("\n")
}

fn render_settings_header(settings: &LegacySettings, text: &str) -> Vec<String> {
    let mut lines = vec![
        format!("( Code generated by r-engrave-{RENGRAVE_VERSION} )"),
        format!("( Compatibility target: f-engrave-{FENGRAVE_VERSION}.py )"),
        "(Settings used in f-engrave when this file was created)".to_owned(),
    ];
    if settings.get_last("input_type") == Some("text") {
        lines.push(format!("(Engrave Text:{} )", sanitized_text_comment(text)));
    }
    lines.push("(=========================================================)".to_owned());

    lines.extend(settings.to_comments());
    lines.push("(#########################################################)".to_owned());
    lines
}

fn sanitized_text_comment(text: &str) -> String {
    let mut output = String::new();
    for ch in text.chars().take(40) {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
        } else {
            output.push(' ');
        }
    }
    if text.chars().count() > 40 {
        output.push_str("___");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn batch_text_uses_legacy_pipe_newline_convention() {
        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            text: Some("Line 1|Line 2".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        assert!(
            output.gcode.contains(
                "(fengrave_set TCODE       076 105 110 101 032 049 010 076 105 110 101 )"
            )
        );
        assert!(output.gcode.contains("(fengrave_set TCODE       032 050 )"));
    }

    #[test]
    fn font_path_updates_text_input_settings() {
        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(PathBuf::from("/tmp/example.ttf")),
            ..BatchRequest::default()
        })
        .unwrap();

        assert!(
            output
                .gcode
                .contains("(fengrave_set fontfile    \"example.ttf\" )")
        );
        assert!(output.gcode.contains("(fengrave_set input_type  text )"));
    }

    #[test]
    fn batch_generates_basic_gcode_for_cxf_text() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-basic-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 1\nL 0,0,0,10\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("G90\nG20\nG17 G64 P0.001 M3 S3000"));
        assert!(output.gcode.contains("G1 Z-0.0050"));
        assert!(output.gcode.contains("G1 X0.0000 Y1.9900"));
    }

    #[test]
    fn batch_generates_basic_gcode_for_dxf_image() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-basic-{}-{}.dxf",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &path,
            "0\nSECTION\n0\nLINE\n10\n0\n20\n0\n11\n0\n21\n10\n0\nENDSEC\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("(fengrave_set input_type  image )"));
        assert!(!output.gcode.contains("(Engrave Text:"));
        assert!(output.gcode.contains("G1 X0.0000 Y1.9900"));
    }
}
