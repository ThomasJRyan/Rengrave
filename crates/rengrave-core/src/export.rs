use crate::layout::EngraveSegment;
use crate::settings::LegacySettings;

#[derive(Debug, Clone, PartialEq)]
pub struct ExportOptions {
    pub stroke_thickness: f64,
    pub units: String,
}

impl ExportOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        Self {
            stroke_thickness: get_f64(settings, "STHICK", 0.01),
            units: settings.get_last("units").unwrap_or("in").to_owned(),
        }
    }
}

pub fn write_svg(segments: &[EngraveSegment], options: &ExportOptions) -> String {
    let Some(bounds) = export_bounds(segments, options.stroke_thickness) else {
        return empty_svg(&options.units);
    };

    let dpi = 100.0;
    let width_in = bounds.max_x - bounds.min_x;
    let height_in = bounds.max_y - bounds.min_y;
    let width = width_in * dpi;
    let height = height_in * dpi;
    let stroke_width = options.stroke_thickness * dpi;

    let mut lines = Vec::new();
    lines.push("<?xml version=\"1.0\" standalone=\"no\"?>".to_owned());
    lines.push("<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"  ".to_owned());
    lines.push("  \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">  ".to_owned());
    lines.push(format!(
        "<svg width=\"{width_in:.6}{}\" height=\"{height_in:.6}{}\" viewBox=\"0 0 {width:.6} {height:.6}\"  ",
        options.units, options.units
    ));
    lines.push("     xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\">".to_owned());
    lines.push("  <title> R-Engrave Output </title>".to_owned());
    lines.push("  <desc>SVG File Created By R-Engrave</desc>".to_owned());

    for segment in segments {
        lines.push(format!(
            "  <path d=\"M {:.6} {:.6} L {:.6} {:.6}\"",
            (segment.start.x - bounds.min_x) * dpi,
            (-segment.start.y + bounds.max_y) * dpi,
            (segment.end.x - bounds.min_x) * dpi,
            (-segment.end.y + bounds.max_y) * dpi
        ));
        lines.push(format!(
            "        fill=\"none\" stroke=\"blue\" stroke-width=\"{stroke_width:.6}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>"
        ));
    }

    lines.push("</svg>".to_owned());
    lines.push(String::new());
    lines.join("\n")
}

pub fn write_dxf(segments: &[EngraveSegment]) -> String {
    let mut lines = vec![
        "999",
        "DXF created by R-Engrave",
        "0",
        "SECTION",
        "2",
        "HEADER",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "TABLES",
        "0",
        "TABLE",
        "2",
        "LTYPE",
        "70",
        "1",
        "0",
        "LTYPE",
        "2",
        "CONTINUOUS",
        "70",
        "64",
        "3",
        "Solid line",
        "72",
        "65",
        "73",
        "0",
        "40",
        "0.000000",
        "0",
        "ENDTAB",
        "0",
        "TABLE",
        "2",
        "LAYER",
        "70",
        "6",
        "0",
        "LAYER",
        "2",
        "1",
        "70",
        "64",
        "62",
        "7",
        "6",
        "CONTINUOUS",
        "0",
        "ENDTAB",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "BLOCKS",
        "0",
        "ENDSEC",
        "0",
        "SECTION",
        "2",
        "ENTITIES",
        "  0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();

    for segment in segments {
        lines.extend(
            [
                "LINE",
                "  5",
                "30",
                "100",
                "AcDbEntity",
                "  8",
                "1",
                " 62",
                "150",
                "100",
                "AcDbLine",
                " 10",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        lines.push(format!("{:.4}", segment.start.x));
        lines.push(" 20".to_owned());
        lines.push(format!("{:.4}", segment.start.y));
        lines.push(" 30".to_owned());
        lines.push(format!("{:.4}", 0.0));
        lines.push(" 11".to_owned());
        lines.push(format!("{:.4}", segment.end.x));
        lines.push(" 21".to_owned());
        lines.push(format!("{:.4}", segment.end.y));
        lines.push(" 31".to_owned());
        lines.push(format!("{:.4}", 0.0));
        lines.push("  0".to_owned());
    }

    lines.push("ENDSEC".to_owned());
    lines.push("0".to_owned());
    lines.push("EOF".to_owned());
    lines.push(String::new());
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExportBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

fn export_bounds(segments: &[EngraveSegment], thickness: f64) -> Option<ExportBounds> {
    let mut segments = segments.iter();
    let first = segments.next()?;
    let mut bounds = ExportBounds {
        min_x: first.start.x.min(first.end.x),
        min_y: first.start.y.min(first.end.y),
        max_x: first.start.x.max(first.end.x),
        max_y: first.start.y.max(first.end.y),
    };

    for segment in segments {
        bounds.min_x = bounds.min_x.min(segment.start.x).min(segment.end.x);
        bounds.min_y = bounds.min_y.min(segment.start.y).min(segment.end.y);
        bounds.max_x = bounds.max_x.max(segment.start.x).max(segment.end.x);
        bounds.max_y = bounds.max_y.max(segment.start.y).max(segment.end.y);
    }

    let pad = thickness / 2.0;
    bounds.min_x -= pad;
    bounds.min_y -= pad;
    bounds.max_x += pad;
    bounds.max_y += pad;
    Some(bounds)
}

fn empty_svg(units: &str) -> String {
    [
        "<?xml version=\"1.0\" standalone=\"no\"?>".to_owned(),
        format!("<svg width=\"0.000000{units}\" height=\"0.000000{units}\" viewBox=\"0 0 0.000000 0.000000\" xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\">"),
        "</svg>".to_owned(),
        String::new(),
    ]
    .join("\n")
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::settings::default_legacy_settings;

    #[test]
    fn svg_export_uses_legacy_units_stroke_and_y_flip() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("STHICK", "0.02", false);
        let svg = write_svg(&segments(), &ExportOptions::from_legacy(&settings));

        assert!(svg.contains("<svg width=\"1.020000in\" height=\"1.020000in\""));
        assert!(svg.contains("stroke-width=\"2.000000\""));
        assert!(svg.contains("M 1.000000 101.000000 L 101.000000 1.000000"));
    }

    #[test]
    fn dxf_export_writes_line_entities() {
        let dxf = write_dxf(&segments());

        assert!(dxf.contains("SECTION\n2\nENTITIES"));
        assert!(dxf.contains("LINE\n  5\n30"));
        assert!(dxf.contains(" 10\n0.0000\n 20\n0.0000"));
        assert!(dxf.contains(" 11\n1.0000\n 21\n1.0000"));
        assert!(dxf.ends_with("EOF\n"));
    }

    fn segments() -> Vec<EngraveSegment> {
        vec![EngraveSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
            loop_id: 1,
        }]
    }
}
