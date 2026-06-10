use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::thread;

use eframe::egui;
use rengrave_core::batch::{BatchOutput, BatchRequest, prepare_batch_output};
use rengrave_core::geometry::{Point, ViewTransform};
use rengrave_core::project::{DocumentRequest, RengraveDocument, load_document};
use rengrave_core::settings::{LegacySetting, LegacySettings, get_legacy_bool};

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
    settings_path: String,
    input_path: String,
    default_dir_path: String,
    controls: UiControls,
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
    browser: Option<FileBrowser>,
    preferences_path: Option<PathBuf>,
    calculation: Option<CalculationJob>,
    next_calculation_id: u64,
    warnings: Vec<String>,
}

impl RengraveApp {
    fn new(cc: &eframe::CreationContext<'_>, options: UiLaunchOptions) -> Self {
        apply_theme(&cc.egui_ctx);
        let preferences_path = default_preferences_path();
        let preferences = preferences_path
            .as_deref()
            .and_then(|path| UiPreferences::load(path).ok())
            .unwrap_or_default();
        let gcode_file = options
            .gcode_file
            .clone()
            .or_else(|| path_from_text(&preferences.settings_path));
        let font_or_image = options
            .font_or_image
            .clone()
            .or_else(|| path_from_text(&preferences.input_path));
        let default_dir = options
            .default_dir
            .clone()
            .or_else(|| path_from_text(&preferences.default_dir_path));
        let document_request = DocumentRequest {
            gcode_file: gcode_file.clone(),
            font_or_image: font_or_image.clone(),
            default_dir: default_dir.clone(),
            text: options.text.clone(),
            settings_overrides: Vec::new(),
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
            settings_path: path_to_text(&gcode_file),
            input_path: path_to_text(&font_or_image),
            default_dir_path: path_to_text(&default_dir),
            controls: UiControls::from_settings(&document.settings),
            gcode: String::new(),
            svg: None,
            dxf: None,
            gcode_lines: 0,
            preview_segments: Vec::new(),
            preview_bounds: None,
            gcode_path: if preferences.gcode_path.trim().is_empty() {
                default_output_path(&default_dir, "rengrave_output.ngc")
            } else {
                preferences.gcode_path
            },
            svg_path: if preferences.svg_path.trim().is_empty() {
                default_output_path(&default_dir, "rengrave_output.svg")
            } else {
                preferences.svg_path
            },
            dxf_path: if preferences.dxf_path.trim().is_empty() {
                default_output_path(&default_dir, "rengrave_output.dxf")
            } else {
                preferences.dxf_path
            },
            show_toolpath: true,
            show_bounds: true,
            show_v_area: false,
            browser: None,
            preferences_path,
            calculation: None,
            next_calculation_id: 1,
            warnings: document.warnings,
        };
        app.start_calculation(cc.egui_ctx.clone());
        app
    }

    fn batch_request(&self, include_exports: bool) -> BatchRequest {
        BatchRequest {
            batch: true,
            gcode_file: path_from_text(&self.settings_path),
            font_or_image: path_from_text(&self.input_path),
            default_dir: path_from_text(&self.default_dir_path),
            text: Some(self.text.clone()),
            output: None,
            svg_output: include_exports.then(|| PathBuf::from(&self.svg_path)),
            dxf_output: include_exports.then(|| PathBuf::from(&self.dxf_path)),
            settings_overrides: self.controls.overrides(),
        }
    }

    fn reload_document(&mut self, ctx: egui::Context) {
        match load_document(&DocumentRequest {
            gcode_file: path_from_text(&self.settings_path),
            font_or_image: path_from_text(&self.input_path),
            default_dir: path_from_text(&self.default_dir_path),
            text: None,
            settings_overrides: Vec::new(),
        }) {
            Ok(document) => {
                self.text = document.text;
                self.controls = UiControls::from_settings(&document.settings);
                self.settings_count = document.settings.entries.len();
                self.warnings = document.warnings;
                self.status = "Document loaded".to_owned();
                self.save_preferences();
                self.start_calculation(ctx);
            }
            Err(err) => {
                self.cancel_calculation("Load failed");
                self.status = "Load failed".to_owned();
                self.warnings = vec![err.to_string()];
            }
        }
    }

    fn start_calculation(&mut self, ctx: egui::Context) {
        self.cancel_calculation("Calculation superseded");
        let id = self.next_calculation_id;
        self.next_calculation_id += 1;
        let request = self.batch_request(true);
        let worker_request = request.clone();
        let (sender, receiver) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let worker_cancel_flag = Arc::clone(&cancel_flag);
        thread::spawn(move || {
            let result = prepare_batch_output(&worker_request).map_err(|err| err.to_string());
            let canceled = worker_cancel_flag.load(Ordering::Relaxed);
            let _ = sender.send(CalculationMessage {
                id,
                result,
                canceled,
            });
            ctx.request_repaint();
        });
        self.calculation = Some(CalculationJob {
            id,
            request,
            receiver,
            cancel_flag,
        });
        self.status = "Calculating".to_owned();
    }

    fn cancel_calculation(&mut self, status: &str) {
        if let Some(job) = self.calculation.take() {
            job.cancel_flag.store(true, Ordering::Relaxed);
            self.status = status.to_owned();
        }
    }

    fn poll_calculation(&mut self) {
        let Some(job) = self.calculation.take() else {
            return;
        };
        match job.receiver.try_recv() {
            Ok(message) => self.apply_calculation_message(job, message),
            Err(TryRecvError::Empty) => {
                self.calculation = Some(job);
            }
            Err(TryRecvError::Disconnected) => {
                self.status = "Calculation worker stopped".to_owned();
            }
        }
    }

