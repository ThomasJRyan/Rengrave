use std::path::PathBuf;

use crate::bitmap::vectorize_bitmap_to_dxf;
use crate::cleanup::{CleanupBit, CleanupOptions, generate_cleanup_points};
use crate::dxf::{dxf_font_from_str, read_dxf_font};
use crate::external::requires_potrace;
use crate::font::{read_cxf, read_ttf};
use crate::gcode::{GcodeOptions, write_cleanup_gcode, write_engrave_gcode, write_vcarve_gcode};
use crate::layout::{LayoutSettings, layout_text};
use crate::project::{DocumentError, DocumentRequest, load_document};
use crate::project::{InputKind, resolve_input_kind};
use crate::settings::LegacySettings;
use crate::vcarve::{VCarveOptions, generate_vcarve_points};
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
    pub secondary_gcode: Vec<SecondaryGcode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondaryGcode {
    pub suffix: String,
    pub gcode: String,
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
    let generated_gcode = generate_engrave_gcode(
        &document.settings,
        &document.text,
        request.output.is_some(),
        &mut warnings,
    );
    let gcode = if let Some(gcode_lines) = generated_gcode {
        let primary = render_gcode(&document.settings, &document.text, &gcode_lines.primary);
        let secondary_gcode = gcode_lines
            .secondary
            .into_iter()
            .map(|secondary| SecondaryGcode {
                suffix: secondary.suffix,
                gcode: render_secondary_gcode(&document.settings, &document.text, &secondary.lines),
            })
            .collect();
        return Ok(BatchOutput {
            gcode: primary,
            warnings,
            secondary_gcode,
        });
    } else {
        warnings.push(
            "toolpath generation is not available for this input yet; output contains compatible settings comments only"
                .to_owned(),
        );
        render_settings_only_gcode(&document.settings, &document.text)
    };

    Ok(BatchOutput {
        gcode,
        warnings,
        secondary_gcode: Vec::new(),
    })
}

fn generate_engrave_gcode(
    settings: &LegacySettings,
    text: &str,
    include_secondary: bool,
    warnings: &mut Vec<String>,
) -> Option<GeneratedToolpaths> {
    if settings.get_last("input_type") == Some("image") {
        return generate_dxf_engrave_gcode(settings, include_secondary, warnings);
    }

    generate_text_engrave_gcode(settings, text, include_secondary, warnings)
}

fn generate_text_engrave_gcode(
    settings: &LegacySettings,
    text: &str,
    include_secondary: bool,
    warnings: &mut Vec<String>,
) -> Option<GeneratedToolpaths> {
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

    write_layout_gcode(settings, &layout.segments, include_secondary, warnings)
}

