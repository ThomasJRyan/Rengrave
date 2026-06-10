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
use rengrave_core::batch::{BatchOutput, BatchRequest, SecondaryGcode, prepare_batch_output};
use rengrave_core::dxf::read_dxf_font;
use rengrave_core::external::{PotraceStatus, detect_potrace, requires_potrace};
use rengrave_core::font::{Font, Stroke, read_cxf, read_ttf};
use rengrave_core::geometry::{Point, ViewTransform};
use rengrave_core::project::{DocumentRequest, RengraveDocument, load_document};
use rengrave_core::settings::{LegacySetting, LegacySettings, get_legacy_bool, legacy_bool_value};
use rfd::FileDialog;

const DEFAULT_PREVIEW_ZOOM: f64 = 80.0;
const PREVIEW_FIT_PADDING: f32 = 24.0;
const OUTPUT_PREVIEW_CHARS: usize = 8000;
const INPUT_PREVIEW_VECTOR_HEIGHT: f32 = 180.0;
const INPUT_PREVIEW_THUMBNAIL_WIDTH: u32 = 300;
const INPUT_PREVIEW_THUMBNAIL_HEIGHT: u32 = 180;

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
    secondary_gcode: Vec<SecondaryGcode>,
    gcode_lines: usize,
    preview_segments: Vec<PreviewSegment>,
    preview_rapids: Vec<PreviewSegment>,
    preview_bounds: Option<PreviewBounds>,
    gcode_path: String,
    svg_path: String,
    dxf_path: String,
    show_toolpath: bool,
    show_rapids: bool,
    show_bounds: bool,
    show_axes: bool,
    browser: Option<FileBrowser>,
    input_catalog: InputCatalog,
    input_preview: InputPreview,
    preferences_path: Option<PathBuf>,
    calculation: Option<CalculationJob>,
    next_calculation_id: u64,
    warnings: Vec<String>,
    potrace_status: PotraceStatus,
    fit_preview_requested: bool,
    last_output_request: Option<BatchRequest>,
    bottom_tab: BottomTab,
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
        let input_catalog =
            InputCatalog::scan(input_catalog_start_dir(&font_or_image, &default_dir));
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
                zoom: DEFAULT_PREVIEW_ZOOM,
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
            secondary_gcode: Vec::new(),
            gcode_lines: 0,
            preview_segments: Vec::new(),
            preview_rapids: Vec::new(),
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
            show_toolpath: get_legacy_bool(&document.settings, "show_v_path", true),
            show_rapids: true,
            show_bounds: get_legacy_bool(&document.settings, "show_box", true),
            show_axes: get_legacy_bool(&document.settings, "show_axis", true),
            browser: None,
            input_catalog,
            input_preview: InputPreview::default(),
            preferences_path,
            calculation: None,
            next_calculation_id: 1,
            warnings: document.warnings,
            potrace_status: detect_potrace(),
            fit_preview_requested: false,
            last_output_request: None,
            bottom_tab: BottomTab::Status,
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
            include_secondary: include_exports,
            settings_overrides: self.controls.overrides(),
        }
    }

    fn settings_request_for_save(&self) -> DocumentRequest {
        let mut settings_overrides = self.controls.overrides();
        append_view_setting_overrides(
            &mut settings_overrides,
            self.show_toolpath,
            self.show_bounds,
            self.show_axes,
        );

        DocumentRequest {
            gcode_file: settings_base_path_for_save(&self.settings_path),
            font_or_image: path_from_text(&self.input_path),
            default_dir: path_from_text(&self.default_dir_path),
            text: Some(self.text.clone()),
            settings_overrides,
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
                self.show_toolpath = get_legacy_bool(&document.settings, "show_v_path", true);
                self.show_bounds = get_legacy_bool(&document.settings, "show_box", true);
                self.show_axes = get_legacy_bool(&document.settings, "show_axis", true);
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

    fn save_current_settings(&mut self) {
        let Some(path) = path_from_text(&self.settings_path) else {
            self.status = "Settings path is empty".to_owned();
            return;
        };

        match settings_file_contents(&self.settings_request_for_save()) {
            Ok((contents, warnings)) => match write_text_file(&self.settings_path, &contents) {
                Ok(_) => {
                    self.status = format!("Settings saved: {}", path.display());
                    self.warnings = warnings;
                    self.save_preferences();
                }
                Err(err) => {
                    self.status = "Settings save failed".to_owned();
                    self.warnings.push(err);
                }
            },
            Err(err) => {
                self.status = "Settings save failed".to_owned();
                self.warnings.push(err);
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
                self.last_output_request = Some(job.request);
                self.save_preferences();
            }
            Err(err) => {
                self.status = "Generation failed".to_owned();
                self.warnings = vec![err];
                self.gcode.clear();
                self.svg = None;
                self.dxf = None;
                self.secondary_gcode.clear();
                self.gcode_lines = 0;
                self.preview_segments.clear();
                self.preview_rapids.clear();
                self.preview_bounds = None;
                self.last_output_request = None;
            }
        }
    }

    fn apply_batch_output(&mut self, output: BatchOutput) {
        self.gcode_lines = output.gcode.lines().count();
        let preview_motion = parse_preview_motion(&output.gcode);
        self.preview_segments = preview_motion.cuts;
        self.preview_rapids = preview_motion.rapids;
        self.preview_bounds =
            PreviewBounds::from_segment_layers(&self.preview_segments, &self.preview_rapids);
        if self.preview_bounds.is_some() {
            self.fit_preview_requested = true;
        }
        self.status = if self.preview_segments.is_empty() {
            "Settings loaded".to_owned()
        } else {
            format!("Generated {} lines", self.gcode_lines)
        };
        self.warnings = output.warnings;
        self.gcode = output.gcode;
        self.svg = output.svg;
        self.dxf = output.dxf;
        self.secondary_gcode = output.secondary_gcode;
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

    fn export_secondary_outputs(&mut self) {
        if self.secondary_gcode.is_empty() {
            self.status = "Cleanup export unavailable".to_owned();
            return;
        }

        let primary_path = PathBuf::from(self.gcode_path.trim());
        if primary_path.as_os_str().is_empty() {
            self.status = "Cleanup export failed".to_owned();
            self.warnings.push("G-code output path is empty".to_owned());
            return;
        }

        let mut written = 0usize;
        for output in &self.secondary_gcode {
            let path = secondary_output_path(&primary_path, &output.suffix);
            match fs::write(&path, &output.gcode) {
                Ok(_) => written += 1,
                Err(err) => {
                    self.status = "Cleanup export failed".to_owned();
                    self.warnings
                        .push(format!("unable to write `{}`: {err}", path.display()));
                    return;
                }
            }
        }

        self.status = format!("Cleanup exported: {written} files");
        self.save_preferences();
    }

    fn open_browser(&mut self, target: FileBrowserTarget) {
        let start_dir = browser_start_dir(
            target,
            self.browser_value(target),
            path_from_text(&self.default_dir_path),
        );
        self.browser = Some(FileBrowser::new(target, start_dir));
    }

    fn choose_path(&mut self, target: FileBrowserTarget) {
        if let Some(path) = choose_native_path(
            target,
            self.browser_value(target),
            path_from_text(&self.default_dir_path),
        ) {
            self.apply_browser_selection(target, path);
        } else {
            self.open_browser(target);
            self.status = "Using in-app browser".to_owned();
        }
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
            FileBrowserTarget::Input => {
                self.input_path = text;
                self.refresh_input_catalog();
            }
            FileBrowserTarget::DefaultDir => {
                self.default_dir_path = text;
                self.refresh_input_catalog();
            }
            FileBrowserTarget::GcodeOutput => self.gcode_path = text,
            FileBrowserTarget::SvgOutput => self.svg_path = text,
            FileBrowserTarget::DxfOutput => self.dxf_path = text,
        }
        self.status = format!("Selected {}", target.label());
        self.save_preferences();
    }

    fn refresh_input_catalog(&mut self) {
        let start_dir = input_catalog_start_dir(
            &path_from_text(&self.input_path),
            &path_from_text(&self.default_dir_path),
        );
        self.input_catalog = InputCatalog::scan(start_dir);
    }

    fn select_input_catalog_entry(&mut self, path: PathBuf, ctx: egui::Context) {
        self.input_path = path.display().to_string();
        self.status = "Input selected".to_owned();
        self.save_preferences();
        self.start_calculation(ctx);
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

    fn refresh_potrace_status(&mut self) {
        self.potrace_status = detect_potrace();
        self.status = if self.potrace_status.available {
            "Potrace detected".to_owned()
        } else {
            "Potrace missing".to_owned()
        };
    }
}

impl eframe::App for RengraveApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_calculation();
        egui::Panel::top("toolbar")
            .exact_size(68.0)
            .show_inside(ui, |ui| {
                self.show_menu_bar(ui);
                ui.horizontal(|ui| {
                    if ui.button("Load").clicked() {
                        self.reload_document(ui.ctx().clone());
                    }
                    if ui.button("Calculate").clicked() {
                        self.start_calculation(ui.ctx().clone());
                    }
                    if ui.button("Fit").clicked() {
                        self.fit_preview_requested = true;
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
                    } else if self.output_is_stale() {
                        ui.colored_label(egui::Color32::from_rgb(225, 176, 84), "Output stale");
                    }
                    ui.separator();
                    ui.add(
                        egui::Slider::new(&mut self.transform.zoom, 1.0..=500.0)
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
                        self.choose_path(FileBrowserTarget::Settings);
                    }
                    if path_row(ui, "Input", &mut self.input_path) {
                        self.choose_path(FileBrowserTarget::Input);
                    }
                    if path_row(ui, "Default dir", &mut self.default_dir_path) {
                        self.choose_path(FileBrowserTarget::DefaultDir);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Load").clicked() {
                            self.reload_document(ui.ctx().clone());
                        }
                        if ui
                            .add_enabled(
                                !self.settings_path.trim().is_empty(),
                                egui::Button::new("Save Settings"),
                            )
                            .clicked()
                        {
                            self.save_current_settings();
                        }
                        if ui.button("Calculate").clicked() {
                            self.start_calculation(ui.ctx().clone());
                        }
                    });
                    ui.separator();
                    ui.heading("Input Catalog");
                    ui.horizontal(|ui| {
                        if ui.button("Scan").clicked() {
                            self.refresh_input_catalog();
                        }
                        if let Some(dir) = &self.input_catalog.dir {
                            ui.monospace(dir.display().to_string());
                        }
                    });
                    if let Some(error) = &self.input_catalog.error {
                        ui.colored_label(egui::Color32::from_rgb(225, 176, 84), error);
                    }
                    if self.input_catalog.entries.is_empty() {
                        ui.label("No supported files found");
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(150.0)
                            .show(ui, |ui| {
                                for entry in self.input_catalog.entries.clone() {
                                    let selected = path_from_text(&self.input_path).as_ref()
                                        == Some(&entry.path);
                                    let label = format!(
                                        "{}  {}  {}",
                                        entry.kind.label(),
                                        entry.name,
                                        format_bytes(entry.size_bytes)
                                    );
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.select_input_catalog_entry(
                                            entry.path,
                                            ui.ctx().clone(),
                                        );
                                    }
                                }
                            });
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.heading("Input Preview");
                        if ui.button("Refresh").clicked() {
                            self.reload_input_preview();
                        }
                    });
                    self.ensure_input_preview();
                    draw_input_preview(ui, &mut self.input_preview);
                    ui.separator();
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
                    });

                    ui.separator();
                    ui.heading("Bitmap");
                    ui.horizontal_wrapped(|ui| {
                        let color = if self.potrace_status.available {
                            egui::Color32::from_rgb(94, 176, 132)
                        } else {
                            egui::Color32::from_rgb(225, 176, 84)
                        };
                        ui.colored_label(color, &self.potrace_status.message);
                        if ui.button("Refresh").clicked() {
                            self.refresh_potrace_status();
                        }
                    });
                    if input_path_requires_potrace(&self.input_path) {
                        if self.potrace_status.available {
                            ui.label("Selected bitmap input will be traced with Potrace");
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(225, 176, 84),
                                "Bitmap tracing needs Potrace in PATH",
                            );
                        }
                    } else {
                        ui.label("Select a bitmap input to trace through Potrace");
                    }
                    combo_row(
                        ui,
                        "Turn policy",
                        self.controls.bmp_turn_policy.label(),
                        |ui| {
                            for value in BitmapTurnPolicy::ALL {
                                ui.selectable_value(
                                    &mut self.controls.bmp_turn_policy,
                                    value,
                                    value.label(),
                                );
                            }
                        },
                    );
                    number_row(ui, "Turd size", &mut self.controls.bmp_turds, 1.0);
                    number_row(ui, "Alpha max", &mut self.controls.bmp_alpha, 0.05);
                    number_row(ui, "Opt tolerance", &mut self.controls.bmp_optto, 0.01);
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.controls.use_image_size, "Image size");
                        ui.checkbox(&mut self.controls.bmp_long, "Long curves");
                    });

                    ui.separator();
                    ui.heading("Cleanup");
                    number_row(ui, "Clean dia", &mut self.controls.clean_dia, 0.01);
                    number_row(ui, "Clean step %", &mut self.controls.clean_step, 1.0);
                    number_row(ui, "Clean V", &mut self.controls.clean_v, 0.01);
                    ui.label("Straight");
                    ui.horizontal_wrapped(|ui| {
                        clean_path_checkbox(ui, "Profile", &mut self.controls.clean_paths, 0);
                        clean_path_checkbox(ui, "X", &mut self.controls.clean_paths, 1);
                        clean_path_checkbox(ui, "Y", &mut self.controls.clean_paths, 2);
                        clean_path_checkbox(ui, "Loops", &mut self.controls.clean_paths, 6);
                    });
                    ui.label("V-bit");
                    ui.horizontal_wrapped(|ui| {
                        clean_path_checkbox(ui, "Profile", &mut self.controls.clean_paths, 3);
                        clean_path_checkbox(ui, "X", &mut self.controls.clean_paths, 5);
                        clean_path_checkbox(ui, "Y", &mut self.controls.clean_paths, 4);
                        clean_path_checkbox(ui, "Loops", &mut self.controls.clean_paths, 7);
                    });
                    ui.checkbox(&mut self.controls.v_flop, "Flip normals");

                    ui.separator();
                    ui.heading("Advanced");
                    combo_row(ui, "Height calc", self.controls.height_calc.label(), |ui| {
                        ui.selectable_value(
                            &mut self.controls.height_calc,
                            HeightCalcChoice::MaxUse,
                            HeightCalcChoice::MaxUse.label(),
                        );
                        ui.selectable_value(
                            &mut self.controls.height_calc,
                            HeightCalcChoice::MaxAll,
                            HeightCalcChoice::MaxAll.label(),
                        );
                    });
                    text_row(ui, "Preamble", &mut self.controls.gpre);
                    text_row(ui, "Postamble", &mut self.controls.gpost);
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut self.controls.recovery_comments, "Recovery comments");
                        ui.checkbox(&mut self.controls.var_dis, "Disable variables");
                        ui.checkbox(&mut self.controls.ext_char, "Extended chars");
                    });

                    ui.separator();
                    ui.heading("Output");
                    if path_row(ui, "G-code", &mut self.gcode_path) {
                        self.choose_path(FileBrowserTarget::GcodeOutput);
                    }
                    if ui
                        .add_enabled(!self.gcode.is_empty(), egui::Button::new("Export G-code"))
                        .clicked()
                    {
                        self.export_current(ExportKind::Gcode);
                    }
                    ui.label(format!("Cleanup files: {}", self.secondary_gcode.len()));
                    if ui
                        .add_enabled(
                            !self.secondary_gcode.is_empty(),
                            egui::Button::new("Export cleanup files"),
                        )
                        .clicked()
                    {
                        self.export_secondary_outputs();
                    }
                    if path_row(ui, "SVG", &mut self.svg_path) {
                        self.choose_path(FileBrowserTarget::SvgOutput);
                    }
                    if ui
                        .add_enabled(self.svg.is_some(), egui::Button::new("Export SVG"))
                        .clicked()
                    {
                        self.export_current(ExportKind::Svg);
                    }
                    if path_row(ui, "DXF", &mut self.dxf_path) {
                        self.choose_path(FileBrowserTarget::DxfOutput);
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
                        self.copy_gcode(ui.ctx());
                    }

                    ui.separator();
                    ui.heading("Preview");
                    ui.checkbox(&mut self.show_toolpath, "Toolpath");
                    ui.checkbox(&mut self.show_rapids, "Rapids");
                    ui.checkbox(&mut self.show_bounds, "Bounds");
                    ui.checkbox(&mut self.show_axes, "Axes");
                    ui.label(format!("G-code lines: {}", self.gcode_lines));
                    ui.label(format!("Cut moves: {}", self.preview_segments.len()));
                    ui.label(format!("Rapid moves: {}", self.preview_rapids.len()));
                });
            });

        egui::Panel::bottom("status_log")
            .exact_size(150.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Status, "Status");
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Gcode, "G-code");
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Cleanup, "Cleanup");
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Svg, "SVG");
                    ui.selectable_value(&mut self.bottom_tab, BottomTab::Dxf, "DXF");
                    ui.separator();
                    ui.monospace(&self.status);
                    if self.output_is_stale() {
                        ui.separator();
                        ui.colored_label(egui::Color32::from_rgb(225, 176, 84), "Output stale");
                    }
                    ui.separator();
                    ui.monospace(format!(
                        "{} lines, {} cut moves, {} rapid moves",
                        self.gcode_lines,
                        self.preview_segments.len(),
                        self.preview_rapids.len()
                    ));
                });
                ui.separator();
                match self.bottom_tab {
                    BottomTab::Status => draw_status_log(ui, &self.warnings),
                    BottomTab::Gcode => {
                        draw_output_preview(ui, Some(&self.gcode), "No G-code generated")
                    }
                    BottomTab::Cleanup => {
                        let preview = secondary_output_preview_text(&self.secondary_gcode);
                        draw_output_preview(ui, preview.as_deref(), "No cleanup G-code generated")
                    }
                    BottomTab::Svg => {
                        draw_output_preview(ui, self.svg.as_deref(), "No SVG generated")
                    }
                    BottomTab::Dxf => {
                        draw_output_preview(ui, self.dxf.as_deref(), "No DXF generated")
                    }
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            if self.fit_preview_requested {
                self.fit_preview_to_rect(rect);
                self.fit_preview_requested = false;
            }
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
                &self.preview_rapids,
                self.preview_bounds,
                self.show_toolpath,
                self.show_rapids,
                self.show_bounds,
                self.show_axes,
            );
        });

        self.show_browser(ui.ctx());
    }
}

