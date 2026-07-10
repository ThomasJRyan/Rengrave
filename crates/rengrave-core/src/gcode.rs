use crate::cleanup::{CleanupBit, CleanupOptions, CleanupPoint};
use crate::layout::{EngraveCircle, EngraveSegment};
use crate::profile::{ProfileOperation, ProfileTab, ProfileTool};
use crate::settings::{
    DEFAULT_GCODE_POSTAMBLE, DEFAULT_GCODE_PREAMBLE, LegacySettings, get_legacy_bool,
};
use crate::vcarve::{VCarveOptions, VCarvePoint, reorder_vcarve_points};

const ZERO: f64 = 0.00001;
const MAX_RADIUS: f64 = 1.0e30;
const PROFILE_TAB_RAMP_ANGLE_DEGREES: f64 = 45.0;

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
    pub return_to_origin: bool,
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
                .unwrap_or(DEFAULT_GCODE_PREAMBLE)
                .to_owned(),
            postamble: settings
                .get_last("gpost")
                .unwrap_or(DEFAULT_GCODE_POSTAMBLE)
                .to_owned(),
            return_to_origin: get_legacy_bool(settings, "return_to_origin", true),
            variables_disabled: get_legacy_bool(settings, "var_dis", true),
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
    write_engrave_gcode_with_circle(segments, None, options)
}

pub fn write_engrave_gcode_with_circle(
    segments: &[EngraveSegment],
    circle: Option<EngraveCircle>,
    options: &GcodeOptions,
) -> Vec<String> {
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

    if let Some(circle) = circle {
        emit_circle_border(
            &mut lines,
            circle,
            &safe_value,
            &depth_value,
            &feed,
            &plunge,
            dp,
        );
    }

    finish_gcode(&mut lines, options, &safe_value, dp);
    lines
}

pub fn write_vcarve_gcode(
    points: &[VCarvePoint],
    gcode_options: &GcodeOptions,
    vcarve_options: &VCarveOptions,
) -> Vec<String> {
    let points = reorder_vcarve_points(
        points,
        gcode_options.accuracy,
        gcode_options
            .return_to_origin
            .then_some(crate::geometry::Point::new(0.0, 0.0)),
    );
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

    let max_depth = points
        .iter()
        .map(|point| vcarve_options.depth_for_radius(point.radius))
        .reduce(f64::min)
        .unwrap_or(0.0);

    for rough_cap in vcarve_options.rough_pass_caps(max_depth) {
        let mut current_loop = None;
        let mut loop_moves = Vec::new();
        let mut emit_state = VCarveEmitState::new(gcode_options.safe_z, dp);
        for point in &points {
            let z = vcarve_options.pass_depth_for_radius(point.radius, rough_cap);
            if current_loop != Some(point.loop_id) {
                flush_vcarve_moves(
                    &mut lines,
                    &mut emit_state,
                    &loop_moves,
                    gcode_options.accuracy,
                    dp,
                );
                loop_moves.clear();
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
            }
            loop_moves.push(VCarveMove::new(point.position.x, point.position.y, z));
            current_loop = Some(point.loop_id);
        }
        flush_vcarve_moves(
            &mut lines,
            &mut emit_state,
            &loop_moves,
            gcode_options.accuracy,
            dp,
        );
    }

    finish_gcode(&mut lines, gcode_options, &safe_value, dp);
    lines
}

