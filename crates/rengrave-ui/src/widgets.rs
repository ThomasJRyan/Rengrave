//! Reusable egui form widgets and small settings helpers: labelled rows
//! (path/number/text/combo), layout helpers, menu buttons, and the
//! `LegacySettings` read/write helpers used when serializing controls.

use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PathRowAction {
    pub(crate) browse_clicked: bool,
    pub(crate) value_changed: bool,
}

pub(crate) fn parameter_help(label: &str) -> Option<&'static str> {
    Some(match label {
        "Units" => "Choose the units used for dimensions, feeds, and generated G-code.",
        "Origin" => "Choose the reference point used to place the artwork in the job coordinates.",
        "Height" => "Set the target artwork height; width follows the selected width percentage.",
        "Width %" => "Scale artwork width as a percentage of its height-based size.",
        "X origin" => "Offset the artwork along X after the selected origin is applied.",
        "Y origin" => "Offset the artwork along Y after the selected origin is applied.",
        "Justify" => "Align each text line left, centered, or right within the layout.",
        "Line space" => "Set the multiplier between lines of text.",
        "Character space %" => "Adjust the spacing between adjacent characters as a percentage.",
        "Word space %" => "Adjust the spacing used between words as a percentage.",
        "Text angle" => "Rotate the text around the layout origin, in degrees.",
        "Text radius" => "Wrap text around a circle with this radius; zero keeps it straight.",
        "Outer" => "Use the outside of the text circle for the text baseline.",
        "Upper" => "Place curved text on the upper half of the text circle.",
        "Image size" => "Use the source image dimensions when converting the height scale.",
        "Flip" => "Mirror the artwork across the horizontal axis.",
        "Mirror" => "Mirror the artwork across the vertical axis.",
        "Box" => "Add a rectangular box around the artwork to the generated output.",
        "Box gap" => "Set the clearance between the artwork and the optional box.",
        "Safe Z" => "Set the height the cutter retracts to for rapid travel between paths.",
        "Cut Z" => "Set the constant cutting depth for engraving operations.",
        "Stroke" => "Set the displayed and exported stroke thickness for engraving geometry.",
        "Feed" => "Set the cutting feed rate used for XY tool motion.",
        "Plunge" => "Set the Z plunge feed rate; zero uses the cutting feed.",
        "Accuracy" => "Set the geometric tolerance used when simplifying and fitting toolpaths.",
        "Arc fit" => "Replace suitable line runs with arcs to reduce G-code size.",
        "Enable profile cut" => "Add a companion toolpath around the final project bounds.",
        "Margin" => "Set the clearance between the artwork and the profile cut.",
        "Corner radius" => "Round the corners of the generated profile path.",
        "Thickness" => "Set the material thickness used to calculate profile depth.",
        "Steps" => "Set the number of depth passes used for the straight profile cut.",
        "Endmill dia" => "Set the straight endmill diameter used for the profile offset.",
        "Tabs" => "Set how many uncut tabs hold the workpiece during the profile cut.",
        "Tab height" => "Set the remaining material height at each profile tab.",
        "Max tab width" => "Limit the width of each profile tab; zero uses the default width.",
        "V-bit chamfer" => "Add a V-bit chamfer pass before the straight profile cut.",
        "Chamfer depth" => "Set the depth of the V-bit profile chamfer.",
        "Chamfer angle" => "Set the included angle of the V-bit profile chamfer.",
        "Width (0 = auto)" => "Set a fixed profile width; zero derives it from the artwork bounds.",
        "Height (0 = auto)" => {
            "Set a fixed profile height; zero derives it from the artwork bounds."
        }
        "Aspect W/H (0 = free)" => {
            "Constrain profile width divided by height; zero leaves it unconstrained."
        }
        "Trace detail %" => "Control how closely a traced profile follows the source artwork.",
        "Profile alignment" => "Choose how a fixed-size profile is aligned to the artwork.",
        "Turn policy" => "Choose how bitmap tracing resolves ambiguous path turns.",
        "Turd size" => "Ignore bitmap regions smaller than this size before tracing.",
        "Alpha max" => "Set the bitmap tracing threshold for transparent or anti-aliased pixels.",
        "Opt tolerance" => {
            "Simplify traced curves more at higher values; preserve detail at lower values."
        }
        "Long curves" => "Prefer longer continuous curves when tracing bitmap contours.",
        "Bit" => "Choose the cutter shape used by the V-carve depth model.",
        "V angle" => "Set the included angle of the V-bit.",
        "V diameter" => "Set the maximum effective diameter of the V-bit.",
        "V step" => "Set the sampling distance along V-carve paths.",
        "Allowance" => "Add or remove the inlay allowance from the V-carve boundary.",
        "Depth limit" => "Limit the maximum V-carve depth; zero disables the limit.",
        "Drive corner" => "Set the corner angle below which the cutter drives through the turn.",
        "Step corner" => "Set the corner angle above which intermediate V-carve samples are added.",
        "Check scope" => {
            "Choose whether V-carve clearance checks use all geometry or the current loop."
        }
        "Inlay" => "Use inlay toolpath depth and allowance rules for the selected geometry.",
        "Flip normals" => "Reverse the side used for V-carve normal calculations.",
        "Finish stock" => "Leave this much material for a final V-carve pass after roughing.",
        "Max depth/pass" => "Limit the depth removed in each roughing pass.",
        "Clean dia" => "Set the diameter of the straight cleanup cutter.",
        "Clean diameters" => "Set the straight cleanup cutters from largest to smallest.",
        "Clean step %" => "Set straight cleanup step-over as a percentage of cutter diameter.",
        "Clean V" => "Set the diameter used for V-bit cleanup reach calculations.",
        "Height calc" => {
            "Choose whether text height uses only glyphs in use or the full font height."
        }
        "Arc segments" => "Set the maximum arc segmentation detail used when importing curves.",
        "Preamble" => "Set G-code commands emitted before the toolpath begins.",
        "Postamble" => "Set G-code commands emitted after the toolpath finishes.",
        "G-code" => "Choose where the generated primary G-code file is written.",
        "Return to origin X/Y after job" => {
            "Retract to safe Z, then rapid to X0 Y0 before the postamble."
        }
        "Recovery comments" => "Include compatibility comments that help recover legacy settings.",
        "Disable variables" => "Write numeric safe and cut values instead of controller variables.",
        "Extended chars" => "Allow extended character ranges when reading TTF fonts.",
        "Show thickness" => "Include stroke thickness when displaying the input geometry.",
        "Show V area" => "Display the calculated V-carve area in the preview.",
        "Plot during V-carve" => "Update the preview while V-carve paths are being calculated.",
        _ => return None,
    })
}

