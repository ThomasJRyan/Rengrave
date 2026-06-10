use crate::font::{Font, Glyph, Stroke};
use crate::geometry::Point;

#[derive(Debug, thiserror::Error)]
pub enum DxfError {
    #[error("unable to read DXF `{path}`: {source}")]
    Read {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Copy)]
struct Vertex {
    point: Point,
    bulge: f64,
}

pub fn read_dxf_font(path: &std::path::Path, segarc_degrees: f64) -> Result<Font, DxfError> {
    let input = std::fs::read_to_string(path).map_err(|source| DxfError::Read {
        path: path.to_owned(),
        source,
    })?;
    Ok(dxf_font_from_str(&input, segarc_degrees))
}

pub fn dxf_font_from_str(input: &str, segarc_degrees: f64) -> Font {
    let strokes = parse_dxf_segments(input, segarc_degrees);
    let mut font = Font::default();
    font.glyphs.insert(
        'F' as u32,
        Glyph {
            key: 'F' as u32,
            strokes,
        },
    );
    font
}

pub fn parse_dxf_segments(input: &str, segarc_degrees: f64) -> Vec<Stroke> {
    let pairs = group_pairs(input);
    let mut strokes = Vec::new();
    let mut idx = 0;

    while idx < pairs.len() {
        if pairs[idx].0 != 0 {
            idx += 1;
            continue;
        }

        match pairs[idx].1.as_str() {
            "LINE" => {
                let (entity, next) = collect_entity(&pairs, idx + 1);
                parse_line_entity(entity, &mut strokes);
                idx = next;
            }
            "LWPOLYLINE" => {
                let (entity, next) = collect_entity(&pairs, idx + 1);
                parse_lwpolyline_entity(entity, segarc_degrees, &mut strokes);
                idx = next;
            }
            "POLYLINE" => {
                idx = parse_polyline_entities(&pairs, idx + 1, segarc_degrees, &mut strokes);
            }
            _ => idx += 1,
        }
    }

    strokes
}

fn group_pairs(input: &str) -> Vec<(i32, String)> {
    let mut lines = input.lines();
    let mut pairs = Vec::new();

    while let Some(code) = lines.next() {
        let Some(value) = lines.next() else {
            break;
        };
        if let Ok(code) = code.trim().parse() {
            pairs.push((code, value.trim().to_owned()));
        }
    }

    pairs
}

fn collect_entity(pairs: &[(i32, String)], mut idx: usize) -> (&[(i32, String)], usize) {
    let start = idx;
    while idx < pairs.len() && pairs[idx].0 != 0 {
        idx += 1;
    }
    (&pairs[start..idx], idx)
}

fn parse_line_entity(entity: &[(i32, String)], strokes: &mut Vec<Stroke>) {
    let mut x1 = None;
    let mut y1 = None;
    let mut x2 = None;
    let mut y2 = None;

    for (code, value) in entity {
        match *code {
            10 => x1 = value.parse().ok(),
            20 => y1 = value.parse().ok(),
            11 => x2 = value.parse().ok(),
            21 => y2 = value.parse().ok(),
            _ => {}
        }
    }

    if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (x1, y1, x2, y2) {
        strokes.push(Stroke::new(x1, y1, x2, y2));
    }
}

fn parse_lwpolyline_entity(
    entity: &[(i32, String)],
    segarc_degrees: f64,
    strokes: &mut Vec<Stroke>,
) {
    let mut closed = false;
    let mut vertices = Vec::new();
    let mut current_x = None;
    let mut current_y = None;
    let mut current_bulge = 0.0;

    for (code, value) in entity {
        match *code {
            10 => {
                if let (Some(x), Some(y)) = (current_x.take(), current_y.take()) {
                    vertices.push(Vertex {
                        point: Point::new(x, y),
                        bulge: current_bulge,
                    });
                    current_bulge = 0.0;
                }
                current_x = value.parse().ok();
            }
            20 => current_y = value.parse().ok(),
            42 => current_bulge = value.parse().unwrap_or(0.0),
            70 => {
                let flags = value.parse::<i32>().unwrap_or(0);
                closed = flags & 1 == 1;
            }
            _ => {}
        }
    }

    if let (Some(x), Some(y)) = (current_x, current_y) {
        vertices.push(Vertex {
            point: Point::new(x, y),
            bulge: current_bulge,
        });
    }

    append_polyline_segments(&vertices, closed, segarc_degrees, strokes);
}