fn generate_dxf_engrave_gcode(
    settings: &LegacySettings,
    include_secondary: bool,
    warnings: &mut Vec<String>,
) -> Option<GeneratedToolpaths> {
    let InputKind::Image(path) = resolve_input_kind(settings) else {
        warnings.push("image input path is missing".to_owned());
        return None;
    };

    let is_dxf = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("dxf"))
        .unwrap_or(false);
    let segarc = settings
        .get_last("segarc")
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);
    let font = if is_dxf {
        match read_dxf_font(&path, segarc) {
            Ok(font) => font,
            Err(err) => {
                warnings.push(err.to_string());
                return None;
            }
        }
    } else if requires_potrace(&path) {
        match vectorize_bitmap_to_dxf(&path, settings) {
            Ok(dxf) => dxf_font_from_str(&dxf, segarc),
            Err(err) => {
                warnings.push(err.to_string());
                return None;
            }
        }
    } else {
        warnings.push("unsupported image input format".to_owned());
        return None;
    };
    let layout = layout_text(&font, "F", &LayoutSettings::from_legacy(settings));
    if layout.segments.is_empty() {
        warnings.push("DXF contained no engraveable segments".to_owned());
        return None;
    }

    write_layout_gcode(settings, &layout.segments, include_secondary, warnings)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedToolpaths {
    primary: Vec<String>,
    secondary: Vec<GeneratedSecondary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedSecondary {
    suffix: String,
    lines: Vec<String>,
}

fn write_layout_gcode(
    settings: &LegacySettings,
    segments: &[crate::layout::EngraveSegment],
    include_secondary: bool,
    warnings: &mut Vec<String>,
) -> Option<GeneratedToolpaths> {
    let gcode_options = GcodeOptions::from_legacy(settings);
    if settings.get_last("cut_type") != Some("v-carve") {
        return Some(GeneratedToolpaths {
            primary: write_engrave_gcode(segments, &gcode_options),
            secondary: Vec::new(),
        });
    }

    let vcarve_options = VCarveOptions::from_legacy(settings);
    if vcarve_options.bit_shape == crate::vcarve::BitShape::Flat {
        return Some(GeneratedToolpaths {
            primary: write_engrave_gcode(segments, &gcode_options),
            secondary: Vec::new(),
        });
    }

    let points = generate_vcarve_points(segments, &vcarve_options, gcode_options.accuracy);
    if points.is_empty() {
        warnings.push("v-carve generated no toolpath points".to_owned());
        return None;
    }

    let mut secondary = Vec::new();
    if include_secondary {
        let cleanup_options = CleanupOptions::from_legacy(settings);
        for bit in [CleanupBit::Straight, CleanupBit::VBit] {
            let cleanup_points = generate_cleanup_points(
                segments,
                &cleanup_options,
                &vcarve_options,
                bit,
                gcode_options.accuracy,
            );
            if cleanup_points.is_empty() {
                continue;
            }
            secondary.push(GeneratedSecondary {
                suffix: bit.suffix().to_owned(),
                lines: write_cleanup_gcode(
                    &cleanup_points,
                    &gcode_options,
                    &cleanup_options,
                    &vcarve_options,
                    bit,
                ),
            });
        }
    }

    Some(GeneratedToolpaths {
        primary: write_vcarve_gcode(&points, &gcode_options, &vcarve_options),
        secondary,
    })
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

fn render_secondary_gcode(settings: &LegacySettings, text: &str, gcode_lines: &[String]) -> String {
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

    #[test]
    fn batch_generates_vcarve_gcode_for_closed_cxf_text() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-vcarve-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-vcarve-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &font_path,
            "[A] 4\nL 0,0,10,0\nL 10,0,10,10\nL 10,10,0,10\nL 0,10,0,0\n",
        )
        .unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set cut_type   v-carve )\n(fengrave_set v_step_len  0.5 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("cut_type"));
        assert!(output.gcode.contains("v-carve"));
        assert!(output.gcode.contains("G1 X"));
        assert!(output.gcode.contains(" Z-"));
        assert!(
            !output
                .gcode
                .contains("v-carve generation is not ported yet")
        );
        assert!(output.secondary_gcode.is_empty());
    }

    #[test]
    fn batch_prepares_cleanup_companion_gcode_when_output_path_is_set() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        let output_path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-{}-{}.out.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &font_path,
            "[A] 4\nL 0,0,10,0\nL 10,0,10,10\nL 10,10,0,10\nL 0,10,0,0\n",
        )
        .unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set cut_type   v-carve )\n(fengrave_set clean_paths  1,0,0,0,0,0,0,0 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            output: Some(output_path),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert_eq!(output.secondary_gcode.len(), 1);
        assert_eq!(output.secondary_gcode[0].suffix, "clean");
        assert!(
            output.secondary_gcode[0]
                .gcode
                .contains("secondary cleanup operation")
        );
    }

    #[test]
    fn bitmap_decode_failure_falls_back_to_settings_only_output() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-invalid-bitmap-{}-{}.png",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, b"not a png").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.contains("unable to decode bitmap"))
        );
        assert!(
            output
                .gcode
                .contains("( R-Engrave scaffold: toolpath generation is not implemented yet )")
        );
    }
}
