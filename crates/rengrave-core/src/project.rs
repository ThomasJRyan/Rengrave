use std::fs;
use std::path::{Path, PathBuf};

use crate::external::{PotraceStatus, detect_potrace, requires_potrace};
use crate::settings::{LegacySettings, default_legacy_settings, tcode_settings};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentRequest {
    pub gcode_file: Option<PathBuf>,
    pub font_or_image: Option<PathBuf>,
    pub default_dir: Option<PathBuf>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RengraveDocument {
    pub settings: LegacySettings,
    pub text: String,
    pub warnings: Vec<String>,
}

impl Default for RengraveDocument {
    fn default() -> Self {
        let text = "F-Engrave".to_owned();
        let mut settings = default_legacy_settings();
        settings.entries.extend(tcode_settings(&text));

        Self {
            settings,
            text,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("unable to read `{path}`: {source}")]
    ReadSettings {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    CxfFont(PathBuf),
    TtfFont(PathBuf),
    Image(PathBuf),
    Missing,
}

pub fn resolve_input_kind(settings: &LegacySettings) -> InputKind {
    match settings.get_last("input_type") {
        Some("image") => settings
            .get_last("imagefile")
            .map(|path| InputKind::Image(PathBuf::from(path)))
            .unwrap_or(InputKind::Missing),
        _ => {
            let fontfile = settings.get_last("fontfile").unwrap_or_default().trim();
            if fontfile.is_empty() {
                return InputKind::Missing;
            }
            let font_path = PathBuf::from(fontfile);
            let path = if font_path.is_absolute() {
                font_path
            } else {
                PathBuf::from(settings.get_last("fontdir").unwrap_or_default()).join(font_path)
            };
            match path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("cxf") => InputKind::CxfFont(path),
                Some("ttf") => InputKind::TtfFont(path),
                _ => InputKind::Missing,
            }
        }
    }
}

pub fn load_document(request: &DocumentRequest) -> Result<RengraveDocument, DocumentError> {
    load_document_with_potrace_probe(request, detect_potrace)
}

fn load_document_with_potrace_probe(
    request: &DocumentRequest,
    potrace_probe: impl FnOnce() -> PotraceStatus,
) -> Result<RengraveDocument, DocumentError> {
    let mut warnings = Vec::new();
    let mut settings = if let Some(path) = &request.gcode_file {
        let input = fs::read_to_string(path).map_err(|source| DocumentError::ReadSettings {
            path: path.clone(),
            source,
        })?;
        LegacySettings::parse(&input)
    } else {
        default_legacy_settings()
    };

    if let Some(font_or_image) = &request.font_or_image {
        apply_font_or_image(&mut settings, font_or_image);
        if requires_potrace(font_or_image) {
            let status = potrace_probe();
            if !status.available {
                warnings.push(status.message);
            }
        }
    }

    if let Some(default_dir) = &request.default_dir {
        settings.set_or_push("NGC_DIR", default_dir.display().to_string(), true);
    }

    let text_from_settings = match settings.text_from_tcode() {
        Ok(text) => text,
        Err(err) => {
            warnings.push(format!("ignored invalid legacy TCODE text: {err}"));
            None
        }
    };

    let text = request
        .text
        .as_deref()
        .map(normalize_cli_text)
        .or(text_from_settings)
        .unwrap_or_else(|| "F-Engrave".to_owned());

    settings.entries.retain(|entry| entry.key != "TCODE");
    settings.entries.extend(tcode_settings(&text));

    Ok(RengraveDocument {
        settings,
        text,
        warnings,
    })
}

pub fn apply_font_or_image(settings: &mut LegacySettings, path: &Path) {
    if path.is_dir() {
        settings.set_or_push("fontdir", path.display().to_string(), true);
        return;
    }

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "cxf" | "ttf" => {
            if let Some(parent) = path.parent() {
                settings.set_or_push("fontdir", parent.display().to_string(), true);
            }
            if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
                settings.set_or_push("fontfile", file_name, true);
            }
            settings.set_or_push("input_type", "text", false);
        }
        _ => {
            settings.set_or_push("imagefile", path.display().to_string(), true);
            settings.set_or_push("input_type", "image", false);
        }
    }
}

pub fn normalize_cli_text(text: &str) -> String {
    text.replace('|', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_text_overrides_default_and_uses_pipe_newlines() {
        let document = load_document(&DocumentRequest {
            text: Some("Line 1|Line 2".to_owned()),
            ..DocumentRequest::default()
        })
        .unwrap();

        assert_eq!(document.text, "Line 1\nLine 2");
        assert_eq!(
            document.settings.text_from_tcode().unwrap(),
            Some("Line 1\nLine 2".to_owned())
        );
    }

    #[test]
    fn image_path_switches_input_type_to_image() {
        let document = load_document(&DocumentRequest {
            font_or_image: Some(PathBuf::from("/tmp/example.dxf")),
            ..DocumentRequest::default()
        })
        .unwrap();

        assert_eq!(document.settings.get_last("input_type"), Some("image"));
        assert_eq!(
            document.settings.get_last("imagefile"),
            Some("/tmp/example.dxf")
        );
    }

    #[test]
    fn bitmap_input_warns_when_potrace_is_missing() {
        let document = load_document_with_potrace_probe(
            &DocumentRequest {
                font_or_image: Some(PathBuf::from("/tmp/example.png")),
                ..DocumentRequest::default()
            },
            || PotraceStatus::missing("missing potrace"),
        )
        .unwrap();

        assert_eq!(document.settings.get_last("input_type"), Some("image"));
        assert_eq!(document.warnings, vec!["missing potrace"]);
    }

    #[test]
    fn bitmap_input_continues_without_warning_when_potrace_exists() {
        let document = load_document_with_potrace_probe(
            &DocumentRequest {
                font_or_image: Some(PathBuf::from("/tmp/example.bmp")),
                ..DocumentRequest::default()
            },
            || PotraceStatus::found(Some("potrace 1.16".to_owned())),
        )
        .unwrap();

        assert!(document.warnings.is_empty());
    }

    #[test]
    fn invalid_tcode_is_a_warning_not_a_document_failure() {
        let mut settings = default_legacy_settings();
        settings.entries.push(crate::settings::LegacySetting::new(
            "TCODE",
            "not-a-code",
            false,
        ));
        let input = settings.to_string();
        let path = std::env::temp_dir().join("rengrave-invalid-tcode.ngc");
        fs::write(&path, input).unwrap();

        let document = load_document(&DocumentRequest {
            gcode_file: Some(path.clone()),
            ..DocumentRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert_eq!(document.text, "F-Engrave");
        assert_eq!(document.warnings.len(), 1);
    }

    #[test]
    fn resolves_relative_cxf_font_from_legacy_settings() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("fontdir", "/tmp/fonts", true);
        settings.set_or_push("fontfile", "romanc.cxf", true);

        assert_eq!(
            resolve_input_kind(&settings),
            InputKind::CxfFont(PathBuf::from("/tmp/fonts/romanc.cxf"))
        );
    }
}