    fn apply_calculation_message(&mut self, job: CalculationJob, message: CalculationMessage) {
        if message.id != job.id || message.canceled {
            self.status = "Stale calculation ignored".to_owned();
            return;
        }
        let current_request = self.batch_request(true);
        if calculation_request_is_stale(&current_request, &job.request) {
            self.status = "Stale calculation ignored".to_owned();
            return;
        }
        match message.result {
            Ok(output) => {
                self.apply_batch_output(output);
                self.save_preferences();
            }
            Err(err) => {
                self.status = "Generation failed".to_owned();
                self.warnings = vec![err];
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
            Ok(path) => {
                self.status = format!("{label} exported: {}", path.display());
                self.save_preferences();
            }
            Err(err) => {
                self.status = format!("{label} export failed");
                self.warnings.push(err);
            }
        }
    }

    fn open_browser(&mut self, target: FileBrowserTarget) {
        let start_dir = browser_start_dir(
            target,
            self.browser_value(target),
            path_from_text(&self.default_dir_path),
        );
        self.browser = Some(FileBrowser::new(target, start_dir));
    }

    fn browser_value(&self, target: FileBrowserTarget) -> &str {
        match target {
            FileBrowserTarget::Settings => &self.settings_path,
            FileBrowserTarget::Input => &self.input_path,
            FileBrowserTarget::DefaultDir => &self.default_dir_path,
            FileBrowserTarget::GcodeOutput => &self.gcode_path,
            FileBrowserTarget::SvgOutput => &self.svg_path,
            FileBrowserTarget::DxfOutput => &self.dxf_path,
        }
    }

    fn apply_browser_selection(&mut self, target: FileBrowserTarget, path: PathBuf) {
        let text = path.display().to_string();
        match target {
            FileBrowserTarget::Settings => self.settings_path = text,
            FileBrowserTarget::Input => self.input_path = text,
            FileBrowserTarget::DefaultDir => self.default_dir_path = text,
            FileBrowserTarget::GcodeOutput => self.gcode_path = text,
            FileBrowserTarget::SvgOutput => self.svg_path = text,
            FileBrowserTarget::DxfOutput => self.dxf_path = text,
        }
        self.status = format!("Selected {}", target.label());
        self.save_preferences();
    }

    fn save_preferences(&mut self) {
        let Some(path) = &self.preferences_path else {
            return;
        };
        let preferences = UiPreferences {
            settings_path: self.settings_path.clone(),
            input_path: self.input_path.clone(),
            default_dir_path: self.default_dir_path.clone(),
            gcode_path: self.gcode_path.clone(),
            svg_path: self.svg_path.clone(),
            dxf_path: self.dxf_path.clone(),
        };
        if let Err(err) = preferences.save(path) {
            self.warnings
                .push(format!("unable to save UI preferences: {err}"));
        }
    }
}

impl eframe::App for RengraveApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_calculation();
        egui::Panel::top("toolbar")
            .exact_size(42.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Load").clicked() {
                        self.reload_document(ui.ctx().clone());
                    }
                    if ui.button("Calculate").clicked() {
                        self.start_calculation(ui.ctx().clone());
                    }
                    if ui.button("Fit").clicked() {
                        self.fit_preview();
                    }
                    if self.calculation.is_some() {
                        ui.spinner();
                        ui.label("Calculating");
                        if self.active_calculation_is_stale() {
                            ui.colored_label(
                                egui::Color32::from_rgb(225, 176, 84),
                                "Input changed",
                            );
                        }
                        if ui.button("Cancel").clicked() {
                            self.cancel_calculation("Calculation canceled");
                        }
                    }
                    ui.separator();
                    ui.add(
                        egui::Slider::new(&mut self.transform.zoom, 10.0..=300.0)
                            .text("Zoom")
                            .clamping(egui::SliderClamping::Always),
                    );
                    ui.add(
                        egui::Slider::new(
                            &mut self.transform.viewport_rotation_degrees,
                            -180.0..=180.0,
                        )
                        .text("View")
                        .clamping(egui::SliderClamping::Always),
                    );
                    ui.separator();
                    ui.label("Status");
                    ui.monospace(&self.status);
                });
            });

        egui::Panel::left("input_settings")
            .exact_size(340.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Input");
                    if path_row(ui, "Settings", &mut self.settings_path) {
                        self.open_browser(FileBrowserTarget::Settings);
                    }
                    if path_row(ui, "Input", &mut self.input_path) {
                        self.open_browser(FileBrowserTarget::Input);
                    }
                    if path_row(ui, "Default dir", &mut self.default_dir_path) {
                        self.open_browser(FileBrowserTarget::DefaultDir);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Load").clicked() {
                            self.reload_document(ui.ctx().clone());
                        }
                        if ui.button("Calculate").clicked() {
                            self.start_calculation(ui.ctx().clone());
                        }
                    });
                    ui.label("Text");
                    ui.add_sized(
                        [ui.available_width(), 120.0],
                        egui::TextEdit::multiline(&mut self.text),
                    );

                    ui.separator();
                    ui.heading("Layout");
                    combo_row(ui, "Mode", self.controls.cut_type.label(), |ui| {
                        ui.selectable_value(
                            &mut self.controls.cut_type,
                            CutTypeChoice::Engrave,
                            CutTypeChoice::Engrave.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.cut_type,
                            CutTypeChoice::VCarve,
                            CutTypeChoice::VCarve.label(),
                        );
                    });
                    combo_row(ui, "Units", self.controls.units.label(), |ui| {
                        ui.selectable_value(
                            &mut self.controls.units,
                            UnitsChoice::Inch,
                            UnitsChoice::Inch.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.units,
                            UnitsChoice::Mm,
                            UnitsChoice::Mm.label(),
                        );
                    });
                    combo_row(ui, "Justify", self.controls.justify.label(), |ui| {
                        for value in JustifyChoice::ALL {
                            ui.selectable_value(&mut self.controls.justify, value, value.label());
                        }
                    });
                    combo_row(ui, "Origin", self.controls.origin.label(), |ui| {
                        for value in OriginChoice::ALL {
                            ui.selectable_value(&mut self.controls.origin, value, value.label());
                        }
                    });
                    number_row(ui, "Height", &mut self.controls.yscale, 0.05);
                    number_row(ui, "Width %", &mut self.controls.xscale_percent, 1.0);
                    number_row(ui, "Line space", &mut self.controls.line_space, 0.05);
                    number_row(
                        ui,
                        "Character space %",
                        &mut self.controls.char_space_percent,
                        1.0,
                    );
                    number_row(
                        ui,
                        "Word space %",
                        &mut self.controls.word_space_percent,
                        1.0,
                    );
                    number_row(ui, "Text angle", &mut self.controls.angle_degrees, 1.0);
                    number_row(ui, "Text radius", &mut self.controls.text_radius, 0.05);
                    number_row(ui, "X origin", &mut self.controls.xorigin, 0.01);
                    number_row(ui, "Y origin", &mut self.controls.yorigin, 0.01);
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.controls.flip, "Flip");
                        ui.checkbox(&mut self.controls.mirror, "Mirror");
                        ui.checkbox(&mut self.controls.outer, "Outer");
                        ui.checkbox(&mut self.controls.upper, "Upper");
                        ui.checkbox(&mut self.controls.plotbox, "Box");
                    });
                    number_row(ui, "Box gap", &mut self.controls.boxgap, 0.01);
                    ui.label(format!("Legacy keys: {}", self.settings_count));
                });
            });

        egui::Panel::right("output_tools")
            .exact_size(310.0)
            .resizable(false)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Tool");
                    combo_row(ui, "Bit", self.controls.bit_shape.label(), |ui| {
                        ui.selectable_value(
                            &mut self.controls.bit_shape,
                            BitShapeChoice::VBit,
                            BitShapeChoice::VBit.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.bit_shape,
                            BitShapeChoice::Ball,
                            BitShapeChoice::Ball.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.bit_shape,
                            BitShapeChoice::Flat,
                            BitShapeChoice::Flat.label(),
                        );
                    });
                    combo_row(ui, "Arc fit", self.controls.arc_fit.label(), |ui| {
                        ui.selectable_value(
                            &mut self.controls.arc_fit,
                            ArcFitChoice::NoFit,
                            ArcFitChoice::NoFit.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.arc_fit,
                            ArcFitChoice::Center,
                            ArcFitChoice::Center.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.arc_fit,
                            ArcFitChoice::Radius,
                            ArcFitChoice::Radius.label(),
                        );
                    });
                    number_row(ui, "Safe Z", &mut self.controls.safe_z, 0.01);
                    number_row(ui, "Cut Z", &mut self.controls.depth_z, 0.001);
                    number_row(ui, "Stroke", &mut self.controls.stroke_thickness, 0.001);
                    number_row(ui, "Feed", &mut self.controls.feed, 0.5);
                    number_row(ui, "Plunge", &mut self.controls.plunge, 0.5);
                    number_row(ui, "Accuracy", &mut self.controls.accuracy, 0.0005);
                    number_row(ui, "Arc segments", &mut self.controls.segarc, 0.5);
                    number_row(ui, "V angle", &mut self.controls.v_bit_angle, 1.0);
                    number_row(ui, "V diameter", &mut self.controls.v_bit_dia, 0.01);
                    number_row(ui, "V step", &mut self.controls.v_step_len, 0.001);
                    number_row(ui, "Allowance", &mut self.controls.allowance, 0.001);
                    number_row(ui, "Max cut", &mut self.controls.v_max_cut, 0.01);
                    number_row(ui, "Rough stock", &mut self.controls.v_rough_stk, 0.01);
                    number_row(ui, "Depth limit", &mut self.controls.v_depth_lim, 0.01);
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.controls.inlay, "Inlay");
                        ui.checkbox(&mut self.controls.use_image_size, "Image size");
                        ui.checkbox(&mut self.controls.bmp_long, "Bitmap long");
                    });

                    ui.separator();
                    ui.heading("Cleanup");
                    number_row(ui, "Clean dia", &mut self.controls.clean_dia, 0.01);
                    number_row(ui, "Clean step %", &mut self.controls.clean_step, 1.0);
                    number_row(ui, "Clean V", &mut self.controls.clean_v, 0.01);

                    ui.separator();
                    ui.heading("Output");
                    if path_row(ui, "G-code", &mut self.gcode_path) {
                        self.open_browser(FileBrowserTarget::GcodeOutput);
                    }
                    if ui
                        .add_enabled(!self.gcode.is_empty(), egui::Button::new("Export G-code"))
                        .clicked()
                    {
                        self.export_current(ExportKind::Gcode);
                    }
                    if path_row(ui, "SVG", &mut self.svg_path) {
                        self.open_browser(FileBrowserTarget::SvgOutput);
                    }
                    if ui
                        .add_enabled(self.svg.is_some(), egui::Button::new("Export SVG"))
                        .clicked()
                    {
                        self.export_current(ExportKind::Svg);
                    }
                    if path_row(ui, "DXF", &mut self.dxf_path) {
                        self.open_browser(FileBrowserTarget::DxfOutput);
                    }
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
                    ui.heading("Preview");
                    ui.checkbox(&mut self.show_toolpath, "Toolpath");
                    ui.checkbox(&mut self.show_bounds, "Bounds");
                    ui.checkbox(&mut self.show_v_area, "V-carve area");
                    ui.label(format!("G-code lines: {}", self.gcode_lines));
                    ui.label(format!("Preview moves: {}", self.preview_segments.len()));
                });
            });

        egui::Panel::bottom("status_log")
            .exact_size(96.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Status:");
                    ui.monospace(&self.status);
                    ui.separator();
                    ui.monospace(format!(
                        "{} lines, {} preview moves",
                        self.gcode_lines,
                        self.preview_segments.len()
                    ));
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for warning in &self.warnings {
                        ui.colored_label(egui::Color32::from_rgb(225, 176, 84), warning);
                    }
                });
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

        self.show_browser(ui.ctx());
    }
}

