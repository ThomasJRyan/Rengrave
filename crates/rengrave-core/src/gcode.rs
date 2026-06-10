use crate::layout::EngraveSegment;
use crate::settings::LegacySettings;
use crate::vcarve::{VCarveOptions, VCarvePoint};

const ZERO: f64 = 0.00001;
const MAX_RADIUS: f64 = 1.0e30;

#[derive(Debug, Clone, PartialEq)]
pub struct GcodeOptions {
    pub safe_z: f64,
    pub depth_z: f64,
    pub feed: f64,
    pub plunge: f64,
    pub accuracy: f64,
    pub units: Units,
    pub preamble: String,
    pub postamble: String,
    pub variables_disabled: bool,
    pub arc_fit: ArcFit,
}

impl GcodeOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        Self {
            safe_z: get_f64(settings, "ZSAFE", 0.25),
            depth_z: get_f64(settings, "ZCUT", -0.005),
            feed: get_f64(settings, "FEED", 5.0),
            plunge: get_f64(settings, "PLUNGE", 0.0),
            accuracy: get_f64(settings, "accuracy", 0.001),
            units: Units::parse(settings.get_last("units").unwrap_or("in")),
            preamble: settings
                .get_last("gpre")
                .unwrap_or("G17 G64 P0.001 M3 S3000")
                .to_owned(),
            postamble: settings.get_last("gpost").unwrap_or("M5|M2").to_owned(),
            variables_disabled: get_bool(settings, "var_dis", true),
            arc_fit: ArcFit::parse(settings.get_last("arc_fit").unwrap_or("none")),
        }
    }

    fn coord_digits(&self) -> usize {
        match self.units {
            Units::Inch => 4,
            Units::Mm => 3,
        }
    }

    fn feed_digits(&self) -> usize {
        match self.units {
            Units::Inch => 2,
            Units::Mm => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcFit {
    None,
    Center,
    Radius,
}

impl ArcFit {
    fn parse(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            "radius" => Self::Radius,
            _ => Self::None,
        }
    }

    fn enabled(self) -> bool {
        self != Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Inch,
    Mm,
}

impl Units {
    fn parse(value: &str) -> Self {
        if value == "mm" { Self::Mm } else { Self::Inch }
    }

    fn gcode(self) -> &'static str {
        match self {
            Self::Inch => "G20",
            Self::Mm => "G21",
        }
    }
}

pub fn write_engrave_gcode(segments: &[EngraveSegment], options: &GcodeOptions) -> Vec<String> {
    let dp = options.coord_digits();
    let dpfeed = options.feed_digits();
    let safe_number = format_number(options.safe_z, dp);
    let depth_number = format_number(options.depth_z, dp);
    let safe_value = if options.variables_disabled {
        safe_number.clone()
    } else {
        "#1".to_owned()
    };
    let depth_value = if options.variables_disabled {
        depth_number.clone()
    } else {
        "#2".to_owned()
    };
    let feed = format_number(options.feed, dpfeed);
    let mut plunge = format_number(options.plunge, dpfeed);
    let zero_feed = format_number(0.0, dpfeed);
    if plunge == zero_feed {
        plunge = feed.clone();
    }

    let mut lines = Vec::new();
    if options.arc_fit.enabled() {
        lines.push("G17".to_owned());
    }
    if !options.variables_disabled {
        lines.push(format!("#1 = {}  ( Safe Z )", safe_number));
        lines.push(format!("#2 = {}  ( Engraving Depth Z )", depth_number));
    }
    lines.push("G90".to_owned());
    if options.arc_fit == ArcFit::Center {
        lines.push("G91.1".to_owned());
    }
    lines.push(options.units.gcode().to_owned());
    lines.extend(split_gcode_lines(&options.preamble));
    lines.push(format!("F{feed}"));

    for path in order_paths(segments, options.accuracy) {
        let Some(first) = path.first() else {
            continue;
        };
        lines.push(format!("G0 Z{safe_value}"));
        lines.push(format!(
            "G0 X{} Y{}",
            format_number(first.x, dp),
            format_number(first.y, dp)
        ));
        if plunge == feed {
            lines.push(format!("G1 Z{depth_value}"));
        } else {
            lines.push(format!("G1 Z{depth_value} F{plunge}"));
        }

        emit_cut_path(&mut lines, &path, options, dp);
    }

    lines.push(format!("G0 Z{safe_value}"));
    lines.extend(split_gcode_lines(&options.postamble));
    lines
}

pub fn write_vcarve_gcode(
    points: &[VCarvePoint],
    gcode_options: &GcodeOptions,
    vcarve_options: &VCarveOptions,
) -> Vec<String> {
    let dp = gcode_options.coord_digits();
    let dpfeed = gcode_options.feed_digits();
    let safe_number = format_number(gcode_options.safe_z, dp);
    let safe_value = if gcode_options.variables_disabled {
        safe_number.clone()
    } else {
        "#1".to_owned()
    };
    let feed = format_number(gcode_options.feed, dpfeed);
    let mut plunge = format_number(gcode_options.plunge, dpfeed);
    let zero_feed = format_number(0.0, dpfeed);
    if plunge == zero_feed {
        plunge = feed.clone();
    }

    let mut lines = Vec::new();
    if !gcode_options.variables_disabled {
        lines.push(format!("#1 = {}  ( Safe Z )", safe_number));
    }
    lines.push("G90".to_owned());
    lines.push(gcode_options.units.gcode().to_owned());
    lines.extend(split_gcode_lines(&gcode_options.preamble));
    lines.push(format!("F{feed}"));

    let mut current_loop = None;
    for point in points {
        let z = vcarve_options.depth_for_radius(point.radius);
        if current_loop != Some(point.loop_id) {
            lines.push(format!("G0 Z{safe_value}"));
            lines.push(format!(
                "G0 X{} Y{}",
                format_number(point.position.x, dp),
                format_number(point.position.y, dp)
            ));
            if plunge == feed {
                lines.push(format!("G1 Z{}", format_number(z, dp)));
            } else {
                lines.push(format!("G1 Z{} F{plunge}", format_number(z, dp)));
                lines.push(format!("F{feed}"));
            }
        } else {
            lines.push(format!(
                "G1 X{} Y{} Z{}",
                format_number(point.position.x, dp),
                format_number(point.position.y, dp),
                format_number(z, dp)
            ));
        }
        current_loop = Some(point.loop_id);
    }

    lines.push(format!("G0 Z{safe_value}"));
    lines.extend(split_gcode_lines(&gcode_options.postamble));
    lines
}

fn emit_cut_path(
    lines: &mut Vec<String>,
    path: &[crate::geometry::Point],
    options: &GcodeOptions,
    digits: usize,
) {
    let moves = fit_path(path, options.accuracy, options.arc_fit);
    let mut current = None;

    for motion in moves {
        match motion {
            Motion::Line(point) => {
                let command = format!(
                    "G1 X{} Y{}",
                    format_number(point.x, digits),
                    format_number(point.y, digits)
                );
                lines.push(command);
                current = Some(point);
            }
            Motion::Arc { cw, end, center } => {
                let Some(start) = current else {
                    lines.push(format!(
                        "G1 X{} Y{}",
                        format_number(end.x, digits),
                        format_number(end.y, digits)
                    ));
                    current = Some(end);
                    continue;
                };
                let code = if cw { "G2" } else { "G3" };
                if options.arc_fit == ArcFit::Radius {
                    let r1 = distance(start, center);
                    let r2 = distance(end, center);
                    let radius = (r1 + r2) / 2.0;
                    lines.push(format!(
                        "{code} X{} Y{} R{}",
                        format_number(end.x, digits),
                        format_number(end.y, digits),
                        format_number(radius, digits)
                    ));
                } else {
                    lines.push(format!(
                        "{code} X{} Y{} I{} J{}",
                        format_number(end.x, digits),
                        format_number(end.y, digits),
                        format_number(center.x - start.x, digits),
                        format_number(center.y - start.y, digits)
                    ));
                }
                current = Some(end);
            }
        }
    }
}

fn split_gcode_lines(value: &str) -> impl Iterator<Item = String> + '_ {
    value.split('|').map(str::to_owned)
}