impl RengraveApp {
    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if menu_action(ui, "Open settings...", true) {
                    self.choose_path(FileBrowserTarget::Settings);
                }
                if menu_action(ui, "Open input...", true) {
                    self.choose_path(FileBrowserTarget::Input);
                }
                if menu_action(ui, "Set default directory...", true) {
                    self.choose_path(FileBrowserTarget::DefaultDir);
                }
                ui.separator();
                if menu_action(ui, "Load", true) {
                    self.reload_document(ui.ctx().clone());
                }
                if menu_action(ui, "Save settings", !self.settings_path.trim().is_empty()) {
                    self.save_current_settings();
                }
                ui.separator();
                if menu_action(ui, "Choose G-code output...", true) {
                    self.choose_path(FileBrowserTarget::GcodeOutput);
                }
                if menu_action(ui, "Export G-code", !self.gcode.is_empty()) {
                    self.export_current(ExportKind::Gcode);
                }
                if menu_action(ui, "Export cleanup files", !self.secondary_gcode.is_empty()) {
                    self.export_secondary_outputs();
                }
                if menu_action(ui, "Export SVG", self.svg.is_some()) {
                    self.export_current(ExportKind::Svg);
                }
                if menu_action(ui, "Export DXF", self.dxf.is_some()) {
                    self.export_current(ExportKind::Dxf);
                }
                ui.separator();
                if menu_action(ui, "Copy G-code", !self.gcode.is_empty()) {
                    self.copy_gcode(ui.ctx());
                }
            });

            ui.menu_button("Run", |ui| {
                if menu_action(ui, "Calculate", true) {
                    self.start_calculation(ui.ctx().clone());
                }
                if menu_action(ui, "Cancel calculation", self.calculation.is_some()) {
                    self.cancel_calculation("Calculation canceled");
                }
                if self.active_calculation_is_stale() {
                    ui.colored_label(egui::Color32::from_rgb(225, 176, 84), "Input changed");
                }
                ui.separator();
                if menu_action(ui, "Refresh Potrace", true) {
                    self.refresh_potrace_status();
                }
            });

            ui.menu_button("View", |ui| {
                if menu_action(ui, "Fit preview", self.preview_bounds.is_some()) {
                    self.fit_preview_requested = true;
                }
                if menu_action(ui, "Reset pan/zoom", true) {
                    self.reset_preview_pan_zoom();
                }
                if menu_action(ui, "Reset view rotation", true) {
                    self.transform.viewport_rotation_degrees = 0.0;
                }
                ui.separator();
                ui.checkbox(&mut self.show_toolpath, "Toolpath layer");
                ui.checkbox(&mut self.show_rapids, "Rapid layer");
                ui.checkbox(&mut self.show_bounds, "Bounds layer");
                ui.checkbox(&mut self.show_axes, "Axes layer");
            });
        });
    }

    fn fit_preview_to_rect(&mut self, rect: egui::Rect) {
        fit_transform_to_bounds(&mut self.transform, self.preview_bounds, rect);
    }

    fn reset_preview_pan_zoom(&mut self) {
        self.transform.pan = Point::default();
        self.transform.zoom = DEFAULT_PREVIEW_ZOOM;
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

    fn output_is_stale(&self) -> bool {
        output_request_is_stale(
            &self.batch_request(true),
            self.last_output_request.as_ref(),
            !self.gcode.is_empty(),
        )
    }

    fn ensure_input_preview(&mut self) {
        let path = path_from_text(&self.input_path);
        let sample_text = input_preview_sample_for_path(path.as_deref(), &self.text);
        if self.input_preview.path != path || self.input_preview.sample_text != sample_text {
            self.input_preview = InputPreview::load(path, sample_text);
        }
    }

    fn reload_input_preview(&mut self) {
        let path = path_from_text(&self.input_path);
        let sample_text = input_preview_sample_for_path(path.as_deref(), &self.text);
        self.input_preview = InputPreview::load(path, sample_text);
        self.status = "Input preview refreshed".to_owned();
    }

    fn copy_gcode(&mut self, ctx: &egui::Context) {
        ctx.copy_text(self.gcode.clone());
        self.status = "G-code copied".to_owned();
    }
}

