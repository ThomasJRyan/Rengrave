use std::f64::consts::{FRAC_PI_2, PI};

use crate::geometry::Point;
use crate::layout::{Bounds, EngraveSegment};
use crate::settings::{LegacySettings, get_legacy_bool};

const MIN_ANGLE_DEGREES: f64 = 1.0;
const MAX_ANGLE_DEGREES: f64 = 179.0;
const MIN_CORNER_STEPS: usize = 4;
const MAX_CORNER_STEPS: usize = 64;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
struct TenThousand;

impl clipper2::PointScaler for TenThousand {
    const MULTIPLIER: f64 = 10000.0;
}

type ClipPaths = clipper2::Paths<TenThousand>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileOptions {
    pub enabled: bool,
    pub margin: f64,
    pub corner_radius: f64,
    pub depth: f64,
    pub step_count: usize,
    pub endmill_diameter: f64,
    pub tab_count: usize,
    pub tab_height: f64,
    pub tab_width: f64,
    pub chamfer_enabled: bool,
    pub chamfer_depth: f64,
    pub chamfer_angle_degrees: f64,
    pub width: f64,
    pub height: f64,
    pub aspect_ratio: f64,
    pub trace_detail: f64,
    pub stroke_thickness: f64,
    pub origin: ProfileOrigin,
    pub alignment: ProfileOrigin,
    pub x_origin: f64,
    pub y_origin: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileOrigin {
    Default,
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    MidCenter,
    MidRight,
    BotLeft,
    BotCenter,
    BotRight,
    ArcCenter,
}

impl ProfileOrigin {
    fn parse(value: &str) -> Self {
        match value {
            "Top-Left" => Self::TopLeft,
            "Top-Center" => Self::TopCenter,
            "Top-Right" => Self::TopRight,
            "Mid-Left" => Self::MidLeft,
            "Mid-Center" => Self::MidCenter,
            "Mid-Right" => Self::MidRight,
            "Bot-Left" => Self::BotLeft,
            "Bot-Center" => Self::BotCenter,
            "Bot-Right" => Self::BotRight,
            "Arc-Center" => Self::ArcCenter,
            _ => Self::Default,
        }
    }
}

impl ProfileOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        Self {
            enabled: get_legacy_bool(settings, "profile_cut", false),
            margin: get_f64(settings, "profile_margin", 0.25),
            corner_radius: get_f64(settings, "profile_radius", 0.0),
            depth: get_f64(settings, "profile_depth", 0.125),
            step_count: get_usize(settings, "profile_steps", 1),
            endmill_diameter: get_f64(settings, "profile_endmill_dia", 0.25),
            tab_count: get_usize_allow_zero(settings, "profile_tabs", 0),
            tab_height: get_f64(settings, "profile_tab_height", 1.0 / 25.4),
            tab_width: get_f64(settings, "profile_tab_width", 0.0),
            chamfer_enabled: get_legacy_bool(settings, "profile_chamfer", false),
            chamfer_depth: get_f64(settings, "profile_chamfer_depth", 0.02),
            chamfer_angle_degrees: get_f64(settings, "profile_chamfer_angle", 60.0),
            width: get_f64(settings, "profile_width", 0.0),
            height: get_f64(settings, "profile_height", 0.0),
            aspect_ratio: get_f64(settings, "profile_aspect", 0.0),
            trace_detail: get_f64(settings, "profile_trace", 0.0),
            stroke_thickness: get_f64(settings, "STHICK", 0.01),
            origin: ProfileOrigin::parse(settings.get_last("origin").unwrap_or("Default")),
            alignment: ProfileOrigin::parse(
                settings.get_last("profile_align").unwrap_or("Mid-Center"),
            ),
            x_origin: get_f64(settings, "xorigin", 0.0),
            y_origin: get_f64(settings, "yorigin", 0.0),
        }
    }

    pub fn is_usable(self) -> bool {
        self.enabled && self.depth.abs() > f64::EPSILON && self.endmill_diameter > 0.0
    }

    pub fn chamfer_width(self) -> f64 {
        if !self.chamfer_enabled || self.chamfer_depth <= 0.0 {
            return 0.0;
        }
        let angle = self
            .chamfer_angle_degrees
            .clamp(MIN_ANGLE_DEGREES, MAX_ANGLE_DEGREES)
            .to_radians();
        self.chamfer_depth * (angle / 2.0).tan()
    }

    pub fn straight_offset(self) -> f64 {
        self.endmill_diameter / 2.0 + self.chamfer_width()
    }

    pub fn chamfer_offset(self) -> f64 {
        self.chamfer_width()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileTool {
    VBitChamfer,
    StraightEndmill,
}

impl ProfileTool {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::VBitChamfer => "profile_chamfer",
            Self::StraightEndmill => "profile",
        }
    }

    pub fn comment(self) -> &'static str {
        match self {
            Self::VBitChamfer => "R-Engrave V-bit profile chamfer operation",
            Self::StraightEndmill => "R-Engrave straight profile cut operation",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileOperation {
    pub tool: ProfileTool,
    pub path: Vec<Point>,
    pub depths: Vec<f64>,
    pub offset: f64,
    pub corner_radius: f64,
    pub tabs: Vec<ProfileTab>,
    pub tab_height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileTab {
    pub start_distance: f64,
    pub end_distance: f64,
}

pub fn generate_profile_operations(
    bounds: Bounds,
    options: &ProfileOptions,
    accuracy: f64,
) -> Vec<ProfileOperation> {
    generate_profile_operations_for_segments(bounds, &[], options, accuracy)
}

pub fn generate_profile_operations_for_segments(
    bounds: Bounds,
    segments: &[EngraveSegment],
    options: &ProfileOptions,
    accuracy: f64,
) -> Vec<ProfileOperation> {
    if !options.is_usable() {
        return Vec::new();
    }

    let mut operations = Vec::new();
    if options.chamfer_enabled && options.chamfer_depth > 0.0 {
        let offset = options.chamfer_offset();
        let path = profile_path_for_segments(bounds, segments, options, offset, accuracy);
        operations.push(ProfileOperation {
            tool: ProfileTool::VBitChamfer,
            path,
            depths: vec![-options.chamfer_depth.abs()],
            offset,
            corner_radius: options.corner_radius.max(0.0),
            tabs: Vec::new(),
            tab_height: 0.0,
        });
    }

    let offset = options.straight_offset();
    let path = profile_path_for_segments(bounds, segments, options, offset, accuracy);
    let tabs = profile_tabs(&path, options);
    operations.push(ProfileOperation {
        tool: ProfileTool::StraightEndmill,
        path,
        depths: profile_depths(options.depth, options.step_count),
        offset,
        corner_radius: options.corner_radius.max(0.0),
        tabs,
        tab_height: options.tab_height.max(0.0),
    });
    operations
}

pub fn profile_depths(depth: f64, step_count: usize) -> Vec<f64> {
    let target = -depth.abs();
    let step_count = step_count.max(1);
    if step_count == 1 {
        return vec![target];
    }

    let mut depths = Vec::new();
    let step_depth = depth.abs() / step_count as f64;
    for step in 1..=step_count {
        depths.push(-step_depth * step as f64);
    }
    *depths.last_mut().unwrap() = target;
    depths
}

pub fn profile_path(
    bounds: Bounds,
    options: &ProfileOptions,
    cutter_offset: f64,
    accuracy: f64,
) -> Vec<Point> {
    let margin = options.margin.max(0.0);
    let cutter_offset = cutter_offset.max(0.0);
    let expand = margin + cutter_offset;
    let min = Point::new(bounds.min.x - expand, bounds.min.y - expand);
    let max = Point::new(bounds.max.x + expand, bounds.max.y + expand);
    rounded_rect_path(
        min,
        max,
        options.corner_radius.max(0.0) + cutter_offset,
        accuracy,
    )
}

fn profile_path_for_segments(
    bounds: Bounds,
    segments: &[EngraveSegment],
    options: &ProfileOptions,
    cutter_offset: f64,
    accuracy: f64,
) -> Vec<Point> {
    let mut path = if options.trace_detail > 0.0 && !segments.is_empty() {
        trace_outline(segments, options, cutter_offset, accuracy)
            .unwrap_or_else(|| profile_path(bounds, options, cutter_offset, accuracy))
    } else {
        profile_path(bounds, options, cutter_offset, accuracy)
    };
    fit_profile_dimensions(&mut path, bounds, options);
    path
}

fn trace_outline(
    segments: &[EngraveSegment],
    options: &ProfileOptions,
    cutter_offset: f64,
    accuracy: f64,
) -> Option<Vec<Point>> {
    let paths = segments
        .iter()
        .filter(|segment| distance(segment.start, segment.end) > f64::EPSILON)
        .map(|segment| {
            vec![
                (segment.start.x, segment.start.y),
                (segment.end.x, segment.end.y),
            ]
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return None;
    }

    let offset = options.stroke_thickness.max(accuracy).max(0.001) / 2.0
        + options.margin.max(0.0)
        + cutter_offset.max(0.0);
    let tolerance = trace_tolerance(&paths, options.trace_detail, accuracy);
    let inflated = to_clip_paths(&paths)
        .inflate(
            offset,
            clipper2::JoinType::Round,
            clipper2::EndType::Round,
            0.0,
        )
        .simplify(tolerance, false);
    let fallback = inflated.clone();
    let union = clipper2::union(inflated.clone(), inflated, clipper2::FillRule::NonZero)
        .ok()
        .unwrap_or(fallback)
        .simplify(tolerance, false);
    let traced = from_clip_paths(union);
    if traced.is_empty() {
        return None;
    }
    if traced.len() == 1 {
        return traced.into_iter().next();
    }

    Some(convex_hull(
        &traced.into_iter().flatten().collect::<Vec<_>>(),
    ))
}

fn trace_tolerance(paths: &[Vec<(f64, f64)>], detail: f64, accuracy: f64) -> f64 {
    let (min_x, max_x, min_y, max_y) = paths.iter().flatten().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), (x, y)| {
            (min_x.min(*x), max_x.max(*x), min_y.min(*y), max_y.max(*y))
        },
    );
    let span = (max_x - min_x).max(max_y - min_y).max(accuracy.abs());
    let detail = detail.clamp(0.0, 100.0) / 100.0;
    (span * (1.0 - detail).powi(2))
        .max(accuracy.abs())
        .max(0.0001)
}

fn fit_profile_dimensions(path: &mut [Point], bounds: Bounds, options: &ProfileOptions) {
    if path.is_empty() {
        return;
    }
    let current = point_bounds(path);
    let current_width = current.max.x - current.min.x;
    let current_height = current.max.y - current.min.y;
    if current_width <= f64::EPSILON || current_height <= f64::EPSILON {
        return;
    }

    let ratio = (options.aspect_ratio > 0.0).then_some(options.aspect_ratio);
    let mut width = if options.width > 0.0 {
        options.width
    } else {
        current_width
    };
    let mut height = if options.height > 0.0 {
        options.height
    } else {
        current_height
    };
    if let Some(ratio) = ratio {
        if options.width > 0.0 {
            height = width / ratio;
        } else if options.height > 0.0 {
            width = height * ratio;
        } else {
            height = width / ratio;
        }
    }
    if options.width <= 0.0 && options.height <= 0.0 && ratio.is_none() {
        return;
    }

    let center = profile_anchor(current, options.alignment);
    let target_center = profile_anchor(bounds, options.alignment);
    for point in path {
        point.x = target_center.x + (point.x - center.x) * width / current_width;
        point.y = target_center.y + (point.y - center.y) * height / current_height;
    }
}

fn profile_anchor(bounds: Bounds, origin: ProfileOrigin) -> Point {
    let mid_x = (bounds.min.x + bounds.max.x) / 2.0;
    let mid_y = (bounds.min.y + bounds.max.y) / 2.0;
    match origin {
        ProfileOrigin::TopLeft => Point::new(bounds.min.x, bounds.max.y),
        ProfileOrigin::TopCenter => Point::new(mid_x, bounds.max.y),
        ProfileOrigin::TopRight => Point::new(bounds.max.x, bounds.max.y),
        ProfileOrigin::MidLeft => Point::new(bounds.min.x, mid_y),
        ProfileOrigin::MidCenter | ProfileOrigin::Default | ProfileOrigin::ArcCenter => {
            Point::new(mid_x, mid_y)
        }
        ProfileOrigin::MidRight => Point::new(bounds.max.x, mid_y),
        ProfileOrigin::BotLeft => Point::new(bounds.min.x, bounds.min.y),
        ProfileOrigin::BotCenter => Point::new(mid_x, bounds.min.y),
        ProfileOrigin::BotRight => Point::new(bounds.max.x, bounds.min.y),
    }
}

fn point_bounds(points: &[Point]) -> Bounds {
    let mut bounds = Bounds {
        min: points[0],
        max: points[0],
    };
    for point in points.iter().skip(1) {
        bounds.min.x = bounds.min.x.min(point.x);
        bounds.min.y = bounds.min.y.min(point.y);
        bounds.max.x = bounds.max.x.max(point.x);
        bounds.max.y = bounds.max.y.max(point.y);
    }
    bounds
}

fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut points = points.to_vec();
    points.sort_by(|left, right| {
        left.x
            .total_cmp(&right.x)
            .then_with(|| left.y.total_cmp(&right.y))
    });
    points.dedup_by(|left, right| distance(*left, *right) <= f64::EPSILON);
    if points.len() <= 2 {
        return points;
    }

    fn cross(origin: Point, a: Point, b: Point) -> f64 {
        (a.x - origin.x) * (b.y - origin.y) - (a.y - origin.y) * (b.x - origin.x)
    }
    let mut lower = Vec::new();
    for point in &points {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], *lower.last().unwrap(), *point) <= f64::EPSILON
        {
            lower.pop();
        }
        lower.push(*point);
    }
    let mut upper = Vec::new();
    for point in points.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], *upper.last().unwrap(), *point) <= f64::EPSILON
        {
            upper.pop();
        }
        upper.push(*point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower.push(lower[0]);
    lower
}

