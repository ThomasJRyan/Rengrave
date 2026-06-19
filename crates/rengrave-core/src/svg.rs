use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use kurbo::{Affine, BezPath, PathEl, Vec2};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::font::{Font, Glyph, Stroke};
use crate::geometry::Point;

const SVG_CURVE_TOLERANCE: f64 = 0.25;
const ELLIPSE_SEGMENTS: usize = 72;

#[derive(Debug, thiserror::Error)]
pub enum SvgError {
    #[error("unable to read SVG `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unable to parse SVG XML: {source}")]
    Xml { source: quick_xml::Error },
    #[error("invalid SVG path data: {source}")]
    PathData { source: kurbo::SvgParseError },
    #[error("invalid SVG data: {message}")]
    Invalid { message: String },
    #[error("SVG parsing canceled")]
    Canceled,
}

pub fn is_svg_input(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

pub fn read_svg_font(path: &Path) -> Result<Font, SvgError> {
    read_svg_font_with_cancel(path, &|| false)
}

pub fn read_svg_font_with_cancel(path: &Path, cancel: &dyn Fn() -> bool) -> Result<Font, SvgError> {
    check_canceled(cancel)?;
    let input = fs::read_to_string(path).map_err(|source| SvgError::Read {
        path: path.to_owned(),
        source,
    })?;
    svg_font_from_str_with_cancel(&input, cancel)
}

pub fn svg_font_from_str(input: &str) -> Result<Font, SvgError> {
    svg_font_from_str_with_cancel(input, &|| false)
}

pub fn svg_font_from_str_with_cancel(
    input: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<Font, SvgError> {
    let strokes = parse_svg_segments_with_cancel(input, cancel)?;
    let mut font = Font::default();
    font.glyphs.insert(
        'F' as u32,
        Glyph {
            key: 'F' as u32,
            strokes,
        },
    );
    Ok(font)
}

pub fn parse_svg_segments(input: &str) -> Result<Vec<Stroke>, SvgError> {
    parse_svg_segments_with_cancel(input, &|| false)
}

pub fn parse_svg_segments_with_cancel(
    input: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Stroke>, SvgError> {
    check_canceled(cancel)?;
    let mut reader = Reader::from_str(input);
    let mut buffer = Vec::new();
    let mut transform_stack = vec![Affine::IDENTITY];
    let mut strokes = Vec::new();

    loop {
        check_canceled(cancel)?;
        match reader
            .read_event_into(&mut buffer)
            .map_err(|source| SvgError::Xml { source })?
        {
            Event::Start(start) => {
                let tag = tag_name(&start);
                let attrs = collect_attrs(&reader, &start)?;
                let transform = combined_transform(&transform_stack, &attrs);
                append_svg_element(&tag, &attrs, transform, &mut strokes)?;
                transform_stack.push(transform);
            }
            Event::Empty(start) => {
                let tag = tag_name(&start);
                let attrs = collect_attrs(&reader, &start)?;
                let transform = combined_transform(&transform_stack, &attrs);
                append_svg_element(&tag, &attrs, transform, &mut strokes)?;
            }
            Event::End(_) => {
                if transform_stack.len() > 1 {
                    transform_stack.pop();
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    normalize_svg_strokes(&mut strokes);
    Ok(strokes)
}

fn check_canceled(cancel: &dyn Fn() -> bool) -> Result<(), SvgError> {
    if cancel() {
        Err(SvgError::Canceled)
    } else {
        Ok(())
    }
}

fn tag_name(start: &BytesStart<'_>) -> String {
    String::from_utf8_lossy(start.local_name().as_ref()).into_owned()
}

fn collect_attrs(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>, SvgError> {
    let mut attrs = BTreeMap::new();
    for attr in start.attributes().with_checks(false) {
        let attr = attr.map_err(|err| SvgError::Invalid {
            message: format!("invalid attribute: {err}"),
        })?;
        let key = String::from_utf8_lossy(attr.key.local_name().as_ref()).into_owned();
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|source| SvgError::Xml { source })?
            .into_owned();
        attrs.insert(key, value);
    }
    Ok(attrs)
}

fn combined_transform(stack: &[Affine], attrs: &BTreeMap<String, String>) -> Affine {
    let parent = stack.last().copied().unwrap_or(Affine::IDENTITY);
    let local = attrs
        .get("transform")
        .map(|value| parse_transform(value))
        .unwrap_or(Affine::IDENTITY);
    parent * local
}

fn append_svg_element(
    tag: &str,
    attrs: &BTreeMap<String, String>,
    transform: Affine,
    strokes: &mut Vec<Stroke>,
) -> Result<(), SvgError> {
    match tag {
        "path" => {
            if let Some(data) = attrs.get("d") {
                let mut path =
                    BezPath::from_svg(data).map_err(|source| SvgError::PathData { source })?;
                path.apply_affine(transform);
                append_bez_path(&path, strokes);
            }
        }
        "polyline" | "polygon" => {
            let Some(points) = attrs.get("points") else {
                return Ok(());
            };
            let mut points = parse_points(points)
                .into_iter()
                .map(|point| transform_point(transform, point))
                .collect::<Vec<_>>();
            if tag == "polygon" && points.len() > 1 {
                points.push(points[0]);
            }
            append_point_chain(&points, strokes);
        }
        "line" => {
            let start = Point::new(
                attr_number(attrs, "x1").unwrap_or(0.0),
                attr_number(attrs, "y1").unwrap_or(0.0),
            );
            let end = Point::new(
                attr_number(attrs, "x2").unwrap_or(0.0),
                attr_number(attrs, "y2").unwrap_or(0.0),
            );
            append_transformed_line(start, end, transform, strokes);
        }
        "rect" => {
            let x = attr_number(attrs, "x").unwrap_or(0.0);
            let y = attr_number(attrs, "y").unwrap_or(0.0);
            let width = attr_number(attrs, "width").unwrap_or(0.0);
            let height = attr_number(attrs, "height").unwrap_or(0.0);
            if width <= 0.0 || height <= 0.0 {
                return Ok(());
            }
            let points = [
                Point::new(x, y),
                Point::new(x + width, y),
                Point::new(x + width, y + height),
                Point::new(x, y + height),
                Point::new(x, y),
            ]
            .into_iter()
            .map(|point| transform_point(transform, point))
            .collect::<Vec<_>>();
            append_point_chain(&points, strokes);
        }
        "circle" => {
            let cx = attr_number(attrs, "cx").unwrap_or(0.0);
            let cy = attr_number(attrs, "cy").unwrap_or(0.0);
            let radius = attr_number(attrs, "r").unwrap_or(0.0);
            append_ellipse(cx, cy, radius, radius, transform, strokes);
        }
        "ellipse" => {
            let cx = attr_number(attrs, "cx").unwrap_or(0.0);
            let cy = attr_number(attrs, "cy").unwrap_or(0.0);
            let rx = attr_number(attrs, "rx").unwrap_or(0.0);
            let ry = attr_number(attrs, "ry").unwrap_or(0.0);
            append_ellipse(cx, cy, rx, ry, transform, strokes);
        }
        _ => {}
    }
    Ok(())
}

fn append_bez_path(path: &BezPath, strokes: &mut Vec<Stroke>) {
    let mut current = None;
    let mut subpath_start = None;
    kurbo::flatten(path.iter(), SVG_CURVE_TOLERANCE, |element| match element {
        PathEl::MoveTo(point) => {
            let point = point_from_kurbo(point);
            current = Some(point);
            subpath_start = Some(point);
        }
        PathEl::LineTo(point) => {
            let point = point_from_kurbo(point);
            if let Some(start) = current {
                strokes.push(Stroke { start, end: point });
            }
            current = Some(point);
        }
        PathEl::ClosePath => {
            if let (Some(start), Some(end)) = (current, subpath_start) {
                strokes.push(Stroke { start, end });
            }
            current = None;
            subpath_start = None;
        }
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
}

fn append_point_chain(points: &[Point], strokes: &mut Vec<Stroke>) {
    for pair in points.windows(2) {
        strokes.push(Stroke {
            start: pair[0],
            end: pair[1],
        });
    }
}

fn append_transformed_line(start: Point, end: Point, transform: Affine, strokes: &mut Vec<Stroke>) {
    strokes.push(Stroke {
        start: transform_point(transform, start),
        end: transform_point(transform, end),
    });
}

fn append_ellipse(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    transform: Affine,
    strokes: &mut Vec<Stroke>,
) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    let mut points = Vec::with_capacity(ELLIPSE_SEGMENTS + 1);
    for index in 0..=ELLIPSE_SEGMENTS {
        let angle = std::f64::consts::TAU * index as f64 / ELLIPSE_SEGMENTS as f64;
        points.push(transform_point(
            transform,
            Point::new(cx + rx * angle.cos(), cy + ry * angle.sin()),
        ));
    }
    append_point_chain(&points, strokes);
}

fn normalize_svg_strokes(strokes: &mut [Stroke]) {
    let Some((min, max)) = stroke_bounds(strokes) else {
        return;
    };
    for stroke in strokes {
        stroke.start.x -= min.x;
        stroke.end.x -= min.x;
        stroke.start.y = max.y - stroke.start.y;
        stroke.end.y = max.y - stroke.end.y;
    }
}

fn stroke_bounds(strokes: &[Stroke]) -> Option<(Point, Point)> {
    let mut min = Point::new(0.0, 0.0);
    let mut max = Point::new(0.0, 0.0);
    let mut seen = false;
    for stroke in strokes {
        for point in [stroke.start, stroke.end] {
            if !point.x.is_finite() || !point.y.is_finite() {
                continue;
            }
            if seen {
                min.x = min.x.min(point.x);
                min.y = min.y.min(point.y);
                max.x = max.x.max(point.x);
                max.y = max.y.max(point.y);
            } else {
                min = point;
                max = point;
                seen = true;
            }
        }
    }
    seen.then_some((min, max))
}

fn attr_number(attrs: &BTreeMap<String, String>, key: &str) -> Option<f64> {
    attrs.get(key).and_then(|value| parse_svg_number(value))
}

fn parse_points(value: &str) -> Vec<Point> {
    let numbers = parse_number_list(value);
    numbers
        .chunks_exact(2)
        .map(|pair| Point::new(pair[0], pair[1]))
        .collect()
}

fn parse_transform(value: &str) -> Affine {
    let mut transform = Affine::IDENTITY;
    for part in value.split(')').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        let Some((name, args)) = part.split_once('(') else {
            continue;
        };
        let args = parse_number_list(args);
        let next = match name.trim() {
            "matrix" if args.len() == 6 => {
                Affine::new([args[0], args[1], args[2], args[3], args[4], args[5]])
            }
            "translate" => {
                let x = args.first().copied().unwrap_or(0.0);
                let y = args.get(1).copied().unwrap_or(0.0);
                Affine::translate(Vec2::new(x, y))
            }
            "scale" => {
                let x = args.first().copied().unwrap_or(1.0);
                let y = args.get(1).copied().unwrap_or(x);
                Affine::scale_non_uniform(x, y)
            }
            "rotate" => {
                let angle = args.first().copied().unwrap_or(0.0).to_radians();
                match args.as_slice() {
                    [_, cx, cy, ..] => Affine::rotate_about(angle, kurbo::Point::new(*cx, *cy)),
                    _ => Affine::rotate(angle),
                }
            }
            _ => Affine::IDENTITY,
        };
        transform *= next;
    }
    transform
}

fn parse_number_list(value: &str) -> Vec<f64> {
    value
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .filter_map(parse_svg_number)
        .collect()
}

fn parse_svg_number(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut end = 0;
    for (index, ch) in value.char_indices() {
        let valid = ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | 'e' | 'E');
        if !valid {
            break;
        }
        end = index + ch.len_utf8();
    }
    value.get(..end)?.parse().ok()
}

fn transform_point(transform: Affine, point: Point) -> Point {
    point_from_kurbo(transform * kurbo::Point::new(point.x, point.y))
}

fn point_from_kurbo(point: kurbo::Point) -> Point {
    Point::new(point.x, point.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_svg_path_and_normalizes_to_content_origin() {
        let svg = r#"<svg viewBox="0 0 100 100"><path d="M 50 20 L 70 20 L 70 40 Z"/></svg>"#;

        let strokes = parse_svg_segments(svg).unwrap();

        assert_eq!(strokes.len(), 3);
        assert_eq!(strokes[0].start, Point::new(0.0, 20.0));
        assert_eq!(strokes[0].end, Point::new(20.0, 20.0));
        assert_eq!(strokes[1].end, Point::new(20.0, 0.0));
    }

    #[test]
    fn parses_svg_basic_shapes_and_transform() {
        let svg = r#"<svg><g transform="translate(10 5)"><rect x="1" y="2" width="3" height="4"/></g></svg>"#;

        let strokes = parse_svg_segments(svg).unwrap();

        assert_eq!(strokes.len(), 4);
        assert_eq!(strokes[0].start, Point::new(0.0, 4.0));
        assert_eq!(strokes[0].end, Point::new(3.0, 4.0));
        assert_eq!(strokes[1].end, Point::new(3.0, 0.0));
    }

    #[test]
    fn svg_font_wraps_segments_as_image_glyph() {
        let font = svg_font_from_str(r#"<svg><polyline points="0,0 2,0 2,2"/></svg>"#).unwrap();

        assert_eq!(font.get_char('F').unwrap().strokes.len(), 2);
    }
}