fn order_paths(segments: &[EngraveSegment], accuracy: f64) -> Vec<Vec<crate::geometry::Point>> {
    let mut paths: Vec<Vec<crate::geometry::Point>> = Vec::new();
    let mut last_end = None;
    let mut current_loop = None;

    for segment in segments {
        let starts_new = current_loop != Some(segment.loop_id)
            || last_end
                .map(|last: crate::geometry::Point| distance(last, segment.start) > accuracy)
                .unwrap_or(true);
        if starts_new {
            paths.push(vec![segment.start]);
        }
        paths.last_mut().unwrap().push(segment.end);
        last_end = Some(segment.end);
        current_loop = Some(segment.loop_id);
    }

    sort_paths(paths)
}

fn sort_paths(mut paths: Vec<Vec<crate::geometry::Point>>) -> Vec<Vec<crate::geometry::Point>> {
    if paths.is_empty() {
        return paths;
    }

    let mut ordered = vec![paths.remove(0)];
    while !paths.is_empty() {
        let current = *ordered.last().and_then(|path| path.last()).unwrap();
        let mut best_index = 0;
        let mut best_reverse = false;
        let mut best_distance = distance(current, paths[0][0]);
        let mut best_end_distance = distance(current, *paths[0].last().unwrap());

        for (idx, path) in paths.iter().enumerate().skip(1) {
            let begin_distance = distance(current, path[0]);
            if begin_distance < best_distance {
                best_distance = begin_distance;
                best_index = idx;
                best_reverse = false;
            }
            let end_distance = distance(current, *path.last().unwrap());
            if end_distance < best_end_distance {
                best_end_distance = end_distance;
                best_index = idx;
                best_reverse = true;
            }
        }

        let mut next = paths.remove(best_index);
        if best_reverse {
            next.reverse();
        }
        ordered.push(next);
    }

    ordered
}