fn clean_path_help(index: usize) -> &'static str {
    match index {
        0 => "Generate straight-bit cleanup around the source profile.",
        1 => "Generate straight-bit cleanup along horizontal spans.",
        2 => "Generate straight-bit cleanup along vertical spans.",
        3 => "Generate V-bit cleanup around the source profile.",
        4 => "Generate V-bit cleanup along vertical spans.",
        5 => "Generate V-bit cleanup along horizontal spans.",
        6 => "Generate straight-bit cleanup along closed loop offsets.",
        7 => "Generate V-bit cleanup along closed loop offsets.",
        _ => "Choose whether this cleanup path family is generated.",
    }
}

fn with_parameter_help(response: egui::Response, label: &str) -> egui::Response {
    match parameter_help(label) {
        Some(help) => response.on_hover_text(help),
        None => response,
    }
}

pub(crate) fn parameter_checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    label: &str,
) -> egui::Response {
    with_parameter_help(ui.checkbox(checked, label), label)
}

pub(crate) fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> PathRowAction {
    let mut action = PathRowAction::default();
    ui.horizontal(|ui| {
        row_label(ui, label, 88.0);
        right_aligned_group(ui, PATH_CONTROL_WIDTH, |ui| {
            let text_width = (ui.available_width() - 74.0).max(80.0);
            let response = ui.add_sized(
                [text_width, 22.0],
                egui::TextEdit::singleline(value).horizontal_align(egui::Align::RIGHT),
            );
            action.value_changed = response.changed();
            with_parameter_help(response, label);
            action.browse_clicked = with_parameter_help(ui.button("Browse"), label).clicked();
        });
    });
    action
}

