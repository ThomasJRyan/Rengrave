use std::fs;
use std::path::{Path, PathBuf};

use rengrave_core::batch::{BatchRequest, prepare_batch_output};
use rengrave_core::settings::{LegacySetting, LegacySettings};

const SIMPLE_CXF: &str = "tests/fixtures/inputs/simple.cxf";
const ARC_CXF: &str = "tests/fixtures/inputs/arc.cxf";
const SQUARE_CXF: &str = "tests/fixtures/inputs/square.cxf";
const SIMPLE_DXF: &str = "tests/fixtures/inputs/simple.dxf";
const GCODE_TOLERANCE: f64 = 0.0001;
const EXPECTED_SIMPLE_NGC: &str = include_str!("fixtures/expected/simple_text.ngc");
const EXPECTED_SIMPLE_SVG: &str = include_str!("fixtures/expected/simple_text.svg");

#[test]
fn simple_cxf_text_matches_checked_golden_outputs() {
    let fixture = core_fixture_path(SIMPLE_CXF);
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture.clone()),
        text: Some("AB".to_owned()),
        svg_output: Some(PathBuf::from("simple_text.svg")),
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    assert_gcode_eq_with_tolerance(
        normalize_fixture_paths(&output.gcode, &fixture),
        EXPECTED_SIMPLE_NGC,
        GCODE_TOLERANCE,
    );
    assert_text_eq(
        trim_trailing_line_whitespace(output.svg.as_deref().unwrap()),
        EXPECTED_SIMPLE_SVG,
    );
}

#[test]
fn multiline_cxf_text_generates_separate_rows_and_legacy_tcode() {
    let fixture = core_fixture_path(SIMPLE_CXF);
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture),
        text: Some("A|B".to_owned()),
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    assert!(
        output
            .gcode
            .contains("(fengrave_set TCODE       065 010 066 )")
    );

    let points = motion_xy_points(&output.gcode);
    assert!(
        points.iter().any(|(_, y)| *y > 1.8),
        "expected first text row above origin in:\n{}",
        output.gcode
    );
    assert!(
        points.iter().any(|(_, y)| *y < -2.0),
        "expected second text row below origin in:\n{}",
        output.gcode
    );
}

#[test]
fn text_on_circle_with_add_circle_emits_circle_gcode_and_svg() {
    let fixture = core_fixture_path(SIMPLE_CXF);
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture),
        text: Some("AB".to_owned()),
        svg_output: Some(PathBuf::from("circle.svg")),
        settings_overrides: vec![
            LegacySetting::new("TRADIUS", "5", false),
            LegacySetting::new("plotbox", "1", false),
            LegacySetting::new("boxgap", "0.25", false),
        ],
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    assert!(output.gcode.contains("(fengrave_set TRADIUS     5 )"));
    assert!(
        output.gcode.contains("\nG2 I") || output.gcode.contains("\nG3 I"),
        "expected add-circle arc move in:\n{}",
        output.gcode
    );
    assert!(output.svg.as_deref().unwrap().contains("<circle cx="));
}

#[test]
fn add_box_flat_text_emits_rectangular_border_in_batch_outputs() {
    let fixture = core_fixture_path(SIMPLE_CXF);
    let baseline = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture.clone()),
        text: Some("A".to_owned()),
        svg_output: Some(PathBuf::from("baseline.svg")),
        ..BatchRequest::default()
    })
    .unwrap();
    let boxed = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture),
        text: Some("A".to_owned()),
        svg_output: Some(PathBuf::from("boxed.svg")),
        settings_overrides: vec![
            LegacySetting::new("plotbox", "1", false),
            LegacySetting::new("boxgap", "0.25", false),
        ],
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        baseline.warnings.is_empty(),
        "unexpected baseline warnings: {:?}",
        baseline.warnings
    );
    assert!(
        boxed.warnings.is_empty(),
        "unexpected boxed warnings: {:?}",
        boxed.warnings
    );
    assert!(boxed.gcode.contains("(fengrave_set plotbox     1 )"));
    assert!(boxed.gcode.contains("(fengrave_set boxgap      0.25 )"));
    assert!(
        count_xy_cut_moves(&boxed.gcode) >= count_xy_cut_moves(&baseline.gcode) + 4,
        "expected Add Box to append rectangular border moves:\n{}",
        boxed.gcode
    );
    assert!(
        boxed.svg.as_deref().unwrap().matches("<path").count()
            > baseline.svg.as_deref().unwrap().matches("<path").count()
    );
}