impl RengraveApp {
    fn fit_preview(&mut self) {
        self.transform.pan = Point::default();
        self.transform.zoom = 80.0;
    }

    fn show_browser(&mut self, ctx: &egui::Context) {
        let Some(mut browser) = self.browser.take() else {
            return;
        };
        let mut action = BrowserAction::Keep;
        egui::Window::new(format!("Browse {}", browser.target.label()))
            .collapsible(false)
            .resizable(true)
            .default_size([640.0, 440.0])
            .show(ctx, |ui| {
                action = browser.ui(ui);
            });

        match action {
            BrowserAction::Keep => self.browser = Some(browser),
            BrowserAction::Close => {}
            BrowserAction::Select(path) => {
                self.apply_browser_selection(browser.target, path);
            }
        }
    }

    fn active_calculation_is_stale(&self) -> bool {
        self.calculation
            .as_ref()
            .map(|job| calculation_request_is_stale(&self.batch_request(true), &job.request))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct UiControls {
    cut_type: CutTypeChoice,
    units: UnitsChoice,
    bit_shape: BitShapeChoice,
    arc_fit: ArcFitChoice,
    origin: OriginChoice,
    justify: JustifyChoice,
    yscale: f64,
    xscale_percent: f64,
    line_space: f64,
    char_space_percent: f64,
    word_space_percent: f64,
    angle_degrees: f64,
    text_radius: f64,
    safe_z: f64,
    depth_z: f64,
    stroke_thickness: f64,
    xorigin: f64,
    yorigin: f64,
    segarc: f64,
    accuracy: f64,
    feed: f64,
    plunge: f64,
    boxgap: f64,
    v_bit_angle: f64,
    v_bit_dia: f64,
    v_step_len: f64,
    allowance: f64,
    v_max_cut: f64,
    v_rough_stk: f64,
    v_depth_lim: f64,
    clean_dia: f64,
    clean_step: f64,
    clean_v: f64,
    flip: bool,
    mirror: bool,
    outer: bool,
    upper: bool,
    plotbox: bool,
    use_image_size: bool,
    inlay: bool,
    bmp_long: bool,
}

impl UiControls {
    fn from_settings(settings: &LegacySettings) -> Self {
        Self {
            cut_type: CutTypeChoice::parse(settings.get_last("cut_type").unwrap_or("engrave")),
            units: UnitsChoice::parse(settings.get_last("units").unwrap_or("in")),
            bit_shape: BitShapeChoice::parse(settings.get_last("bit_shape").unwrap_or("VBIT")),
            arc_fit: ArcFitChoice::parse(settings.get_last("arc_fit").unwrap_or("none")),
            origin: OriginChoice::parse(settings.get_last("origin").unwrap_or("Default")),
            justify: JustifyChoice::parse(settings.get_last("justify").unwrap_or("Left")),
            yscale: setting_f64(settings, "YSCALE", 2.0),
            xscale_percent: setting_f64(settings, "XSCALE", 100.0),
            line_space: setting_f64(settings, "LSPACE", 1.1),
            char_space_percent: setting_f64(settings, "CSPACE", 25.0),
            word_space_percent: setting_f64(settings, "WSPACE", 100.0),
            angle_degrees: setting_f64(settings, "TANGLE", 0.0),
            text_radius: setting_f64(settings, "TRADIUS", 0.0),
            safe_z: setting_f64(settings, "ZSAFE", 0.25),
            depth_z: setting_f64(settings, "ZCUT", -0.005),
            stroke_thickness: setting_f64(settings, "STHICK", 0.01),
            xorigin: setting_f64(settings, "xorigin", 0.0),
            yorigin: setting_f64(settings, "yorigin", 0.0),
            segarc: setting_f64(settings, "segarc", 5.0),
            accuracy: setting_f64(settings, "accuracy", 0.001),
            feed: setting_f64(settings, "FEED", 5.0),
            plunge: setting_f64(settings, "PLUNGE", 0.0),
            boxgap: setting_f64(settings, "boxgap", 0.25),
            v_bit_angle: setting_f64(settings, "v_bit_angle", 60.0),
            v_bit_dia: setting_f64(settings, "v_bit_dia", 0.5),
            v_step_len: setting_f64(settings, "v_step_len", 0.01),
            allowance: setting_f64(settings, "allowance", 0.0),
            v_max_cut: setting_f64(settings, "v_max_cut", -1.0),
            v_rough_stk: setting_f64(settings, "v_rough_stk", 0.0),
            v_depth_lim: setting_f64(settings, "v_depth_lim", 0.0),
            clean_dia: setting_f64(settings, "clean_dia", 0.25),
            clean_step: setting_f64(settings, "clean_step", 50.0),
            clean_v: setting_f64(settings, "clean_v", 0.05),
            flip: get_legacy_bool(settings, "flip", false),
            mirror: get_legacy_bool(settings, "mirror", false),
            outer: get_legacy_bool(settings, "outer", true),
            upper: get_legacy_bool(settings, "upper", true),
            plotbox: get_legacy_bool(settings, "plotbox", false),
            use_image_size: get_legacy_bool(settings, "useIMGsize", false),
            inlay: get_legacy_bool(settings, "inlay", false),
            bmp_long: get_legacy_bool(settings, "bmp_long", true),
        }
    }

    fn overrides(&self) -> Vec<LegacySetting> {
        let mut entries = Vec::new();
        push_setting(&mut entries, "cut_type", self.cut_type.value(), false);
        push_setting(&mut entries, "units", self.units.value(), false);
        push_setting(&mut entries, "bit_shape", self.bit_shape.value(), false);
        push_setting(&mut entries, "arc_fit", self.arc_fit.value(), false);
        push_setting(&mut entries, "origin", self.origin.value(), false);
        push_setting(&mut entries, "justify", self.justify.value(), false);
        push_setting(
            &mut entries,
            "YSCALE",
            format_setting_number(self.yscale),
            false,
        );
        push_setting(
            &mut entries,
            "XSCALE",
            format_setting_number(self.xscale_percent),
            false,
        );
        push_setting(
            &mut entries,
            "LSPACE",
            format_setting_number(self.line_space),
            false,
        );
        push_setting(
            &mut entries,
            "CSPACE",
            format_setting_number(self.char_space_percent),
            false,
        );
        push_setting(
            &mut entries,
            "WSPACE",
            format_setting_number(self.word_space_percent),
            false,
        );
        push_setting(
            &mut entries,
            "TANGLE",
            format_setting_number(self.angle_degrees),
            false,
        );
        push_setting(
            &mut entries,
            "TRADIUS",
            format_setting_number(self.text_radius),
            false,
        );
        push_setting(
            &mut entries,
            "ZSAFE",
            format_setting_number(self.safe_z),
            false,
        );
        push_setting(
            &mut entries,
            "ZCUT",
            format_setting_number(self.depth_z),
            false,
        );
        push_setting(
            &mut entries,
            "STHICK",
            format_setting_number(self.stroke_thickness),
            false,
        );
        push_setting(
            &mut entries,
            "xorigin",
            format_setting_number(self.xorigin),
            false,
        );
        push_setting(
            &mut entries,
            "yorigin",
            format_setting_number(self.yorigin),
            false,
        );
        push_setting(
            &mut entries,
            "segarc",
            format_setting_number(self.segarc),
            false,
        );
        push_setting(
            &mut entries,
            "accuracy",
            format_setting_number(self.accuracy),
            false,
        );
        push_setting(
            &mut entries,
            "FEED",
            format_setting_number(self.feed),
            false,
        );
        push_setting(
            &mut entries,
            "PLUNGE",
            format_setting_number(self.plunge),
            false,
        );
        push_setting(
            &mut entries,
            "boxgap",
            format_setting_number(self.boxgap),
            false,
        );
        push_setting(
            &mut entries,
            "v_bit_angle",
            format_setting_number(self.v_bit_angle),
            false,
        );
        push_setting(
            &mut entries,
            "v_bit_dia",
            format_setting_number(self.v_bit_dia),
            false,
        );
        push_setting(
            &mut entries,
            "v_step_len",
            format_setting_number(self.v_step_len),
            false,
        );
        push_setting(
            &mut entries,
            "allowance",
            format_setting_number(self.allowance),
            false,
        );
        push_setting(
            &mut entries,
            "v_max_cut",
            format_setting_number(self.v_max_cut),
            false,
        );
        push_setting(
            &mut entries,
            "v_rough_stk",
            format_setting_number(self.v_rough_stk),
            false,
        );
        push_setting(
            &mut entries,
            "v_depth_lim",
            format_setting_number(self.v_depth_lim),
            false,
        );
        push_setting(
            &mut entries,
            "clean_dia",
            format_setting_number(self.clean_dia),
            false,
        );
        push_setting(
            &mut entries,
            "clean_step",
            format_setting_number(self.clean_step),
            false,
        );
        push_setting(
            &mut entries,
            "clean_v",
            format_setting_number(self.clean_v),
            false,
        );
        push_bool(&mut entries, "flip", self.flip);
        push_bool(&mut entries, "mirror", self.mirror);
        push_bool(&mut entries, "outer", self.outer);
        push_bool(&mut entries, "upper", self.upper);
        push_bool(&mut entries, "plotbox", self.plotbox);
        push_bool(&mut entries, "useIMGsize", self.use_image_size);
        push_bool(&mut entries, "inlay", self.inlay);
        push_bool(&mut entries, "bmp_long", self.bmp_long);
        entries
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CutTypeChoice {
    Engrave,
    VCarve,
}

impl CutTypeChoice {
    fn parse(value: &str) -> Self {
        if value == "v-carve" {
            Self::VCarve
        } else {
            Self::Engrave
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Engrave => "engrave",
            Self::VCarve => "v-carve",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Engrave => "Engrave",
            Self::VCarve => "V-carve",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitsChoice {
    Inch,
    Mm,
}

impl UnitsChoice {
    fn parse(value: &str) -> Self {
        if value == "mm" { Self::Mm } else { Self::Inch }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Inch => "in",
            Self::Mm => "mm",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Inch => "Inch",
            Self::Mm => "mm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitShapeChoice {
    VBit,
    Ball,
    Flat,
}

impl BitShapeChoice {
    fn parse(value: &str) -> Self {
        match value {
            "BALL" => Self::Ball,
            "FLAT" => Self::Flat,
            _ => Self::VBit,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::VBit => "VBIT",
            Self::Ball => "BALL",
            Self::Flat => "FLAT",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::VBit => "V-bit",
            Self::Ball => "Ball",
            Self::Flat => "Flat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArcFitChoice {
    NoFit,
    Center,
    Radius,
}

impl ArcFitChoice {
    fn parse(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            "radius" => Self::Radius,
            _ => Self::NoFit,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::NoFit => "none",
            Self::Center => "center",
            Self::Radius => "radius",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::NoFit => "None",
            Self::Center => "Center",
            Self::Radius => "Radius",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JustifyChoice {
    Left,
    Center,
    Right,
}

impl JustifyChoice {
    const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    fn parse(value: &str) -> Self {
        match value {
            "Center" => Self::Center,
            "Right" => Self::Right,
            _ => Self::Left,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }

    fn label(self) -> &'static str {
        self.value()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginChoice {
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

impl OriginChoice {
    const ALL: [Self; 11] = [
        Self::Default,
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::MidLeft,
        Self::MidCenter,
        Self::MidRight,
        Self::BotLeft,
        Self::BotCenter,
        Self::BotRight,
        Self::ArcCenter,
    ];

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

    fn value(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::TopLeft => "Top-Left",
            Self::TopCenter => "Top-Center",
            Self::TopRight => "Top-Right",
            Self::MidLeft => "Mid-Left",
            Self::MidCenter => "Mid-Center",
            Self::MidRight => "Mid-Right",
            Self::BotLeft => "Bot-Left",
            Self::BotCenter => "Bot-Center",
            Self::BotRight => "Bot-Right",
            Self::ArcCenter => "Arc-Center",
        }
    }

    fn label(self) -> &'static str {
        self.value()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileBrowserTarget {
    Settings,
    Input,
    DefaultDir,
    GcodeOutput,
    SvgOutput,
    DxfOutput,
}

impl FileBrowserTarget {
    fn label(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Input => "input",
            Self::DefaultDir => "default directory",
            Self::GcodeOutput => "G-code output",
            Self::SvgOutput => "SVG output",
            Self::DxfOutput => "DXF output",
        }
    }

    fn default_file_name(self) -> Option<&'static str> {
        match self {
            Self::GcodeOutput => Some("rengrave_output.ngc"),
            Self::SvgOutput => Some("rengrave_output.svg"),
            Self::DxfOutput => Some("rengrave_output.dxf"),
            _ => None,
        }
    }

    fn can_select(self, path: &Path) -> bool {
        match self {
            Self::DefaultDir => path.is_dir(),
            Self::Settings | Self::Input => path.is_file(),
            Self::GcodeOutput | Self::SvgOutput | Self::DxfOutput => !path.is_dir(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserEntry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileBrowser {
    target: FileBrowserTarget,
    current_dir: PathBuf,
    selected_path: Option<PathBuf>,
    entries: Vec<BrowserEntry>,
    error: Option<String>,
}

impl FileBrowser {
    fn new(target: FileBrowserTarget, current_dir: PathBuf) -> Self {
        let mut browser = Self {
            target,
            current_dir,
            selected_path: None,
            entries: Vec::new(),
            error: None,
        };
        browser.refresh();
        browser
    }

    fn refresh(&mut self) {
        match read_browser_entries(&self.current_dir) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(err) => {
                self.entries.clear();
                self.error = Some(err);
            }
        }
    }

    fn set_dir(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.current_dir = path;
            self.selected_path = None;
            self.refresh();
        } else {
            self.error = Some(format!("not a directory: {}", path.display()));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) -> BrowserAction {
        let mut action = BrowserAction::Keep;
        ui.horizontal(|ui| {
            if ui.button("Parent").clicked() {
                if let Some(parent) = self.current_dir.parent() {
                    self.set_dir(parent.to_path_buf());
                }
            }
            if ui.button("Home").clicked() {
                if let Some(home) = user_home_dir() {
                    self.set_dir(home);
                }
            }
            if ui.button("Refresh").clicked() {
                self.refresh();
            }
            ui.monospace(self.current_dir.display().to_string());
        });

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(225, 176, 84), error);
        }

        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .show(ui, |ui| {
                for entry in self.entries.clone() {
                    let selected = self.selected_path.as_ref() == Some(&entry.path);
                    let label = if entry.is_dir {
                        format!("[DIR] {}", entry.name)
                    } else {
                        entry.name.clone()
                    };
                    let response = ui.selectable_label(selected, label);
                    if response.clicked() {
                        self.selected_path = Some(entry.path.clone());
                    }
                    if response.double_clicked() && entry.is_dir {
                        self.set_dir(entry.path);
                    }
                }
            });

        ui.separator();
        ui.horizontal(|ui| {
            let selected = self.selected_path.clone();
            let can_use_selected = selected
                .as_deref()
                .map(|path| self.target.can_select(path))
                .unwrap_or(false);
            if ui
                .add_enabled(can_use_selected, egui::Button::new("Use selected"))
                .clicked()
            {
                if let Some(path) = selected {
                    action = BrowserAction::Select(path);
                }
            }

            if let Some(file_name) = self.target.default_file_name() {
                if ui.button("Use current dir").clicked() {
                    action = BrowserAction::Select(self.current_dir.join(file_name));
                }
            } else if self.target == FileBrowserTarget::DefaultDir
                && ui.button("Use current dir").clicked()
            {
                action = BrowserAction::Select(self.current_dir.clone());
            }

            if ui.button("Cancel").clicked() {
                action = BrowserAction::Close;
            }
        });

        action
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserAction {
    Keep,
    Close,
    Select(PathBuf),
}

struct CalculationJob {
    id: u64,
    request: BatchRequest,
    receiver: Receiver<CalculationMessage>,
    cancel_flag: Arc<AtomicBool>,
}

struct CalculationMessage {
    id: u64,
    result: Result<BatchOutput, String>,
    canceled: bool,
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

fn calculation_request_is_stale(current: &BatchRequest, expected: &BatchRequest) -> bool {
    current.batch != expected.batch
        || current.gcode_file != expected.gcode_file
        || current.font_or_image != expected.font_or_image
        || current.default_dir != expected.default_dir
        || current.text != expected.text
        || current.settings_overrides != expected.settings_overrides
        || current.svg_output.is_some() != expected.svg_output.is_some()
        || current.dxf_output.is_some() != expected.dxf_output.is_some()
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

fn path_to_text(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

fn path_from_text(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    let mut browse_clicked = false;
    ui.horizontal(|ui| {
        ui.add_sized([88.0, 20.0], egui::Label::new(label));
        let text_width = (ui.available_width() - 74.0).max(80.0);
        ui.add_sized([text_width, 22.0], egui::TextEdit::singleline(value));
        browse_clicked = ui.button("Browse").clicked();
    });
    browse_clicked
}

fn number_row(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        ui.add_sized([124.0, 20.0], egui::Label::new(label));
        ui.add(egui::DragValue::new(value).speed(speed).max_decimals(4));
    });
}

fn combo_row(
    ui: &mut egui::Ui,
    label: &str,
    selected_text: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.add_sized([124.0, 20.0], egui::Label::new(label));
        egui::ComboBox::from_id_salt(label)
            .selected_text(selected_text)
            .show_ui(ui, body);
    });
}

fn setting_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn format_setting_number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }

    let value = if value.abs() < 0.0000005 { 0.0 } else { value };
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn push_setting(
    entries: &mut Vec<LegacySetting>,
    key: &'static str,
    value: impl Into<String>,
    quoted: bool,
) {
    entries.push(LegacySetting::new(key, value, quoted));
}

fn push_bool(entries: &mut Vec<LegacySetting>, key: &'static str, value: bool) {
    push_setting(entries, key, if value { "1" } else { "0" }, false);
}

fn browser_start_dir(
    target: FileBrowserTarget,
    current_value: &str,
    default_dir: Option<PathBuf>,
) -> PathBuf {
    if let Some(path) = path_from_text(current_value) {
        if target == FileBrowserTarget::DefaultDir && path.is_dir() {
            return path;
        }
        if path.is_dir() {
            return path;
        }
        if let Some(parent) = non_empty_parent(&path) {
            return parent.to_path_buf();
        }
    }

    if let Some(default_dir) = default_dir {
        if default_dir.is_dir() {
            return default_dir;
        }
        if let Some(parent) = non_empty_parent(&default_dir) {
            return parent.to_path_buf();
        }
    }

    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

fn read_browser_entries(dir: &Path) -> Result<Vec<BrowserEntry>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| format!("unable to read directory: {err}"))? {
        let entry = entry.map_err(|err| format!("unable to read directory entry: {err}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("unable to read file type: {err}"))?;
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        entries.push(BrowserEntry {
            path,
            name,
            is_dir: file_type.is_dir(),
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UiPreferences {
    settings_path: String,
    input_path: String,
    default_dir_path: String,
    gcode_path: String,
    svg_path: String,
    dxf_path: String,
}

impl UiPreferences {
    fn load(path: &Path) -> Result<Self, String> {
        let input = fs::read_to_string(path)
            .map_err(|err| format!("unable to read `{}`: {err}", path.display()))?;
        Ok(Self::parse(&input))
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("unable to create `{}`: {err}", parent.display()))?;
        }
        fs::write(path, self.to_text())
            .map_err(|err| format!("unable to write `{}`: {err}", path.display()))
    }

    fn parse(input: &str) -> Self {
        let mut preferences = Self::default();
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = unescape_pref_value(value);
            match key {
                "settings_path" => preferences.settings_path = value,
                "input_path" => preferences.input_path = value,
                "default_dir_path" => preferences.default_dir_path = value,
                "gcode_path" => preferences.gcode_path = value,
                "svg_path" => preferences.svg_path = value,
                "dxf_path" => preferences.dxf_path = value,
                _ => {}
            }
        }
        preferences
    }

    fn to_text(&self) -> String {
        [
            ("settings_path", self.settings_path.as_str()),
            ("input_path", self.input_path.as_str()),
            ("default_dir_path", self.default_dir_path.as_str()),
            ("gcode_path", self.gcode_path.as_str()),
            ("svg_path", self.svg_path.as_str()),
            ("dxf_path", self.dxf_path.as_str()),
        ]
        .into_iter()
        .map(|(key, value)| format!("{key}={}", escape_pref_value(value)))
        .collect::<Vec<_>>()
        .join("\n")
            + "\n"
    }
}

fn default_preferences_path() -> Option<PathBuf> {
    config_base_dir().map(|dir| dir.join("rengrave").join("ui-state.conf"))
}

fn config_base_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        user_home_dir().map(|home| home.join("Library").join("Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| user_home_dir().map(|home| home.join(".config")))
    }
}

fn user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn escape_pref_value(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn unescape_pref_value(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => output.push('\\'),
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
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
    fn calculation_staleness_ignores_output_path_text() {
        let expected = BatchRequest {
            batch: true,
            text: Some("A".to_owned()),
            svg_output: Some(PathBuf::from("/tmp/old.svg")),
            dxf_output: Some(PathBuf::from("/tmp/old.dxf")),
            ..BatchRequest::default()
        };
        let current = BatchRequest {
            svg_output: Some(PathBuf::from("/tmp/new.svg")),
            dxf_output: Some(PathBuf::from("/tmp/new.dxf")),
            ..expected.clone()
        };

        assert!(!calculation_request_is_stale(&current, &expected));
    }

    #[test]
    fn calculation_staleness_detects_generation_inputs() {
        let expected = BatchRequest {
            batch: true,
            text: Some("A".to_owned()),
            settings_overrides: vec![LegacySetting::new("YSCALE", "2", false)],
            ..BatchRequest::default()
        };
        let text_changed = BatchRequest {
            text: Some("B".to_owned()),
            ..expected.clone()
        };
        let settings_changed = BatchRequest {
            settings_overrides: vec![LegacySetting::new("YSCALE", "3", false)],
            ..expected.clone()
        };
        let export_toggle_changed = BatchRequest {
            svg_output: Some(PathBuf::from("/tmp/out.svg")),
            ..expected.clone()
        };

        assert!(calculation_request_is_stale(&text_changed, &expected));
        assert!(calculation_request_is_stale(&settings_changed, &expected));
        assert!(calculation_request_is_stale(
            &export_toggle_changed,
            &expected
        ));
    }

    #[test]
    fn path_from_text_trims_empty_paths() {
        assert_eq!(path_from_text("  "), None);
        assert_eq!(
            path_from_text("  /tmp/rengrave.ngc  "),
            Some(PathBuf::from("/tmp/rengrave.ngc"))
        );
        assert_eq!(
            path_to_text(&Some(PathBuf::from("/tmp/rengrave.ngc"))),
            "/tmp/rengrave.ngc"
        );
    }

    #[test]
    fn browser_start_dir_prefers_current_directory_or_parent() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-browser-start-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.ngc");
        fs::write(&file, "G90").unwrap();

        assert_eq!(
            browser_start_dir(
                FileBrowserTarget::DefaultDir,
                &dir.display().to_string(),
                None
            ),
            dir
        );
        assert_eq!(
            browser_start_dir(
                FileBrowserTarget::Settings,
                &file.display().to_string(),
                None
            ),
            file.parent().unwrap()
        );

        let _ = fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn browser_entries_sort_directories_before_files() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-ui-browser-entries-{}",
            std::process::id()
        ));
        fs::create_dir_all(dir.join("z_dir")).unwrap();
        fs::write(dir.join("a_file.ngc"), "G90").unwrap();

        let entries = read_browser_entries(&dir).unwrap();

        let _ = fs::remove_dir_all(&dir);
        assert_eq!(entries[0].name, "z_dir");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "a_file.ngc");
        assert!(!entries[1].is_dir);
    }

    #[test]
    fn ui_preferences_round_trip_escaped_values() {
        let preferences = UiPreferences {
            settings_path: "/tmp/settings=a.ngc".to_owned(),
            input_path: "/tmp/font\\romanc.cxf".to_owned(),
            default_dir_path: "/tmp/default".to_owned(),
            gcode_path: "/tmp/out.ngc".to_owned(),
            svg_path: "/tmp/out.svg".to_owned(),
            dxf_path: "/tmp/out.dxf".to_owned(),
        };

        let encoded = preferences.to_text();
        let parsed = UiPreferences::parse(&encoded);

        assert_eq!(parsed, preferences);
    }

    #[test]
    fn ui_preferences_save_and_load_file() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-preferences-{}", std::process::id()));
        let path = dir.join("ui-state.conf");
        let preferences = UiPreferences {
            input_path: "/tmp/example.cxf".to_owned(),
            ..UiPreferences::default()
        };

        preferences.save(&path).unwrap();
        let loaded = UiPreferences::load(&path).unwrap();

        let _ = fs::remove_dir_all(dir);
        assert_eq!(loaded, preferences);
    }

    #[test]
    fn ui_controls_emit_core_overrides() {
        let mut settings = LegacySettings::default();
        settings.set_or_push("cut_type", "engrave", false);
        settings.set_or_push("units", "in", false);
        settings.set_or_push("bit_shape", "VBIT", false);
        settings.set_or_push("arc_fit", "none", false);
        settings.set_or_push("origin", "Default", false);
        settings.set_or_push("justify", "Left", false);

        let mut controls = UiControls::from_settings(&settings);
        controls.cut_type = CutTypeChoice::VCarve;
        controls.units = UnitsChoice::Mm;
        controls.bit_shape = BitShapeChoice::Ball;
        controls.arc_fit = ArcFitChoice::Center;
        controls.origin = OriginChoice::BotLeft;
        controls.justify = JustifyChoice::Right;
        controls.yscale = 4.25;
        controls.plotbox = true;
        controls.mirror = true;

        let overrides = controls.overrides();
        let value_for = |key: &str| {
            overrides
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.as_str())
        };

        assert_eq!(value_for("cut_type"), Some("v-carve"));
        assert_eq!(value_for("units"), Some("mm"));
        assert_eq!(value_for("bit_shape"), Some("BALL"));
        assert_eq!(value_for("arc_fit"), Some("center"));
        assert_eq!(value_for("origin"), Some("Bot-Left"));
        assert_eq!(value_for("justify"), Some("Right"));
        assert_eq!(value_for("YSCALE"), Some("4.25"));
        assert_eq!(value_for("plotbox"), Some("1"));
        assert_eq!(value_for("mirror"), Some("1"));
    }

    #[test]
    fn write_text_file_reports_empty_paths() {
        let err = write_text_file("  ", "G90").unwrap_err();

        assert_eq!(err, "output path is empty");
    }
}
