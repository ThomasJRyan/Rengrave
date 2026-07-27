//! Persistent toolbit definitions and the compatibility bridge to legacy settings.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::LegacySettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRole {
    Primary,
    Cleanup,
    ProfileEndmill,
    ProfileChamfer,
    VBitCleanup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Toolbit {
    pub id: String,
    pub name: String,
    /// A string keeps future tool types loadable without rejecting the library.
    pub kind: String,
    pub diameter_mm: f64,
    #[serde(default)]
    pub angle_deg: Option<f64>,
    #[serde(default)]
    pub corner_radius_mm: Option<f64>,
    #[serde(default)]
    pub feed_mm_min: Option<f64>,
    #[serde(default)]
    pub plunge_mm_min: Option<f64>,
}

impl Default for Toolbit {
    fn default() -> Self {
        Self {
            id: "toolbit-1".into(),
            name: "New tool".into(),
            kind: "straight_endmill".into(),
            diameter_mm: 0.0,
            angle_deg: None,
            corner_radius_mm: None,
            feed_mm_min: None,
            plunge_mm_min: None,
        }
    }
}

impl Toolbit {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.name.trim().is_empty() {
            errors.push("Name is required".into());
        }
        if !self.diameter_mm.is_finite() || self.diameter_mm <= 0.0 {
            errors.push("Diameter must be greater than 0 mm".into());
        }
        match self.kind.as_str() {
            "v_bit" => match self.angle_deg {
                Some(angle) if angle > 0.0 && angle < 180.0 => {}
                _ => errors.push("V-bit angle must be between 0 and 180 degrees".into()),
            },
            "bullnose" => {
                if self.corner_radius_mm.unwrap_or(0.0) <= 0.0 {
                    errors.push("Bullnose corner radius must be greater than 0 mm".into());
                }
            }
            _ => {}
        }
        for (label, value) in [("Feed", self.feed_mm_min), ("Plunge", self.plunge_mm_min)] {
            if let Some(value) = value
                && (!value.is_finite() || value < 0.0)
            {
                errors.push(format!("{label} must not be negative"));
            }
        }
        errors
    }

    pub fn eligible_for(&self, role: ToolRole) -> bool {
        match role {
            ToolRole::Primary | ToolRole::VBitCleanup => {
                self.kind == "v_bit" || self.kind == "straight_endmill"
            }
            ToolRole::Cleanup | ToolRole::ProfileEndmill => self.kind == "straight_endmill",
            ToolRole::ProfileChamfer => self.kind == "v_bit",
        }
    }

    pub fn apply_to_settings(
        &self,
        settings: &mut LegacySettings,
        role: ToolRole,
        units_inch: bool,
    ) {
        let factor = if units_inch { 1.0 / 25.4 } else { 1.0 };
        let number = |value: f64| format_setting_number(value * factor);
        match role {
            ToolRole::Primary | ToolRole::VBitCleanup => {
                if self.kind == "v_bit" {
                    settings.set_or_push("bit_shape", "VBIT", false);
                    settings.set_or_push("v_bit_dia", number(self.diameter_mm), false);
                    if let Some(angle) = self.angle_deg {
                        settings.set_or_push("v_bit_angle", format_setting_number(angle), false);
                    }
                } else if self.kind == "straight_endmill" {
                    settings.set_or_push("bit_shape", "ENDMILL", false);
                    settings.set_or_push("v_bit_dia", number(self.diameter_mm), false);
                }
            }
            ToolRole::Cleanup => {
                settings.set_or_push("clean_dias", number(self.diameter_mm), false);
            }
            ToolRole::ProfileEndmill => {
                settings.set_or_push("profile_endmill_dia", number(self.diameter_mm), false)
            }
            ToolRole::ProfileChamfer => {
                settings.set_or_push("profile_chamfer", "1", false);
                settings.set_or_push(
                    "profile_chamfer_angle",
                    format_setting_number(self.angle_deg.unwrap_or(60.0)),
                    false,
                );
            }
        }
        if let Some(feed) = self.feed_mm_min {
            settings.set_or_push("FEED", number(feed), false);
        }
        if let Some(plunge) = self.plunge_mm_min {
            settings.set_or_push("PLUNGE", number(plunge), false);
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolbitLibrary {
    #[serde(default)]
    pub toolbits: Vec<Toolbit>,
}

impl ToolbitLibrary {
    pub fn load(path: &Path) -> Result<Self, String> {
        let input = fs::read_to_string(path)
            .map_err(|e| format!("unable to read toolbit library `{}`: {e}", path.display()))?;
        serde_json::from_str(&input)
            .map_err(|e| format!("unable to parse toolbit library `{}`: {e}", path.display()))
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("unable to create `{}`: {e}", parent.display()))?;
        }
        let output = serde_json::to_string_pretty(self)
            .map_err(|e| format!("unable to serialize toolbit library: {e}"))?;
        fs::write(path, output)
            .map_err(|e| format!("unable to write toolbit library `{}`: {e}", path.display()))
    }
    pub fn eligible(&self, role: ToolRole) -> impl Iterator<Item = &Toolbit> {
        self.toolbits
            .iter()
            .filter(move |tool| tool.eligible_for(role))
    }
}

pub fn default_library_path() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|path| path.join("rengrave").join("toolbits.json"))
}

fn format_setting_number(value: f64) -> String {
    let mut output = format!("{value:.8}");
    while output.contains('.') && output.ends_with('0') {
        output.pop();
    }
    if output.ends_with('.') {
        output.pop();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_known_geometry_and_preserves_unknown_kind() {
        let mut tool = Toolbit {
            kind: "v_bit".into(),
            diameter_mm: 3.175,
            ..Toolbit::default()
        };
        assert!(!tool.validate().is_empty());
        tool.angle_deg = Some(60.0);
        assert!(tool.validate().is_empty());
        let json = serde_json::to_string(&Toolbit {
            kind: "laser_future".into(),
            ..tool
        })
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Toolbit>(&json).unwrap().kind,
            "laser_future"
        );
    }

    #[test]
    fn applies_canonical_mm_as_active_inch_setting() {
        let tool = Toolbit {
            diameter_mm: 25.4,
            kind: "straight_endmill".into(),
            ..Toolbit::default()
        };
        let mut settings = LegacySettings::default();
        tool.apply_to_settings(&mut settings, ToolRole::ProfileEndmill, true);
        assert_eq!(settings.get_last("profile_endmill_dia"), Some("1"));
    }

    #[test]
    fn role_filter_rejects_bullnose_operations() {
        let tool = Toolbit {
            kind: "bullnose".into(),
            diameter_mm: 3.0,
            corner_radius_mm: Some(1.0),
            ..Toolbit::default()
        };
        assert!(!tool.eligible_for(ToolRole::Primary));
        assert!(!tool.eligible_for(ToolRole::ProfileEndmill));
    }

    #[test]
    fn library_round_trips_and_reports_malformed_json() {
        let path =
            std::env::temp_dir().join(format!("rengrave-toolbits-{}.json", std::process::id()));
        let library = ToolbitLibrary {
            toolbits: vec![Toolbit {
                name: "Router".into(),
                diameter_mm: 3.175,
                ..Toolbit::default()
            }],
        };
        library.save(&path).unwrap();
        assert_eq!(ToolbitLibrary::load(&path).unwrap(), library);
        fs::write(&path, "{ definitely not json").unwrap();
        assert!(ToolbitLibrary::load(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
