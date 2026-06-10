use crate::font::{Font, Glyph, Stroke};
use crate::geometry::Point;
use std::collections::BTreeMap;

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

#[derive(Debug, Clone)]
struct Block {
    base: Point,
    pairs: Vec<(i32, String)>,
}

#[derive(Debug, Clone, Copy)]
struct Transform {
    offset: Point,
    scale: Point,
    rotate_degrees: f64,
}

impl Transform {
    fn identity() -> Self {
        Self {
            offset: Point::new(0.0, 0.0),
            scale: Point::new(1.0, 1.0),
            rotate_degrees: 0.0,
        }
    }

    fn apply(self, point: Point) -> Point {
        let scaled = Point::new(point.x * self.scale.x, point.y * self.scale.y);
        let rotated = if self.rotate_degrees.abs() > 1.0e-12 {
            let radians = self.rotate_degrees.to_radians();
            Point::new(
                scaled.x * radians.cos() - scaled.y * radians.sin(),
                scaled.x * radians.sin() + scaled.y * radians.cos(),
            )
        } else {
            scaled
        };

        Point::new(rotated.x + self.offset.x, rotated.y + self.offset.y)
    }
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
    let blocks = collect_blocks(&pairs);
    let entity_ranges = section_ranges(&pairs, "ENTITIES");
    let mut strokes = Vec::new();

    if entity_ranges.is_empty() {
        parse_entity_pairs(
            &pairs,
            segarc_degrees,
            &blocks,
            Transform::identity(),
            &mut strokes,
        );
    } else {
        for (start, end) in entity_ranges {
            parse_entity_pairs(
                &pairs[start..end],
                segarc_degrees,
                &blocks,
                Transform::identity(),
                &mut strokes,
            );
        }
    }

    strokes
}

fn parse_entity_pairs(
    pairs: &[(i32, String)],
    segarc_degrees: f64,
    blocks: &BTreeMap<String, Block>,
    transform: Transform,
    strokes: &mut Vec<Stroke>,
) {
    let mut idx = 0;

    while idx < pairs.len() {
        if pairs[idx].0 != 0 {
            idx += 1;
            continue;
        }

        match pairs[idx].1.as_str() {
            "LINE" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                let mut entity_strokes = Vec::new();
                parse_line_entity(entity, &mut entity_strokes);
                append_transformed_strokes(&entity_strokes, transform, strokes);
                idx = next;
            }
            "ARC" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                let mut entity_strokes = Vec::new();
                parse_arc_entity(entity, segarc_degrees, &mut entity_strokes);
                append_transformed_strokes(&entity_strokes, transform, strokes);
                idx = next;
            }
            "CIRCLE" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                let mut entity_strokes = Vec::new();
                parse_circle_entity(entity, segarc_degrees, &mut entity_strokes);
                append_transformed_strokes(&entity_strokes, transform, strokes);
                idx = next;
            }
            "LEADER" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                let mut entity_strokes = Vec::new();
                parse_leader_entity(entity, &mut entity_strokes);
                append_transformed_strokes(&entity_strokes, transform, strokes);
                idx = next;
            }
            "LWPOLYLINE" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                let mut entity_strokes = Vec::new();
                parse_lwpolyline_entity(entity, segarc_degrees, &mut entity_strokes);
                append_transformed_strokes(&entity_strokes, transform, strokes);
                idx = next;
            }
            "POLYLINE" => {
                let mut entity_strokes = Vec::new();
                idx = parse_polyline_entities(pairs, idx + 1, segarc_degrees, &mut entity_strokes);
                append_transformed_strokes(&entity_strokes, transform, strokes);
            }
            "INSERT" => {
                let (entity, next) = collect_entity(pairs, idx + 1);
                parse_insert_entity(entity, segarc_degrees, blocks, transform, strokes);
                idx = next;
            }
            _ => idx += 1,
        }
    }
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

fn section_ranges(pairs: &[(i32, String)], name: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut idx = 0;

    while idx < pairs.len() {
        if pairs[idx].0 == 0 && pairs[idx].1 == "SECTION" {
            idx += 1;
            let section_name = if idx < pairs.len() && pairs[idx].0 == 2 {
                let value = pairs[idx].1.clone();
                idx += 1;
                value
            } else {
                String::new()
            };
            let start = idx;
            while idx < pairs.len() && !(pairs[idx].0 == 0 && pairs[idx].1 == "ENDSEC") {
                idx += 1;
            }
            if section_name == name {
                ranges.push((start, idx));
            }
        }
        idx += 1;
    }

    ranges
}

