use std::path::{Path, PathBuf};

use rengrave_core::batch::{BatchRequest, prepare_batch_output};

const SIMPLE_CXF: &str = "tests/fixtures/inputs/simple.cxf";
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
    assert_text_eq(
        normalize_fixture_paths(&output.gcode, &fixture),
        EXPECTED_SIMPLE_NGC,
    );
    assert_text_eq(
        trim_trailing_line_whitespace(output.svg.as_deref().unwrap()),
        EXPECTED_SIMPLE_SVG,
    );
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