fn distance(a: crate::geometry::Point, b: crate::geometry::Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Motion {
    Line(crate::geometry::Point),
    Arc {
        cw: bool,
        end: crate::geometry::Point,
        center: crate::geometry::Point,
    },
}

fn fit_path(path: &[crate::geometry::Point], tolerance: f64, arc_fit: ArcFit) -> Vec<Motion> {
    if path.is_empty() {
        Vec::new()
    } else {
        douglas(path.to_vec(), tolerance, arc_fit, true)
    }
}

fn douglas(
    mut points: Vec<crate::geometry::Point>,
    tolerance: f64,
    arc_fit: ArcFit,
    first: bool,
) -> Vec<Motion> {
    if points.len() == 1 {
        return vec![Motion::Line(points[0])];
    }

    let start = points[0];
    let mut end = *points.last().unwrap();
    let mut closed_point = None;

    while same_point(start, end) {
        closed_point = points.pop();
        let Some(last) = points.last().copied() else {
            return Vec::new();
        };
        end = last;
    }

    let mut worst_dist = 0.0;
    let mut worst_index = 0usize;
    let mut min_radius = MAX_RADIUS;
    let mut arc_index = None;

    for (idx, point) in points.iter().enumerate() {
        if idx == 0 || idx == points.len() - 1 {
            continue;
        }
        let dist = dist_segment(start, end, *point);
        if dist > worst_dist {
            worst_dist = dist;
            worst_index = idx;
            if arc_fit.enabled() {
                let radius = arc_radius(start, *point, end);
                if radius < min_radius {
                    min_radius = radius;
                    arc_index = Some(idx);
                }
            }
        }
    }

    let arc = arc_index.and_then(|idx| fit_arc(&points, idx, min_radius));
    if let Some(arc) = arc {
        if arc.worst_dist < tolerance && arc.worst_dist < worst_dist {
            let mut output = Vec::new();
            output.push(Motion::Line(start));
            output.push(Motion::Arc {
                cw: !arc.ccw,
                end,
                center: arc.center,
            });
            if closed_point.is_some() {
                output.push(Motion::Line(start));
            }
            return output;
        }
    }

    let mut output = Vec::new();
    if worst_dist > tolerance {
        if first {
            output.push(Motion::Line(start));
        }
        output.extend(douglas(
            points[..=worst_index].to_vec(),
            tolerance,
            arc_fit,
            false,
        ));
        output.push(Motion::Line(points[worst_index]));
        output.extend(douglas(
            points[worst_index..].to_vec(),
            tolerance,
            arc_fit,
            false,
        ));
        if first {
            output.push(Motion::Line(end));
        }
    } else {
        if first {
            output.push(Motion::Line(start));
            output.push(Motion::Line(end));
        }
    }

    if closed_point.is_some() {
        output.push(Motion::Line(start));
    }
    output
}

#[derive(Debug, Clone, Copy)]
struct ArcCandidate {
    center: crate::geometry::Point,
    ccw: bool,
    worst_dist: f64,
}

fn fit_arc(
    points: &[crate::geometry::Point],
    arc_index: usize,
    radius: f64,
) -> Option<ArcCandidate> {
    if !radius.is_finite() || radius >= MAX_RADIUS {
        return None;
    }
    let start = points[0];
    let mid = points[arc_index];
    let end = *points.last().unwrap();
    let center = arc_center(start, mid, end)?;
    if !one_quadrant(center, start, mid, end) {
        return None;
    }

    let mut worst_dist = 0.0;
    let mut previous = start;
    for point in points {
        let dist = (distance(center, *point) - radius).abs();
        if dist > worst_dist {
            worst_dist = dist;
        }

        let midpoint =
            crate::geometry::Point::new((point.x + previous.x) / 2.0, (point.y + previous.y) / 2.0);
        let mid_dist = (distance(center, midpoint) - radius).abs();
        if mid_dist > worst_dist {
            worst_dist = mid_dist;
        }
        previous = *point;
    }

    Some(ArcCandidate {
        center,
        ccw: arc_ccw(start, mid, end),
        worst_dist,
    })
}

fn same_point(a: crate::geometry::Point, b: crate::geometry::Point) -> bool {
    (a.x - b.x).abs() < ZERO && (a.y - b.y).abs() < ZERO
}

fn dist_segment(
    start: crate::geometry::Point,
    end: crate::geometry::Point,
    point: crate::geometry::Point,
) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let d2 = dx * dx + dy * dy;
    if d2 == 0.0 {
        return 0.0;
    }

    let mut t = (dx * (point.x - start.x) + dy * (point.y - start.y)) / d2;
    t = t.clamp(0.0, 1.0);
    let x = start.x + t * dx;
    let y = start.y + t * dy;
    let ex = point.x - x;
    let ey = point.y - y;
    (ex * ex + ey * ey).sqrt()
}

