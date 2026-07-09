use crate::geometry::Point;
use crate::layout::EngraveSegment;
use crate::settings::{LegacySettings, get_legacy_bool, legacy_bool_value};
use crate::vcarve::VCarveOptions;

const ZERO: f64 = 0.00001;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
struct TenThousand;

impl clipper2::PointScaler for TenThousand {
    const MULTIPLIER: f64 = 10000.0;
}

type ClipPaths = clipper2::Paths<TenThousand>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupBit {
    Straight,
    VBit,
}

impl CleanupBit {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Straight => "clean",
            Self::VBit => "v_clean",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanupSelection {
    pub profile: bool,
    pub x: bool,
    pub y: bool,
    pub loops: bool,
}

impl CleanupSelection {
    pub fn any(self) -> bool {
        self.profile || self.x || self.y || self.loops
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanupOptions {
    pub bit_diameter: f64,
    pub step_over_percent: f64,
    pub vbit_diameter: f64,
    pub v_flop: bool,
    pub straight: CleanupSelection,
    pub vbit: CleanupSelection,
}

impl CleanupOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        let paths = parse_clean_paths(settings.get_last("clean_paths"));
        Self {
            bit_diameter: get_f64(settings, "clean_dia", 0.25).max(ZERO),
            step_over_percent: get_f64(settings, "clean_step", 50.0).max(ZERO),
            vbit_diameter: get_f64(settings, "clean_v", 0.05).max(ZERO),
            v_flop: get_legacy_bool(settings, "v_flop", false),
            straight: CleanupSelection {
                profile: paths.first().copied().unwrap_or(true),
                x: paths.get(1).copied().unwrap_or(true),
                y: paths.get(2).copied().unwrap_or(false),
                loops: paths.get(6).copied().unwrap_or(false),
            },
            vbit: CleanupSelection {
                profile: paths.get(3).copied().unwrap_or(true),
                y: paths.get(4).copied().unwrap_or(false),
                x: paths.get(5).copied().unwrap_or(true),
                loops: paths.get(7).copied().unwrap_or(false),
            },
        }
    }

    pub fn selection(&self, bit: CleanupBit) -> CleanupSelection {
        match bit {
            CleanupBit::Straight => self.straight,
            CleanupBit::VBit => self.vbit,
        }
    }

    pub fn tool_diameter(&self, bit: CleanupBit) -> f64 {
        match bit {
            CleanupBit::Straight => self.bit_diameter,
            CleanupBit::VBit => self.vbit_diameter,
        }
    }