pub fn write_cleanup_gcode(
    points: &[CleanupPoint],
    gcode_options: &GcodeOptions,
    cleanup_options: &CleanupOptions,
    vcarve_options: &VCarveOptions,
    bit: CleanupBit,
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
    lines.push("( R-Engrave secondary cleanup operation )".to_owned());
    match bit {
        CleanupBit::Straight => lines.push(format!(
            "( Straight cleanup bit diameter: {} )",
            format_number(cleanup_options.bit_diameter, dp)
        )),
        CleanupBit::VBit => lines.push(format!(
            "( V-bit cleanup effective diameter: {} )",
            format_number(cleanup_options.vbit_diameter, dp)
        )),
    }
    if !gcode_options.variables_disabled {
        lines.push(format!("#1 = {}  ( Safe Z )", safe_number));
    }
    lines.push("G90".to_owned());
    if gcode_options.arc_fit == ArcFit::Center {
        lines.push("G91.1".to_owned());
    }
    lines.push(gcode_options.units.gcode().to_owned());
    lines.extend(split_gcode_lines(&gcode_options.preamble));
    lines.push(format!("F{feed}"));

    let paths = cleanup_paths(points);
    for depth in cleanup_pass_depths(cleanup_depth(vcarve_options), vcarve_options) {
        let depth_value = format_number(depth, dp);
        let mut feed_current = feed.clone();
        for path in sort_paths(paths.clone()) {
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
                feed_current = feed.clone();
            } else if feed_current == plunge {
                lines.push(format!("G1 Z{depth_value}"));
            } else {
                lines.push(format!("G1 Z{depth_value} F{plunge}"));
                feed_current = plunge.clone();
            }

            for point in path.iter().skip(1) {
                if feed_current == feed {
                    lines.push(format!(
                        "G1 X{} Y{}",
                        format_number(point.x, dp),
                        format_number(point.y, dp)
                    ));
                } else {
                    lines.push(format!(
                        "G1 X{} Y{} F{feed}",
                        format_number(point.x, dp),
                        format_number(point.y, dp)
                    ));
                    feed_current = feed.clone();
                }
            }
        }
    }

    finish_gcode(&mut lines, gcode_options, &safe_value, dp);
    lines
}

pub fn write_profile_gcode(
    operation: &ProfileOperation,
    gcode_options: &GcodeOptions,
) -> Vec<String> {
    let Some(first) = operation.path.first() else {
        return Vec::new();
    };
    if operation.path.len() < 2 || operation.depths.is_empty() {
        return Vec::new();
    }

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
    lines.push(format!("( {} )", operation.tool.comment()));
    lines.push(format!(
        "( Profile offset: {} )",
        format_number(operation.offset, dp)
    ));
    lines.push(format!(
        "( Profile corner radius: {} )",
        format_number(operation.corner_radius, dp)
    ));
    if operation.tool == ProfileTool::StraightEndmill {
        lines.push(format!("( Profile passes: {} )", operation.depths.len()));
        if !operation.tabs.is_empty() {
            lines.push(format!("( Profile tabs: {} )", operation.tabs.len()));
            lines.push(format!(
                "( Profile tab height: {} )",
                format_number(operation.tab_height, dp)
            ));
        }
    }
    if gcode_options.arc_fit.enabled() {
        lines.push("G17".to_owned());
    }
    if !gcode_options.variables_disabled {
        lines.push(format!("#1 = {}  ( Safe Z )", safe_number));
    }
    lines.push("G90".to_owned());
    if gcode_options.arc_fit == ArcFit::Center {
        lines.push("G91.1".to_owned());
    }
    lines.push(gcode_options.units.gcode().to_owned());
    lines.extend(split_gcode_lines(&gcode_options.preamble));
    lines.push(format!("F{feed}"));

    for depth in &operation.depths {
        let depth_value = format_number(*depth, dp);
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
            lines.push(format!("F{feed}"));
        }
        if operation.tabs.is_empty() {
            emit_cut_path(&mut lines, &operation.path, gcode_options, dp);
        } else {
            emit_profile_path_with_tabs(&mut lines, operation, *depth, dp);
        }
    }

    finish_gcode(&mut lines, gcode_options, &safe_value, dp);
    lines
}

