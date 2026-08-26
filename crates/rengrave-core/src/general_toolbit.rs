//! Toolbits owned by the General workbench.
//!
//! This model is deliberately separate from the legacy machining-workbench
//! toolbit compatibility bridge. It describes the physical cutter library
//! that the General workbench can assign to future CAM operations.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const GENERAL_TOOLBIT_LIBRARY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralToolbitKind {
    Endmill,
    Ballnose,
    Bullnose,
    VBit,
    Chamfer,
    Drill,
    SlittingSaw,
    Probe,
}

impl GeneralToolbitKind {
    pub const ALL: [Self; 8] = [
        Self::Endmill,
        Self::Ballnose,
        Self::Bullnose,
        Self::VBit,
        Self::Chamfer,
        Self::Drill,
        Self::SlittingSaw,
        Self::Probe,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Endmill => "Endmill",
            Self::Ballnose => "Ballnose",
            Self::Bullnose => "Bullnose",
            Self::VBit => "V-bit",
            Self::Chamfer => "Chamfer",
            Self::Drill => "Drill",
            Self::SlittingSaw => "Slitting saw",
            Self::Probe => "Probe",
        }
    }

    pub const fn asset_name(self) -> &'static str {
        match self {
            Self::Endmill => "endmill",
            Self::Ballnose => "ballend",
            Self::Bullnose => "bullnose",
            Self::VBit => "v-bit",
            Self::Chamfer => "chamfer",
            Self::Drill => "drill",
            Self::SlittingSaw => "slittingsaw",
            Self::Probe => "probe",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralSpindleDirection {
    Forward,
    Reverse,
}

impl GeneralSpindleDirection {
    pub const ALL: [Self; 2] = [Self::Forward, Self::Reverse];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Forward => "Forward",
            Self::Reverse => "Reverse",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneralToolbit {
    pub id: String,
    pub label: String,
    pub kind: GeneralToolbitKind,
    pub tool_number: u32,
    pub spindle_direction: GeneralSpindleDirection,
    pub material: String,
    pub cutting_edge_height_mm: f64,
    pub diameter_mm: f64,
    pub flutes: u32,
    pub length_mm: f64,
    pub shank_diameter_mm: f64,
    pub chipload_mm: f64,
    pub feed_mm_min: f64,
    pub plunge_mm_min: f64,
    #[serde(default)]
    pub v_angle_deg: Option<f64>,
    #[serde(default)]
    pub tip_diameter_mm: Option<f64>,
    #[serde(default)]
    pub corner_radius_mm: Option<f64>,
    #[serde(default)]
    pub point_angle_deg: Option<f64>,
    #[serde(default)]
    pub chamfer_angle_deg: Option<f64>,
    #[serde(default)]
    pub saw_thickness_mm: Option<f64>,
}

impl Default for GeneralToolbit {
    fn default() -> Self {
        Self {
            id: "general-toolbit-1".to_owned(),
            label: "New toolbit".to_owned(),
            kind: GeneralToolbitKind::Endmill,
            tool_number: 1,
            spindle_direction: GeneralSpindleDirection::Forward,
            material: "Carbide".to_owned(),
            cutting_edge_height_mm: 0.0,
            diameter_mm: 0.0,
            flutes: 0,
            length_mm: 0.0,
            shank_diameter_mm: 0.0,
            chipload_mm: 0.0,
            feed_mm_min: 0.0,
            plunge_mm_min: 0.0,
            v_angle_deg: None,
            tip_diameter_mm: None,
            corner_radius_mm: None,
            point_angle_deg: None,
            chamfer_angle_deg: None,
            saw_thickness_mm: None,
        }
    }
}

impl GeneralToolbit {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.label.trim().is_empty() {
            errors.push("Label is required".to_owned());
        }
        if self.tool_number == 0 {
            errors.push("Tool number must be greater than 0".to_owned());
        }
        for (label, value) in [
            ("Cutting edge height", self.cutting_edge_height_mm),
            ("Diameter", self.diameter_mm),
            ("Length", self.length_mm),
            ("Shank diameter", self.shank_diameter_mm),
        ] {
            if !value.is_finite() || value <= 0.0 {
                errors.push(format!("{label} must be greater than 0 mm"));
            }
        }
        if self.flutes == 0 && self.kind != GeneralToolbitKind::Probe {
            errors.push("Flutes must be greater than 0".to_owned());
        }
        for (label, value) in [
            ("Chipload", self.chipload_mm),
            ("Feed", self.feed_mm_min),
            ("Plunge", self.plunge_mm_min),
        ] {
            if !value.is_finite() || value < 0.0 {
                errors.push(format!("{label} must not be negative"));
            }
        }
        match self.kind {
            GeneralToolbitKind::VBit => {
                if !valid_optional_range(self.v_angle_deg, 0.0, 180.0) {
                    errors.push("V-bit angle must be between 0 and 180 degrees".to_owned());
                }
                if !valid_positive_optional(self.tip_diameter_mm) {
                    errors.push("V-bit tip diameter must be greater than 0 mm".to_owned());
                }
            }
            GeneralToolbitKind::Bullnose => {
                if !valid_positive_optional(self.corner_radius_mm) {
                    errors.push("Corner radius must be greater than 0 mm".to_owned());
                }
            }
            GeneralToolbitKind::Drill => {
                if !valid_optional_range(self.point_angle_deg, 0.0, 180.0) {
                    errors.push("Point angle must be between 0 and 180 degrees".to_owned());
                }
            }
            GeneralToolbitKind::Chamfer => {
                if !valid_optional_range(self.chamfer_angle_deg, 0.0, 180.0) {
                    errors.push("Chamfer angle must be between 0 and 180 degrees".to_owned());
                }
            }
            GeneralToolbitKind::SlittingSaw => {
                if !valid_positive_optional(self.saw_thickness_mm) {
                    errors.push("Saw thickness must be greater than 0 mm".to_owned());
                }
            }
            _ => {}
        }
        errors
    }
}

fn valid_positive_optional(value: Option<f64>) -> bool {
    value.is_some_and(|value| value.is_finite() && value > 0.0)
}

fn valid_optional_range(value: Option<f64>, min: f64, max: f64) -> bool {
    value.is_some_and(|value| value.is_finite() && value > min && value < max)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneralToolbitLibrary {
    #[serde(default = "default_library_version")]
    pub format_version: u32,
    #[serde(default)]
    pub toolbits: Vec<GeneralToolbit>,
}

fn default_library_version() -> u32 {
    GENERAL_TOOLBIT_LIBRARY_VERSION
}

impl Default for GeneralToolbitLibrary {
    fn default() -> Self {
        Self {
            format_version: GENERAL_TOOLBIT_LIBRARY_VERSION,
            toolbits: Vec::new(),
        }
    }
}

impl GeneralToolbitLibrary {
    pub fn load(path: &Path) -> Result<Self, String> {
        let input = fs::read_to_string(path).map_err(|error| {
            format!(
                "Unable to read toolbit library `{}`: {error}",
                path.display()
            )
        })?;
        let library: Self = serde_json::from_str(&input).map_err(|error| {
            format!(
                "Unable to parse toolbit library `{}`: {error}",
                path.display()
            )
        })?;
        if library.format_version > GENERAL_TOOLBIT_LIBRARY_VERSION {
            return Err(format!(
                "Toolbit library format {} is newer than supported format {}",
                library.format_version, GENERAL_TOOLBIT_LIBRARY_VERSION
            ));
        }
        Ok(library)
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Unable to create `{}`: {error}", parent.display()))?;
        }
        let output = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Unable to serialize toolbit library: {error}"))?;
        let temporary = temporary_path(path);
        fs::write(&temporary, output)
            .map_err(|error| format!("Unable to write `{}`: {error}", temporary.display()))?;
        fs::rename(&temporary, path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            format!(
                "Unable to replace toolbit library `{}`: {error}",
                path.display()
            )
        })
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.to_owned();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("json");
    temporary.set_extension(format!("{extension}.tmp"));
    temporary
}

pub fn default_general_toolbit_library_path() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    base.map(|path| path.join("rengrave").join("general_toolbits.json"))
}