#[derive(Debug, Clone, PartialEq)]
struct UiControls {
    cut_type: CutTypeChoice,
    units: UnitsChoice,
    bit_shape: BitShapeChoice,
    arc_fit: ArcFitChoice,
    height_calc: HeightCalcChoice,
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
    clean_paths: String,
    bmp_turn_policy: BitmapTurnPolicy,
    bmp_turds: f64,
    bmp_alpha: f64,
    bmp_optto: f64,
    gpre: String,
    gpost: String,
    flip: bool,
    mirror: bool,
    outer: bool,
    upper: bool,
    plotbox: bool,
    use_image_size: bool,
    inlay: bool,
    bmp_long: bool,
    recovery_comments: bool,
    var_dis: bool,
    ext_char: bool,
    v_flop: bool,
}

impl UiControls {
    fn from_settings(settings: &LegacySettings) -> Self {
        Self {
            cut_type: CutTypeChoice::parse(settings.get_last("cut_type").unwrap_or("engrave")),
            units: UnitsChoice::parse(settings.get_last("units").unwrap_or("in")),
            bit_shape: BitShapeChoice::parse(settings.get_last("bit_shape").unwrap_or("VBIT")),
            arc_fit: ArcFitChoice::parse(settings.get_last("arc_fit").unwrap_or("none")),
            height_calc: HeightCalcChoice::parse(settings.get_last("H_CALC").unwrap_or("max_use")),
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
            clean_paths: settings
                .get_last("clean_paths")
                .unwrap_or("1,1,0,1,0,1,0,0")
                .to_owned(),
            bmp_turn_policy: BitmapTurnPolicy::parse(
                settings.get_last("bmp_turnp").unwrap_or("minority"),
            ),
            bmp_turds: setting_f64(settings, "bmp_turds", 2.0),
            bmp_alpha: setting_f64(settings, "bmp_alpha", 1.0),
            bmp_optto: setting_f64(settings, "bmp_optto", 0.2),
            gpre: settings
                .get_last("gpre")
                .unwrap_or("G17 G64 P0.001 M3 S3000")
                .to_owned(),
            gpost: settings.get_last("gpost").unwrap_or("M5|M2").to_owned(),
            flip: get_legacy_bool(settings, "flip", false),
            mirror: get_legacy_bool(settings, "mirror", false),
            outer: get_legacy_bool(settings, "outer", true),
            upper: get_legacy_bool(settings, "upper", true),
            plotbox: get_legacy_bool(settings, "plotbox", false),
            use_image_size: get_legacy_bool(settings, "useIMGsize", false),
            inlay: get_legacy_bool(settings, "inlay", false),
            bmp_long: get_legacy_bool(settings, "bmp_long", true),
            recovery_comments: !get_legacy_bool(settings, "no_comments", false),
            var_dis: get_legacy_bool(settings, "var_dis", true),
            ext_char: get_legacy_bool(settings, "ext_char", false),
            v_flop: get_legacy_bool(settings, "v_flop", false),
        }
    }