fn to_clip_paths(paths: &[Vec<(f64, f64)>]) -> ClipPaths {
    paths.to_vec().into()
}

fn from_clip_paths(paths: ClipPaths) -> Vec<Vec<Point>> {
    let paths: Vec<Vec<(f64, f64)>> = paths.into();
    paths
        .into_iter()
        .filter_map(|path| {
            let points = path
                .into_iter()
                .map(|(x, y)| Point::new(x, y))
                .collect::<Vec<_>>();
            (points.len() >= 3).then_some(points)
        })
        .collect()
}

fn rounded_rect_path(min: Point, max: Point, radius: f64, accuracy: f64) -> Vec<Point> {
    let width = max.x - min.x;
    let height = max.y - min.y;
    if width <= f64::EPSILON || height <= f64::EPSILON {
        return Vec::new();
    }

    let radius = radius.min(width / 2.0).min(height / 2.0);
    if radius <= f64::EPSILON {
        return vec![
            min,
            Point::new(max.x, min.y),
            max,
            Point::new(min.x, max.y),
            min,
        ];
    }

    let steps = rounded_corner_steps(radius, accuracy);
    let mut points = vec![Point::new(min.x + radius, min.y)];
    points.push(Point::new(max.x - radius, min.y));
    append_arc(
        &mut points,
        Point::new(max.x - radius, min.y + radius),
        radius,
        -FRAC_PI_2,
        0.0,
        steps,
    );
    points.push(Point::new(max.x, max.y - radius));
    append_arc(
        &mut points,
        Point::new(max.x - radius, max.y - radius),
        radius,
        0.0,
        FRAC_PI_2,
        steps,
    );
    points.push(Point::new(min.x + radius, max.y));
    append_arc(
        &mut points,
        Point::new(min.x + radius, max.y - radius),
        radius,
        FRAC_PI_2,
        PI,
        steps,
    );
    points.push(Point::new(min.x, min.y + radius));
    append_arc(
        &mut points,
        Point::new(min.x + radius, min.y + radius),
        radius,
        PI,
        PI + FRAC_PI_2,
        steps,
    );
    points.push(Point::new(min.x + radius, min.y));
    dedupe_points(points)
}