#[test]
fn arc_fit_modes_emit_expected_batch_gcode_formats() {
    let fixture = core_fixture_path(ARC_CXF);
    let without_arcs = batch_arc_fit_output(&fixture, "none");
    let center_arcs = batch_arc_fit_output(&fixture, "center");
    let radius_arcs = batch_arc_fit_output(&fixture, "radius");

    assert!(
        !contains_arc_command(&without_arcs.gcode),
        "expected no arc commands with arc_fit none:\n{}",
        without_arcs.gcode
    );
    assert!(
        contains_arc_command(&center_arcs.gcode),
        "expected center arc fitting to emit G2/G3:\n{}",
        center_arcs.gcode
    );
    assert!(
        center_arcs
            .gcode
            .lines()
            .any(|line| is_arc_command(line) && line.contains(" I") && line.contains(" J")),
        "expected center arc fitting to use I/J center offsets:\n{}",
        center_arcs.gcode
    );
    assert!(
        radius_arcs
            .gcode
            .lines()
            .any(|line| is_arc_command(line) && line.contains(" R")),
        "expected radius arc fitting to use R radius format:\n{}",
        radius_arcs.gcode
    );
}

#[test]
fn vcarve_closed_text_generates_depth_moves_and_cleanup_companion() {
    let fixture = core_fixture_path(SQUARE_CXF);
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture),
        text: Some("A".to_owned()),
        include_secondary: true,
        settings_overrides: vec![
            LegacySetting::new("cut_type", "v-carve", false),
            LegacySetting::new("v_step_len", "0.5", false),
            LegacySetting::new("clean_paths", "1,0,0,0,0,0,0,0", false),
        ],
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    let parsed = LegacySettings::parse(&output.gcode);
    assert_eq!(parsed.get_last("cut_type"), Some("v-carve"));
    assert_eq!(parsed.get_last("clean_paths"), Some("1,0,0,0,0,0,0,0"));
    assert!(output.gcode.contains("G1 X"));
    assert!(output.gcode.contains(" Z-"));
    assert!(
        !output
            .gcode
            .contains("v-carve generation is not ported yet")
    );
    assert_eq!(output.secondary_gcode.len(), 1);
    assert_eq!(output.secondary_gcode[0].suffix, "clean");
    assert!(
        output.secondary_gcode[0]
            .gcode
            .contains("secondary cleanup operation")
    );
    assert!(output.secondary_gcode[0].gcode.contains("G1 X"));
}

#[test]
fn transform_settings_round_trip_through_generated_settings_comments() {
    let fixture = core_fixture_path(SIMPLE_CXF);
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture),
        text: Some("A|B".to_owned()),
        settings_overrides: vec![
            LegacySetting::new("justify", "Right", false),
            LegacySetting::new("origin", "Bot-Left", false),
            LegacySetting::new("flip", "1", false),
            LegacySetting::new("mirror", "1", false),
            LegacySetting::new("TANGLE", "45", false),
            LegacySetting::new("xorigin", "1.25", false),
            LegacySetting::new("yorigin", "-0.75", false),
        ],
        ..BatchRequest::default()
    })
    .unwrap();

    let parsed = LegacySettings::parse(&output.gcode);
    assert_eq!(parsed.get_last("justify"), Some("Right"));
    assert_eq!(parsed.get_last("origin"), Some("Bot-Left"));
    assert_eq!(parsed.get_last("flip"), Some("1"));
    assert_eq!(parsed.get_last("mirror"), Some("1"));
    assert_eq!(parsed.get_last("TANGLE"), Some("45"));
    assert_eq!(parsed.get_last("xorigin"), Some("1.25"));
    assert_eq!(parsed.get_last("yorigin"), Some("-0.75"));
    assert_eq!(parsed.text_from_tcode().unwrap(), Some("A\nB".to_owned()));

    let settings_path =
        std::env::temp_dir().join(format!("rengrave-round-trip-{}.ngc", std::process::id()));
    fs::write(&settings_path, &output.gcode).unwrap();
    let reloaded = prepare_batch_output(&BatchRequest {
        batch: true,
        gcode_file: Some(settings_path.clone()),
        ..BatchRequest::default()
    })
    .unwrap();
    let _ = fs::remove_file(settings_path);

    assert!(
        reloaded.warnings.is_empty(),
        "unexpected warnings: {:?}",
        reloaded.warnings
    );
    assert!(reloaded.gcode.contains("(fengrave_set mirror      1 )"));
    assert!(reloaded.gcode.contains("G1 X"));
    assert!(!reloaded.gcode.contains("settings-only output"));
}

#[test]
fn dxf_image_input_generates_gcode_svg_and_dxf_payloads() {
    let fixture = core_fixture_path(SIMPLE_DXF);
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture),
        svg_output: Some(PathBuf::from("simple_dxf.svg")),
        dxf_output: Some(PathBuf::from("simple_dxf.dxf")),
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        output.warnings.is_empty(),
        "unexpected warnings: {:?}",
        output.warnings
    );
    assert!(output.gcode.contains("(fengrave_set input_type  image )"));
    assert!(!output.gcode.contains("(Engrave Text:"));
    assert!(output.gcode.contains("G1 X0.0000 Y1.9900"));
    assert!(output.svg.as_deref().unwrap().contains("<path"));
    assert!(output.dxf.as_deref().unwrap().contains("SECTION"));
}

fn core_fixture_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn normalize_fixture_paths(output: &str, fixture: &Path) -> String {
    let Some(parent) = fixture.parent() else {
        return output.to_owned();
    };
    output.replace(
        &format!("(fengrave_set fontdir     \"{}\" )", parent.display()),
        "(fengrave_set fontdir     \"tests/fixtures/inputs\" )",
    )
}

