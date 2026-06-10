use crate::geometry::Point;
use crate::layout::EngraveSegment;
use crate::settings::LegacySettings;

const ZERO: f64 = 0.00001;

#[derive(Debug, Clone, PartialEq)]
pub struct VCarveOptions {
    pub step_len: f64,
    pub bit_shape: BitShape,
    pub bit_angle_degrees: f64,
    pub bit_diameter: f64,
    pub depth_limit: f64,
    pub inlay: bool,
    pub allowance: f64,
    pub inlay_depth: f64,
}

impl VCarveOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        let mut step_len = get_f64(settings, "v_step_len", 0.01);
        if settings.get_last("units") == Some("mm") {
            step_len = step_len.max(0.01);
        } else {
            step_len = step_len.max(0.0005);
        }

        Self {
            step_len,
            bit_shape: BitShape::parse(settings.get_last("bit_shape").unwrap_or("VBIT")),
            bit_angle_degrees: get_f64(settings, "v_bit_angle", 60.0),
            bit_diameter: get_f64(settings, "v_bit_dia", 0.5),
            depth_limit: get_f64(settings, "v_depth_lim", 0.0),
            inlay: get_bool(settings, "inlay", false),
            allowance: get_f64(settings, "allowance", 0.0),
            inlay_depth: get_f64(settings, "v_max_cut", 0.0),
        }
    }

    pub fn effective_bit_diameter(&self) -> f64 {
        if self.inlay && self.bit_shape == BitShape::VBit {
            let diameter = -2.0 * self.allowance * self.half_angle().tan();
            return diameter.max(0.001);
        }

        if self.depth_limit < 0.0 {
            match self.bit_shape {
                BitShape::VBit => -2.0 * self.depth_limit * self.half_angle().tan(),
                BitShape::Ball => {
                    let radius = self.bit_diameter / 2.0;
                    if self.depth_limit > -radius {
                        2.0 * (radius * radius - (radius + self.depth_limit).powi(2)).sqrt()
                    } else {
                        self.bit_diameter
                    }
                }
                BitShape::Flat => self.bit_diameter,
            }
        } else {
            self.bit_diameter
        }
    }

    pub fn max_radius(&self) -> f64 {
        (self.effective_bit_diameter() / 2.0).max(0.0)
    }

    pub fn depth_for_radius(&self, radius: f64) -> f64 {
        match self.bit_shape {
            BitShape::VBit => {
                let mut depth = -radius / self.half_angle().tan();
                if self.inlay {
                    depth += self.inlay_depth;
                }
                depth
            }
            BitShape::Ball => {
                let bit_radius = (self.bit_diameter / 2.0).max(ZERO);
                let radius = radius.min(bit_radius);
                let theta = (radius / bit_radius).clamp(-1.0, 1.0).acos();
                -bit_radius * (1.0 - theta.sin())
            }
            BitShape::Flat => -self.bit_diameter / 2.0,
        }
    }

    fn half_angle(&self) -> f64 {
        (self.bit_angle_degrees / 2.0).to_radians()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitShape {
    VBit,
    Ball,
    Flat,
}

impl BitShape {
    fn parse(value: &str) -> Self {
        match value {
            "BALL" => Self::Ball,
            "FLAT" => Self::Flat,
            _ => Self::VBit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VCarvePoint {
    pub position: Point,
    pub radius: f64,
    pub loop_id: usize,
}

pub fn generate_vcarve_points(
    segments: &[EngraveSegment],
    options: &VCarveOptions,
    accuracy: f64,
) -> Vec<VCarvePoint> {
    let mut output = Vec::new();
    for (loop_index, path) in collect_paths(segments, accuracy).into_iter().enumerate() {
        if path.len() < 2 {
            continue;
        }
        append_path_vcarve_points(&path, loop_index + 1, options, &mut output);
    }
    output
}

fn append_path_vcarve_points(
    path: &[Point],
    loop_id: usize,
    options: &VCarveOptions,
    output: &mut Vec<VCarvePoint>,
) {
    let closed = point_distance(path[0], *path.last().unwrap()) <= ZERO;
    let area = signed_area(path);
    let max_radius = options.max_radius();

    for (idx, pair) in path.windows(2).enumerate() {
        let start = pair[0];
        let end = pair[1];
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length = (dx * dx + dy * dy).sqrt();
        if length <= ZERO {
            continue;
        }

        let steps = ((length / options.step_len).floor() as usize).max(2);
        let tangent = Point::new(dx / length, dy / length);
        let inward = inward_normal(tangent, area, closed);

        for step in 0..steps {
            let t = step as f64 / steps as f64;
            let outline = Point::new(start.x + dx * t, start.y + dy * t);
            let radius = if step == 0 {
                0.0
            } else {
                max_clear_radius(path, idx, outline, inward, max_radius)
            };
            output.push(VCarvePoint {
                position: Point::new(outline.x + inward.x * radius, outline.y + inward.y * radius),
                radius,
                loop_id,
            });
        }
    }

    if closed {
        output.push(VCarvePoint {
            position: path[0],
            radius: 0.0,
            loop_id,
        });
    }
}

fn collect_paths(segments: &[EngraveSegment], accuracy: f64) -> Vec<Vec<Point>> {
    let mut paths = Vec::new();
    let mut last_end = None;
    let mut current_loop = None;

    for segment in segments {
        let starts_new = current_loop != Some(segment.loop_id)
            || last_end
                .map(|last| point_distance(last, segment.start) > accuracy)
                .unwrap_or(true);
        if starts_new {
            paths.push(vec![segment.start]);
        }
        paths.last_mut().unwrap().push(segment.end);
        last_end = Some(segment.end);
        current_loop = Some(segment.loop_id);
    }

    paths
}

fn inward_normal(tangent: Point, area: f64, closed: bool) -> Point {
    if closed && area >= 0.0 {
        Point::new(-tangent.y, tangent.x)
    } else {
        Point::new(tangent.y, -tangent.x)
    }
}

fn max_clear_radius(
    path: &[Point],
    current_segment: usize,
    outline: Point,
    inward: Point,
    max_radius: f64,
) -> f64 {
    let mut radius = max_radius;
    for (idx, pair) in path.windows(2).enumerate() {
        if idx == current_segment {
            continue;
        }
        let nearest = nearest_point_on_segment(outline, pair[0], pair[1]);
        let toward = Point::new(nearest.x - outline.x, nearest.y - outline.y);
        if dot(toward, inward) <= ZERO {
            continue;
        }
        radius = radius.min(point_distance(outline, nearest) / 2.0);
    }
    radius.max(0.0)
}

fn nearest_point_on_segment(point: Point, start: Point, end: Point) -> Point {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length2 = dx * dx + dy * dy;
    if length2 <= ZERO {
        return start;
    }
    let t = ((point.x - start.x) * dx + (point.y - start.y) * dy) / length2;
    Point::new(
        start.x + dx * t.clamp(0.0, 1.0),
        start.y + dy * t.clamp(0.0, 1.0),
    )
}

fn signed_area(path: &[Point]) -> f64 {
    path.windows(2)
        .map(|pair| pair[0].x * pair[1].y - pair[1].x * pair[0].y)
        .sum::<f64>()
        / 2.0
}

fn point_distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_bool(settings: &LegacySettings, key: &str, default: bool) -> bool {
    settings
        .get_last(key)
        .map(|value| matches!(value, "1" | "true" | "True"))
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_legacy_settings;

    #[test]
    fn vbit_depth_uses_included_angle() {
        let options = VCarveOptions::from_legacy(&default_legacy_settings());

        assert!((options.depth_for_radius(0.5) + 0.8660254038).abs() < 1e-9);
    }

    #[test]
    fn depth_limit_reduces_effective_vbit_diameter() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("v_depth_lim", "-0.2", false);
        let options = VCarveOptions::from_legacy(&settings);

        assert!((options.effective_bit_diameter() - 0.2309401077).abs() < 1e-9);
    }

    #[test]
    fn inlay_vbit_diameter_uses_allowance() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("inlay", "1", false);
        settings.set_or_push("allowance", "-0.1", false);
        let options = VCarveOptions::from_legacy(&settings);

        assert!((options.effective_bit_diameter() - 0.1154700538).abs() < 1e-9);
    }

    #[test]
    fn generates_variable_radius_points_for_closed_square() {
        let segments = square_segments();
        let mut settings = default_legacy_settings();
        settings.set_or_push("v_step_len", "0.5", false);
        let options = VCarveOptions::from_legacy(&settings);

        let points = generate_vcarve_points(&segments, &options, 0.001);

        assert!(points.iter().any(|point| point.radius == 0.0));
        assert!(points.iter().any(|point| point.radius > 0.24));
        assert!(points.iter().any(|point| point.position.y > 0.2));
    }

    fn square_segments() -> Vec<EngraveSegment> {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
            Point::new(0.0, 0.0),
        ];
        points
            .windows(2)
            .map(|pair| EngraveSegment {
                start: pair[0],
                end: pair[1],
                loop_id: 1,
            })
            .collect()
    }
}
