//! Reusable egui form widgets and small settings helpers: labelled rows
//! (path/number/text/combo), layout helpers, menu buttons, and the
//! `LegacySettings` read/write helpers used when serializing controls.

use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PathRowAction {
    pub(crate) browse_clicked: bool,
    pub(crate) value_changed: bool,
}

pub(crate) fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> PathRowAction {
    let mut action = PathRowAction::default();
    ui.horizontal(|ui| {
        row_label(ui, label, 88.0);
        right_aligned_group(ui, PATH_CONTROL_WIDTH, |ui| {
            let text_width = (ui.available_width() - 74.0).max(80.0);
            action.value_changed = ui
                .add_sized(
                    [text_width, 22.0],
                    egui::TextEdit::singleline(value).horizontal_align(egui::Align::RIGHT),
                )
                .changed();
            action.browse_clicked = ui.button("Browse").clicked();
        });
    });
    action
}

pub(crate) fn number_row(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [FORM_CONTROL_WIDTH, 22.0],
                    egui::DragValue::new(value).speed(speed).max_decimals(4),
                );
            });
        });
    });
}

pub(crate) fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> PathRowAction {
    let mut action = PathRowAction::default();
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            action.value_changed = ui
                .add_sized(
                    [FORM_CONTROL_WIDTH, 22.0],
                    egui::TextEdit::singleline(value).horizontal_align(egui::Align::RIGHT),
                )
                .changed();
        });
    });
    action
}

pub(crate) fn clean_path_checkbox(
    ui: &mut egui::Ui,
    label: &str,
    clean_paths: &mut String,
    index: usize,
) {
    let mut values = parse_clean_path_values(clean_paths);
    let mut checked = values[index];
    if ui.checkbox(&mut checked, label).changed() {
        values[index] = checked;
        *clean_paths = format_clean_path_values(values);
    }
}

pub(crate) fn parse_clean_path_values(value: &str) -> [bool; 8] {
    let mut values = [true, true, false, true, false, true, false, false];
    if value.trim().is_empty() {
        return values;
    }
    for (index, token) in value.split(',').take(values.len()).enumerate() {
        values[index] = legacy_bool_value(token.trim());
    }
    values
}

pub(crate) fn format_clean_path_values(values: [bool; 8]) -> String {
    values
        .into_iter()
        .map(|value| if value { "1" } else { "0" })
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn combo_row(
    ui: &mut egui::Ui,
    label: &str,
    selected_text: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            egui::ComboBox::from_id_salt(label)
                .selected_text(selected_text)
                .width(FORM_CONTROL_WIDTH)
                .show_ui(ui, body);
        });
    });
}

pub(crate) fn right_aligned_group(ui: &mut egui::Ui, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    let spacing = ui.spacing().item_spacing.x;
    let spacer = (ui.available_width() - width - spacing).max(0.0);
    ui.add_space(spacer);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 22.0),
        egui::Layout::left_to_right(egui::Align::Center),
        body,
    );
}

pub(crate) fn row_label(ui: &mut egui::Ui, label: &str, width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 20.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(label);
        },
    );
}

pub(crate) fn menu_action(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label)).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

pub(crate) fn setting_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

pub(crate) fn format_setting_number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }

    let value = if value.abs() < 0.0000005 { 0.0 } else { value };
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

pub(crate) fn push_setting(
    entries: &mut Vec<LegacySetting>,
    key: &'static str,
    value: impl Into<String>,
    quoted: bool,
) {
    entries.push(LegacySetting::new(key, value, quoted));
}

pub(crate) fn push_bool(entries: &mut Vec<LegacySetting>, key: &'static str, value: bool) {
    push_setting(entries, key, if value { "1" } else { "0" }, false);
}

pub(crate) fn append_view_setting_overrides(
    entries: &mut Vec<LegacySetting>,
    show_toolpath: bool,
    show_bounds: bool,
    show_axes: bool,
) {
    push_bool(entries, "show_v_path", show_toolpath);
    push_bool(entries, "show_box", show_bounds);
    push_bool(entries, "show_axis", show_axes);
}
