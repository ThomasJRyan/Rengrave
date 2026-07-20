use std::fmt;

use serde::{Deserialize, Serialize};

pub const DEFAULT_GCODE_PREAMBLE: &str = "G17 M3 S3000";
pub const DEFAULT_GCODE_POSTAMBLE: &str = "M5|M2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySetting {
    pub key: String,
    pub value: String,
    pub quoted: bool,
}

impl LegacySetting {
    pub fn new(key: impl Into<String>, value: impl Into<String>, quoted: bool) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            quoted,
        }
    }

    pub fn to_comment(&self) -> String {
        if self.quoted {
            format!(
                "(fengrave_set {:<11} \"{}\" )",
                self.key,
                self.value.replace('"', "\\\"")
            )
        } else {
            format!("(fengrave_set {:<11} {} )", self.key, self.value)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySettings {
    pub entries: Vec<LegacySetting>,
}

impl LegacySettings {
    pub fn parse(input: &str) -> Self {
        let entries = input.lines().filter_map(parse_legacy_line).collect();
        Self { entries }
    }

    pub fn get_last(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.as_str())
    }

    pub fn set_or_push(&mut self, key: impl Into<String>, value: impl Into<String>, quoted: bool) {
        let key = key.into();
        let value = value.into();
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
            entry.value = value;
            entry.quoted = quoted;
        } else {
            self.entries.push(LegacySetting::new(key, value, quoted));
        }
    }

    pub fn text_from_tcode(&self) -> Result<Option<String>, LegacySettingsError> {
        let mut text = String::new();
        let mut found = false;

        for entry in self.entries.iter().filter(|entry| entry.key == "TCODE") {
            found = true;
            for token in entry.value.split_whitespace() {
                let code =
                    token
                        .parse::<u32>()
                        .map_err(|_| LegacySettingsError::InvalidTextCode {
                            token: token.to_owned(),
                        })?;
                let ch = char::from_u32(code)
                    .ok_or(LegacySettingsError::InvalidUnicodeScalar { code })?;
                text.push(ch);
            }
        }

        Ok(found.then_some(text))
    }

    pub fn to_comments(&self) -> Vec<String> {
        self.entries.iter().map(LegacySetting::to_comment).collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LegacySettingsError {
    #[error("invalid TCODE token `{token}`")]
    InvalidTextCode { token: String },
    #[error("invalid Unicode scalar in TCODE: {code}")]
    InvalidUnicodeScalar { code: u32 },
}

pub fn default_legacy_settings() -> LegacySettings {
    let mut settings = LegacySettings::default();

    for (key, value) in [
        ("show_axis", "1"),
        ("show_box", "1"),
        ("show_thick", "1"),
        ("flip", "0"),
        ("mirror", "0"),
        ("outer", "1"),
        ("upper", "1"),
        ("v_flop", "0"),
        ("v_pplot", "0"),
        ("inlay", "0"),
        ("bmp_long", "1"),
        ("var_dis", "1"),
        ("ext_char", "0"),
        ("useIMGsize", "0"),
        ("no_comments", "0"),
        ("plotbox", "0"),
        ("show_v_path", "1"),
        ("show_v_area", "1"),
        ("arc_fit", "none"),
        ("YSCALE", "2.0"),
        ("XSCALE", "100"),
        ("LSPACE", "1.1"),
        ("CSPACE", "25"),
        ("WSPACE", "100"),
        ("TANGLE", "0.0"),
        ("TRADIUS", "0.0"),
        ("ZSAFE", "0.25"),
        ("ZCUT", "-0.005"),
        ("STHICK", "0.01"),
        ("origin", "Default"),
        ("justify", "Left"),
        ("units", "in"),
        ("xorigin", "0.0"),
        ("yorigin", "0.0"),
        ("segarc", "5.0"),
        ("accuracy", "0.001"),
        ("FEED", "5.0"),
        ("PLUNGE", "0.0"),
        ("H_CALC", "max_use"),
        ("boxgap", "0.25"),
        ("cut_type", "engrave"),
        ("bit_shape", "VBIT"),
        ("v_bit_angle", "60"),
        ("v_bit_dia", "0.5"),
        ("v_drv_crner", "135"),
        ("v_stp_crner", "200"),
        ("v_step_len", "0.01"),
        ("allowance", "0.0"),
        ("v_max_cut", "-1.0"),
        ("v_rough_stk", "0.0"),
        ("v_depth_lim", "0.0"),
        ("v_check_all", "all"),
        ("bmp_turnp", "minority"),
        ("bmp_turds", "2"),
        ("bmp_alpha", "1"),
        ("bmp_optto", "0.2"),
        ("bitmap_backend", "native-potrace"),
        ("profile_cut", "0"),
        ("profile_margin", "0.25"),
        ("profile_radius", "0.0"),
        ("profile_depth", "0.125"),
        ("profile_steps", "1"),
        ("profile_endmill_dia", "0.25"),
        ("profile_tabs", "0"),
        ("profile_tab_height", "0.0393701"),
        ("profile_tab_width", "0"),
        ("profile_chamfer", "0"),
        ("profile_chamfer_depth", "0.02"),
        ("profile_chamfer_angle", "60"),
        ("profile_width", "0"),
        ("profile_height", "0"),
        ("profile_aspect", "0"),
        ("profile_trace", "0"),
        ("profile_align", "Mid-Center"),
        ("gpre", DEFAULT_GCODE_PREAMBLE),
        ("gpost", DEFAULT_GCODE_POSTAMBLE),
        ("return_to_origin", "1"),
        ("input_type", "text"),
        ("clean_dia", ".25"),
        ("clean_dias", ".25"),
        ("clean_step", "50"),
        ("clean_v", "0.05"),
        ("clean_paths", "1,1,0,1,0,1,0,0"),
    ] {
        settings.entries.push(LegacySetting::new(key, value, false));
    }

    for (key, value) in [
        ("fontfile", " "),
        ("fontdir", "fonts"),
        ("imagefile", "~/None"),
        ("NGC_DIR", "~"),
    ] {
        settings.entries.push(LegacySetting::new(key, value, true));
    }

    settings
}

pub fn tcode_settings(text: &str) -> Vec<LegacySetting> {
    let mut lines = Vec::new();
    let mut chunk = Vec::new();
    let mut saw_text = false;

    for ch in text.chars() {
        saw_text = true;
        chunk.push(format!("{:03}", ch as u32));
        if chunk.len() > 10 {
            lines.push(LegacySetting::new("TCODE", chunk.join(" "), false));
            chunk.clear();
        }
    }

    if !chunk.is_empty() || !saw_text {
        lines.push(LegacySetting::new("TCODE", chunk.join(" "), false));
    }
    lines
}

fn parse_legacy_line(line: &str) -> Option<LegacySetting> {
    let ident_at = line.find("fengrave_set")?;
    let rest = line[ident_at + "fengrave_set".len()..].trim();
    let (key, value) = split_key_value(rest)?;
    let value = value.strip_suffix(')').unwrap_or(value).trim();

    if let Some(stripped) = value.strip_prefix('"') {
        let end = stripped.find('"').unwrap_or(stripped.len());
        Some(LegacySetting::new(key, &stripped[..end], true))
    } else {
        Some(LegacySetting::new(key, value, false))
    }
}

fn split_key_value(input: &str) -> Option<(&str, &str)> {
    let mut key_end = None;
    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() {
            key_end = Some(idx);
            break;
        }
    }

    let key_end = key_end?;
    let key = &input[..key_end];
    let value = input[key_end..].trim();
    Some((key, value))
}

impl fmt::Display for LegacySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in self.to_comments() {
            writeln!(f, "{line}")?;
        }
        Ok(())
    }
}