    pub fn step_over(&self, bit: CleanupBit) -> f64 {
        match bit {
            CleanupBit::Straight => (self.step_over_percent / 100.0).max(ZERO),
            CleanupBit::VBit => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CleanupPoint {
    pub position: Point,
    pub radius: f64,
    pub loop_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupCanceled;

pub fn generate_cleanup_points(
    segments: &[EngraveSegment],
    cleanup: &CleanupOptions,
    vcarve: &VCarveOptions,
    bit: CleanupBit,
    accuracy: f64,
) -> Vec<CleanupPoint> {
    generate_cleanup_points_with_cancel(segments, cleanup, vcarve, bit, accuracy, &|| false)
        .expect("non-canceling cleanup generation should not cancel")
}

pub fn generate_cleanup_points_with_cancel(
    segments: &[EngraveSegment],
    cleanup: &CleanupOptions,
    vcarve: &VCarveOptions,
    bit: CleanupBit,
    accuracy: f64,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Vec<CleanupPoint>, CleanupCanceled> {
    check_canceled(cancel)?;
    let selection = cleanup.selection(bit);
    if !selection.any() {
        return Ok(Vec::new());
    }

    let source_paths = collect_closed_paths(segments, accuracy.max(ZERO), cancel)?;
    if source_paths.is_empty() {
        return Ok(Vec::new());
    }

    check_canceled(cancel)?;
    let area_paths = cleanup_area_paths(&source_paths, cleanup, vcarve, bit, accuracy.max(ZERO));
    check_canceled(cancel)?;
    if area_paths.is_empty() {
        return Ok(Vec::new());
    }

    let tool_diameter = cleanup.tool_diameter(bit);
    let step_over = cleanup.step_over(bit);
    let radius = tool_diameter / 2.0;
    let mut output = Vec::new();
    let mut next_loop_id = 1;

    if selection.profile {
        append_closed_paths(&area_paths, radius, &mut next_loop_id, &mut output, cancel)?;
    }

    if selection.loops {
        append_loop_offsets(
            &area_paths,
            tool_diameter * step_over / 2.0,
            radius,
            accuracy.max(ZERO),
            &mut next_loop_id,
            &mut output,
            cancel,
        )?;
    }

    if selection.x {
        let segments = horizontal_scanlines(
            &area_paths,
            tool_diameter,
            step_over,
            cleanup.v_flop,
            cancel,
        )?;
        append_ordered_segments(segments, radius, &mut next_loop_id, &mut output, cancel)?;
    }

    if selection.y {
        let segments = vertical_scanlines(
            &area_paths,
            tool_diameter,
            step_over,
            cleanup.v_flop,
            cancel,
        )?;
        append_ordered_segments(segments, radius, &mut next_loop_id, &mut output, cancel)?;
    }

    Ok(output)
}

fn check_canceled(cancel: &dyn Fn() -> bool) -> Result<(), CleanupCanceled> {
    if cancel() {
        Err(CleanupCanceled)
    } else {
        Ok(())
    }
}

fn cleanup_area_paths(
    source_paths: &[Vec<Point>],
    cleanup: &CleanupOptions,
    vcarve: &VCarveOptions,
    bit: CleanupBit,
    accuracy: f64,
) -> Vec<Vec<Point>> {
    let rbit = vcarve.effective_bit_diameter() / 2.0;
    let sign = if cleanup.v_flop { -1.0 } else { 1.0 };

    match bit {
        CleanupBit::Straight => {
            let radial_adjust = cleanup.bit_diameter / 2.0 + rbit;
            offset_paths(source_paths, sign * -radial_adjust, accuracy)
        }
        CleanupBit::VBit => {
            let flat_radius = cleanup.bit_diameter / 2.0;
            let vclean_radius = cleanup.vbit_diameter / 4.0;
            let step1 = offset_paths(source_paths, sign * -(flat_radius + rbit), accuracy);
            if step1.is_empty() {
                return Vec::new();
            }
            let inner_reach = offset_paths(&step1, sign * (flat_radius + vclean_radius), accuracy);
            let full_depth_reach =
                offset_paths(source_paths, sign * -(rbit + vclean_radius), accuracy);
            if area_magnitude(&inner_reach) >= area_magnitude(&full_depth_reach) {
                difference_paths(&inner_reach, &full_depth_reach)
            } else {
                difference_paths(&full_depth_reach, &inner_reach)
            }
        }
    }
}

fn collect_closed_paths(
    segments: &[EngraveSegment],
    accuracy: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Vec<Point>>, CleanupCanceled> {
    let mut paths = Vec::new();
    let mut current = Vec::new();
    let mut last_loop = None;
    let mut last_end = None;

    for segment in segments {
        check_canceled(cancel)?;
        let starts_new = last_loop != Some(segment.loop_id)
            || last_end
                .map(|point| distance(point, segment.start) > accuracy)
                .unwrap_or(true);
        if starts_new {
            push_if_closed(&mut paths, std::mem::take(&mut current), accuracy);
            current.push(segment.start);
        }
        current.push(segment.end);
        last_loop = Some(segment.loop_id);
        last_end = Some(segment.end);
    }

    push_if_closed(&mut paths, current, accuracy);
    Ok(paths)
}

fn push_if_closed(paths: &mut Vec<Vec<Point>>, mut path: Vec<Point>, accuracy: f64) {
    if path.len() < 3 {
        return;
    }
    let first = path[0];
    let last = *path.last().unwrap();
    if distance(first, last) > accuracy {
        return;
    }
    path.pop();
    if path.len() >= 3 {
        paths.push(path);
    }
}

fn offset_paths(paths: &[Vec<Point>], delta: f64, accuracy: f64) -> Vec<Vec<Point>> {
    if paths.is_empty() || delta.abs() <= ZERO {
        return paths.to_vec();
    }

    let clip_paths = to_clip_paths(paths);
    let output = clip_paths
        .inflate(
            delta,
            clipper2::JoinType::Round,
            clipper2::EndType::Polygon,
            0.0,
        )
        .simplify(accuracy, false);
    from_clip_paths(output)
}

fn difference_paths(subject: &[Vec<Point>], clip: &[Vec<Point>]) -> Vec<Vec<Point>> {
    if subject.is_empty() {
        return Vec::new();
    }
    if clip.is_empty() {
        return subject.to_vec();
    }

    to_clip_paths(subject)
        .to_clipper_subject()
        .add_clip(to_clip_paths(clip))
        .difference(clipper2::FillRule::EvenOdd)
        .map(from_clip_paths)
        .unwrap_or_default()
}

fn append_closed_paths(
    paths: &[Vec<Point>],
    radius: f64,
    next_loop_id: &mut usize,
    output: &mut Vec<CleanupPoint>,
    cancel: &dyn Fn() -> bool,
) -> Result<(), CleanupCanceled> {
    for path in order_paths_with_cancel(paths.to_vec(), cancel)? {
        check_canceled(cancel)?;
        if path.is_empty() {
            continue;
        }
        let loop_id = *next_loop_id;
        *next_loop_id += 1;
        for point in path.iter().copied().chain(std::iter::once(path[0])) {
            check_canceled(cancel)?;
            output.push(CleanupPoint {
                position: point,
                radius,
                loop_id,
            });
        }
    }
    Ok(())
}

fn append_loop_offsets(
    paths: &[Vec<Point>],
    step: f64,
    radius: f64,
    accuracy: f64,
    next_loop_id: &mut usize,
    output: &mut Vec<CleanupPoint>,
    cancel: &dyn Fn() -> bool,
) -> Result<(), CleanupCanceled> {
    if step <= ZERO {
        return Ok(());
    }

    let mut current = paths.to_vec();
    for _ in 0..1000 {
        check_canceled(cancel)?;
        current = offset_paths(&current, -step, accuracy);
        check_canceled(cancel)?;
        if current.is_empty() {
            break;
        }
        append_closed_paths(&current, radius, next_loop_id, output, cancel)?;
    }
    Ok(())
}

fn horizontal_scanlines(
    paths: &[Vec<Point>],
    tool_diameter: f64,
    step_over: f64,
    v_flop: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Vec<Point>>, CleanupCanceled> {
    let Some(bounds) = bounds(paths) else {
        return Ok(Vec::new());
    };
    let spacing = tool_diameter * step_over;
    if spacing <= ZERO {
        return Ok(Vec::new());
    }

    let offset = spacing / 2.0;
    let min_y = bounds.min.y + offset;
    let max_y = bounds.max.y - offset;
    let y_size = max_y - min_y;
    if y_size <= ZERO {
        return Ok(Vec::new());
    }

    let edge = usize::from(v_flop);
    let steps = (y_size / spacing).ceil() as usize;
    if steps == 0 {
        return Ok(Vec::new());
    }

    let mut output = Vec::new();
    for idx in 0..=steps {
        check_canceled(cancel)?;
        let y = min_y + idx as f64 / steps as f64 * y_size;
        let xs = horizontal_intersections(paths, y);
        for pair in paired_intersections(&xs, edge) {
            let x1 = pair.0 + offset;
            let x2 = pair.1 - offset;
            if x2 - x1 > ZERO {
                output.push(vec![Point::new(x1, y), Point::new(x2, y)]);
            }
        }
    }
    Ok(output)
}

fn vertical_scanlines(
    paths: &[Vec<Point>],
    tool_diameter: f64,
    step_over: f64,
    v_flop: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Vec<Point>>, CleanupCanceled> {
    let Some(bounds) = bounds(paths) else {
        return Ok(Vec::new());
    };
    let spacing = tool_diameter * step_over;
    if spacing <= ZERO {
        return Ok(Vec::new());
    }

    let offset = spacing / 2.0;
    let min_x = bounds.min.x + offset;
    let max_x = bounds.max.x - offset;
    let x_size = max_x - min_x;
    if x_size <= ZERO {
        return Ok(Vec::new());
    }

    let edge = usize::from(v_flop);
    let steps = (x_size / spacing).ceil() as usize;
    if steps == 0 {
        return Ok(Vec::new());
    }

    let mut output = Vec::new();
    for idx in 0..=steps {
        check_canceled(cancel)?;
        let x = min_x + idx as f64 / steps as f64 * x_size;
        let ys = vertical_intersections(paths, x);
        for pair in paired_intersections(&ys, edge) {
            let y1 = pair.0 + offset;
            let y2 = pair.1 - offset;
            if y2 - y1 > ZERO {
                output.push(vec![Point::new(x, y1), Point::new(x, y2)]);
            }
        }
    }
    Ok(output)
}

fn horizontal_intersections(paths: &[Vec<Point>], y: f64) -> Vec<f64> {
    let mut xs = Vec::new();
    for path in paths {
        for (a, b) in cyclic_edges(path) {
            if (a.y - b.y).abs() <= ZERO {
                continue;
            }
            let min_y = a.y.min(b.y);
            let max_y = a.y.max(b.y);
            if y < min_y || y >= max_y {
                continue;
            }
            let t = (y - a.y) / (b.y - a.y);
            xs.push(a.x + (b.x - a.x) * t);
        }
    }
    sorted_dedup(xs)
}

fn vertical_intersections(paths: &[Vec<Point>], x: f64) -> Vec<f64> {
    let mut ys = Vec::new();
    for path in paths {
        for (a, b) in cyclic_edges(path) {
            if (a.x - b.x).abs() <= ZERO {
                continue;
            }
            let min_x = a.x.min(b.x);
            let max_x = a.x.max(b.x);
            if x < min_x || x >= max_x {
                continue;
            }
            let t = (x - a.x) / (b.x - a.x);
            ys.push(a.y + (b.y - a.y) * t);
        }
    }
    sorted_dedup(ys)
}

fn paired_intersections(values: &[f64], edge: usize) -> impl Iterator<Item = (f64, f64)> + '_ {
    let end = values.len().saturating_sub(edge);
    values[edge..end]
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
}

fn append_ordered_segments(
    paths: Vec<Vec<Point>>,
    radius: f64,
    next_loop_id: &mut usize,
    output: &mut Vec<CleanupPoint>,
    cancel: &dyn Fn() -> bool,
) -> Result<(), CleanupCanceled> {
    for path in order_paths_with_cancel(paths, cancel)? {
        check_canceled(cancel)?;
        if path.len() < 2 {
            continue;
        }
        let loop_id = *next_loop_id;
        *next_loop_id += 1;
        for point in path {
            check_canceled(cancel)?;
            output.push(CleanupPoint {
                position: point,
                radius,
                loop_id,
            });
        }
    }
    Ok(())
}

fn order_paths_with_cancel(
    mut paths: Vec<Vec<Point>>,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Vec<Point>>, CleanupCanceled> {
    if paths.is_empty() {
        return Ok(paths);
    }

    let mut ordered = vec![paths.remove(0)];
    while !paths.is_empty() {
        check_canceled(cancel)?;
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

    Ok(ordered)
}

fn to_clip_paths(paths: &[Vec<Point>]) -> ClipPaths {
    paths
        .iter()
        .map(|path| {
            path.iter()
                .map(|point| (point.x, point.y))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .into()
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

fn cyclic_edges(path: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    path.iter()
        .copied()
        .zip(path.iter().copied().cycle().skip(1))
        .take(path.len())
}

fn bounds(paths: &[Vec<Point>]) -> Option<crate::layout::Bounds> {
    let mut points = paths.iter().flatten().copied();
    let first = points.next()?;
    let mut min = first;
    let mut max = first;
    for point in points {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
    }
    Some(crate::layout::Bounds { min, max })
}

fn area_magnitude(paths: &[Vec<Point>]) -> f64 {
    paths
        .iter()
        .map(|path| {
            cyclic_edges(path)
                .map(|(a, b)| a.x * b.y - b.x * a.y)
                .sum::<f64>()
                .abs()
                / 2.0
        })
        .sum()
}

fn sorted_dedup(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= ZERO);
    values
}

fn parse_clean_paths(value: Option<&str>) -> Vec<bool> {
    value
        .unwrap_or("1,1,0,1,0,1,0,0")
        .split(',')
        .map(legacy_bool_value)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_legacy_settings;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn parses_legacy_clean_paths_vbit_xy_order() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("clean_paths", "1,0,1,0,1,0,1,1", false);

        let options = CleanupOptions::from_legacy(&settings);

        assert!(options.straight.profile);
        assert!(!options.straight.x);
        assert!(options.straight.y);
        assert!(options.straight.loops);
        assert!(!options.vbit.profile);
        assert!(!options.vbit.x);
        assert!(options.vbit.y);
        assert!(options.vbit.loops);
    }

    #[test]
    fn generates_profile_and_scanline_cleanup_for_closed_square() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("clean_paths", "1,1,1,0,0,0,1,0", false);
        settings.set_or_push("clean_dia", "0.1", false);
        settings.set_or_push("clean_step", "50", false);
        let cleanup = CleanupOptions::from_legacy(&settings);
        let vcarve = VCarveOptions::from_legacy(&settings);

        let points = generate_cleanup_points(
            &square_segments(),
            &cleanup,
            &vcarve,
            CleanupBit::Straight,
            0.001,
        );

        assert!(points.iter().any(|point| point.loop_id == 1));
        assert!(points.iter().any(|point| point.loop_id > 1));
        assert!(
            points
                .iter()
                .all(|point| (point.radius - 0.05).abs() < 1e-9)
        );
        assert!(points.iter().any(|point| point.position.x > 0.5));
    }

    #[test]
    fn generates_vbit_cleanup_band_for_closed_square() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("clean_paths", "0,0,0,1,0,0,0,0", false);
        settings.set_or_push("clean_dia", "0.1", false);
        settings.set_or_push("clean_v", "0.05", false);
        let cleanup = CleanupOptions::from_legacy(&settings);
        let vcarve = VCarveOptions::from_legacy(&settings);

        let points = generate_cleanup_points(
            &square_segments(),
            &cleanup,
            &vcarve,
            CleanupBit::VBit,
            0.001,
        );

        assert!(!points.is_empty());
        assert!(
            points
                .iter()
                .all(|point| (point.radius - 0.025).abs() < 1e-9)
        );
    }

    #[test]
    fn cleanup_generation_can_cancel_inside_scanline_loop() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("clean_paths", "0,1,0,0,0,0,0,0", false);
        settings.set_or_push("clean_dia", "0.01", false);
        settings.set_or_push("clean_step", "10", false);
        let cleanup = CleanupOptions::from_legacy(&settings);
        let vcarve = VCarveOptions::from_legacy(&settings);
        let calls = AtomicUsize::new(0);

        let err = generate_cleanup_points_with_cancel(
            &square_segments(),
            &cleanup,
            &vcarve,
            CleanupBit::Straight,
            0.001,
            &|| {
                let next = calls.fetch_add(1, Ordering::Relaxed) + 1;
                next > 12
            },
        )
        .unwrap_err();

        assert_eq!(err, CleanupCanceled);
        assert!(calls.load(Ordering::Relaxed) > 12);
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
