//! Persistent UI preferences and platform configuration paths.
//!
//! This module owns the on-disk UI state format (a small `key=value` file),
//! the helpers that locate the per-user config directory, and the egui theme.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UiPreferences {
    pub(crate) settings_path: String,
    pub(crate) input_path: String,
    pub(crate) default_dir_path: String,
    pub(crate) gcode_path: String,
    pub(crate) svg_path: String,
    pub(crate) dxf_path: String,
    pub(crate) show_rapids: bool,
    pub(crate) show_grid: bool,
    pub(crate) show_cleanup: bool,
    pub(crate) show_tabs: bool,
    pub(crate) viewport_rotation_degrees: f64,
    pub(crate) auto_recalculate: bool,
    pub(crate) show_input_overlay: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            settings_path: String::new(),
            input_path: String::new(),
            default_dir_path: String::new(),
            gcode_path: String::new(),
            svg_path: String::new(),
            dxf_path: String::new(),
            show_rapids: true,
            show_grid: true,
            show_cleanup: true,
            show_tabs: true,
            viewport_rotation_degrees: 0.0,
            auto_recalculate: false,
            show_input_overlay: true,
        }
    }
}

impl UiPreferences {
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let input = fs::read_to_string(path)
            .map_err(|err| format!("unable to read `{}`: {err}", path.display()))?;
        Ok(Self::parse(&input))
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("unable to create `{}`: {err}", parent.display()))?;
        }
        fs::write(path, self.to_text())
            .map_err(|err| format!("unable to write `{}`: {err}", path.display()))
    }

    pub(crate) fn parse(input: &str) -> Self {
        let mut preferences = Self::default();
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = unescape_pref_value(value);
            match key {
                "settings_path" => preferences.settings_path = value,
                "input_path" => preferences.input_path = value,
                "default_dir_path" => preferences.default_dir_path = value,
                "gcode_path" => preferences.gcode_path = value,
                "svg_path" => preferences.svg_path = value,
                "dxf_path" => preferences.dxf_path = value,
                "show_rapids" => preferences.show_rapids = value != "0" && value != "false",
                "show_grid" => preferences.show_grid = value != "0" && value != "false",
                "show_cleanup" => preferences.show_cleanup = value != "0" && value != "false",
                "show_tabs" => preferences.show_tabs = value != "0" && value != "false",
                "viewport_rotation_degrees" => {
                    if let Ok(rotation) = value.parse::<f64>() {
                        preferences.viewport_rotation_degrees = rotation.clamp(-180.0, 180.0);
                    }
                }
                "auto_recalculate" => {
                    preferences.auto_recalculate = value != "0" && value != "false"
                }
                "show_input_overlay" => {
                    preferences.show_input_overlay = value != "0" && value != "false"
                }
                _ => {}
            }
        }
        preferences
    }

    pub(crate) fn to_text(&self) -> String {
        let viewport_rotation_degrees = format_setting_number(self.viewport_rotation_degrees);
        [
            ("settings_path", self.settings_path.as_str()),
            ("input_path", self.input_path.as_str()),
            ("default_dir_path", self.default_dir_path.as_str()),
            ("gcode_path", self.gcode_path.as_str()),
            ("svg_path", self.svg_path.as_str()),
            ("dxf_path", self.dxf_path.as_str()),
            ("show_rapids", if self.show_rapids { "1" } else { "0" }),
            ("show_grid", if self.show_grid { "1" } else { "0" }),
            ("show_cleanup", if self.show_cleanup { "1" } else { "0" }),
            ("show_tabs", if self.show_tabs { "1" } else { "0" }),
            (
                "viewport_rotation_degrees",
                viewport_rotation_degrees.as_str(),
            ),
            (
                "auto_recalculate",
                if self.auto_recalculate { "1" } else { "0" },
            ),
            (
                "show_input_overlay",
                if self.show_input_overlay { "1" } else { "0" },
            ),
        ]
        .into_iter()
        .map(|(key, value)| format!("{key}={}", escape_pref_value(value)))
        .collect::<Vec<_>>()
        .join("\n")
            + "\n"
    }
}

pub(crate) fn default_preferences_path() -> Option<PathBuf> {
    config_base_dir().map(|dir| dir.join("rengrave").join("ui-state.conf"))
}

pub(crate) fn config_base_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        user_home_dir().map(|home| home.join("Library").join("Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| user_home_dir().map(|home| home.join(".config")))
    }
}

pub(crate) fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(crate) fn escape_pref_value(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub(crate) fn unescape_pref_value(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => output.push('\\'),
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

pub(crate) fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(36, 39, 42);
    visuals.window_fill = egui::Color32::from_rgb(42, 45, 48);
    visuals.selection.bg_fill = egui::Color32::from_rgb(54, 115, 141);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(93, 127, 143);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(62, 72, 78);
    ctx.set_visuals(visuals);
}