fn emit_profile_path_with_tabs(
    lines: &mut Vec<String>,
    operation: &ProfileOperation,
    depth: f64,
    digits: usize,
) {
    let tab_depth = tab_depth(depth, operation);
    let tab_delta = (tab_depth - depth).abs();
    let mut distance_cursor = 0.0;
    let mut current_z = depth;
    for pair in operation.path.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let segment_length = distance(start, end);
        if segment_length <= f64::EPSILON {
            continue;
        }

        let mut splits = vec![0.0, segment_length];
        for tab in &operation.tabs {
            let ramp_length = profile_tab_ramp_length(tab, tab_delta);
            let start_ramp = tab.start_distance - ramp_length - distance_cursor;
            let end_ramp = tab.end_distance + ramp_length - distance_cursor;
            if start_ramp > 0.0 && start_ramp < segment_length {
                splits.push(start_ramp);
            }
            let start_t = tab.start_distance - distance_cursor;
            let end_t = tab.end_distance - distance_cursor;
            if start_t > 0.0 && start_t < segment_length {
                splits.push(start_t);
            }
            if end_t > 0.0 && end_t < segment_length {
                splits.push(end_t);
            }
            if end_ramp > 0.0 && end_ramp < segment_length {
                splits.push(end_ramp);
            }
        }
        splits.sort_by(|left, right| left.total_cmp(right));
        splits.dedup_by(|left, right| (*left - *right).abs() <= f64::EPSILON);

        for split in splits.windows(2) {
            let sub_start = split[0];
            let sub_end = split[1];
            if sub_end - sub_start <= f64::EPSILON {
                continue;
            }
            let target_distance = distance_cursor + sub_end;
            let target_z = profile_tab_z(target_distance, depth, tab_depth, operation);
            if (target_z - current_z).abs() > f64::EPSILON {
                let point = lerp_point(start, end, sub_end / segment_length);
                lines.push(format!(
                    "G1 X{} Y{} Z{}",
                    format_number(point.x, digits),
                    format_number(point.y, digits),
                    format_number(target_z, digits)
                ));
                current_z = target_z;
            } else {
                let point = lerp_point(start, end, sub_end / segment_length);
                lines.push(format!(
                    "G1 X{} Y{}",
                    format_number(point.x, digits),
                    format_number(point.y, digits)
                ));
            }
        }
        distance_cursor += segment_length;
    }
}

fn tab_depth(depth: f64, operation: &ProfileOperation) -> f64 {
    let final_depth = operation.depths.last().copied().unwrap_or(depth).min(depth);
    let tab_floor = final_depth + operation.tab_height.max(0.0);
    depth.max(tab_floor)
}

fn profile_tab_z(distance: f64, depth: f64, tab_depth: f64, operation: &ProfileOperation) -> f64 {
    let ramp_delta = (tab_depth - depth).abs();
    operation
        .tabs
        .iter()
        .map(|tab| {
            let ramp_length = profile_tab_ramp_length(tab, ramp_delta);
            if ramp_length <= f64::EPSILON {
                return depth;
            }

            if distance < tab.start_distance - ramp_length
                || distance > tab.end_distance + ramp_length
            {
                depth
            } else if distance < tab.start_distance {
                let progress = (distance - (tab.start_distance - ramp_length)) / ramp_length;
                depth + (tab_depth - depth) * progress
            } else if distance <= tab.end_distance {
                tab_depth
            } else {
                let progress = (distance - tab.end_distance) / ramp_length;
                tab_depth + (depth - tab_depth) * progress
            }
        })
        .fold(depth, f64::max)
}

fn profile_tab_ramp_length(tab: &ProfileTab, z_delta: f64) -> f64 {
    let angle = PROFILE_TAB_RAMP_ANGLE_DEGREES.to_radians();
    let rise_run = z_delta / angle.tan();
    rise_run.min((tab.end_distance - tab.start_distance).abs() / 2.0)
}

