use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::RENGRAVE_VERSION;
use crate::bitmap::BitmapBackend;
use crate::settings::{LegacySetting, LegacySettings, default_legacy_settings, tcode_settings};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentRequest {
    pub gcode_file: Option<PathBuf>,
    pub font_or_image: Option<PathBuf>,
    pub default_dir: Option<PathBuf>,
    pub text: Option<String>,
    pub settings_overrides: Vec<LegacySetting>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RengraveDocument {
    pub settings: LegacySettings,
    pub text: String,
    pub input_path: Option<PathBuf>,
    pub warnings: Vec<String>,
}

pub const RENGRAVE_PROJECT_EXTENSION: &str = "rgrv";
pub const RENGRAVE_PROJECT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RengraveProjectFile {
    #[serde(default = "current_project_format_version")]
    pub format_version: u32,
    #[serde(default = "current_application_version")]
    pub application_version: String,
    #[serde(default = "default_project_text")]
    pub text: String,
    #[serde(default = "default_legacy_settings")]
    pub settings: LegacySettings,
    #[serde(default)]
    pub input_path: Option<PathBuf>,
    #[serde(default)]
    pub default_dir: Option<PathBuf>,
    #[serde(default)]
    pub legacy_settings_path: Option<PathBuf>,
    #[serde(default)]
    pub workbench: String,
    #[serde(default)]
    pub outputs: RengraveProjectOutputs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RengraveProjectOutputs {
    #[serde(default)]
    pub gcode_path: Option<PathBuf>,
    #[serde(default)]
    pub svg_path: Option<PathBuf>,
    #[serde(default)]
    pub dxf_path: Option<PathBuf>,
}

impl Default for RengraveProjectFile {
    fn default() -> Self {
        Self {
            format_version: RENGRAVE_PROJECT_FORMAT_VERSION,
            application_version: RENGRAVE_VERSION.to_owned(),
            text: default_project_text(),
            settings: default_legacy_settings(),
            input_path: None,
            default_dir: None,
            legacy_settings_path: None,
            workbench: String::new(),
            outputs: RengraveProjectOutputs::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectFileError {
    #[error("unable to read R-Engrave project `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unable to parse R-Engrave project `{path}`: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(
        "unsupported R-Engrave project version {version}; this build supports up to {supported}"
    )]
    UnsupportedVersion { version: u32, supported: u32 },
    #[error("unable to serialize R-Engrave project: {source}")]
    Serialize { source: serde_json::Error },
    #[error("unable to create R-Engrave project directory `{path}`: {source}")]
    CreateParent {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unable to write R-Engrave project `{path}`: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Default for RengraveDocument {
    fn default() -> Self {
        let text = "R-Engrave".to_owned();
        let mut settings = default_legacy_settings();
        settings.entries.extend(tcode_settings(&text));

        Self {
            input_path: resolve_input_path(&settings),
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

pub fn is_rengrave_project_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(RENGRAVE_PROJECT_EXTENSION))
}

pub fn read_rengrave_project(path: &Path) -> Result<RengraveProjectFile, ProjectFileError> {
    let input = fs::read_to_string(path).map_err(|source| ProjectFileError::Read {
        path: path.to_owned(),
        source,
    })?;
    let project: RengraveProjectFile =
        serde_json::from_str(&input).map_err(|source| ProjectFileError::Parse {
            path: path.to_owned(),
            source,
        })?;
    validate_project_version(project.format_version)?;
    Ok(project)
}

pub fn write_rengrave_project(
    path: &Path,
    project: &RengraveProjectFile,
) -> Result<(), ProjectFileError> {
    validate_project_version(project.format_version)?;
    let contents = serde_json::to_string_pretty(project)
        .map_err(|source| ProjectFileError::Serialize { source })?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| ProjectFileError::CreateParent {
            path: parent.to_owned(),
            source,
        })?;
    }
    fs::write(path, contents).map_err(|source| ProjectFileError::Write {
        path: path.to_owned(),
        source,
    })
}

fn validate_project_version(version: u32) -> Result<(), ProjectFileError> {
    if version <= RENGRAVE_PROJECT_FORMAT_VERSION {
        Ok(())
    } else {
        Err(ProjectFileError::UnsupportedVersion {
            version,
            supported: RENGRAVE_PROJECT_FORMAT_VERSION,
        })
    }
}

fn current_project_format_version() -> u32 {
    RENGRAVE_PROJECT_FORMAT_VERSION
}

fn current_application_version() -> String {
    RENGRAVE_VERSION.to_owned()
}

fn default_project_text() -> String {
    "R-Engrave".to_owned()
}

pub fn resolve_input_kind(settings: &LegacySettings) -> InputKind {
    match settings.get_last("input_type") {
        Some("image") => settings
            .get_last("imagefile")
            .map(|path| InputKind::Image(resolve_image_path(settings, path)))
            .unwrap_or(InputKind::Missing),
        _ => {
            let fontfile = settings.get_last("fontfile").unwrap_or_default().trim();
            if fontfile.is_empty() {
                return InputKind::Missing;
            }
            let font_path = expand_user_path(fontfile);
            let path = if font_path.is_absolute() {
                font_path
            } else {
                expand_user_path(settings.get_last("fontdir").unwrap_or_default()).join(font_path)
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

pub fn resolve_input_path(settings: &LegacySettings) -> Option<PathBuf> {
    match resolve_input_kind(settings) {
        InputKind::CxfFont(path) | InputKind::TtfFont(path) | InputKind::Image(path) => Some(path),
        InputKind::Missing => None,
    }
}

fn resolve_image_path(settings: &LegacySettings, imagefile: &str) -> PathBuf {
    let raw = expand_user_path(imagefile.trim());
    let Some(file_name) = raw.file_name().map(OsStr::to_owned) else {
        return raw;
    };

    let mut candidates = vec![raw.clone(), PathBuf::from(&file_name)];
    if let Some(ngc_dir) = settings.get_last("NGC_DIR").map(str::trim).filter(|value| {
        !value.is_empty() && !value.ends_with("/None") && !value.ends_with("\\None")
    }) {
        candidates.push(expand_user_path(ngc_dir).join(&file_name));
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or(raw)
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn load_document(request: &DocumentRequest) -> Result<RengraveDocument, DocumentError> {
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
    }

    if let Some(default_dir) = &request.default_dir {
        settings.set_or_push("NGC_DIR", default_dir.display().to_string(), true);
    }

    for override_entry in &request.settings_overrides {
        if override_entry.key == "TCODE" {
            continue;
        }
        settings.set_or_push(
            override_entry.key.clone(),
            override_entry.value.clone(),
            override_entry.quoted,
        );
    }

    normalize_bitmap_backend_setting(&mut settings);

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
        .unwrap_or_else(|| "R-Engrave".to_owned());

    settings.entries.retain(|entry| entry.key != "TCODE");
    settings.entries.extend(tcode_settings(&text));

    Ok(RengraveDocument {
        input_path: resolve_input_path(&settings),
        settings,
        text,
        warnings,
    })
}

fn normalize_bitmap_backend_setting(settings: &mut LegacySettings) {
    let backend = BitmapBackend::from_settings(settings);
    settings.set_or_push("bitmap_backend", backend.value(), false);
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
        assert_eq!(document.input_path, Some(PathBuf::from("/tmp/example.dxf")));
    }

    #[test]
    fn settings_overrides_are_applied_after_loaded_settings() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("YSCALE", "2.0", false);
        settings.set_or_push("plotbox", "0", false);
        let input = settings.to_string();
        let path = std::env::temp_dir().join(format!(
            "rengrave-override-settings-{}.ngc",
            std::process::id()
        ));
        fs::write(&path, input).unwrap();

        let document = load_document(&DocumentRequest {
            gcode_file: Some(path.clone()),
            settings_overrides: vec![
                LegacySetting::new("YSCALE", "3.5", false),
                LegacySetting::new("plotbox", "1", false),
            ],
            ..DocumentRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert_eq!(document.settings.get_last("YSCALE"), Some("3.5"));
        assert_eq!(document.settings.get_last("plotbox"), Some("1"));
    }

    #[test]
    fn resolves_stale_imagefile_from_ngc_dir_basename() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ngc-dir-image-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("part.dxf");
        fs::write(&image_path, "0\nSECTION\n0\nENDSEC\n").unwrap();

        let mut settings = default_legacy_settings();
        settings.set_or_push("input_type", "image", false);
        settings.set_or_push("imagefile", "/missing/original/part.dxf", true);
        settings.set_or_push("NGC_DIR", dir.display().to_string(), true);

        assert_eq!(resolve_input_kind(&settings), InputKind::Image(image_path));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_document_exposes_resolved_font_path_from_settings() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-settings-font-path-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let settings_path = dir.join("settings.ngc");
        fs::write(
            &settings_path,
            format!(
                "(fengrave_set input_type  text )\n(fengrave_set fontdir     \"{}\" )\n(fengrave_set fontfile    \"romanc.cxf\" )\n",
                dir.display()
            ),
        )
        .unwrap();

        let document = load_document(&DocumentRequest {
            gcode_file: Some(settings_path),
            ..DocumentRequest::default()
        })
        .unwrap();

        assert_eq!(document.input_path, Some(dir.join("romanc.cxf")));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_document_exposes_resolved_stale_image_path() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-settings-image-path-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let image_path = dir.join("part.dxf");
        fs::write(&image_path, "0\nSECTION\n0\nENDSEC\n").unwrap();
        let settings_path = dir.join("settings.ngc");
        fs::write(
            &settings_path,
            format!(
                "(fengrave_set input_type  image )\n(fengrave_set imagefile   \"/missing/original/part.dxf\" )\n(fengrave_set NGC_DIR     \"{}\" )\n",
                dir.display()
            ),
        )
        .unwrap();

        let document = load_document(&DocumentRequest {
            gcode_file: Some(settings_path),
            ..DocumentRequest::default()
        })
        .unwrap();

        assert_eq!(document.input_path, Some(image_path));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn bitmap_input_defaults_to_native_potrace_without_external_warning() {
        let document = load_document(&DocumentRequest {
            font_or_image: Some(PathBuf::from("/tmp/example.png")),
            ..DocumentRequest::default()
        })
        .unwrap();

        assert_eq!(document.settings.get_last("input_type"), Some("image"));
        assert_eq!(
            document.settings.get_last("bitmap_backend"),
            Some("native-potrace")
        );
        assert!(document.warnings.is_empty());
    }

    #[test]
    fn legacy_vtracer_bitmap_backend_normalizes_to_native_potrace() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-legacy-vtracer-settings-{}.ngc",
            std::process::id()
        ));
        fs::write(
            &path,
            "(fengrave_set input_type  image )\n(fengrave_set bitmap_backend vtracer )\n",
        )
        .unwrap();

        let document = load_document(&DocumentRequest {
            gcode_file: Some(path.clone()),
            font_or_image: Some(PathBuf::from("/tmp/example.png")),
            ..DocumentRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert_eq!(
            document.settings.get_last("bitmap_backend"),
            Some("native-potrace")
        );
    }

    #[test]
    fn legacy_potrace_sidecar_bitmap_backend_normalizes_to_native_potrace() {
        let document = load_document(&DocumentRequest {
            font_or_image: Some(PathBuf::from("/tmp/example.bmp")),
            settings_overrides: vec![LegacySetting::new(
                "bitmap_backend",
                "potrace-sidecar",
                false,
            )],
            ..DocumentRequest::default()
        })
        .unwrap();

        assert!(document.warnings.is_empty());
        assert_eq!(
            document.settings.get_last("bitmap_backend"),
            Some("native-potrace")
        );
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
        assert_eq!(document.text, "R-Engrave");
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

    #[test]
    fn rengrave_project_file_round_trips_versioned_json() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-project-round-trip-{}",
            std::process::id()
        ));
        let path = dir.join("job.rgrv");
        let mut settings = default_legacy_settings();
        settings.set_or_push("units", "mm", false);
        let project = RengraveProjectFile {
            format_version: RENGRAVE_PROJECT_FORMAT_VERSION,
            application_version: "test".to_owned(),
            text: "Saved text".to_owned(),
            settings,
            input_path: Some(PathBuf::from("/tmp/input.ttf")),
            default_dir: Some(PathBuf::from("/tmp")),
            legacy_settings_path: Some(PathBuf::from("/tmp/legacy.ngc")),
            workbench: "text-v-carve".to_owned(),
            outputs: RengraveProjectOutputs {
                gcode_path: Some(PathBuf::from("/tmp/out.ngc")),
                svg_path: Some(PathBuf::from("/tmp/out.svg")),
                dxf_path: Some(PathBuf::from("/tmp/out.dxf")),
            },
        };

        write_rengrave_project(&path, &project).unwrap();
        let loaded = read_rengrave_project(&path).unwrap();

        let _ = fs::remove_dir_all(dir);
        assert_eq!(loaded, project);
        assert!(is_rengrave_project_path(Path::new("job.RGRV")));
        assert!(!is_rengrave_project_path(Path::new("job.ngc")));
    }

    #[test]
    fn rengrave_project_file_loads_minimal_older_shape_with_defaults() {
        let dir = std::env::temp_dir().join(format!("rengrave-project-old-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.rgrv");
        fs::write(&path, r#"{"text":"Old project","settings":{"entries":[]}}"#).unwrap();

        let loaded = read_rengrave_project(&path).unwrap();

        let _ = fs::remove_dir_all(dir);
        assert_eq!(loaded.format_version, RENGRAVE_PROJECT_FORMAT_VERSION);
        assert_eq!(loaded.text, "Old project");
        assert_eq!(loaded.application_version, RENGRAVE_VERSION);
        assert_eq!(loaded.outputs, RengraveProjectOutputs::default());
    }

    #[test]
    fn rengrave_project_file_rejects_future_versions() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-project-future-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("future.rgrv");
        fs::write(&path, r#"{"format_version":999,"settings":{"entries":[]}}"#).unwrap();

        let err = read_rengrave_project(&path).unwrap_err();

        let _ = fs::remove_dir_all(dir);
        assert!(matches!(
            err,
            ProjectFileError::UnsupportedVersion {
                version: 999,
                supported: RENGRAVE_PROJECT_FORMAT_VERSION
            }
        ));
    }
}
