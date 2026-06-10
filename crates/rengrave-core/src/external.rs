use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PotraceStatus {
    pub available: bool,
    pub version: Option<String>,
    pub message: String,
}

impl PotraceStatus {
    pub fn found(version: Option<String>) -> Self {
        let message = version
            .as_ref()
            .map(|version| format!("Potrace detected: {version}"))
            .unwrap_or_else(|| "Potrace detected".to_owned());
        Self {
            available: true,
            version,
            message,
        }
    }

    pub fn missing(message: impl Into<String>) -> Self {
        Self {
            available: false,
            version: None,
            message: message.into(),
        }
    }
}

pub fn detect_potrace() -> PotraceStatus {
    match Command::new("potrace").arg("-v").output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}\n{stderr}");
            PotraceStatus::found(parse_potrace_version(&combined))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            PotraceStatus::missing(format!(
                "Potrace is present but failed version check: {}",
                stderr.trim()
            ))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => PotraceStatus::missing(
            "Potrace is required for bitmap vectorization but was not found in PATH",
        ),
        Err(err) => PotraceStatus::missing(format!("Unable to run Potrace version check: {err}")),
    }
}

pub fn parse_potrace_version(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.to_ascii_lowercase().starts_with("potrace"))
        .map(str::to_owned)
}

pub fn requires_potrace(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("bmp" | "jpg" | "jpeg" | "png" | "tif" | "tiff" | "pbm" | "ppm" | "pgm" | "pnm")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_potrace_version_from_stdout_or_stderr() {
        assert_eq!(
            parse_potrace_version("potrace 1.16. Copyright 2001-2019 Peter Selinger"),
            Some("potrace 1.16. Copyright 2001-2019 Peter Selinger".to_owned())
        );
        assert_eq!(parse_potrace_version("other output"), None);
    }

    #[test]
    fn recognizes_bitmap_inputs_that_need_potrace() {
        assert!(requires_potrace(Path::new("input.png")));
        assert!(requires_potrace(Path::new("input.jpeg")));
        assert!(requires_potrace(Path::new("input.PBM")));
        assert!(!requires_potrace(Path::new("input.dxf")));
        assert!(!requires_potrace(Path::new("input.cxf")));
    }
}
