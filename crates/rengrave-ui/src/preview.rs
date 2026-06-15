//! Toolpath preview/canvas rendering: G-code motion parsing, coordinate
//! transforms, and all egui drawing for the central preview (grid, axes,
//! bounds, scale bar, layer overlay, and the input outline overlay).

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PreviewSegment {
    pub(crate) start: Point,
    pub(crate) end: Point,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PreviewBounds {
    pub(crate) min: Point,
    pub(crate) max: Point,
}

impl PreviewBounds {
    pub(crate) fn from_segments(segments: &[PreviewSegment]) -> Option<Self> {
        let mut points = segments
            .iter()
            .flat_map(|segment| [segment.start, segment.end].into_iter());
        let first = points.next()?;
        let mut min = first;
        let mut max = first;
        for point in points {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }
        Some(Self { min, max })
    }

    pub(crate) fn corners(self) -> [Point; 4] {
        [
            Point::new(self.min.x, self.min.y),
            Point::new(self.max.x, self.min.y),
            Point::new(self.max.x, self.max.y),
            Point::new(self.min.x, self.max.y),
        ]
    }

    pub(crate) fn from_segment_layers(layers: &[&[PreviewSegment]]) -> Option<Self> {
        let mut points = layers
            .iter()
            .flat_map(|layer| layer.iter())
            .flat_map(|segment| [segment.start, segment.end].into_iter());
        let first = points.next()?;
        let mut min = first;
        let mut max = first;
        for point in points {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }
        Some(Self { min, max })
    }
}

pub(crate) fn preview_bounds_readout(bounds: Option<PreviewBounds>) -> Option<(String, String)> {
    let bounds = bounds?;
    let width = (bounds.max.x - bounds.min.x).abs();
    let height = (bounds.max.y - bounds.min.y).abs();
    Some((
        format!(
            "Extents: {} x {}",
            format_preview_coord(width),
            format_preview_coord(height)
        ),
        format!(
            "X {}..{}  Y {}..{}",
            format_preview_coord(bounds.min.x),
            format_preview_coord(bounds.max.x),
            format_preview_coord(bounds.min.y),
            format_preview_coord(bounds.max.y)
        ),
    ))
}

pub(crate) fn preview_length_readout(label: &str, segments: &[PreviewSegment]) -> String {
    format!(
        "{label}: {}",
        format_preview_coord(total_segment_length(segments))
    )
}

pub(crate) fn total_segment_length(segments: &[PreviewSegment]) -> f64 {
    segments
        .iter()
        .map(|segment| {
            let dx = segment.end.x - segment.start.x;
            let dy = segment.end.y - segment.start.y;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

pub(crate) fn format_preview_coord(value: f64) -> String {
    let value = if value.abs() < 0.00005 { 0.0 } else { value };
    format!("{value:.4}")
}

pub(crate) fn fit_transform_to_bounds(
    transform: &mut ViewTransform,
    bounds: Option<PreviewBounds>,
    rect: egui::Rect,
) {
    let Some(bounds) = bounds else {
        transform.pan = Point::default();
        transform.zoom = DEFAULT_PREVIEW_ZOOM;
        return;
    };

    let (sin, cos) = transform.total_rotation_radians().sin_cos();
    let mut rotated_min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut rotated_max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for point in bounds.corners() {
        let rotated = Point::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos);
        rotated_min.x = rotated_min.x.min(rotated.x);
        rotated_min.y = rotated_min.y.min(rotated.y);
        rotated_max.x = rotated_max.x.max(rotated.x);
        rotated_max.y = rotated_max.y.max(rotated.y);
    }

    let model_width = (rotated_max.x - rotated_min.x).abs().max(0.001);
    let model_height = (rotated_max.y - rotated_min.y).abs().max(0.001);
    let available_width = (rect.width() - PREVIEW_FIT_PADDING * 2.0).max(1.0) as f64;
    let available_height = (rect.height() - PREVIEW_FIT_PADDING * 2.0).max(1.0) as f64;
    let zoom = (available_width / model_width)
        .min(available_height / model_height)
        .clamp(1.0, 500.0);
    let center = Point::new(
        (rotated_min.x + rotated_max.x) / 2.0,
        (rotated_min.y + rotated_max.y) / 2.0,
    );

    transform.zoom = zoom;
    transform.pan = Point::new(-center.x * zoom, center.y * zoom);
}

pub(crate) fn zoom_transform_at_screen_point(
    transform: &mut ViewTransform,
    rect: egui::Rect,
    anchor: egui::Pos2,
    zoom_factor: f64,
) {
    if !zoom_factor.is_finite() || zoom_factor <= 0.0 || transform.zoom <= 0.0 {
        return;
    }

    let old_zoom = transform.zoom;
    let new_zoom = (old_zoom * zoom_factor).clamp(1.0, 500.0);
    if (new_zoom - old_zoom).abs() <= f64::EPSILON {
        return;
    }

    let applied_factor = new_zoom / old_zoom;
    let relative_x = f64::from(anchor.x - rect.center().x);
    let relative_y = f64::from(anchor.y - rect.center().y);

    transform.pan = Point::new(
        relative_x - (relative_x - transform.pan.x) * applied_factor,
        relative_y + (transform.pan.y - relative_y) * applied_factor,
    );
    transform.zoom = new_zoom;
}

pub(crate) fn screen_point_to_model(
    rect: egui::Rect,
    transform: ViewTransform,
    screen: egui::Pos2,
) -> Point {
    let rotated = Point::new(
        f64::from(screen.x - rect.center().x) - transform.pan.x,
        f64::from(rect.center().y - screen.y) + transform.pan.y,
    );
    let rotated = Point::new(rotated.x / transform.zoom, rotated.y / transform.zoom);
    let (sin, cos) = transform.total_rotation_radians().sin_cos();
    Point::new(
        rotated.x * cos + rotated.y * sin,
        -rotated.x * sin + rotated.y * cos,
    )
}

pub(crate) fn draw_preview_cursor_readout(
    painter: &egui::Painter,
    rect: egui::Rect,
    cursor: Point,
) {
    let text = format!("X {:+.4}  Y {:+.4}", cursor.x, cursor.y);
    let pos = rect.left_bottom() + egui::vec2(8.0, -8.0);
    painter.text(
        pos,
        egui::Align2::LEFT_BOTTOM,
        text,
        egui::FontId::monospace(12.0),
        egui::Color32::from_rgb(214, 220, 224),
    );
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PreviewMotion {
    pub(crate) cuts: Vec<PreviewSegment>,
    pub(crate) rapids: Vec<PreviewSegment>,
}

pub(crate) fn parse_preview_motion(gcode: &str) -> PreviewMotion {
    let mut current = None;
    let mut motion = PreviewMotion::default();

    for line in gcode.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('(') || trimmed.is_empty() {
            continue;
        }

        let command = trimmed.split_whitespace().next().unwrap_or_default();
        let is_motion = matches!(
            command,
            "G0" | "G00" | "G1" | "G01" | "G2" | "G02" | "G3" | "G03"
        );
        if !is_motion {
            continue;
        }

        let params = motion_params(trimmed);
        if matches!(command, "G0" | "G00") {
            let Some(next) = params.point(current) else {
                continue;
            };
            if let Some(start) = current {
                if point_distance(start, next) > 0.00001 {
                    motion.rapids.push(PreviewSegment { start, end: next });
                }
            }
            current = Some(next);
            continue;
        }

        if matches!(command, "G2" | "G02" | "G3" | "G03") {
            if let Some(start) = current {
                if let (Some(i), Some(j)) = (params.i, params.j) {
                    let end = params.point(current).unwrap_or(start);
                    let center = Point::new(start.x + i, start.y + j);
                    append_preview_arc(
                        &mut motion.cuts,
                        start,
                        end,
                        center,
                        matches!(command, "G2" | "G02"),
                    );
                    current = Some(end);
                    continue;
                }
                if let Some(radius) = params.r {
                    if let Some(end) = params.point(current) {
                        append_preview_radius_arc(
                            &mut motion.cuts,
                            start,
                            end,
                            radius,
                            matches!(command, "G2" | "G02"),
                        );
                        current = Some(end);
                        continue;
                    }
                }
            }
        }

        let Some(next) = params.point(current) else {
            continue;
        };
        if matches!(command, "G1" | "G01" | "G2" | "G02" | "G3" | "G03") {
            if let Some(start) = current {
                if point_distance(start, next) > 0.00001 {
                    motion.cuts.push(PreviewSegment { start, end: next });
                }
            }
        }
        current = Some(next);
    }

    motion
}

pub(crate) fn cleanup_preview_segments(outputs: &[SecondaryGcode]) -> Vec<PreviewSegment> {
    outputs
        .iter()
        .flat_map(|output| parse_preview_motion(&output.gcode).cuts)
        .collect()
}

#[derive(Debug, Default)]
struct MotionParams {
    x: Option<f64>,
    y: Option<f64>,
    i: Option<f64>,
    j: Option<f64>,
    r: Option<f64>,
    saw_xy: bool,
}

impl MotionParams {
    fn point(&self, current: Option<Point>) -> Option<Point> {
        if self.saw_xy {
            Some(Point::new(
                self.x
                    .or_else(|| current.map(|point| point.x))
                    .unwrap_or(0.0),
                self.y
                    .or_else(|| current.map(|point| point.y))
                    .unwrap_or(0.0),
            ))
        } else {
            current
        }
    }
}

fn motion_params(line: &str) -> MotionParams {
    let mut params = MotionParams::default();

    for token in line.split_whitespace().skip(1) {
        if let Some(value) = axis_value(token, 'X') {
            params.x = Some(value);
            params.saw_xy = true;
        } else if let Some(value) = axis_value(token, 'Y') {
            params.y = Some(value);
            params.saw_xy = true;
        } else if let Some(value) = axis_value(token, 'I') {
            params.i = Some(value);
        } else if let Some(value) = axis_value(token, 'J') {
            params.j = Some(value);
        } else if let Some(value) = axis_value(token, 'R') {
            params.r = Some(value);
        }
    }

    params
}

fn axis_value(token: &str, axis: char) -> Option<f64> {
    token
        .strip_prefix(axis)
        .and_then(|value| value.parse().ok())
}

fn append_preview_arc(
    segments: &mut Vec<PreviewSegment>,
    start: Point,
    end: Point,
    center: Point,
    clockwise: bool,
) {
    let radius = point_distance(start, center);
    if radius <= 0.00001 {
        return;
    }

    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let end_angle = (end.y - center.y).atan2(end.x - center.x);
    let full_circle = point_distance(start, end) <= 0.00001;
    let mut sweep = if full_circle {
        if clockwise {
            -std::f64::consts::TAU
        } else {
            std::f64::consts::TAU
        }
    } else {
        end_angle - start_angle
    };

    if clockwise && sweep >= 0.0 {
        sweep -= std::f64::consts::TAU;
    } else if !clockwise && sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }

    let steps = ((sweep.abs() / std::f64::consts::TAU) * 64.0)
        .ceil()
        .max(4.0) as usize;
    let mut previous = start;
    for step in 1..=steps {
        let angle = start_angle + sweep * step as f64 / steps as f64;
        let next = Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        segments.push(PreviewSegment {
            start: previous,
            end: next,
        });
        previous = next;
    }
}

fn append_preview_radius_arc(
    segments: &mut Vec<PreviewSegment>,
    start: Point,
    end: Point,
    radius: f64,
    clockwise: bool,
) {
    let chord = point_distance(start, end);
    let radius_abs = radius.abs();
    if chord <= 0.00001 || radius_abs <= 0.00001 || chord > 2.0 * radius_abs + 0.00001 {
        if chord > 0.00001 {
            segments.push(PreviewSegment { start, end });
        }
        return;
    }

    let midpoint = Point::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
    let half_chord = chord / 2.0;
    let offset = (radius_abs * radius_abs - half_chord * half_chord)
        .max(0.0)
        .sqrt();
    let unit_x = (end.x - start.x) / chord;
    let unit_y = (end.y - start.y) / chord;
    let perp = Point::new(-unit_y, unit_x);
    let centers = [
        Point::new(midpoint.x + perp.x * offset, midpoint.y + perp.y * offset),
        Point::new(midpoint.x - perp.x * offset, midpoint.y - perp.y * offset),
    ];
    let wants_long_arc = radius < 0.0;
    let center = centers
        .into_iter()
        .find(|center| {
            let sweep = preview_arc_sweep(start, end, *center, clockwise);
            (sweep.abs() > std::f64::consts::PI) == wants_long_arc
        })
        .unwrap_or(centers[0]);

    append_preview_arc(segments, start, end, center, clockwise);
}

fn preview_arc_sweep(start: Point, end: Point, center: Point, clockwise: bool) -> f64 {
    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let end_angle = (end.y - center.y).atan2(end.x - center.x);
    let mut sweep = end_angle - start_angle;

    if clockwise && sweep >= 0.0 {
        sweep -= std::f64::consts::TAU;
    } else if !clockwise && sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }

    sweep
}

fn point_distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

/// Lays out the input outline in the same engraving coordinate space as the
/// toolpath, so it can be overlaid directly on the toolpath preview. Works for
/// both text and image (bitmap/DXF) inputs. Returns an empty vector when no
/// outline is available.
pub(crate) fn input_outline_segments(request: &BatchRequest) -> Vec<PreviewSegment> {
    match layout_text_outline(request) {
        Ok(Some(outline)) => outline
            .segments
            .iter()
            .map(|segment| PreviewSegment {
                start: segment.start,
                end: segment.end,
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn draw_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    transform: ViewTransform,
    unit_label: &str,
    segments: &[PreviewSegment],
    rapids: &[PreviewSegment],
    cleanup_segments: &[PreviewSegment],
    input_overlay: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
    show_toolpath: bool,
    show_rapids: bool,
    show_cleanup: bool,
    show_input_overlay: bool,
    show_bounds: bool,
    show_axes: bool,
    show_grid: bool,
) {
    painter.rect_filled(rect, 0.0, preview_background_color());

    let center = rect.center();
    let rot = egui::emath::Rot2::from_angle(transform.total_rotation_radians() as f32);
    let to_screen = |point: Point| {
        let point = egui::pos2(point.x as f32, point.y as f32);
        let rotated = rot * point.to_vec2();
        egui::pos2(
            center.x + rotated.x * transform.zoom as f32 + transform.pan.x as f32,
            center.y - rotated.y * transform.zoom as f32 + transform.pan.y as f32,
        )
    };

    if show_grid {
        draw_preview_grid(painter, rect, transform, &to_screen);
    }

    if show_bounds {
        if let Some(bounds) = bounds {
            let points = [
                Point::new(bounds.min.x, bounds.min.y),
                Point::new(bounds.max.x, bounds.min.y),
                Point::new(bounds.max.x, bounds.max.y),
                Point::new(bounds.min.x, bounds.max.y),
                Point::new(bounds.min.x, bounds.min.y),
            ];
            for pair in points.windows(2) {
                painter.line_segment(
                    [to_screen(pair[0]), to_screen(pair[1])],
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 104, 112)),
                );
            }
        }
    }

    if show_rapids {
        for segment in rapids {
            draw_dashed_line(
                painter,
                to_screen(segment.start),
                to_screen(segment.end),
                egui::Stroke::new(1.0, egui::Color32::from_rgb(190, 142, 72)),
                8.0,
                5.0,
            );
        }
    }

    if show_cleanup {
        for segment in cleanup_segments {
            painter.line_segment(
                [to_screen(segment.start), to_screen(segment.end)],
                egui::Stroke::new(1.2, egui::Color32::from_rgb(118, 164, 190)),
            );
        }
    }

    if show_toolpath {
        for segment in segments {
            painter.line_segment(
                [to_screen(segment.start), to_screen(segment.end)],
                egui::Stroke::new(1.4, egui::Color32::from_rgb(94, 176, 132)),
            );
        }
    }

    if show_input_overlay {
        for segment in input_overlay {
            painter.line_segment(
                [to_screen(segment.start), to_screen(segment.end)],
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(230, 168, 220, 205),
                ),
            );
        }
    }

    if show_axes {
        let axis_span = bounds
            .map(|bounds| {
                let width = (bounds.max.x - bounds.min.x).abs().max(2.0);
                let height = (bounds.max.y - bounds.min.y).abs().max(2.0);
                width.max(height)
            })
            .unwrap_or(4.0);
        painter.line_segment(
            [
                to_screen(Point::new(-axis_span, 0.0)),
                to_screen(Point::new(axis_span, 0.0)),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 150, 80)),
        );
        painter.line_segment(
            [
                to_screen(Point::new(0.0, -axis_span)),
                to_screen(Point::new(0.0, axis_span)),
            ],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 130, 160)),
        );
        painter.text(
            to_screen(Point::new(axis_span, 0.0)) + egui::vec2(6.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            "X",
            egui::FontId::monospace(12.0),
            egui::Color32::from_rgb(220, 188, 104),
        );
        painter.text(
            to_screen(Point::new(0.0, axis_span)) + egui::vec2(6.0, -4.0),
            egui::Align2::LEFT_BOTTOM,
            "Y",
            egui::FontId::monospace(12.0),
            egui::Color32::from_rgb(104, 166, 200),
        );
    }

    draw_preview_overlay(
        painter,
        rect,
        &preview_overlay_items(
            segments,
            rapids,
            cleanup_segments,
            bounds,
            show_toolpath,
            show_rapids,
            show_cleanup,
        ),
    );
    draw_preview_scale_bar(painter, rect, transform.zoom, unit_label);
}