fn rounded_corner_steps(radius: f64, accuracy: f64) -> usize {
    let tolerance = accuracy.abs().max(0.001);
    let ratio = (tolerance / radius).clamp(0.0001, 1.0);
    ((FRAC_PI_2 / ratio.sqrt()).ceil() as usize).clamp(MIN_CORNER_STEPS, MAX_CORNER_STEPS)
}

fn append_arc(
    points: &mut Vec<Point>,
    center: Point,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    steps: usize,
) {
    for idx in 1..=steps {
        let t = idx as f64 / steps as f64;
        let angle = start_angle + (end_angle - start_angle) * t;
        points.push(Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }
}

fn dedupe_points(points: Vec<Point>) -> Vec<Point> {
    let mut deduped = Vec::new();
    for point in points {
        if deduped
            .last()
            .is_none_or(|last: &Point| distance(*last, point) > f64::EPSILON)
        {
            deduped.push(point);
        }
    }
    deduped
}

fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_usize(settings: &LegacySettings, key: &str, default: usize) -> usize {
    settings
        .get_last(key)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value.round().max(1.0) as usize)
        .unwrap_or(default)
}

fn get_usize_allow_zero(settings: &LegacySettings, key: &str, default: usize) -> usize {
    settings
        .get_last(key)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value.round().max(0.0) as usize)
        .unwrap_or(default)
}

