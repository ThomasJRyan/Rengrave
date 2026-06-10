use std::fs;
use std::path::PathBuf;

use eframe::egui;
use rengrave_core::batch::{BatchOutput, BatchRequest, prepare_batch_output};
use rengrave_core::geometry::{Point, ViewTransform};
use rengrave_core::project::{DocumentRequest, RengraveDocument, load_document};

#[derive(Debug, Clone, Default)]
pub struct UiLaunchOptions {
    pub gcode_file: Option<PathBuf>,
    pub font_or_image: Option<PathBuf>,
    pub default_dir: Option<PathBuf>,
    pub text: Option<String>,
}

pub fn run(options: UiLaunchOptions) -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("R-Engrave"),
        ..Default::default()
    };

    eframe::run_native(
        "R-Engrave",
        native_options,
        Box::new(|cc| Ok(Box::new(RengraveApp::new(cc, options)))),
    )
}

struct RengraveApp {
    text: String,
    transform: ViewTransform,
    status: String,
    settings_count: usize,
    gcode: String,
    svg: Option<String>,
    dxf: Option<String>,
    gcode_lines: usize,
    preview_segments: Vec<PreviewSegment>,
    preview_bounds: Option<PreviewBounds>,
    gcode_path: String,
    svg_path: String,
    dxf_path: String,
    show_toolpath: bool,
    show_bounds: bool,
    show_v_area: bool,
    gcode_file: Option<PathBuf>,
    font_or_image: Option<PathBuf>,
    default_dir: Option<PathBuf>,
    warnings: Vec<String>,
}

impl RengraveApp {
    fn new(cc: &eframe::CreationContext<'_>, options: UiLaunchOptions) -> Self {
        apply_theme(&cc.egui_ctx);
        let document_request = DocumentRequest {
            gcode_file: options.gcode_file.clone(),
            font_or_image: options.font_or_image.clone(),
            default_dir: options.default_dir.clone(),
            text: options.text,
        };
        let document = match load_document(&document_request) {
            Ok(document) => document,
            Err(err) => {
                let mut document = RengraveDocument::default();
                document.warnings.push(err.to_string());
                document
            }
        };
        let status = if document.warnings.is_empty() {
            "Ready".to_owned()
        } else {
            "Startup warning".to_owned()
        };
        let mut app = Self {
            text: document.text,
            transform: ViewTransform {
                zoom: 80.0,
                ..ViewTransform::default()
            },
            status,
            settings_count: document.settings.entries.len(),
            gcode: String::new(),
            svg: None,
            dxf: None,
            gcode_lines: 0,
            preview_segments: Vec::new(),
            preview_bounds: None,
            gcode_path: default_output_path(&options.default_dir, "rengrave_output.ngc"),
            svg_path: default_output_path(&options.default_dir, "rengrave_output.svg"),
            dxf_path: default_output_path(&options.default_dir, "rengrave_output.dxf"),
            show_toolpath: true,
            show_bounds: true,
            show_v_area: false,
            gcode_file: options.gcode_file,
            font_or_image: options.font_or_image,
            default_dir: options.default_dir,
            warnings: document.warnings,
        };
        app.calculate();
        app
    }

    fn batch_request(&self, include_exports: bool) -> BatchRequest {
        BatchRequest {
            batch: true,
            gcode_file: self.gcode_file.clone(),
            font_or_image: self.font_or_image.clone(),
            default_dir: self.default_dir.clone(),
            text: Some(self.text.clone()),
            output: None,
            svg_output: include_exports.then(|| PathBuf::from(&self.svg_path)),
            dxf_output: include_exports.then(|| PathBuf::from(&self.dxf_path)),
        }
    }

    fn calculate(&mut self) {
        match prepare_batch_output(&self.batch_request(true)) {
            Ok(output) => self.apply_batch_output(output),
            Err(err) => {
                self.status = "Generation failed".to_owned();
                self.warnings = vec![err.to_string()];
                self.gcode.clear();
                self.svg = None;
                self.dxf = None;
                self.gcode_lines = 0;
                self.preview_segments.clear();
                self.preview_bounds = None;
            }
        }
    }

    fn apply_batch_output(&mut self, output: BatchOutput) {
        self.gcode_lines = output.gcode.lines().count();
        self.preview_segments = parse_preview_segments(&output.gcode);
        self.preview_bounds = PreviewBounds::from_segments(&self.preview_segments);
        self.status = if self.preview_segments.is_empty() {
            "Settings loaded".to_owned()
        } else {
            format!("Generated {} lines", self.gcode_lines)
        };
        self.warnings = output.warnings;
        self.gcode = output.gcode;
        self.svg = output.svg;
        self.dxf = output.dxf;
    }