fn preview_background_color() -> egui::Color32 {
    egui::Color32::from_rgb(28, 30, 32)
}

fn draw_preview_grid(
    painter: &egui::Painter,
    rect: egui::Rect,
    transform: ViewTransform,
    to_screen: &impl Fn(Point) -> egui::Pos2,
) {
    let step = nice_grid_step(transform.zoom);
    let corners = [
        screen_point_to_model(rect, transform, rect.left_top()),
        screen_point_to_model(rect, transform, rect.right_top()),
        screen_point_to_model(rect, transform, rect.right_bottom()),
        screen_point_to_model(rect, transform, rect.left_bottom()),
    ];
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for corner in corners {
        min.x = min.x.min(corner.x);
        min.y = min.y.min(corner.y);
        max.x = max.x.max(corner.x);
        max.y = max.y.max(corner.y);
    }

    min.x -= step * 2.0;
    min.y -= step * 2.0;
    max.x += step * 2.0;
    max.y += step * 2.0;

    let minor = egui::Stroke::new(0.6, egui::Color32::from_rgb(43, 48, 51));
    let major = egui::Stroke::new(0.9, egui::Color32::from_rgb(56, 63, 67));
    draw_grid_axis_lines(
        painter,
        min.x,
        max.x,
        step,
        |x| {
            [
                to_screen(Point::new(x, min.y)),
                to_screen(Point::new(x, max.y)),
            ]
        },
        minor,
        major,
    );
    draw_grid_axis_lines(
        painter,
        min.y,
        max.y,
        step,
        |y| {
            [
                to_screen(Point::new(min.x, y)),
                to_screen(Point::new(max.x, y)),
            ]
        },
        minor,
        major,
    );
}