fn lerp_point(
    start: crate::geometry::Point,
    end: crate::geometry::Point,
    t: f64,
) -> crate::geometry::Point {
    crate::geometry::Point::new(
        start.x + (end.x - start.x) * t,
        start.y + (end.y - start.y) * t,
    )
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

fn emit_circle_border(
    lines: &mut Vec<String>,
    circle: EngraveCircle,
    safe_value: &str,
    depth_value: &str,
    feed: &str,
    plunge: &str,
    digits: usize,
) {
    lines.push(format!("G0 Z{safe_value}"));
    lines.push(format!(
        "G0 X{} Y{}",
        format_number(circle.center.x - circle.radius, digits),
        format_number(circle.center.y, digits)
    ));
    if plunge == feed {
        lines.push(format!("G1 Z{depth_value}"));
        lines.push(format!(
            "G2 I{} J{}",
            format_number(circle.radius, digits),
            format_number(0.0, digits)
        ));
    } else {
        lines.push(format!("G1 Z{depth_value} F{plunge}"));
        lines.push(format!(
            "G2 I{} J{} F{}",
            format_number(circle.radius, digits),
            format_number(0.0, digits),
            feed
        ));
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

fn cleanup_paths(points: &[CleanupPoint]) -> Vec<Vec<crate::geometry::Point>> {
    let mut paths = Vec::new();
    let mut current_loop = None;
    for point in points {
        if current_loop != Some(point.loop_id) {
            paths.push(Vec::new());
            current_loop = Some(point.loop_id);
        }
        paths.last_mut().unwrap().push(point.position);
    }
    paths.into_iter().filter(|path| !path.is_empty()).collect()
}

fn cleanup_pass_depths(depth: f64, vcarve_options: &VCarveOptions) -> Vec<f64> {
    if vcarve_options.rough_stock <= 0.0 || vcarve_options.max_cut >= 0.0 {
        return vec![depth];
    }

    let mut max_dz = vcarve_options.max_cut;
    let mut rough_stock = vcarve_options.rough_stock;
    if -max_dz < rough_stock {
        rough_stock = -max_dz;
    }

    let mut zmin = 0.0;
    let mut roughing = true;
    let mut rough_again = true;
    let mut depths = Vec::new();

    while rough_again || roughing {
        if !rough_again {
            roughing = false;
            max_dz = -99999.0;
        }
        rough_again = false;
        zmin += max_dz;

        let mut z = if roughing { depth + rough_stock } else { depth };
        if z < zmin {
            z = zmin;
            rough_again = true;
        }
        depths.push(z);

        if depths.len() > 1000 {
            break;
        }
    }

    depths
}

fn cleanup_depth(vcarve_options: &VCarveOptions) -> f64 {
    let depth = vcarve_options.max_cut_depth();
    if vcarve_options.inlay {
        depth + vcarve_options.allowance
    } else {
        depth
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct VCarveMove {
    x: f64,
    y: f64,
    z: f64,
}

impl VCarveMove {
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
}

#[derive(Debug, Clone)]
struct VCarveEmitState {
    last_x: Option<String>,
    last_y: Option<String>,
    last_z: Option<String>,
}

impl VCarveEmitState {
    fn new(safe_z: f64, dp: usize) -> Self {
        Self {
            last_x: None,
            last_y: None,
            last_z: Some(format_number(safe_z, dp)),
        }
    }
}

fn flush_vcarve_moves(
    lines: &mut Vec<String>,
    state: &mut VCarveEmitState,
    moves: &[VCarveMove],
    tolerance: f64,
    dp: usize,
) {
    if moves.is_empty() {
        return;
    }

    for movement in douglas_vcarve(moves.to_vec(), tolerance, true) {
        push_vcarve_move(lines, state, movement, dp);
    }
}

fn push_vcarve_move(
    lines: &mut Vec<String>,
    state: &mut VCarveEmitState,
    movement: VCarveMove,
    dp: usize,
) {
    let mut line = String::from("G1");
    let mut changed = false;
    let x_value = format_number(movement.x, dp);
    let y_value = format_number(movement.y, dp);
    let z_value = format_number(movement.z, dp);

    if state.last_x.as_deref() != Some(x_value.as_str()) {
        line.push_str(&format!(" X{x_value}"));
        state.last_x = Some(x_value);
        changed = true;
    }
    if state.last_y.as_deref() != Some(y_value.as_str()) {
        line.push_str(&format!(" Y{y_value}"));
        state.last_y = Some(y_value);
        changed = true;
    }
    if state.last_z.as_deref() != Some(z_value.as_str()) {
        line.push_str(&format!(" Z{z_value}"));
        state.last_z = Some(z_value);
        changed = true;
    }

    if changed {
        lines.push(line);
    }
}

fn douglas_vcarve(mut moves: Vec<VCarveMove>, tolerance: f64, first: bool) -> Vec<VCarveMove> {
    if moves.len() == 1 {
        return vec![moves[0]];
    }

    let start = moves[0];
    let mut end = *moves.last().unwrap();
    let mut closed_point = None;
    while same_vcarve_move(start, end) {
        closed_point = moves.pop();
        let Some(last) = moves.last().copied() else {
            return Vec::new();
        };
        end = last;
    }

    let mut worst_dist = 0.0;
    let mut worst_index = 0usize;
    for (index, movement) in moves.iter().enumerate() {
        if index == 0 || index == moves.len() - 1 {
            continue;
        }
        let dist = dist_vcarve_segment(start, end, *movement);
        if dist > worst_dist {
            worst_dist = dist;
            worst_index = index;
        }
    }

    let mut output = Vec::new();
    if worst_dist > tolerance {
        if first {
            output.push(start);
        }
        output.extend(douglas_vcarve(
            moves[..=worst_index].to_vec(),
            tolerance,
            false,
        ));
        output.push(moves[worst_index]);
        output.extend(douglas_vcarve(
            moves[worst_index..].to_vec(),
            tolerance,
            false,
        ));
        if first {
            output.push(end);
        }
    } else if first {
        output.push(start);
        output.push(end);
    }

    if closed_point.is_some() {
        output.push(start);
    }
    output
}

fn same_vcarve_move(a: VCarveMove, b: VCarveMove) -> bool {
    (a.x - b.x).abs() < ZERO && (a.y - b.y).abs() < ZERO && (a.z - b.z).abs() < ZERO
}

fn dist_vcarve_segment(start: VCarveMove, end: VCarveMove, point: VCarveMove) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let dz = end.z - start.z;
    let d2 = dx * dx + dy * dy + dz * dz;
    if d2 == 0.0 {
        return 0.0;
    }

    let mut t =
        (dx * (point.x - start.x) + dy * (point.y - start.y) + dz * (point.z - start.z)) / d2;
    t = t.clamp(0.0, 1.0);
    let ex = point.x - start.x - t * dx;
    let ey = point.y - start.y - t * dy;
    let ez = point.z - start.z - t * dz;
    (ex * ex + ey * ey + ez * ez).sqrt()
}

fn format_number(value: f64, digits: usize) -> String {
    format!("{value:.digits$}")
}

fn finish_gcode(lines: &mut Vec<String>, options: &GcodeOptions, safe_value: &str, digits: usize) {
    lines.push(format!("G0 Z{safe_value}"));
    if options.return_to_origin {
        let origin = format_number(0.0, digits);
        lines.push(format!("G0 X{origin} Y{origin}"));
    }
    lines.extend(split_gcode_lines(&options.postamble));
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

    #[test]
    fn writes_basic_engrave_moves() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
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
    fn writes_circle_border_as_full_clockwise_arc() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 2.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let lines = write_engrave_gcode_with_circle(
            &[EngraveSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(1.0, 0.0),
                loop_id: 1,
            }],
            Some(EngraveCircle {
                center: Point::new(0.0, 0.0),
                radius: 2.0,
            }),
            &options,
        );

        assert!(lines.contains(&"G0 X-2.0000 Y0.0000".to_owned()));
        assert!(lines.contains(&"G1 Z-0.0050 F2.00".to_owned()));
        assert!(lines.contains(&"G2 I2.0000 J0.0000 F5.00".to_owned()));
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
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
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
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
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
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
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
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
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
        assert!(lines.contains(&"G1 X1.0000 Z-0.8660".to_owned()));
    }

    #[test]
    fn writes_vcarve_roughing_passes_before_final_depth() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let mut settings = crate::settings::default_legacy_settings();
        settings.set_or_push("v_rough_stk", "0.1", false);
        settings.set_or_push("v_max_cut", "-0.4", false);
        let vcarve = VCarveOptions::from_legacy(&settings);
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

        assert!(lines.contains(&"G1 X1.0000 Z-0.4000".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Z-0.7660".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Z-0.8660".to_owned()));
        assert_eq!(
            lines
                .iter()
                .filter(|line| *line == "G0 X0.0000 Y0.0000")
                .count(),
            4
        );
    }

    #[test]
    fn vcarve_writer_simplifies_collinear_xyz_samples() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
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
                    position: Point::new(0.5, 0.0),
                    radius: 0.25,
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

        assert!(!lines.iter().any(|line| line.starts_with("G1 X0.5000")));
        assert!(lines.contains(&"G1 X1.0000 Z-0.8660".to_owned()));
    }

    #[test]
    fn writes_cleanup_gcode_at_vcarve_max_depth() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.1,
            feed: 5.0,
            plunge: 1.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let cleanup = crate::cleanup::CleanupOptions::from_legacy(
            &crate::settings::default_legacy_settings(),
        );
        let vcarve = VCarveOptions::from_legacy(&crate::settings::default_legacy_settings());

        let lines = write_cleanup_gcode(
            &[
                CleanupPoint {
                    position: Point::new(0.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
                CleanupPoint {
                    position: Point::new(1.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
            ],
            &options,
            &cleanup,
            &vcarve,
            CleanupBit::Straight,
        );

        assert!(
            lines
                .iter()
                .any(|line| line.contains("secondary cleanup operation"))
        );
        assert!(lines.contains(&"F5.00".to_owned()));
        assert!(lines.contains(&"G1 Z-0.4330 F1.00".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Y0.0000 F5.00".to_owned()));
    }

    #[test]
    fn cleanup_gcode_sets_feed_when_feed_and_plunge_match() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.1,
            feed: 5.0,
            plunge: 5.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let cleanup = crate::cleanup::CleanupOptions::from_legacy(
            &crate::settings::default_legacy_settings(),
        );
        let vcarve = VCarveOptions::from_legacy(&crate::settings::default_legacy_settings());

        let lines = write_cleanup_gcode(
            &[
                CleanupPoint {
                    position: Point::new(0.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
                CleanupPoint {
                    position: Point::new(1.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
            ],
            &options,
            &cleanup,
            &vcarve,
            CleanupBit::Straight,
        );

        let feed_index = lines.iter().position(|line| line == "F5.00").unwrap();
        let plunge_index = lines
            .iter()
            .position(|line| line.starts_with("G1 Z"))
            .unwrap();
        assert!(feed_index < plunge_index);
        assert!(
            !lines
                .iter()
                .any(|line| line.starts_with("G1 Z") && line.contains(" F"))
        );
    }

    #[test]
    fn cleanup_gcode_uses_vcarve_roughing_passes() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.3,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let cleanup = crate::cleanup::CleanupOptions::from_legacy(
            &crate::settings::default_legacy_settings(),
        );
        let mut settings = crate::settings::default_legacy_settings();
        settings.set_or_push("v_rough_stk", "0.05", false);
        settings.set_or_push("v_max_cut", "-0.1", false);
        let vcarve = VCarveOptions::from_legacy(&settings);

        let lines = write_cleanup_gcode(
            &[
                CleanupPoint {
                    position: Point::new(0.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
                CleanupPoint {
                    position: Point::new(1.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
            ],
            &options,
            &cleanup,
            &vcarve,
            CleanupBit::Straight,
        );

        assert!(lines.contains(&"G1 Z-0.1000".to_owned()));
        assert!(lines.contains(&"G1 Z-0.2000".to_owned()));
        assert!(lines.contains(&"G1 Z-0.3000".to_owned()));
        assert!(lines.contains(&"G1 Z-0.3830".to_owned()));
        assert!(lines.contains(&"G1 Z-0.4330".to_owned()));
    }

    #[test]
    fn inlay_cleanup_depth_matches_f_engrave_maxcut_plus_allowance() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let cleanup = crate::cleanup::CleanupOptions::from_legacy(
            &crate::settings::default_legacy_settings(),
        );
        let mut settings = crate::settings::default_legacy_settings();
        settings.set_or_push("inlay", "1", false);
        settings.set_or_push("allowance", "-0.1", false);
        let vcarve = VCarveOptions::from_legacy(&settings);

        assert!((cleanup_depth(&vcarve) + 0.533).abs() < 1e-9);

        let lines = write_cleanup_gcode(
            &[
                CleanupPoint {
                    position: Point::new(0.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
                CleanupPoint {
                    position: Point::new(1.0, 0.0),
                    radius: 0.125,
                    loop_id: 1,
                },
            ],
            &options,
            &cleanup,
            &vcarve,
            CleanupBit::Straight,
        );

        assert!(lines.contains(&"G1 Z-0.5330".to_owned()));
        assert!(!lines.contains(&"G1 Z-0.0050".to_owned()));
    }

    #[test]
    fn writes_stepped_profile_gcode() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 1.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let operation = ProfileOperation {
            tool: ProfileTool::StraightEndmill,
            path: vec![
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
                Point::new(1.0, 1.0),
                Point::new(0.0, 1.0),
                Point::new(0.0, 0.0),
            ],
            depths: vec![-0.1, -0.2],
            offset: 0.125,
            corner_radius: 0.0,
            tabs: Vec::new(),
            tab_height: 0.0,
        };

        let lines = write_profile_gcode(&operation, &options);

        assert!(lines.iter().any(|line| line.contains("profile cut")));
        assert!(lines.contains(&"( Profile passes: 2 )".to_owned()));
        assert!(lines.contains(&"G1 Z-0.1000 F1.00".to_owned()));
        assert!(lines.contains(&"G1 Z-0.2000 F1.00".to_owned()));
        assert_eq!(
            lines
                .iter()
                .filter(|line| *line == "G0 X0.0000 Y0.0000")
                .count(),
            3
        );
        assert!(lines.contains(&"G1 X1.0000 Y0.0000".to_owned()));
    }

    #[test]
    fn writes_profile_chamfer_gcode_as_separate_operation() {
        let options = GcodeOptions {
            safe_z: 3.0,
            depth_z: -0.1,
            feed: 120.0,
            plunge: 60.0,
            accuracy: 0.01,
            units: Units::Mm,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let operation = ProfileOperation {
            tool: ProfileTool::VBitChamfer,
            path: vec![
                Point::new(-1.0, -1.0),
                Point::new(2.0, -1.0),
                Point::new(2.0, 2.0),
                Point::new(-1.0, 2.0),
                Point::new(-1.0, -1.0),
            ],
            depths: vec![-0.5],
            offset: 0.5,
            corner_radius: 0.0,
            tabs: Vec::new(),
            tab_height: 0.0,
        };

        let lines = write_profile_gcode(&operation, &options);

        assert!(lines.iter().any(|line| line.contains("profile chamfer")));
        assert!(!lines.iter().any(|line| line.contains("Profile passes")));
        assert!(lines.contains(&"G21".to_owned()));
        assert!(lines.contains(&"G1 Z-0.500 F60.0".to_owned()));
        assert!(lines.contains(&"G1 X2.000 Y-1.000".to_owned()));
    }

    #[test]
    fn writes_profile_tabs_as_raised_bottom_segments() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: DEFAULT_GCODE_PREAMBLE.to_owned(),
            postamble: "M5|M2".to_owned(),
            return_to_origin: true,
            variables_disabled: true,
            arc_fit: ArcFit::None,
        };
        let operation = ProfileOperation {
            tool: ProfileTool::StraightEndmill,
            path: vec![
                Point::new(0.0, 0.0),
                Point::new(4.0, 0.0),
                Point::new(4.0, 4.0),
                Point::new(0.0, 4.0),
                Point::new(0.0, 0.0),
            ],
            depths: vec![-0.1],
            offset: 0.125,
            corner_radius: 0.0,
            tabs: vec![ProfileTab {
                start_distance: 1.0,
                end_distance: 2.0,
            }],
            tab_height: 0.025,
        };

        let lines = write_profile_gcode(&operation, &options);

        assert!(lines.contains(&"( Profile tabs: 1 )".to_owned()));
        assert!(lines.contains(&"( Profile tab height: 0.0250 )".to_owned()));
        assert!(lines.contains(&"G1 X0.9750 Y0.0000".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Y0.0000 Z-0.0750".to_owned()));
        assert!(lines.contains(&"G1 X2.0000 Y0.0000".to_owned()));
        assert!(lines.contains(&"G1 X2.0250 Y0.0000 Z-0.1000".to_owned()));
        assert_eq!(
            lines.iter().filter(|line| *line == "G1 Z-0.1000").count(),
            1
        );
        assert!(lines.contains(&"G1 X4.0000 Y0.0000".to_owned()));
    }

    #[test]
    fn return_to_origin_setting_defaults_on_and_can_be_disabled() {
        let settings = crate::settings::default_legacy_settings();
        assert!(GcodeOptions::from_legacy(&settings).return_to_origin);

        let mut disabled = settings;
        disabled.set_or_push("return_to_origin", "0", false);
        let options = GcodeOptions::from_legacy(&disabled);
        let lines = write_engrave_gcode(
            &[EngraveSegment {
                start: Point::new(1.0, 1.0),
                end: Point::new(2.0, 1.0),
                loop_id: 1,
            }],
            &options,
        );

        assert!(!lines.iter().any(|line| line == "G0 X0.0000 Y0.0000"));
        assert_eq!(lines.last(), Some(&"M2".to_owned()));
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