fn collect_blocks(pairs: &[(i32, String)]) -> BTreeMap<String, Block> {
    let mut blocks = BTreeMap::new();

    for (start, end) in section_ranges(pairs, "BLOCKS") {
        let mut idx = start;
        while idx < end {
            if !(pairs[idx].0 == 0 && pairs[idx].1 == "BLOCK") {
                idx += 1;
                continue;
            }

            let (header, next) = collect_entity(pairs, idx + 1);
            let name = header
                .iter()
                .find_map(|(code, value)| (*code == 2).then(|| value.clone()));
            let base_x = header
                .iter()
                .find_map(|(code, value)| (*code == 10).then(|| value.parse().ok()).flatten())
                .unwrap_or(0.0);
            let base_y = header
                .iter()
                .find_map(|(code, value)| (*code == 20).then(|| value.parse().ok()).flatten())
                .unwrap_or(0.0);

            let content_start = next;
            let mut content_end = content_start;
            while content_end < end
                && !(pairs[content_end].0 == 0 && pairs[content_end].1 == "ENDBLK")
            {
                content_end += 1;
            }

            if let Some(name) = name {
                blocks.insert(
                    name,
                    Block {
                        base: Point::new(base_x, base_y),
                        pairs: pairs[content_start..content_end].to_vec(),
                    },
                );
            }

            idx = if content_end < end {
                content_end + 1
            } else {
                content_end
            };
        }
    }

    blocks
}