fn profile_tabs(path: &[Point], options: &ProfileOptions) -> Vec<ProfileTab> {
    let count = options.tab_count;
    let total = path_length(path);
    if count == 0 || options.tab_height <= 0.0 || total <= f64::EPSILON {
        return Vec::new();
    }

    let tab_length = if options.tab_width > 0.0 {
        options.tab_width.min(total / count.max(1) as f64).max(0.0)
    } else {
        automatic_tab_length(total, count, options.endmill_diameter)
    };
    if tab_length <= f64::EPSILON {
        return Vec::new();
    }

    let mut tabs = Vec::new();
    for index in 0..count {
        let center = total * (index as f64 + 0.5) / count as f64;
        let start = center - tab_length / 2.0;
        let end = center + tab_length / 2.0;
        if start < 0.0 {
            tabs.push(ProfileTab {
                start_distance: start + total,
                end_distance: total,
            });
            tabs.push(ProfileTab {
                start_distance: 0.0,
                end_distance: end,
            });
        } else if end > total {
            tabs.push(ProfileTab {
                start_distance: start,
                end_distance: total,
            });
            tabs.push(ProfileTab {
                start_distance: 0.0,
                end_distance: end - total,
            });
        } else {
            tabs.push(ProfileTab {
                start_distance: start,
                end_distance: end,
            });
        }
    }
    tabs
}