    fn overrides(&self) -> Vec<LegacySetting> {
        let mut entries = Vec::new();
        push_setting(&mut entries, "cut_type", self.cut_type.value(), false);
        push_setting(&mut entries, "units", self.units.value(), false);
        push_setting(&mut entries, "bit_shape", self.bit_shape.value(), false);
        push_setting(&mut entries, "arc_fit", self.arc_fit.value(), false);
        push_setting(&mut entries, "H_CALC", self.height_calc.value(), false);
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
        push_setting(&mut entries, "clean_paths", self.clean_paths.trim(), false);
        push_setting(
            &mut entries,
            "bmp_turnp",
            self.bmp_turn_policy.value(),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_turds",
            format_setting_number(self.bmp_turds),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_alpha",
            format_setting_number(self.bmp_alpha),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_optto",
            format_setting_number(self.bmp_optto),
            false,
        );
        push_setting(&mut entries, "gpre", self.gpre.trim(), false);
        push_setting(&mut entries, "gpost", self.gpost.trim(), false);
        push_bool(&mut entries, "flip", self.flip);
        push_bool(&mut entries, "mirror", self.mirror);
        push_bool(&mut entries, "outer", self.outer);
        push_bool(&mut entries, "upper", self.upper);
        push_bool(&mut entries, "plotbox", self.plotbox);
        push_bool(&mut entries, "useIMGsize", self.use_image_size);
        push_bool(&mut entries, "inlay", self.inlay);
        push_bool(&mut entries, "bmp_long", self.bmp_long);
        push_bool(&mut entries, "no_comments", !self.recovery_comments);
        push_bool(&mut entries, "var_dis", self.var_dis);
        push_bool(&mut entries, "ext_char", self.ext_char);
        push_bool(&mut entries, "v_flop", self.v_flop);
        entries
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeightCalcChoice {
    MaxUse,
    MaxAll,
}

impl HeightCalcChoice {
    fn parse(value: &str) -> Self {
        if value == "max_all" {
            Self::MaxAll
        } else {
            Self::MaxUse
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::MaxUse => "max_use",
            Self::MaxAll => "max_all",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::MaxUse => "Used chars",
            Self::MaxAll => "All chars",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BitmapTurnPolicy {
    Minority,
    Majority,
    Black,
    White,
    Left,
    Right,
    Random,
}

impl BitmapTurnPolicy {
    const ALL: [Self; 7] = [
        Self::Minority,
        Self::Majority,
        Self::Black,
        Self::White,
        Self::Left,
        Self::Right,
        Self::Random,
    ];

    fn parse(value: &str) -> Self {
        match value {
            "majority" => Self::Majority,
            "black" => Self::Black,
            "white" => Self::White,
            "left" => Self::Left,
            "right" => Self::Right,
            "random" => Self::Random,
            _ => Self::Minority,
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::Minority => "minority",
            Self::Majority => "majority",
            Self::Black => "black",
            Self::White => "white",
            Self::Left => "left",
            Self::Right => "right",
            Self::Random => "random",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Minority => "Minority",
            Self::Majority => "Majority",
            Self::Black => "Black",
            Self::White => "White",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Random => "Random",
        }
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

    fn dialog_title(self) -> &'static str {
        match self {
            Self::Settings => "Open Settings",
            Self::Input => "Open Input",
            Self::DefaultDir => "Choose Default Directory",
            Self::GcodeOutput => "Choose G-code Output",
            Self::SvgOutput => "Choose SVG Output",
            Self::DxfOutput => "Choose DXF Output",
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
            Self::Settings => path.is_file(),
            Self::Input => path.is_file() || path.is_dir(),
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
            } else if matches!(
                self.target,
                FileBrowserTarget::DefaultDir | FileBrowserTarget::Input
            ) && ui.button("Use current dir").clicked()
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InputCatalog {
    dir: Option<PathBuf>,
    entries: Vec<InputCatalogEntry>,
    error: Option<String>,
}

impl InputCatalog {
    fn scan(dir: PathBuf) -> Self {
        match read_input_catalog_entries(&dir) {
            Ok(entries) => Self {
                dir: Some(dir),
                entries,
                error: None,
            },
            Err(err) => Self {
                dir: Some(dir),
                entries: Vec::new(),
                error: Some(err),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputCatalogEntry {
    path: PathBuf,
    name: String,
    kind: InputCatalogKind,
    size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputCatalogKind {
    CxfFont,
    TtfFont,
    Dxf,
    Bitmap,
}

impl InputCatalogKind {
    fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("cxf") => Some(Self::CxfFont),
            Some("ttf") => Some(Self::TtfFont),
            Some("dxf") => Some(Self::Dxf),
            Some(
                "bmp" | "gif" | "jpg" | "jpeg" | "png" | "tif" | "tiff" | "pbm" | "ppm" | "pgm"
                | "pnm",
            ) => Some(Self::Bitmap),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CxfFont => "CXF",
            Self::TtfFont => "TTF",
            Self::Dxf => "DXF",
            Self::Bitmap => "Bitmap",
        }
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::CxfFont => 0,
            Self::TtfFont => 1,
            Self::Dxf => 2,
            Self::Bitmap => 3,
        }
    }
}

struct InputPreview {
    path: Option<PathBuf>,
    sample_text: Option<String>,
    data: InputPreviewData,
    texture: Option<egui::TextureHandle>,
}

impl Default for InputPreview {
    fn default() -> Self {
        Self {
            path: None,
            sample_text: None,
            data: InputPreviewData::Empty,
            texture: None,
        }
    }
}

impl InputPreview {
    fn load(path: Option<PathBuf>, sample_text: Option<String>) -> Self {
        let data = path
            .as_deref()
            .map(|path| load_input_preview_data(path, sample_text.as_deref()))
            .unwrap_or(InputPreviewData::Empty);
        Self {
            path,
            sample_text,
            data,
            texture: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum InputPreviewData {
    Empty,
    Vector {
        label: String,
        segments: Vec<PreviewSegment>,
        bounds: Option<PreviewBounds>,
        segment_count: usize,
    },
    Bitmap {
        original_width: u32,
        original_height: u32,
        thumbnail_width: usize,
        thumbnail_height: usize,
        rgba: Vec<u8>,
    },
    Error(String),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BottomTab {
    Status,
    Gcode,
    Cleanup,
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

fn secondary_output_path(path: &Path, suffix: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    let extension = path.extension().and_then(|value| value.to_str());
    let mut file_name = format!("{stem}_{suffix}");
    if let Some(extension) = extension {
        file_name.push('.');
        file_name.push_str(extension);
    }
    path.with_file_name(file_name)
}

fn calculation_request_is_stale(current: &BatchRequest, expected: &BatchRequest) -> bool {
    current.batch != expected.batch
        || current.gcode_file != expected.gcode_file
        || current.font_or_image != expected.font_or_image
        || current.default_dir != expected.default_dir
        || current.text != expected.text
        || current.settings_overrides != expected.settings_overrides
        || current.include_secondary != expected.include_secondary
        || current.svg_output.is_some() != expected.svg_output.is_some()
        || current.dxf_output.is_some() != expected.dxf_output.is_some()
}

fn output_request_is_stale(
    current: &BatchRequest,
    last_output: Option<&BatchRequest>,
    has_output: bool,
) -> bool {
    has_output
        && last_output
            .map(|last_output| calculation_request_is_stale(current, last_output))
            .unwrap_or(false)
}

fn settings_base_path_for_save(path_text: &str) -> Option<PathBuf> {
    path_from_text(path_text).filter(|path| path.is_file())
}

fn settings_file_contents(request: &DocumentRequest) -> Result<(String, Vec<String>), String> {
    let document = load_document(request).map_err(|err| err.to_string())?;
    Ok((document.settings.to_string(), document.warnings))
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

fn input_path_requires_potrace(path_text: &str) -> bool {
    path_from_text(path_text)
        .as_deref()
        .map(requires_potrace)
        .unwrap_or(false)
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

fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add_sized([124.0, 20.0], egui::Label::new(label));
        ui.add_sized(
            [ui.available_width().max(80.0), 22.0],
            egui::TextEdit::singleline(value),
        );
    });
}

fn clean_path_checkbox(ui: &mut egui::Ui, label: &str, clean_paths: &mut String, index: usize) {
    let mut values = parse_clean_path_values(clean_paths);
    let mut checked = values[index];
    if ui.checkbox(&mut checked, label).changed() {
        values[index] = checked;
        *clean_paths = format_clean_path_values(values);
    }
}

fn parse_clean_path_values(value: &str) -> [bool; 8] {
    let mut values = [true, true, false, true, false, true, false, false];
    if value.trim().is_empty() {
        return values;
    }
    for (index, token) in value.split(',').take(values.len()).enumerate() {
        values[index] = legacy_bool_value(token.trim());
    }
    values
}

fn format_clean_path_values(values: [bool; 8]) -> String {
    values
        .into_iter()
        .map(|value| if value { "1" } else { "0" })
        .collect::<Vec<_>>()
        .join(",")
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

fn menu_action(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let clicked = ui.add_enabled(enabled, egui::Button::new(label)).clicked();
    if clicked {
        ui.close();
    }
    clicked
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

fn append_view_setting_overrides(
    entries: &mut Vec<LegacySetting>,
    show_toolpath: bool,
    show_bounds: bool,
    show_axes: bool,
) {
    push_bool(entries, "show_v_path", show_toolpath);
    push_bool(entries, "show_box", show_bounds);
    push_bool(entries, "show_axis", show_axes);
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

fn choose_native_path(
    target: FileBrowserTarget,
    current_value: &str,
    default_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    let start_dir = browser_start_dir(target, current_value, default_dir);
    let dialog = FileDialog::new()
        .set_title(target.dialog_title())
        .set_directory(start_dir);

    match target {
        FileBrowserTarget::DefaultDir => dialog.pick_folder(),
        FileBrowserTarget::Settings => dialog
            .add_filter("F-Engrave settings", &["ngc", "nc", "tap"])
            .add_filter("All files", &["*"])
            .pick_file(),
        FileBrowserTarget::Input => dialog
            .add_filter(
                "R-Engrave inputs",
                &[
                    "cxf", "ttf", "dxf", "bmp", "gif", "jpg", "jpeg", "png", "tif", "tiff", "pbm",
                    "ppm", "pgm", "pnm",
                ],
            )
            .add_filter("All files", &["*"])
            .pick_file(),
        FileBrowserTarget::GcodeOutput => dialog
            .set_file_name(output_file_name(current_value, target))
            .add_filter("G-code", &["ngc", "nc", "tap"])
            .save_file(),
        FileBrowserTarget::SvgOutput => dialog
            .set_file_name(output_file_name(current_value, target))
            .add_filter("SVG", &["svg"])
            .save_file(),
        FileBrowserTarget::DxfOutput => dialog
            .set_file_name(output_file_name(current_value, target))
            .add_filter("DXF", &["dxf"])
            .save_file(),
    }
}

fn output_file_name(current_value: &str, target: FileBrowserTarget) -> String {
    path_from_text(current_value)
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .or_else(|| target.default_file_name().map(str::to_owned))
        .unwrap_or_else(|| "rengrave_output".to_owned())
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

fn input_catalog_start_dir(input: &Option<PathBuf>, default_dir: &Option<PathBuf>) -> PathBuf {
    if let Some(input) = input {
        if input.is_dir() {
            return input.clone();
        }
        if let Some(parent) = non_empty_parent(input) {
            return parent.to_path_buf();
        }
    }
    if let Some(default_dir) = default_dir {
        if default_dir.is_dir() {
            return default_dir.clone();
        }
        if let Some(parent) = non_empty_parent(default_dir) {
            return parent.to_path_buf();
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn read_input_catalog_entries(dir: &Path) -> Result<Vec<InputCatalogEntry>, String> {
    let mut entries = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|err| format!("unable to read input directory: {err}"))?
    {
        let entry = entry.map_err(|err| format!("unable to read input entry: {err}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("unable to read input file type: {err}"))?;
        if !file_type.is_file() {
            continue;
        }
        let Some(kind) = InputCatalogKind::from_path(&path) else {
            continue;
        };
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let size_bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        entries.push(InputCatalogEntry {
            path,
            name,
            kind,
            size_bytes,
        });
    }
    entries.sort_by(|left, right| {
        left.kind
            .sort_rank()
            .cmp(&right.kind.sort_rank())
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn input_preview_sample_for_path(path: Option<&Path>, text: &str) -> Option<String> {
    let path = path?;
    matches!(
        InputCatalogKind::from_path(path),
        Some(InputCatalogKind::CxfFont | InputCatalogKind::TtfFont)
    )
    .then(|| preview_text_sample(text))
}

fn preview_text_sample(text: &str) -> String {
    text.lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.chars().take(24).collect::<String>())
        })
        .filter(|sample| !sample.is_empty())
        .unwrap_or_else(|| "R-Engrave".to_owned())
}

fn load_input_preview_data(path: &Path, sample_text: Option<&str>) -> InputPreviewData {
    match InputCatalogKind::from_path(path) {
        Some(InputCatalogKind::CxfFont) => match read_cxf(path, 5.0) {
            Ok(font) => {
                vector_input_preview("CXF font", preview_segments_for_font(&font, sample_text))
            }
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::TtfFont) => match read_ttf(path, 5.0, false) {
            Ok(font) => {
                vector_input_preview("TTF font", preview_segments_for_font(&font, sample_text))
            }
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::Dxf) => match read_dxf_font(path, 5.0) {
            Ok(font) => vector_input_preview("DXF artwork", preview_segments_for_font(&font, None)),
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::Bitmap) => load_bitmap_preview(path),
        None => InputPreviewData::Error("unsupported input type".to_owned()),
    }
}

fn vector_input_preview(label: &str, segments: Vec<PreviewSegment>) -> InputPreviewData {
    let segment_count = segments.len();
    InputPreviewData::Vector {
        label: label.to_owned(),
        bounds: PreviewBounds::from_segments(&segments),
        segments,
        segment_count,
    }
}

fn preview_segments_for_font(font: &Font, sample_text: Option<&str>) -> Vec<PreviewSegment> {
    let mut segments = Vec::new();
    let mut cursor_x = 0.0;
    let fallback_advance = font.max_x().max(8.0) * 0.65;
    let sample_text = sample_text.unwrap_or("R-Engrave");

    for ch in sample_text.chars() {
        if ch.is_whitespace() {
            cursor_x += fallback_advance;
            continue;
        }
        let Some(glyph) = font.get_char(ch) else {
            cursor_x += fallback_advance;
            continue;
        };
        append_stroke_segments(&mut segments, &glyph.strokes, Point::new(cursor_x, 0.0));
        cursor_x += glyph.xmax().max(fallback_advance) + 2.0;
    }

    if segments.is_empty() {
        if let Some(glyph) = font.glyphs.values().next() {
            append_stroke_segments(&mut segments, &glyph.strokes, Point::default());
        }
    }

    segments
}

fn append_stroke_segments(segments: &mut Vec<PreviewSegment>, strokes: &[Stroke], offset: Point) {
    segments.extend(strokes.iter().map(|stroke| PreviewSegment {
        start: Point::new(stroke.start.x + offset.x, stroke.start.y + offset.y),
        end: Point::new(stroke.end.x + offset.x, stroke.end.y + offset.y),
    }));
}

fn load_bitmap_preview(path: &Path) -> InputPreviewData {
    match image::open(path) {
        Ok(image) => {
            let original_width = image.width();
            let original_height = image.height();
            let thumbnail = image
                .thumbnail(
                    INPUT_PREVIEW_THUMBNAIL_WIDTH,
                    INPUT_PREVIEW_THUMBNAIL_HEIGHT,
                )
                .to_rgba8();
            InputPreviewData::Bitmap {
                original_width,
                original_height,
                thumbnail_width: thumbnail.width() as usize,
                thumbnail_height: thumbnail.height() as usize,
                rgba: thumbnail.into_raw(),
            }
        }
        Err(err) => InputPreviewData::Error(format!("unable to decode bitmap preview: {err}")),
    }
}

fn draw_input_preview(ui: &mut egui::Ui, preview: &mut InputPreview) {
    match &mut preview.data {
        InputPreviewData::Empty => {
            ui.label("Select an input file");
        }
        InputPreviewData::Error(error) => {
            ui.colored_label(egui::Color32::from_rgb(225, 176, 84), error);
        }
        InputPreviewData::Vector {
            label,
            segments,
            bounds,
            segment_count,
        } => {
            ui.label(format!("{label} · {segment_count} segments"));
            draw_vector_input_preview(ui, segments, *bounds);
        }
        InputPreviewData::Bitmap {
            original_width,
            original_height,
            thumbnail_width,
            thumbnail_height,
            rgba,
        } => {
            ui.label(format!("Bitmap · {original_width} x {original_height} px"));
            if preview.texture.is_none() {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [*thumbnail_width, *thumbnail_height],
                    rgba,
                );
                let texture =
                    ui.ctx()
                        .load_texture("input-preview", image, egui::TextureOptions::LINEAR);
                preview.texture = Some(texture);
            }
            if let Some(texture) = &preview.texture {
                let max_width = ui.available_width().max(80.0);
                let max_height = INPUT_PREVIEW_THUMBNAIL_HEIGHT as f32;
                let image_width = *thumbnail_width as f32;
                let image_height = *thumbnail_height as f32;
                let scale = (max_width / image_width)
                    .min(max_height / image_height)
                    .min(1.0);
                let size = egui::vec2(image_width * scale, image_height * scale);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
    }
}

fn draw_status_log(ui: &mut egui::Ui, warnings: &[String]) {
    if warnings.is_empty() {
        ui.label("No warnings");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for warning in warnings {
            ui.colored_label(egui::Color32::from_rgb(225, 176, 84), warning);
        }
    });
}

fn draw_output_preview(ui: &mut egui::Ui, text: Option<&str>, empty_label: &str) {
    let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
        ui.label(empty_label);
        return;
    };

    let mut preview = output_preview_text(text, OUTPUT_PREVIEW_CHARS);
    ui.add_sized(
        [ui.available_width(), ui.available_height().max(40.0)],
        egui::TextEdit::multiline(&mut preview)
            .font(egui::TextStyle::Monospace)
            .interactive(false),
    );
}

fn output_preview_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut output = text.chars().take(max_chars).collect::<String>();
    output.push_str("\n... output truncated in preview ...");
    output
}

fn secondary_output_preview_text(outputs: &[SecondaryGcode]) -> Option<String> {
    if outputs.is_empty() {
        return None;
    }

    let mut preview = String::new();
    for output in outputs {
        if !preview.is_empty() {
            preview.push('\n');
        }
        preview.push_str(&format!("( cleanup output: _{} )\n", output.suffix));
        preview.push_str(output.gcode.trim_end());
        preview.push('\n');
    }
    Some(preview)
}

fn draw_vector_input_preview(
    ui: &mut egui::Ui,
    segments: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
) {
    let desired = egui::vec2(ui.available_width().max(80.0), INPUT_PREVIEW_VECTOR_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 30, 32));
    let Some(bounds) = bounds else {
        return;
    };

    let width = (bounds.max.x - bounds.min.x).abs().max(0.001) as f32;
    let height = (bounds.max.y - bounds.min.y).abs().max(0.001) as f32;
    let scale = ((rect.width() - 16.0) / width)
        .min((rect.height() - 16.0) / height)
        .max(0.001);
    let preview_width = width * scale;
    let preview_height = height * scale;
    let origin = egui::pos2(
        rect.center().x - preview_width / 2.0,
        rect.center().y + preview_height / 2.0,
    );
    let to_screen = |point: Point| {
        egui::pos2(
            origin.x + ((point.x - bounds.min.x) as f32) * scale,
            origin.y - ((point.y - bounds.min.y) as f32) * scale,
        )
    };

    for segment in segments {
        painter.line_segment(
            [to_screen(segment.start), to_screen(segment.end)],
            egui::Stroke::new(1.2, egui::Color32::from_rgb(94, 176, 132)),
        );
    }
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

    fn corners(self) -> [Point; 4] {
        [
            Point::new(self.min.x, self.min.y),
            Point::new(self.max.x, self.min.y),
            Point::new(self.max.x, self.max.y),
            Point::new(self.min.x, self.max.y),
        ]
    }

    fn from_segment_layers(cuts: &[PreviewSegment], rapids: &[PreviewSegment]) -> Option<Self> {
        let mut points = cuts
            .iter()
            .chain(rapids.iter())
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

fn fit_transform_to_bounds(
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

#[derive(Debug, Clone, Default, PartialEq)]
struct PreviewMotion {
    cuts: Vec<PreviewSegment>,
    rapids: Vec<PreviewSegment>,
}

fn parse_preview_motion(gcode: &str) -> PreviewMotion {
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

fn draw_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    transform: ViewTransform,
    segments: &[PreviewSegment],
    rapids: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
    show_toolpath: bool,
    show_rapids: bool,
    show_bounds: bool,
    show_axes: bool,
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

    if show_toolpath {
        for segment in segments {
            painter.line_segment(
                [to_screen(segment.start), to_screen(segment.end)],
                egui::Stroke::new(1.4, egui::Color32::from_rgb(94, 176, 132)),
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
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linear_gcode_moves_for_preview() {
        let motion = parse_preview_motion(
            "G0 X0.0000 Y0.0000\nG1 Z-0.0050\nG1 X1.0000 Y0.0000\nG0 X2.0000 Y2.0000\nG1 X2.0000 Y3.0000\n",
        );

        assert_eq!(motion.cuts.len(), 2);
        assert_eq!(motion.cuts[0].start, Point::new(0.0, 0.0));
        assert_eq!(motion.cuts[0].end, Point::new(1.0, 0.0));
        assert_eq!(motion.cuts[1].start, Point::new(2.0, 2.0));
        assert_eq!(motion.cuts[1].end, Point::new(2.0, 3.0));
        assert_eq!(motion.rapids.len(), 1);
        assert_eq!(motion.rapids[0].start, Point::new(1.0, 0.0));
        assert_eq!(motion.rapids[0].end, Point::new(2.0, 2.0));
    }

    #[test]
    fn parses_full_circle_arc_for_preview() {
        let motion = parse_preview_motion("G0 X-2.0000 Y0.0000\nG1 Z-0.0050\nG2 I2.0000 J0.0000\n");

        assert_eq!(motion.cuts.len(), 64);
        assert!(motion.rapids.is_empty());
        assert_eq!(motion.cuts[0].start, Point::new(-2.0, 0.0));
        assert!((motion.cuts.last().unwrap().end.x + 2.0).abs() < 1e-9);
        assert!(motion.cuts.iter().any(|segment| segment.end.x > 1.99));
        assert!(motion.cuts.iter().any(|segment| segment.end.y > 1.99));
        assert!(motion.cuts.iter().any(|segment| segment.end.y < -1.99));
    }

    #[test]
    fn parses_radius_format_arc_for_preview() {
        let motion =
            parse_preview_motion("G0 X1.0000 Y0.0000\nG1 Z-0.0050\nG3 X-1.0000 Y0.0000 R1.0000\n");

        assert_eq!(motion.cuts.len(), 32);
        assert!(motion.rapids.is_empty());
        assert_eq!(motion.cuts[0].start, Point::new(1.0, 0.0));
        assert!((motion.cuts.last().unwrap().end.x + 1.0).abs() < 1e-9);
        assert!(motion.cuts.iter().any(|segment| segment.end.y > 0.99));
    }

    #[test]
    fn parses_negative_radius_long_arc_for_preview() {
        let motion =
            parse_preview_motion("G0 X1.0000 Y0.0000\nG1 Z-0.0050\nG2 X0.0000 Y1.0000 R-1.0000\n");

        assert!(motion.cuts.len() > 32);
        assert!(motion.rapids.is_empty());
        assert_eq!(motion.cuts[0].start, Point::new(1.0, 0.0));
        assert!(motion.cuts.last().unwrap().end.x.abs() < 1e-9);
        assert!((motion.cuts.last().unwrap().end.y - 1.0).abs() < 1e-9);
        assert!(motion.cuts.iter().any(|segment| segment.end.y < -0.99));
    }

    #[test]
    fn preview_bounds_include_cut_and_rapid_layers() {
        let cuts = vec![PreviewSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        }];
        let rapids = vec![PreviewSegment {
            start: Point::new(-2.0, 3.0),
            end: Point::new(4.0, -1.0),
        }];

        let bounds = PreviewBounds::from_segment_layers(&cuts, &rapids).unwrap();

        assert_eq!(bounds.min, Point::new(-2.0, -1.0));
        assert_eq!(bounds.max, Point::new(4.0, 3.0));
    }

    #[test]
    fn fit_transform_centers_bounds_inside_preview_rect() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(500.0, 300.0));
        let bounds = PreviewBounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 5.0),
        };
        let mut transform = ViewTransform::default();

        fit_transform_to_bounds(&mut transform, Some(bounds), rect);

        assert!((transform.zoom - 45.2).abs() < 1e-9);
        assert_fitted_corners_inside(rect, transform, bounds);
        assert_pos_close(
            preview_screen_point(rect, transform, Point::new(5.0, 2.5)),
            rect.center(),
        );
    }

    #[test]
    fn fit_transform_accounts_for_view_rotation() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 400.0));
        let bounds = PreviewBounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 2.0),
        };
        let mut transform = ViewTransform {
            viewport_rotation_degrees: 90.0,
            ..ViewTransform::default()
        };

        fit_transform_to_bounds(&mut transform, Some(bounds), rect);

        assert_fitted_corners_inside(rect, transform, bounds);
    }

    #[test]
    fn fit_transform_resets_when_no_bounds_are_available() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        let mut transform = ViewTransform {
            pan: Point::new(12.0, -8.0),
            zoom: 42.0,
            viewport_rotation_degrees: 45.0,
            ..ViewTransform::default()
        };

        fit_transform_to_bounds(&mut transform, None, rect);

        assert_eq!(transform.pan, Point::default());
        assert_eq!(transform.zoom, DEFAULT_PREVIEW_ZOOM);
        assert_eq!(transform.viewport_rotation_degrees, 45.0);
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
    fn secondary_output_paths_append_suffix_before_extension() {
        assert_eq!(
            secondary_output_path(Path::new("/tmp/job.ngc"), "clean"),
            PathBuf::from("/tmp/job_clean.ngc")
        );
        assert_eq!(
            secondary_output_path(Path::new("/tmp/job"), "v_clean"),
            PathBuf::from("/tmp/job_v_clean")
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
        let secondary_toggle_changed = BatchRequest {
            include_secondary: true,
            ..expected.clone()
        };

        assert!(calculation_request_is_stale(&text_changed, &expected));
        assert!(calculation_request_is_stale(&settings_changed, &expected));
        assert!(calculation_request_is_stale(
            &export_toggle_changed,
            &expected
        ));
        assert!(calculation_request_is_stale(
            &secondary_toggle_changed,
            &expected
        ));
    }

    #[test]
    fn output_staleness_tracks_last_generated_request() {
        let expected = BatchRequest {
            batch: true,
            text: Some("A".to_owned()),
            settings_overrides: vec![LegacySetting::new("YSCALE", "2", false)],
            svg_output: Some(PathBuf::from("/tmp/out.svg")),
            dxf_output: Some(PathBuf::from("/tmp/out.dxf")),
            ..BatchRequest::default()
        };
        let current = BatchRequest {
            text: Some("B".to_owned()),
            ..expected.clone()
        };
        let output_path_changed = BatchRequest {
            svg_output: Some(PathBuf::from("/tmp/other.svg")),
            dxf_output: Some(PathBuf::from("/tmp/other.dxf")),
            ..expected.clone()
        };

        assert!(output_request_is_stale(&current, Some(&expected), true));
        assert!(!output_request_is_stale(
            &output_path_changed,
            Some(&expected),
            true
        ));
        assert!(!output_request_is_stale(&current, Some(&expected), false));
        assert!(!output_request_is_stale(&current, None, true));
    }

    #[test]
    fn settings_base_path_for_save_uses_only_existing_files() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-settings-base-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let existing = dir.join("settings.ngc");
        let missing = dir.join("new-settings.ngc");
        fs::write(&existing, "(fengrave_set YSCALE      2.0 )\n").unwrap();

        assert_eq!(
            settings_base_path_for_save(&existing.display().to_string()),
            Some(existing)
        );
        assert_eq!(
            settings_base_path_for_save(&missing.display().to_string()),
            None
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_file_contents_include_current_overrides_and_text() {
        let (contents, warnings) = settings_file_contents(&DocumentRequest {
            text: Some("AB".to_owned()),
            settings_overrides: vec![
                LegacySetting::new("YSCALE", "3.25", false),
                LegacySetting::new("plotbox", "1", false),
            ],
            ..DocumentRequest::default()
        })
        .unwrap();

        assert!(warnings.is_empty());
        assert!(contents.contains("(fengrave_set YSCALE      3.25 )"));
        assert!(contents.contains("(fengrave_set plotbox     1 )"));
        assert!(contents.contains("(fengrave_set TCODE       065 066 )"));
    }

    #[test]
    fn settings_file_contents_can_merge_existing_file() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-settings-merge-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.ngc");
        fs::write(
            &path,
            "(fengrave_set units       mm )\n(fengrave_set YSCALE      2.0 )\n",
        )
        .unwrap();

        let (contents, _) = settings_file_contents(&DocumentRequest {
            gcode_file: Some(path),
            settings_overrides: vec![LegacySetting::new("YSCALE", "4.0", false)],
            ..DocumentRequest::default()
        })
        .unwrap();

        let _ = fs::remove_dir_all(dir);
        assert!(contents.contains("(fengrave_set units       mm )"));
        assert!(contents.contains("(fengrave_set YSCALE      4.0 )"));
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
    fn input_path_requires_potrace_only_for_bitmap_files() {
        assert!(input_path_requires_potrace(" /tmp/image.png "));
        assert!(input_path_requires_potrace("/tmp/image.PBM"));
        assert!(!input_path_requires_potrace("/tmp/shape.dxf"));
        assert!(!input_path_requires_potrace("  "));
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
    fn output_file_name_uses_current_name_or_target_default() {
        assert_eq!(
            output_file_name("/tmp/custom.tap", FileBrowserTarget::GcodeOutput),
            "custom.tap"
        );
        assert_eq!(
            output_file_name("  ", FileBrowserTarget::SvgOutput),
            "rengrave_output.svg"
        );
        assert_eq!(
            output_file_name("/tmp/out", FileBrowserTarget::DxfOutput),
            "out"
        );
    }

    #[test]
    fn clean_path_values_parse_legacy_order_and_aliases() {
        assert_eq!(
            parse_clean_path_values("1,0,True,False,box,no_box,1,0,1"),
            [true, false, true, false, true, false, true, false]
        );
        assert_eq!(
            parse_clean_path_values(""),
            [true, true, false, true, false, true, false, false]
        );
    }

    #[test]
    fn clean_path_values_format_f_engrave_order() {
        assert_eq!(
            format_clean_path_values([true, false, true, false, true, false, true, false]),
            "1,0,1,0,1,0,1,0"
        );
    }

    #[test]
    fn view_setting_overrides_emit_legacy_layer_flags() {
        let mut entries = Vec::new();

        append_view_setting_overrides(&mut entries, false, true, false);

        let value_for = |key: &str| {
            entries
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.as_str())
        };
        assert_eq!(value_for("show_v_path"), Some("0"));
        assert_eq!(value_for("show_box"), Some("1"));
        assert_eq!(value_for("show_axis"), Some("0"));
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
    fn input_catalog_start_dir_prefers_input_parent() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-catalog-start-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("font.cxf");
        fs::write(&input, "[A] 1\nL 0,0,1,1\n").unwrap();

        assert_eq!(input_catalog_start_dir(&Some(input), &None), dir.clone());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn input_catalog_reads_supported_files_only() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-input-catalog-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("romanc.cxf"), "[A] 1\nL 0,0,1,1\n").unwrap();
        fs::write(dir.join("shape.dxf"), "0\nSECTION\n").unwrap();
        fs::write(dir.join("image.PNG"), b"not really png").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let entries = read_input_catalog_entries(&dir).unwrap();

        let _ = fs::remove_dir_all(dir);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, InputCatalogKind::CxfFont);
        assert_eq!(entries[1].kind, InputCatalogKind::Dxf);
        assert_eq!(entries[2].kind, InputCatalogKind::Bitmap);
        assert!(entries.iter().all(|entry| entry.name != "notes.txt"));
    }

    #[test]
    fn formats_input_catalog_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn input_preview_loads_cxf_vector_segments() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-cxf-preview-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("font.cxf");
        fs::write(&path, "[R] 2\nL 0,0,0,10\nL 0,10,5,10\n").unwrap();

        let preview = load_input_preview_data(&path, Some("R"));

        let _ = fs::remove_dir_all(dir);
        match preview {
            InputPreviewData::Vector {
                label,
                segment_count,
                ..
            } => {
                assert_eq!(label, "CXF font");
                assert!(segment_count > 0);
            }
            other => panic!("unexpected preview: {other:?}"),
        }
    }

    #[test]
    fn input_preview_uses_current_text_sample_for_fonts() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-ui-cxf-preview-sample-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("font.cxf");
        fs::write(&path, "[A] 1\nL 0,0,1,0\n[B] 2\nL 0,0,0,2\nL 0,2,2,2\n").unwrap();

        let preview = load_input_preview_data(&path, Some("B"));

        let _ = fs::remove_dir_all(dir);
        match preview {
            InputPreviewData::Vector {
                segments,
                segment_count,
                ..
            } => {
                assert_eq!(segment_count, 2);
                assert!(segments.iter().any(|segment| segment.end.y == 2.0));
            }
            other => panic!("unexpected preview: {other:?}"),
        }
    }

    #[test]
    fn input_preview_loads_dxf_vector_segments() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-dxf-preview-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shape.dxf");
        fs::write(
            &path,
            "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n2\n21\n3\n0\nENDSEC\n0\nEOF\n",
        )
        .unwrap();

        let preview = load_input_preview_data(&path, None);

        let _ = fs::remove_dir_all(dir);
        match preview {
            InputPreviewData::Vector {
                label,
                segment_count,
                ..
            } => {
                assert_eq!(label, "DXF artwork");
                assert_eq!(segment_count, 1);
            }
            other => panic!("unexpected preview: {other:?}"),
        }
    }

    #[test]
    fn input_preview_loads_bitmap_thumbnail() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-bitmap-preview-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("image.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();

        let preview = load_input_preview_data(&path, None);

        let _ = fs::remove_dir_all(dir);
        match preview {
            InputPreviewData::Bitmap {
                original_width,
                original_height,
                thumbnail_width,
                thumbnail_height,
                rgba,
            } => {
                assert_eq!(original_width, 4);
                assert_eq!(original_height, 2);
                assert!(thumbnail_width <= INPUT_PREVIEW_THUMBNAIL_WIDTH as usize);
                assert!(thumbnail_height <= INPUT_PREVIEW_THUMBNAIL_HEIGHT as usize);
                assert_eq!(rgba.len(), thumbnail_width * thumbnail_height * 4);
            }
            other => panic!("unexpected preview: {other:?}"),
        }
    }

    #[test]
    fn output_preview_text_truncates_large_payloads() {
        assert_eq!(output_preview_text("G90\nG0 X0\n", 100), "G90\nG0 X0\n");

        let preview = output_preview_text("abcdefghij", 4);

        assert_eq!(preview, "abcd\n... output truncated in preview ...");
    }

    #[test]
    fn secondary_output_preview_groups_cleanup_files_by_suffix() {
        let preview = secondary_output_preview_text(&[
            SecondaryGcode {
                suffix: "clean".to_owned(),
                gcode: "G90\nG1 X0\n".to_owned(),
            },
            SecondaryGcode {
                suffix: "v_clean".to_owned(),
                gcode: "G91\nG1 X1\n".to_owned(),
            },
        ])
        .unwrap();

        assert_eq!(
            preview,
            "( cleanup output: _clean )\nG90\nG1 X0\n\n( cleanup output: _v_clean )\nG91\nG1 X1\n"
        );
        assert_eq!(secondary_output_preview_text(&[]), None);
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
    fn ui_controls_emit_bitmap_potrace_overrides() {
        let mut controls = UiControls::from_settings(&LegacySettings::default());
        controls.bmp_turn_policy = BitmapTurnPolicy::Black;
        controls.bmp_turds = 7.0;
        controls.bmp_alpha = 0.75;
        controls.bmp_optto = 0.125;
        controls.bmp_long = false;
        controls.use_image_size = true;

        let overrides = controls.overrides();
        let value_for = |key: &str| {
            overrides
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.as_str())
        };

        assert_eq!(value_for("bmp_turnp"), Some("black"));
        assert_eq!(value_for("bmp_turds"), Some("7"));
        assert_eq!(value_for("bmp_alpha"), Some("0.75"));
        assert_eq!(value_for("bmp_optto"), Some("0.125"));
        assert_eq!(value_for("bmp_long"), Some("0"));
        assert_eq!(value_for("useIMGsize"), Some("1"));
    }

    #[test]
    fn ui_controls_emit_advanced_core_overrides() {
        let mut settings = LegacySettings::default();
        settings.set_or_push("H_CALC", "max_all", false);
        settings.set_or_push("gpre", "G17|M3 S12000", false);
        settings.set_or_push("gpost", "M5|M2", false);
        settings.set_or_push("clean_paths", "1,0,1,0,1,0,1,1", false);
        settings.set_or_push("no_comments", "1", false);
        settings.set_or_push("var_dis", "0", false);
        settings.set_or_push("ext_char", "1", false);
        settings.set_or_push("v_flop", "1", false);

        let mut controls = UiControls::from_settings(&settings);
        assert_eq!(controls.height_calc, HeightCalcChoice::MaxAll);
        assert_eq!(controls.gpre, "G17|M3 S12000");
        assert_eq!(controls.clean_paths, "1,0,1,0,1,0,1,1");
        assert!(!controls.recovery_comments);
        assert!(!controls.var_dis);
        assert!(controls.ext_char);
        assert!(controls.v_flop);

        controls.height_calc = HeightCalcChoice::MaxUse;
        controls.gpre = " G90|M3 S9000 ".to_owned();
        controls.gpost = " M5|M30 ".to_owned();
        controls.clean_paths = " 0,1,0,1,0,1,0,1 ".to_owned();
        controls.recovery_comments = true;
        controls.var_dis = true;
        controls.ext_char = false;
        controls.v_flop = false;

        let overrides = controls.overrides();
        let value_for = |key: &str| {
            overrides
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.as_str())
        };

        assert_eq!(value_for("H_CALC"), Some("max_use"));
        assert_eq!(value_for("gpre"), Some("G90|M3 S9000"));
        assert_eq!(value_for("gpost"), Some("M5|M30"));
        assert_eq!(value_for("clean_paths"), Some("0,1,0,1,0,1,0,1"));
        assert_eq!(value_for("no_comments"), Some("0"));
        assert_eq!(value_for("var_dis"), Some("1"));
        assert_eq!(value_for("ext_char"), Some("0"));
        assert_eq!(value_for("v_flop"), Some("0"));
    }

    #[test]
    fn bitmap_turn_policy_parses_legacy_values() {
        assert_eq!(
            BitmapTurnPolicy::parse("majority"),
            BitmapTurnPolicy::Majority
        );
        assert_eq!(BitmapTurnPolicy::parse("black"), BitmapTurnPolicy::Black);
        assert_eq!(BitmapTurnPolicy::parse("white"), BitmapTurnPolicy::White);
        assert_eq!(BitmapTurnPolicy::parse("left"), BitmapTurnPolicy::Left);
        assert_eq!(BitmapTurnPolicy::parse("right"), BitmapTurnPolicy::Right);
        assert_eq!(BitmapTurnPolicy::parse("random"), BitmapTurnPolicy::Random);
        assert_eq!(
            BitmapTurnPolicy::parse("unsupported"),
            BitmapTurnPolicy::Minority
        );
    }

    #[test]
    fn write_text_file_reports_empty_paths() {
        let err = write_text_file("  ", "G90").unwrap_err();

        assert_eq!(err, "output path is empty");
    }

    fn assert_fitted_corners_inside(
        rect: egui::Rect,
        transform: ViewTransform,
        bounds: PreviewBounds,
    ) {
        let padded = rect.shrink(PREVIEW_FIT_PADDING - 0.01);
        for point in bounds.corners() {
            let screen = preview_screen_point(rect, transform, point);
            assert!(
                padded.contains(screen),
                "point {point:?} projected outside {padded:?}: {screen:?}"
            );
        }
    }

    fn assert_pos_close(actual: egui::Pos2, expected: egui::Pos2) {
        assert!((actual.x - expected.x).abs() < 0.0001);
        assert!((actual.y - expected.y).abs() < 0.0001);
    }

    fn preview_screen_point(
        rect: egui::Rect,
        transform: ViewTransform,
        point: Point,
    ) -> egui::Pos2 {
        let (sin, cos) = transform.total_rotation_radians().sin_cos();
        let rotated = Point::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos);
        egui::pos2(
            rect.center().x + (rotated.x * transform.zoom + transform.pan.x) as f32,
            rect.center().y - (rotated.y * transform.zoom) as f32 + transform.pan.y as f32,
        )
    }
}