pub(crate) fn number_row(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.add_sized(
                    [FORM_CONTROL_WIDTH, 22.0],
                    egui::DragValue::new(value).speed(speed).max_decimals(4),
                );
                with_parameter_help(response, label);
            });
        });
    });
}

pub(crate) fn cleanup_diameter_rows(ui: &mut egui::Ui, values: &mut String) {
    let mut diameters: Vec<f64> = values
        .split(',')
        .filter_map(|value| value.trim().parse().ok())
        .filter(|value: &f64| *value > 0.0)
        .collect();
    if diameters.is_empty() {
        diameters.push(6.35);
    }

    let mut changed = false;
    let original_count = diameters.len();
    let mut remove_index = None;
    let mut add_after = false;
    for index in 0..original_count {
        let mut diameter = diameters[index];
        ui.horizontal(|ui| {
            row_label(ui, &format!("Clean dia {}", index + 1), 124.0);
            right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
                let response = ui.add_sized(
                    [FORM_CONTROL_WIDTH - 46.0, 22.0],
                    egui::DragValue::new(&mut diameter)
                        .speed(0.01)
                        .max_decimals(4),
                );
                changed |= response.changed();
                let remove = index > 0 && ui.small_button("−").clicked();
                let add = index + 1 == original_count && ui.small_button("+").clicked();
                remove_index = remove.then_some(index);
                add_after = add;
            });
        });
        diameters[index] = diameter;
        if remove_index.is_some() || add_after {
            break;
        }
    }
    if let Some(index) = remove_index {
        diameters.remove(index);
        changed = true;
    } else if add_after {
        let last = *diameters.last().unwrap_or(&0.25);
        diameters.push((last / 2.0).max(0.001));
        changed = true;
    }
    if changed {
        *values = diameters
            .iter()
            .map(|diameter| format_setting_number(*diameter))
            .collect::<Vec<_>>()
            .join(",");
    }
}

pub(crate) fn number_row_with_help(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    speed: f64,
    help: &str,
) {
    ui.horizontal(|ui| {
        row_label_with_help(ui, label, 124.0, help);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [FORM_CONTROL_WIDTH, 22.0],
                    egui::DragValue::new(value).speed(speed).max_decimals(4),
                )
                .on_hover_text(help);
            });
        });
    });
}

pub(crate) fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> PathRowAction {
    let mut action = PathRowAction::default();
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            let response = ui.add_sized(
                [FORM_CONTROL_WIDTH, 22.0],
                egui::TextEdit::singleline(value).horizontal_align(egui::Align::RIGHT),
            );
            action.value_changed = response.changed();
            with_parameter_help(response, label);
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
    let response = ui.checkbox(&mut checked, label);
    let changed = response.changed();
    with_parameter_help(response, label).on_hover_text(clean_path_help(index));
    if changed {
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
            let response = egui::ComboBox::from_id_salt(label)
                .selected_text(selected_text)
                .width(FORM_CONTROL_WIDTH)
                .show_ui(ui, body)
                .response;
            with_parameter_help(response, label);
        });
    });
}

pub(crate) fn combo_row_with_help(
    ui: &mut egui::Ui,
    label: &str,
    selected_text: &str,
    help: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        row_label_with_help(ui, label, 124.0, help);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            egui::ComboBox::from_id_salt(label)
                .selected_text(selected_text)
                .width(FORM_CONTROL_WIDTH)
                .show_ui(ui, body)
                .response
                .on_hover_text(help);
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
            let response = ui.label(label);
            with_parameter_help(response, label);
        },
    );
}

pub(crate) fn row_label_with_help(ui: &mut egui::Ui, label: &str, width: f32, help: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 20.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| ui.label(label).on_hover_text(help),
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