fn draw_grid_axis_lines(
    painter: &egui::Painter,
    min: f64,
    max: f64,
    step: f64,
    points_for_value: impl Fn(f64) -> [egui::Pos2; 2],
    minor: egui::Stroke,
    major: egui::Stroke,
) {
    if step <= 0.0 || !step.is_finite() || !min.is_finite() || !max.is_finite() {
        return;
    }
    let start = (min / step).floor() as i64;
    let end = (max / step).ceil() as i64;
    if end < start || end.saturating_sub(start) > 500 {
        return;
    }
    for index in start..=end {
        let stroke = if index % 5 == 0 { major } else { minor };
        painter.line_segment(points_for_value(index as f64 * step), stroke);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PreviewScaleBar {
    pub(crate) model_length: f64,
    pub(crate) pixel_length: f32,
}

pub(crate) fn preview_scale_bar(zoom: f64) -> Option<PreviewScaleBar> {
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }

    let model_length = nice_scale_length(96.0 / zoom);
    Some(PreviewScaleBar {
        model_length,
        pixel_length: (model_length * zoom) as f32,
    })
}

fn nice_scale_length(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }

    let exponent = raw.log10().floor();
    let magnitude = 10.0_f64.powf(exponent);
    let normalized = raw / magnitude;
    let nice = if normalized < 1.5 {
        1.0
    } else if normalized < 3.5 {
        2.0
    } else if normalized < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

pub(crate) fn format_scale_bar_label(length: f64, unit_label: &str) -> String {
    let decimals = if length >= 100.0 {
        0
    } else if length >= 10.0 {
        1
    } else if length >= 1.0 {
        2
    } else {
        4
    };
    let mut value = format!("{length:.decimals$}");
    if value.contains('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    format!("{value} {unit_label}")
}

fn draw_preview_scale_bar(painter: &egui::Painter, rect: egui::Rect, zoom: f64, unit_label: &str) {
    if rect.width() < 170.0 || rect.height() < 90.0 {
        return;
    }
    let Some(scale) = preview_scale_bar(zoom) else {
        return;
    };

    let x2 = rect.right() - 14.0;
    let x1 = x2 - scale.pixel_length;
    if x1 < rect.left() + 14.0 {
        return;
    }
    let y = rect.bottom() - 22.0;
    let panel = egui::Rect::from_min_max(
        egui::pos2(x1 - 10.0, y - 28.0),
        egui::pos2(x2 + 10.0, y + 10.0),
    );
    painter.rect_filled(
        panel,
        4.0,
        egui::Color32::from_rgba_unmultiplied(18, 20, 22, 210),
    );
    painter.rect_stroke(
        panel,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 66, 70)),
        egui::StrokeKind::Inside,
    );
    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(214, 220, 224));
    painter.line_segment([egui::pos2(x1, y), egui::pos2(x2, y)], stroke);
    painter.line_segment([egui::pos2(x1, y - 5.0), egui::pos2(x1, y + 5.0)], stroke);
    painter.line_segment([egui::pos2(x2, y - 5.0), egui::pos2(x2, y + 5.0)], stroke);
    painter.text(
        egui::pos2((x1 + x2) * 0.5, y - 7.0),
        egui::Align2::CENTER_BOTTOM,
        format_scale_bar_label(scale.model_length, unit_label),
        egui::FontId::monospace(11.0),
        egui::Color32::from_rgb(214, 220, 224),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewOverlayItem {
    pub(crate) text: String,
    pub(crate) color: egui::Color32,
    pub(crate) swatch: bool,
}

pub(crate) fn preview_overlay_items(
    segments: &[PreviewSegment],
    rapids: &[PreviewSegment],
    cleanup_segments: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
    show_toolpath: bool,
    show_rapids: bool,
    show_cleanup: bool,
) -> Vec<PreviewOverlayItem> {
    let mut items = Vec::new();
    if show_toolpath && !segments.is_empty() {
        items.push(PreviewOverlayItem {
            text: format!("Cut {}", segments.len()),
            color: egui::Color32::from_rgb(94, 176, 132),
            swatch: true,
        });
    }
    if show_rapids && !rapids.is_empty() {
        items.push(PreviewOverlayItem {
            text: format!("Rapid {}", rapids.len()),
            color: egui::Color32::from_rgb(190, 142, 72),
            swatch: true,
        });
    }
    if show_cleanup && !cleanup_segments.is_empty() {
        items.push(PreviewOverlayItem {
            text: format!("Cleanup {}", cleanup_segments.len()),
            color: egui::Color32::from_rgb(118, 164, 190),
            swatch: true,
        });
    }
    if let Some(bounds) = bounds {
        items.push(PreviewOverlayItem {
            text: format!(
                "X {}..{}",
                format_preview_coord(bounds.min.x),
                format_preview_coord(bounds.max.x)
            ),
            color: egui::Color32::from_rgb(214, 220, 224),
            swatch: false,
        });
        items.push(PreviewOverlayItem {
            text: format!(
                "Y {}..{}",
                format_preview_coord(bounds.min.y),
                format_preview_coord(bounds.max.y)
            ),
            color: egui::Color32::from_rgb(214, 220, 224),
            swatch: false,
        });
    }
    items
}

fn draw_preview_overlay(painter: &egui::Painter, rect: egui::Rect, items: &[PreviewOverlayItem]) {
    if items.is_empty() || rect.width() < 140.0 || rect.height() < 90.0 {
        return;
    }

    let origin = rect.left_top() + egui::vec2(10.0, 10.0);
    let line_height = 16.0;
    let width = 172.0_f32.min((rect.width() - 20.0).max(120.0));
    let height = items.len() as f32 * line_height + 10.0;
    let overlay = egui::Rect::from_min_size(origin, egui::vec2(width, height));
    painter.rect_filled(
        overlay,
        4.0,
        egui::Color32::from_rgba_unmultiplied(18, 20, 22, 210),
    );
    painter.rect_stroke(
        overlay,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 66, 70)),
        egui::StrokeKind::Inside,
    );

    for (index, item) in items.iter().enumerate() {
        let y = origin.y + 7.0 + index as f32 * line_height;
        let text_x = if item.swatch {
            painter.circle_filled(egui::pos2(origin.x + 9.0, y + 5.5), 3.5, item.color);
            origin.x + 18.0
        } else {
            origin.x + 8.0
        };
        painter.text(
            egui::pos2(text_x, y),
            egui::Align2::LEFT_TOP,
            &item.text,
            egui::FontId::monospace(11.0),
            item.color,
        );
    }
}

pub(crate) fn nice_grid_step(zoom: f64) -> f64 {
    if !zoom.is_finite() || zoom <= 0.0 {
        return 1.0;
    }
    let target_model_units = 64.0 / zoom;
    if target_model_units <= 0.0 || !target_model_units.is_finite() {
        return 1.0;
    }

    let exponent = target_model_units.log10().floor();
    let magnitude = 10.0_f64.powf(exponent);
    let normalized = target_model_units / magnitude;
    let nice = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * magnitude
}

fn draw_dashed_line(
    painter: &egui::Painter,
    start: egui::Pos2,
    end: egui::Pos2,
    stroke: egui::Stroke,
    dash_length: f32,
    gap_length: f32,
) {
    let vector = end - start;
    let length = vector.length();
    if length <= 0.001 {
        return;
    }

    let direction = vector / length;
    let mut offset = 0.0;
    while offset < length {
        let next_offset = (offset + dash_length).min(length);
        painter.line_segment(
            [start + direction * offset, start + direction * next_offset],
            stroke,
        );
        offset += dash_length + gap_length;
    }
}