fn append_transformed_strokes(source: &[Stroke], transform: Transform, strokes: &mut Vec<Stroke>) {
    strokes.extend(source.iter().map(|stroke| Stroke {
        start: transform.apply(stroke.start),
        end: transform.apply(stroke.end),
    }));
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

fn parse_insert_entity(
    entity: &[(i32, String)],
    segarc_degrees: f64,
    blocks: &BTreeMap<String, Block>,
    parent_transform: Transform,
    strokes: &mut Vec<Stroke>,
) {
    let mut name: Option<&str> = None;
    let mut x: Option<f64> = None;
    let mut y: Option<f64> = None;
    let mut xscale = 1.0_f64;
    let mut yscale = 1.0_f64;
    let mut rotate = 0.0_f64;

    for (code, value) in entity {
        match *code {
            2 => name = Some(value.as_str()),
            10 => x = value.parse().ok(),
            20 => y = value.parse().ok(),
            41 => xscale = value.parse().unwrap_or(1.0),
            42 => yscale = value.parse().unwrap_or(1.0),
            50 => rotate = value.parse().unwrap_or(0.0),
            _ => {}
        }
    }

    let (Some(name), Some(x), Some(y)) = (name, x, y) else {
        return;
    };
    let Some(block) = blocks.get(name) else {
        return;
    };

    let transform = Transform {
        offset: Point::new(
            x + parent_transform.offset.x - block.base.x,
            y + parent_transform.offset.y - block.base.y,
        ),
        scale: Point::new(xscale, yscale),
        rotate_degrees: rotate,
    };
    parse_entity_pairs(&block.pairs, segarc_degrees, blocks, transform, strokes);
}

fn parse_leader_entity(entity: &[(i32, String)], strokes: &mut Vec<Stroke>) {
    let vertices = collect_xy_vertices(entity);
    for pair in vertices.windows(2) {
        strokes.push(Stroke {
            start: pair[0],
            end: pair[1],
        });
    }
}

fn collect_xy_vertices(entity: &[(i32, String)]) -> Vec<Point> {
    let mut vertices = Vec::new();
    let mut current_x = None;
    let mut current_y = None;

    for (code, value) in entity {
        match *code {
            10 => {
                if let (Some(x), Some(y)) = (current_x.take(), current_y.take()) {
                    vertices.push(Point::new(x, y));
                }
                current_x = value.parse().ok();
            }
            20 => current_y = value.parse().ok(),
            _ => {}
        }
    }

    if let (Some(x), Some(y)) = (current_x, current_y) {
        vertices.push(Point::new(x, y));
    }

    vertices
}

fn parse_arc_entity(entity: &[(i32, String)], segarc_degrees: f64, strokes: &mut Vec<Stroke>) {
    let mut center_x = None;
    let mut center_y = None;
    let mut radius = None;
    let mut start = None;
    let mut end = None;

    for (code, value) in entity {
        match *code {
            10 => center_x = value.parse().ok(),
            20 => center_y = value.parse().ok(),
            40 => radius = value.parse().ok(),
            50 => start = value.parse().ok(),
            51 => end = value.parse().ok(),
            _ => {}
        }
    }

    if let (Some(x), Some(y), Some(radius), Some(start), Some(end)) =
        (center_x, center_y, radius, start, end)
    {
        append_arc_segments(
            Point::new(x, y),
            radius,
            start,
            end,
            segarc_degrees,
            strokes,
        );
    }
}

fn parse_circle_entity(entity: &[(i32, String)], segarc_degrees: f64, strokes: &mut Vec<Stroke>) {
    let mut center_x = None;
    let mut center_y = None;
    let mut radius = None;

    for (code, value) in entity {
        match *code {
            10 => center_x = value.parse().ok(),
            20 => center_y = value.parse().ok(),
            40 => radius = value.parse().ok(),
            _ => {}
        }
    }

    if let (Some(x), Some(y), Some(radius)) = (center_x, center_y, radius) {
        append_arc_segments(
            Point::new(x, y),
            radius,
            0.0,
            360.0,
            segarc_degrees,
            strokes,
        );
    }
}

fn append_arc_segments(
    center: Point,
    radius: f64,
    start_degrees: f64,
    mut end_degrees: f64,
    segarc_degrees: f64,
    strokes: &mut Vec<Stroke>,
) {
    if radius.abs() <= 1.0e-12 {
        return;
    }

    while end_degrees < start_degrees {
        end_degrees += 360.0;
    }
    let delta = end_degrees - start_degrees;
    let steps = ((delta / segarc_degrees.max(1.0)).floor() as usize).max(2);
    let step = delta.to_radians() / steps as f64;
    let mut previous_angle = start_degrees.to_radians();
    let mut previous = Point::new(
        center.x + radius * previous_angle.cos(),
        center.y + radius * previous_angle.sin(),
    );

    for _ in 0..steps {
        let next_angle = previous_angle + step;
        let next = Point::new(
            center.x + radius * next_angle.cos(),
            center.y + radius * next_angle.sin(),
        );
        strokes.push(Stroke {
            start: previous,
            end: next,
        });
        previous_angle = next_angle;
        previous = next;
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
    fn inserts_block_entities_without_emitting_block_definition() {
        let strokes = parse_dxf_segments(
            "\
0
SECTION
2
BLOCKS
0
BLOCK
2
PART
10
0
20
0
0
LINE
10
0
20
0
11
1
21
0
0
ENDBLK
0
ENDSEC
0
SECTION
2
ENTITIES
0
INSERT
2
PART
10
2
20
3
0
ENDSEC
0
EOF
",
            5.0,
        );

        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].start, Point::new(2.0, 3.0));
        assert_eq!(strokes[0].end, Point::new(3.0, 3.0));
    }

    #[test]
    fn insert_applies_block_base_scale_and_rotation() {
        let strokes = parse_dxf_segments(
            "\
0
SECTION
2
BLOCKS
0
BLOCK
2
PART
10
1
20
1
0
LINE
10
1
20
1
11
2
21
1
0
ENDBLK
0
ENDSEC
0
SECTION
2
ENTITIES
0
INSERT
2
PART
10
2
20
3
41
2
42
3
50
90
0
ENDSEC
0
EOF
",
            5.0,
        );

        assert_eq!(strokes.len(), 1);
        assert_point_close(strokes[0].start, Point::new(-2.0, 4.0));
        assert_point_close(strokes[0].end, Point::new(-2.0, 6.0));
    }

    #[test]
    fn parses_leader_entities() {
        let strokes = parse_dxf_segments(
            "0\nLEADER\n10\n0\n20\n0\n10\n1\n20\n0\n10\n1\n20\n1\n0\nENDSEC\n",
            5.0,
        );

        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[0].start, Point::new(0.0, 0.0));
        assert_eq!(strokes[0].end, Point::new(1.0, 0.0));
        assert_eq!(strokes[1].end, Point::new(1.0, 1.0));
    }

    #[test]
    fn approximates_arc_entities() {
        let strokes = parse_dxf_segments(
            "0\nARC\n10\n0\n20\n0\n40\n1\n50\n0\n51\n90\n0\nENDSEC\n",
            45.0,
        );

        assert_eq!(strokes.len(), 2);
        assert_point_close(strokes[0].start, Point::new(1.0, 0.0));
        assert_point_close(
            strokes[0].end,
            Point::new(2.0f64.sqrt() / 2.0, 2.0f64.sqrt() / 2.0),
        );
        assert_point_close(strokes[1].end, Point::new(0.0, 1.0));
    }

    #[test]
    fn approximates_circle_entities() {
        let strokes = parse_dxf_segments("0\nCIRCLE\n10\n1\n20\n2\n40\n2\n0\nENDSEC\n", 90.0);

        assert_eq!(strokes.len(), 4);
        assert_point_close(strokes[0].start, Point::new(3.0, 2.0));
        assert_point_close(strokes[0].end, Point::new(1.0, 4.0));
        assert_point_close(strokes.last().unwrap().end, Point::new(3.0, 2.0));
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

    fn assert_point_close(actual: Point, expected: Point) {
        assert!((actual.x - expected.x).abs() < 1e-9);
        assert!((actual.y - expected.y).abs() < 1e-9);
    }
}