fn automatic_tab_length(total: f64, count: usize, endmill_diameter: f64) -> f64 {
    let count = count.max(1) as f64;
    let preferred = (endmill_diameter.max(0.0) * 3.0).max(total * 0.02);
    let maximum = total / (count * 3.0);
    preferred.min(maximum).max(0.0)
}

fn path_length(path: &[Point]) -> f64 {
    path.windows(2)
        .map(|pair| distance(pair[0], pair[1]))
        .sum::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_legacy_settings;

    fn bounds() -> Bounds {
        Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(2.0, 1.0),
        }
    }

    #[test]
    fn profile_options_parse_legacy_settings() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("profile_cut", "1", false);
        settings.set_or_push("profile_margin", "0.1", false);
        settings.set_or_push("profile_radius", "0.2", false);
        settings.set_or_push("profile_depth", "0.5", false);
        settings.set_or_push("profile_steps", "4", false);
        settings.set_or_push("profile_endmill_dia", "0.25", false);
        settings.set_or_push("profile_tabs", "6", false);
        settings.set_or_push("profile_tab_height", "0.04", false);
        settings.set_or_push("profile_tab_width", "0.3", false);
        settings.set_or_push("profile_chamfer", "1", false);
        settings.set_or_push("profile_chamfer_depth", "0.05", false);
        settings.set_or_push("profile_chamfer_angle", "90", false);
        settings.set_or_push("profile_width", "4", false);
        settings.set_or_push("profile_height", "2", false);
        settings.set_or_push("profile_aspect", "1", false);
        settings.set_or_push("profile_trace", "75", false);
        settings.set_or_push("profile_align", "Top-Left", false);

        let options = ProfileOptions::from_legacy(&settings);

        assert!(options.is_usable());
        assert_eq!(options.margin, 0.1);
        assert_eq!(options.corner_radius, 0.2);
        assert_eq!(options.depth, 0.5);
        assert_eq!(options.step_count, 4);
        assert_eq!(options.endmill_diameter, 0.25);
        assert_eq!(options.tab_count, 6);
        assert_eq!(options.tab_height, 0.04);
        assert_eq!(options.tab_width, 0.3);
        assert!(options.chamfer_enabled);
        assert_eq!(options.width, 4.0);
        assert_eq!(options.height, 2.0);
        assert_eq!(options.aspect_ratio, 1.0);
        assert_eq!(options.trace_detail, 75.0);
        assert_eq!(options.alignment, ProfileOrigin::TopLeft);
        assert!((options.chamfer_width() - 0.05).abs() < 1.0e-9);
        assert!((options.straight_offset() - 0.175).abs() < 1.0e-9);
    }

    #[test]
    fn profile_depths_divide_material_thickness_by_step_count() {
        assert_depths_close(&profile_depths(0.3, 3), &[-0.1, -0.2, -0.3]);
        assert_eq!(profile_depths(0.3, 1), vec![-0.3]);
        assert_eq!(profile_depths(0.3, 0), vec![-0.3]);
        assert_depths_close(&profile_depths(0.5, 4), &[-0.125, -0.25, -0.375, -0.5]);
    }

    #[test]
    fn profile_path_offsets_box_by_margin_and_cutter() {
        let options = ProfileOptions {
            enabled: true,
            margin: 0.25,
            corner_radius: 0.0,
            depth: 0.1,
            step_count: 1,
            endmill_diameter: 0.25,
            tab_count: 0,
            tab_height: 0.0,
            tab_width: 0.0,
            chamfer_enabled: false,
            chamfer_depth: 0.0,
            chamfer_angle_degrees: 60.0,
            width: 0.0,
            height: 0.0,
            aspect_ratio: 0.0,
            trace_detail: 0.0,
            stroke_thickness: 0.01,
            origin: ProfileOrigin::Default,
            alignment: ProfileOrigin::MidCenter,
            x_origin: 0.0,
            y_origin: 0.0,
        };

        let path = profile_path(bounds(), &options, options.straight_offset(), 0.001);

        let min_x = path.iter().map(|point| point.x).reduce(f64::min).unwrap();
        let min_y = path.iter().map(|point| point.y).reduce(f64::min).unwrap();
        let max_x = path.iter().map(|point| point.x).reduce(f64::max).unwrap();
        let max_y = path.iter().map(|point| point.y).reduce(f64::max).unwrap();

        assert!((min_x + 0.375).abs() < 1.0e-9);
        assert!((min_y + 0.375).abs() < 1.0e-9);
        assert!((max_x - 2.375).abs() < 1.0e-9);
        assert!((max_y - 1.375).abs() < 1.0e-9);
        assert_eq!(path[0], Point::new(-0.25, -0.375));
        assert!(distance(*path.last().unwrap(), path[0]) < 1.0e-9);
    }

    #[test]
    fn profile_dimensions_and_ratio_resize_the_profile_envelope() {
        let mut options = ProfileOptions::from_legacy(&default_legacy_settings());
        options.enabled = true;
        options.depth = 0.1;
        options.endmill_diameter = 0.25;
        options.width = 4.0;
        options.height = 2.0;

        let operations = generate_profile_operations(bounds(), &options, 0.001);
        let path_bounds = point_bounds(&operations[0].path);
        assert!((path_bounds.max.x - path_bounds.min.x - 4.0).abs() < 1.0e-9);
        assert!((path_bounds.max.y - path_bounds.min.y - 2.0).abs() < 1.0e-9);

        options.height = 0.0;
        options.aspect_ratio = 1.0;
        let operations = generate_profile_operations(bounds(), &options, 0.001);
        let path_bounds = point_bounds(&operations[0].path);
        assert!((path_bounds.max.x - path_bounds.min.x - 4.0).abs() < 1.0e-9);
        assert!((path_bounds.max.y - path_bounds.min.y - 4.0).abs() < 1.0e-9);

        options.origin = ProfileOrigin::TopLeft;
        options.alignment = ProfileOrigin::TopLeft;
        options.x_origin = 0.0;
        options.y_origin = 0.0;
        let operations = generate_profile_operations(
            Bounds {
                min: Point::new(0.0, -1.0),
                max: Point::new(2.0, 0.0),
            },
            &options,
            0.001,
        );
        let path_bounds = point_bounds(&operations[0].path);
        assert!(path_bounds.min.x.abs() < 1.0e-9);
        assert!(path_bounds.max.y.abs() < 1.0e-9);
    }

    #[test]
    fn trace_detail_follows_a_connected_l_outline() {
        let mut options = ProfileOptions::from_legacy(&default_legacy_settings());
        options.enabled = true;
        options.depth = 0.1;
        options.endmill_diameter = 0.1;
        options.margin = 0.0;
        options.stroke_thickness = 0.1;
        options.trace_detail = 100.0;
        let segments = [
            EngraveSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(0.0, 2.0),
                loop_id: 1,
            },
            EngraveSegment {
                start: Point::new(0.0, 2.0),
                end: Point::new(1.0, 2.0),
                loop_id: 1,
            },
        ];

        let traced = generate_profile_operations_for_segments(
            Bounds {
                min: Point::new(0.0, 0.0),
                max: Point::new(1.0, 2.0),
            },
            &segments,
            &options,
            0.001,
        );
        assert_eq!(traced.len(), 1);
        assert!(traced[0].path.len() > 4);
        assert!(
            traced[0]
                .path
                .iter()
                .any(|point| { (point.x - 0.1).abs() < 0.01 && (point.y - 1.9).abs() < 0.01 })
        );
    }

    #[test]
    fn profile_path_rounds_corners_inside_box_extents() {
        let options = ProfileOptions {
            enabled: true,
            margin: 0.0,
            corner_radius: 0.25,
            depth: 0.1,
            step_count: 1,
            endmill_diameter: 0.25,
            tab_count: 0,
            tab_height: 0.0,
            tab_width: 0.0,
            chamfer_enabled: false,
            chamfer_depth: 0.0,
            chamfer_angle_degrees: 60.0,
            width: 0.0,
            height: 0.0,
            aspect_ratio: 0.0,
            trace_detail: 0.0,
            stroke_thickness: 0.01,
            origin: ProfileOrigin::Default,
            alignment: ProfileOrigin::MidCenter,
            x_origin: 0.0,
            y_origin: 0.0,
        };

        let path = profile_path(bounds(), &options, 0.0, 0.01);

        assert!(path.len() > 8);
        assert_eq!(path[0], Point::new(0.25, 0.0));
        assert!(path.iter().any(|point| point.x == 2.0 && point.y > 0.25));
        assert!(distance(*path.last().unwrap(), path[0]) < 1.0e-9);
    }

    #[test]
    fn profile_operations_split_chamfer_and_straight_tools() {
        let options = ProfileOptions {
            enabled: true,
            margin: 0.0,
            corner_radius: 0.0,
            depth: 0.3,
            step_count: 3,
            endmill_diameter: 0.25,
            tab_count: 4,
            tab_height: 0.05,
            tab_width: 0.0,
            chamfer_enabled: true,
            chamfer_depth: 0.05,
            chamfer_angle_degrees: 90.0,
            width: 0.0,
            height: 0.0,
            aspect_ratio: 0.0,
            trace_detail: 0.0,
            stroke_thickness: 0.01,
            origin: ProfileOrigin::Default,
            alignment: ProfileOrigin::MidCenter,
            x_origin: 0.0,
            y_origin: 0.0,
        };

        let operations = generate_profile_operations(bounds(), &options, 0.001);

        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].tool, ProfileTool::VBitChamfer);
        assert_eq!(operations[0].depths, vec![-0.05]);
        assert_eq!(operations[1].tool, ProfileTool::StraightEndmill);
        assert_depths_close(&operations[1].depths, &[-0.1, -0.2, -0.3]);
        assert!(operations[0].tabs.is_empty());
        assert_eq!(operations[1].tabs.len(), 4);
        assert_eq!(operations[1].tab_height, 0.05);
        assert!((operations[0].offset - 0.05).abs() < 1.0e-9);
        assert!((operations[1].offset - 0.175).abs() < 1.0e-9);
    }

    #[test]
    fn profile_tabs_are_evenly_spaced_around_path() {
        let options = ProfileOptions {
            enabled: true,
            margin: 0.0,
            corner_radius: 0.0,
            depth: 0.3,
            step_count: 1,
            endmill_diameter: 0.25,
            tab_count: 4,
            tab_height: 0.04,
            tab_width: 0.5,
            chamfer_enabled: false,
            chamfer_depth: 0.0,
            chamfer_angle_degrees: 60.0,
            width: 0.0,
            height: 0.0,
            aspect_ratio: 0.0,
            trace_detail: 0.0,
            stroke_thickness: 0.01,
            origin: ProfileOrigin::Default,
            alignment: ProfileOrigin::MidCenter,
            x_origin: 0.0,
            y_origin: 0.0,
        };
        let path = profile_path(bounds(), &options, 0.0, 0.001);
        let tabs = profile_tabs(&path, &options);

        assert_eq!(tabs.len(), 4);
        assert!(tabs[0].start_distance > 0.0);
        assert!((tabs[0].end_distance - tabs[0].start_distance - 0.5).abs() < 1.0e-9);
        assert!(tabs[1].start_distance > tabs[0].end_distance);
    }

    #[test]
    fn profile_tabs_use_automatic_width_when_max_width_is_zero() {
        let options = ProfileOptions {
            enabled: true,
            margin: 0.0,
            corner_radius: 0.0,
            depth: 0.3,
            step_count: 1,
            endmill_diameter: 0.25,
            tab_count: 4,
            tab_height: 0.04,
            tab_width: 0.0,
            chamfer_enabled: false,
            chamfer_depth: 0.0,
            chamfer_angle_degrees: 60.0,
            width: 0.0,
            height: 0.0,
            aspect_ratio: 0.0,
            trace_detail: 0.0,
            stroke_thickness: 0.01,
            origin: ProfileOrigin::Default,
            alignment: ProfileOrigin::MidCenter,
            x_origin: 0.0,
            y_origin: 0.0,
        };
        let path = profile_path(bounds(), &options, 0.0, 0.001);
        let tabs = profile_tabs(&path, &options);

        assert_eq!(tabs.len(), 4);
        assert!((tabs[0].end_distance - tabs[0].start_distance - 0.5).abs() < 1.0e-9);
    }

    fn assert_depths_close(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 1.0e-9);
        }
    }
}