fn parse_polyline_entities(
    pairs: &[(i32, String)],
    mut idx: usize,
    segarc_degrees: f64,
    strokes: &mut Vec<Stroke>,
) -> usize {
    let mut closed = false;
    let mut vertices = Vec::new();

    while idx < pairs.len() {
        if pairs[idx].0 != 0 {
            if pairs[idx].0 == 70 {
                let flags = pairs[idx].1.parse::<i32>().unwrap_or(0);
                closed = flags & 1 == 1;
            }
            idx += 1;
            continue;
        }

        match pairs[idx].1.as_str() {
            "VERTEX" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                if let Some(vertex) = parse_vertex_entity(entity) {
                    vertices.push(vertex);
                }
                idx = next;
            }
            "SEQEND" => {
                append_polyline_segments(&vertices, closed, segarc_degrees, strokes);
                return idx + 1;
            }
            _ => return idx,
        }
    }

    append_polyline_segments(&vertices, closed, segarc_degrees, strokes);
    idx
}

fn parse_vertex_entity(entity: &[(i32, String)]) -> Option<Vertex> {
    let mut x = None;
    let mut y = None;
    let mut bulge = 0.0;

    for (code, value) in entity {
        match *code {
            10 => x = value.parse().ok(),
            20 => y = value.parse().ok(),
            42 => bulge = value.parse().unwrap_or(0.0),
            _ => {}
        }
    }

    Some(Vertex {
        point: Point::new(x?, y?),
        bulge,
    })
}

fn append_polyline_segments(
    vertices: &[Vertex],
    closed: bool,
    segarc_degrees: f64,
    strokes: &mut Vec<Stroke>,
) {
    for pair in vertices.windows(2) {
        append_segment_or_bulge(pair[0], pair[1], segarc_degrees, strokes);
    }

    if closed && vertices.len() > 1 {
        append_segment_or_bulge(
            *vertices.last().unwrap(),
            vertices[0],
            segarc_degrees,
            strokes,
        );
    }
}

fn append_segment_or_bulge(
    from: Vertex,
    to: Vertex,
    segarc_degrees: f64,
    strokes: &mut Vec<Stroke>,
) {
    if from.bulge.abs() < 1.0e-12 {
        strokes.push(Stroke {
            start: from.point,
            end: to.point,
        });
        return;
    }

    append_bulge_segments(from.point, to.point, from.bulge, segarc_degrees, strokes);
}

fn append_bulge_segments(
    start: Point,
    end: Point,
    bulge: f64,
    segarc_degrees: f64,
    strokes: &mut Vec<Stroke>,
) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let chord = (dx * dx + dy * dy).sqrt();
    if chord <= 1.0e-12 {
        return;
    }

    let theta = 4.0 * bulge.atan();
    let radius = chord * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    let h = chord * (1.0 - bulge * bulge) / (4.0 * bulge);
    let midpoint = Point::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
    let left_normal = Point::new(-dy / chord, dx / chord);
    let center = Point::new(
        midpoint.x - left_normal.x * h,
        midpoint.y - left_normal.y * h,
    );
    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let steps = ((theta.abs().to_degrees() / segarc_degrees.max(1.0)).ceil() as usize).max(1);

    let mut previous = start;
    for step in 1..=steps {
        let angle = start_angle + theta * (step as f64 / steps as f64);
        let next = Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        strokes.push(Stroke {
            start: previous,
            end: next,
        });
        previous = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_line_entities() {
        let strokes = parse_dxf_segments(
            "0\nSECTION\n0\nLINE\n10\n1\n20\n2\n11\n3\n21\n4\n0\nENDSEC\n",
            5.0,
        );

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].start, Point::new(1.0, 2.0));
        assert_eq!(strokes[0].end, Point::new(3.0, 4.0));
    }

    #[test]
    fn parses_closed_lwpolyline_entities() {
        let strokes = parse_dxf_segments(
            "0\nLWPOLYLINE\n70\n1\n10\n0\n20\n0\n10\n1\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n",
            5.0,
        );

        assert_eq!(strokes.len(), 3);
        assert_eq!(strokes[2].end, Point::new(0.0, 0.0));
    }

    #[test]
    fn approximates_lwpolyline_bulges() {
        let strokes = parse_dxf_segments(
            "0\nLWPOLYLINE\n10\n0\n20\n0\n42\n1\n10\n2\n20\n0\n0\nENDSEC\n",
            45.0,
        );

        assert!(strokes.len() > 1);
        assert!((strokes.last().unwrap().end.x - 2.0).abs() < 1e-9);
        assert!(strokes.iter().any(|stroke| stroke.end.y.abs() > 0.1));
    }
}