pub fn general_toolbit_presets() -> Vec<GeneralToolbit> {
    [
        (
            "3.175mm Endmill",
            GeneralToolbitKind::Endmill,
            3.175,
            25.0,
            45.0,
            4,
        ),
        (
            "5mm Endmill",
            GeneralToolbitKind::Endmill,
            5.0,
            30.0,
            50.0,
            4,
        ),
        ("5mm Drill", GeneralToolbitKind::Drill, 5.0, 30.0, 50.0, 2),
        (
            "6mm Ball End",
            GeneralToolbitKind::Ballnose,
            6.0,
            40.0,
            60.0,
            2,
        ),
        (
            "6mm Bullnose",
            GeneralToolbitKind::Bullnose,
            6.0,
            40.0,
            60.0,
            2,
        ),
        (
            "30 Deg. V-Bit",
            GeneralToolbitKind::VBit,
            10.0,
            10.0,
            45.0,
            2,
        ),
        (
            "45 Deg. V-Bit",
            GeneralToolbitKind::VBit,
            10.0,
            10.0,
            45.0,
            2,
        ),
        (
            "60 Deg. V-Bit",
            GeneralToolbitKind::VBit,
            10.0,
            10.0,
            45.0,
            2,
        ),
        (
            "90 Deg. V-Bit",
            GeneralToolbitKind::VBit,
            25.4,
            10.0,
            50.0,
            2,
        ),
        (
            "45 Deg. Chamfer",
            GeneralToolbitKind::Chamfer,
            12.33,
            25.0,
            50.0,
            2,
        ),
        (
            "Slitting Saw",
            GeneralToolbitKind::SlittingSaw,
            76.2,
            3.0,
            20.0,
            2,
        ),
        ("Probe", GeneralToolbitKind::Probe, 6.0, 50.0, 60.0, 0),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, kind, diameter, edge, length, flutes))| {
        let mut tool = GeneralToolbit {
            id: format!("preset-{}", index + 1),
            label: label.to_owned(),
            kind,
            tool_number: index as u32 + 1,
            cutting_edge_height_mm: edge,
            diameter_mm: diameter,
            flutes,
            length_mm: length,
            shank_diameter_mm: diameter,
            chipload_mm: 0.0,
            feed_mm_min: 0.0,
            plunge_mm_min: 0.0,
            ..GeneralToolbit::default()
        };
        match kind {
            GeneralToolbitKind::VBit => {
                tool.v_angle_deg = Some(
                    label
                        .split_whitespace()
                        .next()
                        .unwrap_or("60")
                        .parse()
                        .unwrap_or(60.0),
                );
                tool.tip_diameter_mm = Some(0.1);
            }
            GeneralToolbitKind::Bullnose => tool.corner_radius_mm = Some(1.5),
            GeneralToolbitKind::Drill => tool.point_angle_deg = Some(119.0),
            GeneralToolbitKind::Chamfer => tool.chamfer_angle_deg = Some(45.0),
            GeneralToolbitKind::SlittingSaw => tool.saw_thickness_mm = Some(3.0),
            _ => {}
        }
        tool
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_cover_the_freecad_common_catalog() {
        let presets = general_toolbit_presets();
        assert_eq!(presets.len(), 12);
        assert!(
            presets
                .iter()
                .any(|tool| tool.kind == GeneralToolbitKind::VBit)
        );
        assert!(presets.iter().all(|tool| tool.validate().is_empty()));
    }

    #[test]
    fn known_shape_validation_requires_shape_parameters() {
        let mut tool = GeneralToolbit {
            diameter_mm: 3.0,
            cutting_edge_height_mm: 10.0,
            length_mm: 20.0,
            shank_diameter_mm: 3.0,
            flutes: 2,
            kind: GeneralToolbitKind::VBit,
            ..GeneralToolbit::default()
        };
        assert!(
            tool.validate()
                .iter()
                .any(|error| error.contains("V-bit angle"))
        );
        tool.v_angle_deg = Some(60.0);
        tool.tip_diameter_mm = Some(0.1);
        assert!(tool.validate().is_empty());
    }

    #[test]
    fn library_round_trips_and_rejects_newer_versions() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-general-toolbits-{}.json",
            std::process::id()
        ));
        let library = GeneralToolbitLibrary {
            toolbits: general_toolbit_presets(),
            ..GeneralToolbitLibrary::default()
        };
        library.save_atomic(&path).unwrap();
        assert_eq!(GeneralToolbitLibrary::load(&path).unwrap(), library);
        let newer = serde_json::json!({"format_version": 999, "toolbits": []});
        fs::write(&path, newer.to_string()).unwrap();
        assert!(GeneralToolbitLibrary::load(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