fn arc_radius(
    a: crate::geometry::Point,
    b: crate::geometry::Point,
    c: crate::geometry::Point,
) -> f64 {
    let x12 = a.x - b.x;
    let y12 = a.y - b.y;
    let x23 = b.x - c.x;
    let y23 = b.y - c.y;
    let x31 = c.x - a.x;
    let y31 = c.y - a.y;
    let den = (x12 * y23 - x23 * y12).abs();
    if den < 1.0e-5 {
        return MAX_RADIUS;
    }
    (x12.hypot(y12) * x23.hypot(y23) * x31.hypot(y31)) / (2.0 * den)
}

fn arc_center(
    a: crate::geometry::Point,
    b: crate::geometry::Point,
    c: crate::geometry::Point,
) -> Option<crate::geometry::Point> {
    let ab = crate::geometry::Point::new(a.x - b.x, a.y - b.y);
    let bc = crate::geometry::Point::new(b.x - c.x, b.y - c.y);
    let ac = crate::geometry::Point::new(a.x - c.x, a.y - c.y);
    let den = (ab.x * bc.y - ab.y * bc.x).abs();
    if den < 1.0e-5 {
        return None;
    }

    let den2 = 2.0 * den * den;
    let alpha = mag2(bc) * dot(ab, ac) / den2;
    let beta = mag2(ac) * dot(crate::geometry::Point::new(b.x - a.x, b.y - a.y), bc) / den2;
    let gamma = mag2(ab)
        * dot(
            crate::geometry::Point::new(c.x - a.x, c.y - a.y),
            crate::geometry::Point::new(c.x - b.x, c.y - b.y),
        )
        / den2;

    Some(crate::geometry::Point::new(
        alpha * a.x + beta * b.x + gamma * c.x,
        alpha * a.y + beta * b.y + gamma * c.y,
    ))
}

fn one_quadrant(
    center: crate::geometry::Point,
    start: crate::geometry::Point,
    mid: crate::geometry::Point,
    end: crate::geometry::Point,
) -> bool {
    let la = distance(start, mid);
    let lb = distance(end, mid);
    if la <= ZERO || lb <= ZERO {
        return false;
    }

    let theta_a = (start.y - mid.y)
        .atan2(start.x - mid.x)
        .to_degrees()
        .rem_euclid(360.0);
    let theta_b = (end.y - mid.y)
        .atan2(end.x - mid.x)
        .to_degrees()
        .rem_euclid(360.0);
    let angle = (theta_a - theta_b).abs();
    let test_angle = 36.0;
    if angle > 180.0 + test_angle || angle < 180.0 - test_angle {
        return false;
    }

    let mut signs = std::collections::BTreeSet::new();
    for point in [start, mid, end] {
        signs.insert((sign(point.x - center.x), sign(point.y - center.y)));
    }

    if signs.len() == 1 {
        return true;
    }
    if signs.contains(&(1, 1)) {
        signs.remove(&(1, 0));
        signs.remove(&(0, 1));
    }
    if signs.contains(&(1, -1)) {
        signs.remove(&(1, 0));
        signs.remove(&(0, -1));
    }
    if signs.contains(&(-1, 1)) {
        signs.remove(&(-1, 0));
        signs.remove(&(0, 1));
    }
    if signs.contains(&(-1, -1)) {
        signs.remove(&(-1, 0));
        signs.remove(&(0, -1));
    }
    signs.len() == 1
}