pub fn get_legacy_bool(settings: &LegacySettings, key: &str, default: bool) -> bool {
    settings
        .get_last(key)
        .map(legacy_bool_value)
        .unwrap_or(default)
}

pub fn legacy_bool_value(value: &str) -> bool {
    let value = value.trim();
    matches!(value, "1" | "1.0")
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("yes")
        || value.eq_ignore_ascii_case("on")
        || value.eq_ignore_ascii_case("box")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_and_bare_settings() {
        let parsed = LegacySettings::parse(
            r#"
(fengrave_set fontfile   "romanc.cxf" )
(fengrave_set units      in )
(fengrave_set gpre        G17 G64 P0.001 M3 S3000 )
"#,
        );

        assert_eq!(parsed.get_last("fontfile"), Some("romanc.cxf"));
        assert_eq!(parsed.get_last("units"), Some("in"));
        assert_eq!(parsed.get_last("gpre"), Some("G17 G64 P0.001 M3 S3000"));
    }

    #[test]
    fn reconstructs_text_from_legacy_tcode_chunks() {
        let parsed = LegacySettings::parse(
            r#"
(fengrave_set TCODE    070 045 069 110 103 114 097 118 101 010  )
(fengrave_set TCODE    082 117 115 116 )
"#,
        );

        assert_eq!(
            parsed.text_from_tcode().unwrap(),
            Some("F-Engrave\nRust".to_owned())
        );
    }

    #[test]
    fn tcode_chunks_do_not_add_empty_trailing_line_for_exact_chunks() {
        let chunks = tcode_settings("12345678901");

        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].value,
            "049 050 051 052 053 054 055 056 057 048 049"
        );
    }

    #[test]
    fn emits_all_default_recovery_keys_once() {
        let settings = default_legacy_settings();
        assert_eq!(settings.get_last("cut_type"), Some("engrave"));
        assert_eq!(settings.get_last("bit_shape"), Some("VBIT"));
        assert_eq!(settings.get_last("clean_paths"), Some("1,1,0,1,0,1,0,0"));
        assert_eq!(settings.get_last("no_comments"), Some("0"));
        assert!(
            settings
                .to_string()
                .contains("(fengrave_set fontdir     \"fonts\" )")
        );
    }

    #[test]
    fn parses_legacy_bool_aliases() {
        assert!(legacy_bool_value("1"));
        assert!(legacy_bool_value("True"));
        assert!(legacy_bool_value("box"));
        assert!(!legacy_bool_value("0"));
        assert!(!legacy_bool_value("False"));
        assert!(!legacy_bool_value("no_box"));
    }
}
