use std::path::PathBuf;

use eframe::egui;
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
        Self {
            text: document.text,
            transform: ViewTransform::default(),
            status,
            settings_count: document.settings.entries.len(),
            show_toolpath: true,
            show_bounds: true,
            show_v_area: false,
            gcode_file: options.gcode_file,
            font_or_image: options.font_or_image,
            default_dir: options.default_dir,
            warnings: document.warnings,
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
                            self.transform.zoom = 1.0;
                        }
                    });
                    ui.separator();
                    if ui.button("Fit").clicked() {
                        self.transform.pan = Point::default();
                        self.transform.zoom = 1.0;
                    }
                    ui.add(egui::Slider::new(&mut self.transform.zoom, 0.25..=8.0).text("Zoom"));
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
                    ui.selectable_value(&mut self.status, "Ready".to_owned(), "Engrave");
                    ui.selectable_value(&mut self.status, "V-carve pending".to_owned(), "V-carve");
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
                let _ = ui.button("Calculate");
                let _ = ui.button("Export G-code");
                let _ = ui.button("Copy G-code");
                ui.separator();
                ui.checkbox(&mut self.show_toolpath, "Toolpath");
                ui.checkbox(&mut self.show_bounds, "Bounds");
                ui.checkbox(&mut self.show_v_area, "V-carve area");
            });

        egui::Panel::bottom("status_log")
            .exact_size(96.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Status:");
                    ui.monospace(&self.status);
                });
                ui.separator();
                ui.monospace(
                    "Port scaffold: settings compatibility is active; toolpath generation is pending.",
                );
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

            draw_preview(ui.painter(), rect, self.transform);
        });
    }
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

fn draw_preview(painter: &egui::Painter, rect: egui::Rect, transform: ViewTransform) {
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 30, 32));

    let center = rect.center();
    let model_points = [
        Point::new(-140.0, -50.0),
        Point::new(140.0, -50.0),
        Point::new(140.0, 50.0),
        Point::new(-140.0, 50.0),
        Point::new(-140.0, -50.0),
    ];

    let rot = egui::emath::Rot2::from_angle(transform.total_rotation_radians() as f32);
    let to_screen = |point: Point| {
        let point = egui::pos2(point.x as f32, point.y as f32);
        let rotated = rot * point.to_vec2();
        egui::pos2(
            center.x + rotated.x * transform.zoom as f32 + transform.pan.x as f32,
            center.y - rotated.y * transform.zoom as f32 + transform.pan.y as f32,
        )
    };

    for pair in model_points.windows(2) {
        painter.line_segment(
            [to_screen(pair[0]), to_screen(pair[1])],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(94, 176, 132)),
        );
    }

    painter.line_segment(
        [
            to_screen(Point::new(-180.0, 0.0)),
            to_screen(Point::new(180.0, 0.0)),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 150, 80)),
    );
    painter.line_segment(
        [
            to_screen(Point::new(0.0, -90.0)),
            to_screen(Point::new(0.0, 90.0)),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 130, 160)),
    );
}