    fn export_current(&mut self, kind: ExportKind) {
        let (label, path, contents) = match kind {
            ExportKind::Gcode => ("G-code", self.gcode_path.clone(), Some(self.gcode.clone())),
            ExportKind::Svg => ("SVG", self.svg_path.clone(), self.svg.clone()),
            ExportKind::Dxf => ("DXF", self.dxf_path.clone(), self.dxf.clone()),
        };

        let Some(contents) = contents else {
            self.status = format!("{label} export unavailable");
            return;
        };

        match write_text_file(&path, &contents) {
            Ok(path) => self.status = format!("{label} exported: {}", path.display()),
            Err(err) => {
                self.status = format!("{label} export failed");
                self.warnings.push(err);
            }
        }
    }
}

impl eframe::App for RengraveApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar")
            .exact_size(34.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button("File", |ui| {
                        let _ = ui.button("Open Settings");
                        let _ = ui.button("Save G-code");
                    });
                    ui.menu_button("View", |ui| {
                        if ui.button("Fit").clicked() {
                            self.transform.pan = Point::default();
                            self.transform.zoom = 80.0;
                        }
                    });
                    ui.separator();
                    if ui.button("Fit").clicked() {
                        self.transform.pan = Point::default();
                        self.transform.zoom = 80.0;
                    }
                    ui.add(egui::Slider::new(&mut self.transform.zoom, 10.0..=300.0).text("Zoom"));
                    ui.add(
                        egui::Slider::new(
                            &mut self.transform.viewport_rotation_degrees,
                            -180.0..=180.0,
                        )
                        .text("View"),
                    );
                });
            });

        egui::Panel::left("input_settings")
            .exact_size(270.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.heading("Input");
                ui.label("Text");
                ui.add_sized(
                    [ui.available_width(), 96.0],
                    egui::TextEdit::multiline(&mut self.text),
                );
                ui.separator();
                ui.heading("Settings");
                ui.horizontal(|ui| {
                    ui.label("Mode");
                    ui.monospace(self.current_cut_type());
                });
                ui.add(
                    egui::Slider::new(&mut self.transform.model_rotation_degrees, -360.0..=360.0)
                        .text("Angle"),
                );
                ui.label(format!("Legacy keys: {}", self.settings_count));
                file_hint(ui, "Settings", &self.gcode_file);
                file_hint(ui, "Input", &self.font_or_image);
                file_hint(ui, "Default dir", &self.default_dir);
            });

        egui::Panel::right("output_tools")
            .exact_size(260.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                ui.heading("Output");
                if ui.button("Calculate").clicked() {
                    self.calculate();
                }
                ui.label("G-code path");
                ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::TextEdit::singleline(&mut self.gcode_path),
                );
                if ui
                    .add_enabled(!self.gcode.is_empty(), egui::Button::new("Export G-code"))
                    .clicked()
                {
                    self.export_current(ExportKind::Gcode);
                }
                ui.label("SVG path");
                ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::TextEdit::singleline(&mut self.svg_path),
                );
                if ui
                    .add_enabled(self.svg.is_some(), egui::Button::new("Export SVG"))
                    .clicked()
                {
                    self.export_current(ExportKind::Svg);
                }
                ui.label("DXF path");
                ui.add_sized(
                    [ui.available_width(), 22.0],
                    egui::TextEdit::singleline(&mut self.dxf_path),
                );
                if ui
                    .add_enabled(self.dxf.is_some(), egui::Button::new("Export DXF"))
                    .clicked()
                {
                    self.export_current(ExportKind::Dxf);
                }
                if ui
                    .add_enabled(!self.gcode.is_empty(), egui::Button::new("Copy G-code"))
                    .clicked()
                {
                    ui.ctx().copy_text(self.gcode.clone());
                    self.status = "G-code copied".to_owned();
                }
                ui.separator();
                ui.checkbox(&mut self.show_toolpath, "Toolpath");
                ui.checkbox(&mut self.show_bounds, "Bounds");
                ui.checkbox(&mut self.show_v_area, "V-carve area");
                ui.separator();
                ui.label(format!("G-code lines: {}", self.gcode_lines));
                ui.label(format!("Preview moves: {}", self.preview_segments.len()));
            });

        egui::Panel::bottom("status_log")
            .exact_size(96.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Status:");
                    ui.monospace(&self.status);
                });
                ui.separator();
                ui.monospace(format!(
                    "Primary output: {} lines, {} preview moves",
                    self.gcode_lines,
                    self.preview_segments.len()
                ));
                for warning in &self.warnings {
                    ui.colored_label(egui::Color32::from_rgb(225, 176, 84), warning);
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(rect, egui::Sense::drag());
            if response.dragged() {
                let delta = response.drag_delta();
                self.transform.pan.x += f64::from(delta.x);
                self.transform.pan.y += f64::from(delta.y);
                ui.ctx().request_repaint();
            }

            draw_preview(
                ui.painter(),
                rect,
                self.transform,
                &self.preview_segments,
                self.preview_bounds,
                self.show_toolpath,
                self.show_bounds,
            );
        });
    }
}