fn arc_ccw(
    start: crate::geometry::Point,
    mid: crate::geometry::Point,
    end: crate::geometry::Point,
) -> bool {
    let signed_area = (start.x * mid.y - mid.x * start.y)
        + (mid.x * end.y - end.x * mid.y)
        + (end.x * start.y - start.x * end.y);
    signed_area > 0.0
}

fn mag2(point: crate::geometry::Point) -> f64 {
    point.x * point.x + point.y * point.y
}

fn dot(a: crate::geometry::Point, b: crate::geometry::Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn sign(value: f64) -> i32 {
    if value.abs() < 1.0e-5 {
        0
    } else if value < 0.0 {
        -1
    } else {
        1
    }
}

fn format_number(value: f64, digits: usize) -> String {
    format!("{value:.digits$}")
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
    use crate::geometry::Point;

    #[test]
    fn writes_basic_engrave_moves() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: "G17 G64 P0.001 M3 S3000".to_owned(),
            postamble: "M5|M2".to_owned(),
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let lines = write_engrave_gcode(
            &[EngraveSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(1.0, 0.0),
                loop_id: 1,
            }],
            &options,
        );

        assert!(lines.contains(&"G90".to_owned()));
        assert!(lines.contains(&"G20".to_owned()));
        assert!(lines.contains(&"G1 Z-0.0050".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Y0.0000".to_owned()));
    }

    #[test]
    fn writes_arc_fit_center_format() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.01,
            units: Units::Inch,
            preamble: "G17 G64 P0.001 M3 S3000".to_owned(),
            postamble: "M5|M2".to_owned(),
            variables_disabled: true,
            arc_fit: ArcFit::Center,
        };
        let segments = shallow_circle_segments();
        let lines = write_engrave_gcode(&segments, &options);

        assert!(lines.contains(&"G17".to_owned()));
        assert!(lines.contains(&"G91.1".to_owned()));
        assert!(lines.iter().any(|line| line.starts_with("G3 ")));
        assert!(lines.iter().any(|line| line.contains(" I-1.0000 J0.0000")));
    }

    #[test]
    fn writes_arc_fit_radius_format() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.01,
            units: Units::Inch,
            preamble: "G17 G64 P0.001 M3 S3000".to_owned(),
            postamble: "M5|M2".to_owned(),
            variables_disabled: true,
            arc_fit: ArcFit::Radius,
        };
        let lines = write_engrave_gcode(&shallow_circle_segments(), &options);

        assert!(!lines.contains(&"G91.1".to_owned()));
        assert!(lines.iter().any(|line| line.starts_with("G3 ")));
        assert!(lines.iter().any(|line| line.contains(" R1.0000")));
    }

    #[test]
    fn variables_enabled_use_references_in_motion_lines() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: "G17 G64 P0.001 M3 S3000".to_owned(),
            postamble: "M5|M2".to_owned(),
            variables_disabled: false,
            arc_fit: ArcFit::None,
        };
        let lines = write_engrave_gcode(
            &[EngraveSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(1.0, 0.0),
                loop_id: 1,
            }],
            &options,
        );

        assert!(lines.contains(&"#1 = 0.2500  ( Safe Z )".to_owned()));
        assert!(lines.contains(&"G0 Z#1".to_owned()));
        assert!(lines.contains(&"G1 Z#2".to_owned()));
    }

    #[test]
    fn writes_variable_depth_vcarve_moves() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: "G17 G64 P0.001 M3 S3000".to_owned(),
            postamble: "M5|M2".to_owned(),
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let vcarve = VCarveOptions::from_legacy(&crate::settings::default_legacy_settings());
        let lines = write_vcarve_gcode(
            &[
                VCarvePoint {
                    position: Point::new(0.0, 0.0),
                    radius: 0.0,
                    loop_id: 1,
                },
                VCarvePoint {
                    position: Point::new(1.0, 0.0),
                    radius: 0.5,
                    loop_id: 1,
                },
            ],
            &options,
            &vcarve,
        );

        assert!(lines.contains(&"G1 Z-0.0000".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Y0.0000 Z-0.8660".to_owned()));
    }

    fn shallow_circle_segments() -> Vec<EngraveSegment> {
        let points = [
            Point::new(1.0, 0.0),
            Point::new(0.9914448614, 0.1305261922),
            Point::new(0.9659258263, 0.2588190451),
            Point::new(0.9238795325, 0.3826834324),
            Point::new(0.8660254038, 0.5),
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