fn trim_trailing_line_whitespace(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn motion_xy_points(gcode: &str) -> Vec<(f64, f64)> {
    gcode
        .lines()
        .filter(|line| line.starts_with("G0 ") || line.starts_with("G1 "))
        .filter_map(|line| {
            let mut x = None;
            let mut y = None;
            for word in line.split_whitespace() {
                if let Some(value) = word.strip_prefix('X') {
                    x = value.parse().ok();
                } else if let Some(value) = word.strip_prefix('Y') {
                    y = value.parse().ok();
                }
            }
            Some((x?, y?))
        })
        .collect()
}

fn count_xy_cut_moves(gcode: &str) -> usize {
    gcode
        .lines()
        .filter(|line| line.starts_with("G1 ") && line.contains(" X") && line.contains(" Y"))
        .count()
}

fn batch_arc_fit_output(fixture: &Path, arc_fit: &str) -> rengrave_core::batch::BatchOutput {
    let output = prepare_batch_output(&BatchRequest {
        batch: true,
        font_or_image: Some(fixture.to_path_buf()),
        text: Some("O".to_owned()),
        settings_overrides: vec![
            LegacySetting::new("arc_fit", arc_fit, false),
            LegacySetting::new("accuracy", "0.05", false),
            LegacySetting::new("segarc", "5", false),
        ],
        ..BatchRequest::default()
    })
    .unwrap();

    assert!(
        output.warnings.is_empty(),
        "unexpected arc-fit warnings for {arc_fit}: {:?}",
        output.warnings
    );
    assert_eq!(
        LegacySettings::parse(&output.gcode).get_last("arc_fit"),
        Some(arc_fit)
    );
    output
}

fn contains_arc_command(gcode: &str) -> bool {
    gcode.lines().any(is_arc_command)
}

fn is_arc_command(line: &str) -> bool {
    line.starts_with("G2 ") || line.starts_with("G3 ")
}

fn assert_gcode_eq_with_tolerance(actual: impl AsRef<str>, expected: &str, tolerance: f64) {
    let actual = actual.as_ref();
    if actual == expected {
        return;
    }

    let actual_lines: Vec<_> = actual.lines().collect();
    let expected_lines: Vec<_> = expected.lines().collect();
    for index in 0..actual_lines.len().max(expected_lines.len()) {
        let actual_line = actual_lines.get(index).copied().unwrap_or("<missing>");
        let expected_line = expected_lines.get(index).copied().unwrap_or("<missing>");
        assert_gcode_line_eq(actual_line, expected_line, index + 1, tolerance);
    }
}

fn assert_gcode_line_eq(actual: &str, expected: &str, line_number: usize, tolerance: f64) {
    if actual == expected {
        return;
    }

    let actual_tokens: Vec<_> = actual.split_whitespace().collect();
    let expected_tokens: Vec<_> = expected.split_whitespace().collect();
    if actual_tokens.len() != expected_tokens.len() {
        panic!("golden mismatch at line {line_number}:\nactual:   {actual}\nexpected: {expected}");
    }

    for (actual_token, expected_token) in actual_tokens.iter().zip(expected_tokens) {
        if *actual_token == expected_token {
            continue;
        }
        if numeric_words_match(actual_token, expected_token, tolerance) {
            continue;
        }
        panic!("golden mismatch at line {line_number}:\nactual:   {actual}\nexpected: {expected}");
    }
}

fn numeric_words_match(actual: &str, expected: &str, tolerance: f64) -> bool {
    let Some((actual_prefix, actual_value)) = parse_gcode_numeric_word(actual) else {
        return false;
    };
    let Some((expected_prefix, expected_value)) = parse_gcode_numeric_word(expected) else {
        return false;
    };

    actual_prefix == expected_prefix && (actual_value - expected_value).abs() <= tolerance
}

fn parse_gcode_numeric_word(word: &str) -> Option<(&str, f64)> {
    let split_at = word.find(|character: char| {
        character == '-' || character == '+' || character == '.' || character.is_ascii_digit()
    })?;
    if split_at == 0 {
        return None;
    }

    let (prefix, value) = word.split_at(split_at);
    let value = value.parse().ok()?;
    Some((prefix, value))
}

fn assert_text_eq(actual: impl AsRef<str>, expected: &str) {
    let actual = actual.as_ref();
    if actual == expected {
        return;
    }

    let actual_lines: Vec<_> = actual.lines().collect();
    let expected_lines: Vec<_> = expected.lines().collect();
    for index in 0..actual_lines.len().max(expected_lines.len()) {
        let actual_line = actual_lines.get(index).copied().unwrap_or("<missing>");
        let expected_line = expected_lines.get(index).copied().unwrap_or("<missing>");
        if actual_line != expected_line {
            panic!(
                "golden mismatch at line {}:\nactual:   {actual_line}\nexpected: {expected_line}",
                index + 1
            );
        }
    }
}