impl RengraveApp {
    fn current_cut_type(&self) -> &'static str {
        if self
            .gcode
            .lines()
            .any(|line| line.contains("fengrave_set cut_type") && line.contains("v-carve"))
        {
            "V-carve"
        } else {
            "Engrave"
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportKind {
    Gcode,
    Svg,
    Dxf,
}

fn default_output_path(default_dir: &Option<PathBuf>, file_name: &str) -> String {
    default_dir
        .as_ref()
        .map(|dir| dir.join(file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
        .display()
        .to_string()
}

fn write_text_file(path_text: &str, contents: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path_text.trim());
    if path.as_os_str().is_empty() {
        return Err("output path is empty".to_owned());
    }
    fs::write(&path, contents)
        .map_err(|err| format!("unable to write `{}`: {err}", path.display()))?;
    Ok(path)
}

fn file_hint(ui: &mut egui::Ui, label: &str, value: &Option<PathBuf>) {
    if let Some(path) = value {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.monospace(path.display().to_string());
        });
    }
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(36, 39, 42);
    visuals.window_fill = egui::Color32::from_rgb(42, 45, 48);
    visuals.selection.bg_fill = egui::Color32::from_rgb(54, 115, 141);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(93, 127, 143);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(62, 72, 78);
    ctx.set_visuals(visuals);
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PreviewSegment {
    start: Point,
    end: Point,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PreviewBounds {
    min: Point,
    max: Point,
}

impl PreviewBounds {
    fn from_segments(segments: &[PreviewSegment]) -> Option<Self> {
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
}

fn parse_preview_segments(gcode: &str) -> Vec<PreviewSegment> {
    let mut current = None;
    let mut segments = Vec::new();

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
        if matches!(command, "G2" | "G02" | "G3" | "G03") {
            if let Some(start) = current {
                if let (Some(i), Some(j)) = (params.i, params.j) {
                    let end = params.point(current).unwrap_or(start);
                    let center = Point::new(start.x + i, start.y + j);
                    append_preview_arc(
                        &mut segments,
                        start,
                        end,
                        center,
                        matches!(command, "G2" | "G02"),
                    );
                    current = Some(end);
                    continue;
                }
            }
        }

        let Some(next) = params.point(current) else {
            continue;
        };
        if matches!(command, "G1" | "G01" | "G2" | "G02" | "G3" | "G03") {
            if let Some(start) = current {
                if point_distance(start, next) > 0.00001 {
                    segments.push(PreviewSegment { start, end: next });
                }
            }
        }
        current = Some(next);
    }

    segments
}

#[derive(Debug, Default)]
struct MotionParams {
    x: Option<f64>,
    y: Option<f64>,
    i: Option<f64>,
    j: Option<f64>,
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

fn point_distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn draw_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    transform: ViewTransform,
    segments: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
    show_toolpath: bool,
    show_bounds: bool,
) {
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 30, 32));

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

    if show_toolpath {
        for segment in segments {
            painter.line_segment(
                [to_screen(segment.start), to_screen(segment.end)],
                egui::Stroke::new(1.4, egui::Color32::from_rgb(94, 176, 132)),
            );
        }
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linear_gcode_moves_for_preview() {
        let segments = parse_preview_segments(
            "G0 X0.0000 Y0.0000\nG1 Z-0.0050\nG1 X1.0000 Y0.0000\nG0 X2.0000 Y2.0000\nG1 X2.0000 Y3.0000\n",
        );

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start, Point::new(0.0, 0.0));
        assert_eq!(segments[0].end, Point::new(1.0, 0.0));
        assert_eq!(segments[1].start, Point::new(2.0, 2.0));
        assert_eq!(segments[1].end, Point::new(2.0, 3.0));
    }

    #[test]
    fn parses_full_circle_arc_for_preview() {
        let segments =
            parse_preview_segments("G0 X-2.0000 Y0.0000\nG1 Z-0.0050\nG2 I2.0000 J0.0000\n");

        assert_eq!(segments.len(), 64);
        assert_eq!(segments[0].start, Point::new(-2.0, 0.0));
        assert!((segments.last().unwrap().end.x + 2.0).abs() < 1e-9);
        assert!(segments.iter().any(|segment| segment.end.x > 1.99));
        assert!(segments.iter().any(|segment| segment.end.y > 1.99));
        assert!(segments.iter().any(|segment| segment.end.y < -1.99));
    }

    #[test]
    fn default_output_paths_use_default_dir_when_present() {
        let dir = Some(PathBuf::from("/tmp/rengrave-ui"));

        assert_eq!(
            default_output_path(&dir, "rengrave_output.ngc"),
            "/tmp/rengrave-ui/rengrave_output.ngc"
        );
        assert_eq!(
            default_output_path(&None, "rengrave_output.ngc"),
            "rengrave_output.ngc"
        );
    }

    #[test]
    fn write_text_file_reports_empty_paths() {
        let err = write_text_file("  ", "G90").unwrap_err();

        assert_eq!(err, "output path is empty");
    }
}
