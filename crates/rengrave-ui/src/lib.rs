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
#[cfg(test)]
use rengrave_core::batch::prepare_batch_output;
use rengrave_core::batch::{
    BatchOutput, BatchProgress, BatchRequest, SecondaryGcode,
    prepare_batch_output_with_cancel_and_progress,
};
use rengrave_core::bitmap::{BitmapBackend, BitmapTraceStats, bitmap_trace_mask_and_stats};
use rengrave_core::dxf::read_dxf_font;
use rengrave_core::external::{PotraceStatus, detect_potrace, is_bitmap_input};
use rengrave_core::font::{Font, Stroke, read_cxf, read_ttf};
use rengrave_core::geometry::{Point, ViewTransform};
use rengrave_core::project::{DocumentRequest, RengraveDocument, load_document};
use rengrave_core::settings::{
    DEFAULT_GCODE_POSTAMBLE, DEFAULT_GCODE_PREAMBLE, LegacySetting, LegacySettings,
    default_legacy_settings, get_legacy_bool, legacy_bool_value,
};
use rfd::FileDialog;

const DEFAULT_PREVIEW_ZOOM: f64 = 80.0;
const MM_PER_INCH: f64 = 25.4;
const PREVIEW_FIT_PADDING: f32 = 24.0;
const OUTPUT_PREVIEW_CHARS: usize = 8000;
const INPUT_PREVIEW_VECTOR_HEIGHT: f32 = 180.0;
const INPUT_PREVIEW_THUMBNAIL_WIDTH: u32 = 300;
const INPUT_PREVIEW_THUMBNAIL_HEIGHT: u32 = 180;
const DEFAULT_WINDOW_SIZE: [f32; 2] = [1280.0, 800.0];
const TOOLBAR_HEIGHT: f32 = 104.0;
const INPUT_PANEL_WIDTH: f32 = 380.0;
const INPUT_PANEL_CONTENT_WIDTH: f32 = INPUT_PANEL_WIDTH - 16.0;
const STATUS_PANEL_HEIGHT: f32 = 150.0;
const FORM_CONTROL_WIDTH: f32 = 170.0;
const PATH_CONTROL_WIDTH: f32 = 244.0;

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
            .with_inner_size(DEFAULT_WINDOW_SIZE)
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
    tool_view: ToolView,
    controls: UiControls,
    gcode: String,
    svg: Option<String>,
    dxf: Option<String>,
    secondary_gcode: Vec<SecondaryGcode>,
    gcode_lines: usize,
    preview_segments: Vec<PreviewSegment>,
    preview_rapids: Vec<PreviewSegment>,
    preview_cleanup_segments: Vec<PreviewSegment>,
    preview_bounds: Option<PreviewBounds>,
    gcode_path: String,
    svg_path: String,
    dxf_path: String,
    show_toolpath: bool,
    show_rapids: bool,
    show_cleanup: bool,
    show_bounds: bool,
    show_axes: bool,
    show_grid: bool,
    browser: Option<FileBrowser>,
    input_catalog: InputCatalog,
    input_catalog_filter: InputCatalogFilter,
    input_preview: InputPreview,
    preview_sample_text: String,
    preferences_path: Option<PathBuf>,
    calculation: Option<CalculationJob>,
    next_calculation_id: u64,
    warnings: Vec<String>,
    potrace_status: PotraceStatus,
    fit_preview_requested: bool,
    last_output_request: Option<BatchRequest>,
    bottom_tab: BottomTab,
    #[cfg(debug_assertions)]
    debug_layout_overlay: bool,
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
        let font_or_image = launch_font_or_image_path(&options, &preferences);
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
        let display_input_path = document_input_path_for_display(&font_or_image, &document);
        let tool_view =
            ToolView::from_settings_and_path(&document.settings, display_input_path.as_deref());
        let input_catalog =
            InputCatalog::scan(input_catalog_start_dir(&display_input_path, &default_dir));
        let status = if document.warnings.is_empty() {
            "Ready".to_owned()
        } else {
            "Startup warning".to_owned()
        };
        let use_metric_defaults = gcode_file.is_none();
        let mut controls = UiControls::from_settings(&document.settings);
        if use_metric_defaults {
            controls.convert_units(UnitsChoice::default_ui());
        }
        controls.cut_type = tool_view.cut_type();
        let potrace_status = initial_potrace_status(controls.bitmap_backend);
        let (default_gcode_path, default_svg_path, default_dxf_path) =
            default_output_paths(&default_dir);
        let mut app = Self {
            text: document.text,
            transform: ViewTransform {
                zoom: DEFAULT_PREVIEW_ZOOM,
                viewport_rotation_degrees: preferences.viewport_rotation_degrees,
                ..ViewTransform::default()
            },
            status,
            settings_count: document.settings.entries.len(),
            settings_path: path_to_text(&gcode_file),
            input_path: path_to_text(&display_input_path),
            default_dir_path: path_to_text(&default_dir),
            tool_view,
            controls,
            gcode: String::new(),
            svg: None,
            dxf: None,
            secondary_gcode: Vec::new(),
            gcode_lines: 0,
            preview_segments: Vec::new(),
            preview_rapids: Vec::new(),
            preview_cleanup_segments: Vec::new(),
            preview_bounds: None,
            gcode_path: if preferences.gcode_path.trim().is_empty() {
                default_gcode_path
            } else {
                preferences.gcode_path
            },
            svg_path: if preferences.svg_path.trim().is_empty() {
                default_svg_path
            } else {
                preferences.svg_path
            },
            dxf_path: if preferences.dxf_path.trim().is_empty() {
                default_dxf_path
            } else {
                preferences.dxf_path
            },
            show_toolpath: get_legacy_bool(&document.settings, "show_v_path", true),
            show_rapids: preferences.show_rapids,
            show_cleanup: preferences.show_cleanup,
            show_bounds: get_legacy_bool(&document.settings, "show_box", true),
            show_axes: get_legacy_bool(&document.settings, "show_axis", true),
            show_grid: preferences.show_grid,
            browser: None,
            input_catalog,
            input_catalog_filter: InputCatalogFilter::default(),
            input_preview: InputPreview::default(),
            preview_sample_text: preferences.preview_sample_text,
            preferences_path,
            calculation: None,
            next_calculation_id: 1,
            warnings: document.warnings,
            potrace_status,
            fit_preview_requested: false,
            last_output_request: None,
            bottom_tab: BottomTab::Status,
            #[cfg(debug_assertions)]
            debug_layout_overlay: false,
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
        let requested_input = path_from_text(&self.input_path);
        match load_document(&DocumentRequest {
            gcode_file: path_from_text(&self.settings_path),
            font_or_image: requested_input.clone(),
            default_dir: path_from_text(&self.default_dir_path),
            text: None,
            settings_overrides: Vec::new(),
        }) {
            Ok(document) => {
                let display_input_path =
                    document_input_path_for_display(&requested_input, &document);
                self.text = document.text;
                self.input_path = path_to_text(&display_input_path);
                self.controls = UiControls::from_settings(&document.settings);
                self.tool_view = ToolView::from_settings_and_path(
                    &document.settings,
                    display_input_path.as_deref(),
                );
                self.controls.cut_type = self.tool_view.cut_type();
                self.show_toolpath = get_legacy_bool(&document.settings, "show_v_path", true);
                self.show_bounds = get_legacy_bool(&document.settings, "show_box", true);
                self.show_axes = get_legacy_bool(&document.settings, "show_axis", true);
                self.settings_count = document.settings.entries.len();
                self.warnings = document.warnings;
                self.status = "Document loaded".to_owned();
                self.refresh_input_catalog();
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

    fn reset_controls_to_defaults(&mut self) {
        let defaults = default_legacy_settings();
        self.controls = default_ui_controls();
        self.controls.cut_type = self.tool_view.cut_type();
        self.show_toolpath = get_legacy_bool(&defaults, "show_v_path", true);
        self.show_rapids = true;
        self.show_cleanup = true;
        self.show_bounds = get_legacy_bool(&defaults, "show_box", true);
        self.show_axes = get_legacy_bool(&defaults, "show_axis", true);
        self.show_grid = true;
        self.status = "Controls reset to defaults".to_owned();
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
            let result = prepare_batch_output_with_cancel_and_progress(
                &worker_request,
                || worker_cancel_flag.load(Ordering::Relaxed),
                |progress| {
                    send_calculation_progress(&sender, &ctx, id, CalculationPhase::Batch(progress));
                },
            )
            .map_err(|err| err.to_string());
            send_calculation_progress(&sender, &ctx, id, CalculationPhase::Finalizing);
            let canceled = worker_cancel_flag.load(Ordering::Relaxed);
            let _ = sender.send(CalculationMessage::Finished {
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
        self.status = CalculationPhase::Queued.status_text().to_owned();
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
            Ok(CalculationMessage::Progress { id, phase }) => {
                if id == job.id {
                    self.status = phase.status_text().to_owned();
                }
                self.calculation = Some(job);
            }
            Ok(CalculationMessage::Finished {
                id,
                result,
                canceled,
            }) => self.apply_calculation_result(job, id, result, canceled),
            Err(TryRecvError::Empty) => {
                self.calculation = Some(job);
            }
            Err(TryRecvError::Disconnected) => {
                self.status = "Calculation worker stopped".to_owned();
            }
        }
    }

    fn apply_calculation_result(
        &mut self,
        job: CalculationJob,
        id: u64,
        result: Result<BatchOutput, String>,
        canceled: bool,
    ) {
        if id != job.id || canceled {
            self.status = "Stale calculation ignored".to_owned();
            return;
        }
        let current_request = self.batch_request(true);
        if calculation_request_is_stale(&current_request, &job.request) {
            self.status = "Stale calculation ignored".to_owned();
            return;
        }
        match result {
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
                self.preview_cleanup_segments.clear();
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
        self.preview_cleanup_segments = cleanup_preview_segments(&output.secondary_gcode);
        self.preview_bounds = PreviewBounds::from_segment_layers(&[
            &self.preview_segments,
            &self.preview_rapids,
            &self.preview_cleanup_segments,
        ]);
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

    fn export_all_available(&mut self) {
        if !self.any_export_available() {
            self.status = "No generated output to export".to_owned();
            return;
        }

        let mut written = 0usize;
        for (label, path, contents) in [
            ("G-code", self.gcode_path.clone(), Some(self.gcode.clone())),
            ("SVG", self.svg_path.clone(), self.svg.clone()),
            ("DXF", self.dxf_path.clone(), self.dxf.clone()),
        ] {
            let Some(contents) = contents.filter(|contents| !contents.is_empty()) else {
                continue;
            };
            if let Err(err) = write_text_file(&path, &contents) {
                self.status = format!("{label} export failed");
                self.warnings.push(err);
                return;
            }
            written += 1;
        }

        if !self.secondary_gcode.is_empty() {
            let primary_path = PathBuf::from(self.gcode_path.trim());
            if primary_path.as_os_str().is_empty() {
                self.status = "Cleanup export failed".to_owned();
                self.warnings.push("G-code output path is empty".to_owned());
                return;
            }
            for output in &self.secondary_gcode {
                let path = secondary_output_path(&primary_path, &output.suffix);
                if let Err(err) = fs::write(&path, &output.gcode) {
                    self.status = "Cleanup export failed".to_owned();
                    self.warnings
                        .push(format!("unable to write `{}`: {err}", path.display()));
                    return;
                }
                written += 1;
            }
        }

        self.status = format!("Exported {written} files");
        self.save_preferences();
    }

    fn any_export_available(&self) -> bool {
        export_payloads_available(
            &self.gcode,
            self.svg.as_deref(),
            self.dxf.as_deref(),
            &self.secondary_gcode,
        )
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

    fn reset_output_paths_to_default_dir(&mut self) {
        let default_dir = path_from_text(&self.default_dir_path);
        let (gcode_path, svg_path, dxf_path) = default_output_paths(&default_dir);
        self.gcode_path = gcode_path;
        self.svg_path = svg_path;
        self.dxf_path = dxf_path;
        self.status = "Output paths updated from default directory".to_owned();
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

    fn choose_path(&mut self, target: FileBrowserTarget, ctx: egui::Context) {
        if let Some(path) = choose_native_path(
            target,
            self.browser_value(target),
            path_from_text(&self.default_dir_path),
        ) {
            self.apply_browser_selection(target, path, ctx);
        } else {
            self.open_browser(target);
            self.status = "Using in-app browser".to_owned();
        }
    }

    fn browser_value(&self, target: FileBrowserTarget) -> &str {
        match target {
            FileBrowserTarget::Settings | FileBrowserTarget::SettingsOutput => &self.settings_path,
            FileBrowserTarget::Input => &self.input_path,
            FileBrowserTarget::DefaultDir => &self.default_dir_path,
            FileBrowserTarget::GcodeOutput => &self.gcode_path,
            FileBrowserTarget::SvgOutput => &self.svg_path,
            FileBrowserTarget::DxfOutput => &self.dxf_path,
        }
    }

    fn apply_browser_selection(
        &mut self,
        target: FileBrowserTarget,
        path: PathBuf,
        ctx: egui::Context,
    ) {
        let text = path.display().to_string();
        match target {
            FileBrowserTarget::Settings | FileBrowserTarget::SettingsOutput => {
                self.settings_path = text
            }
            FileBrowserTarget::Input => {
                self.input_path = text;
                self.update_tool_view_for_input_path(&path);
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
        match selection_followup(target) {
            SelectionFollowup::None => {}
            SelectionFollowup::LoadDocument => self.reload_document(ctx),
            SelectionFollowup::SaveSettings => self.save_current_settings(),
            SelectionFollowup::StartCalculation => self.start_calculation(ctx),
        }
    }

    fn refresh_input_catalog(&mut self) {
        let start_dir = input_catalog_start_dir(
            &path_from_text(&self.input_path),
            &path_from_text(&self.default_dir_path),
        );
        self.input_catalog = InputCatalog::scan(start_dir);
    }

    fn select_input_catalog_entry(&mut self, path: PathBuf, ctx: egui::Context) {
        self.update_tool_view_for_input_path(&path);
        self.input_path = path.display().to_string();
        self.status = "Input selected".to_owned();
        self.save_preferences();
        self.start_calculation(ctx);
    }

    fn set_tool_view(&mut self, tool_view: ToolView) {
        if self.tool_view == tool_view {
            self.controls.cut_type = tool_view.cut_type();
            return;
        }
        self.tool_view = tool_view;
        self.controls.cut_type = tool_view.cut_type();
        self.status = format!("{} selected", tool_view.label());
    }

    fn update_tool_view_for_input_path(&mut self, path: &Path) {
        let Some(kind) = InputCatalogKind::from_path(path) else {
            return;
        };
        let next = self.tool_view.with_input_kind(kind);
        self.set_tool_view(next);
    }

    fn show_workbench_selector(&mut self, ui: &mut egui::Ui) {
        let mut selected = self.tool_view;
        egui::ComboBox::from_id_salt("workbench_selector")
            .selected_text(selected.label())
            .width(160.0)
            .show_ui(ui, |ui| {
                ui.label("Workbench");
                for tool_view in ToolView::ALL {
                    ui.selectable_value(&mut selected, tool_view, tool_view.label());
                }
            });
        if selected != self.tool_view {
            self.set_tool_view(selected);
        }
    }

    fn show_workflow_input_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Input");
        self.show_input_paths(ui);

        if self.tool_view.uses_text() {
            ui.separator();
            self.show_text_input_panel(ui);
        }

        ui.separator();
        self.show_input_catalog_panel(ui);

        ui.separator();
        self.show_input_preview_panel(ui);

        ui.separator();
        ui.label(format!("Legacy keys: {}", self.settings_count));
    }

    fn show_input_paths(&mut self, ui: &mut egui::Ui) {
        let settings_path_action = path_row(ui, "Settings", &mut self.settings_path);
        if settings_path_action.browse_clicked {
            self.choose_path(FileBrowserTarget::Settings, ui.ctx().clone());
        }
        let input_path_action = path_row(ui, "Input", &mut self.input_path);
        if input_path_action.browse_clicked {
            self.choose_path(FileBrowserTarget::Input, ui.ctx().clone());
        }
        let default_dir_action = path_row(ui, "Default dir", &mut self.default_dir_path);
        if default_dir_action.browse_clicked {
            self.choose_path(FileBrowserTarget::DefaultDir, ui.ctx().clone());
        }
        if settings_path_action.value_changed
            || input_path_action.value_changed
            || default_dir_action.value_changed
        {
            self.save_preferences();
        }
        ui.horizontal_wrapped(|ui| {
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
            if ui.button("Save As").clicked() {
                self.choose_path(FileBrowserTarget::SettingsOutput, ui.ctx().clone());
            }
            if ui.button("Calculate").clicked() {
                self.start_calculation(ui.ctx().clone());
            }
        });
    }

    fn show_text_input_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Text");
        ui.add_sized(
            [ui.available_width(), 120.0],
            egui::TextEdit::multiline(&mut self.text),
        );
    }

    fn show_input_catalog_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Catalog");
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
            return;
        }

        let visible_entries = visible_input_catalog_entries_for_tool(
            &self.input_catalog.entries,
            self.input_catalog_filter,
            self.tool_view,
        );
        if visible_entries.is_empty() {
            ui.label("No compatible files found");
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(180.0)
            .show(ui, |ui| {
                for entry in visible_entries {
                    let selected = path_from_text(&self.input_path).as_ref() == Some(&entry.path);
                    let label = format!(
                        "{}  {}  {}",
                        entry.kind.label(),
                        entry.name,
                        format_bytes(entry.size_bytes)
                    );
                    if ui.selectable_label(selected, label).clicked() {
                        self.select_input_catalog_entry(entry.path, ui.ctx().clone());
                    }
                }
            });
    }

    fn show_input_preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Preview");
            if ui.button("Refresh").clicked() {
                self.reload_input_preview();
            }
        });
        if self.tool_view.uses_text()
            && input_preview_accepts_sample(path_from_text(&self.input_path).as_deref())
        {
            let sample_action = text_row(ui, "Sample", &mut self.preview_sample_text);
            if sample_action.value_changed {
                self.save_preferences();
            }
            if ui.button("Use engraving text").clicked() {
                self.preview_sample_text.clear();
                self.save_preferences();
            }
        }
        self.ensure_input_preview();
        draw_input_preview(ui, &mut self.input_preview);
    }

    fn show_workflow_settings_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(self.tool_view.settings_heading());
        self.show_units_row(ui);

        ui.separator();
        self.show_layout_settings(ui);

        ui.separator();
        self.show_machine_settings(ui);

        if self.tool_view.uses_image() && input_path_is_bitmap(&self.input_path) {
            ui.separator();
            self.show_bitmap_settings(ui);
        }

        if self.tool_view.uses_vcarve() {
            ui.separator();
            self.show_vcarve_settings(ui);
            ui.separator();
            self.show_multipass_settings(ui);
            ui.separator();
            self.show_cleanup_settings(ui);
        }

        ui.separator();
        self.show_preview_controls(ui);

        ui.separator();
        self.show_advanced_settings(ui);
    }

    fn show_units_row(&mut self, ui: &mut egui::Ui) {
        let mut selected_units = self.controls.units;
        combo_row(ui, "Units", self.controls.units.label(), |ui| {
            ui.selectable_value(
                &mut selected_units,
                UnitsChoice::Inch,
                UnitsChoice::Inch.label(),
            );
            ui.selectable_value(
                &mut selected_units,
                UnitsChoice::Mm,
                UnitsChoice::Mm.label(),
            );
        });
        self.controls.convert_units(selected_units);
    }

    fn show_layout_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Layout");
            if ui.button("Defaults").clicked() {
                self.reset_controls_to_defaults();
            }
        });
        combo_row(ui, "Origin", self.controls.origin.label(), |ui| {
            for value in OriginChoice::ALL {
                ui.selectable_value(&mut self.controls.origin, value, value.label());
            }
        });
        number_row(ui, "Height", &mut self.controls.yscale, 0.05);
        number_row(ui, "Width %", &mut self.controls.xscale_percent, 1.0);
        number_row(ui, "X origin", &mut self.controls.xorigin, 0.01);
        number_row(ui, "Y origin", &mut self.controls.yorigin, 0.01);

        if self.tool_view.uses_text() {
            combo_row(ui, "Justify", self.controls.justify.label(), |ui| {
                for value in JustifyChoice::ALL {
                    ui.selectable_value(&mut self.controls.justify, value, value.label());
                }
            });
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
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.controls.outer, "Outer");
                ui.checkbox(&mut self.controls.upper, "Upper");
            });
        } else {
            let mut use_image_size = self.controls.use_image_size;
            if ui.checkbox(&mut use_image_size, "Image size").changed() {
                let input_path = path_from_text(&self.input_path);
                let image_height =
                    image_preview_model_height(input_path.as_deref(), &self.input_preview.data);
                if let Some(converted_yscale) =
                    convert_image_size_yscale(self.controls.yscale, use_image_size, image_height)
                {
                    self.controls.yscale = converted_yscale;
                    self.status = "Image size scale converted".to_owned();
                }
                self.controls.use_image_size = use_image_size;
            }
        }

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.controls.flip, "Flip");
            ui.checkbox(&mut self.controls.mirror, "Mirror");
            ui.checkbox(&mut self.controls.plotbox, "Box");
        });
        number_row(ui, "Box gap", &mut self.controls.boxgap, 0.01);
    }

    fn show_machine_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Cut");
        number_row(ui, "Safe Z", &mut self.controls.safe_z, 0.01);
        if !self.tool_view.uses_vcarve() {
            number_row(ui, "Cut Z", &mut self.controls.depth_z, 0.001);
            number_row(ui, "Stroke", &mut self.controls.stroke_thickness, 0.001);
        }
        number_row(ui, "Feed", &mut self.controls.feed, 0.5);
        number_row(ui, "Plunge", &mut self.controls.plunge, 0.5);
        number_row(ui, "Accuracy", &mut self.controls.accuracy, 0.0005);
        if !self.tool_view.uses_vcarve() {
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
        }
    }

    fn show_bitmap_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bitmap Trace");
        combo_row(ui, "Backend", self.controls.bitmap_backend.label(), |ui| {
            for backend in BitmapBackend::ALL {
                ui.selectable_value(&mut self.controls.bitmap_backend, backend, backend.label());
            }
        });

        match self.controls.bitmap_backend {
            BitmapBackend::NativePotrace => {
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
                ui.checkbox(&mut self.controls.bmp_long, "Long curves");
            }
            BitmapBackend::PotraceSidecar => {
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
                ui.checkbox(&mut self.controls.bmp_long, "Long curves");
            }
        }
    }

    fn show_vcarve_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("V-carve");
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
        number_row(ui, "V angle", &mut self.controls.v_bit_angle, 1.0);
        number_row(ui, "V diameter", &mut self.controls.v_bit_dia, 0.01);
        number_row(ui, "V step", &mut self.controls.v_step_len, 0.001);
        number_row(ui, "Allowance", &mut self.controls.allowance, 0.001);
        number_row(ui, "Depth limit", &mut self.controls.v_depth_lim, 0.01);
        number_row(ui, "Drive corner", &mut self.controls.v_drv_crner, 1.0);
        number_row(ui, "Step corner", &mut self.controls.v_stp_crner, 1.0);
        combo_row(ui, "Check scope", self.controls.v_check_all.label(), |ui| {
            for value in VCheckScopeChoice::ALL {
                ui.selectable_value(&mut self.controls.v_check_all, value, value.label());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.controls.inlay, "Inlay");
            ui.checkbox(&mut self.controls.v_flop, "Flip normals");
        });
    }

    fn show_multipass_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Multipass");
        number_row(ui, "Finish stock", &mut self.controls.v_rough_stk, 0.01);
        let multipass_enabled = vcarve_multipass_enabled(self.controls.v_rough_stk);
        ui.add_enabled_ui(multipass_enabled, |ui| {
            number_row(ui, "Max depth/pass", &mut self.controls.v_max_cut, 0.01);
        });
        let color = if multipass_enabled {
            egui::Color32::from_rgb(94, 176, 132)
        } else {
            egui::Color32::from_rgb(160, 168, 172)
        };
        ui.colored_label(
            color,
            vcarve_multipass_summary(self.controls.v_rough_stk, self.controls.v_max_cut),
        );
    }

    fn show_cleanup_settings(&mut self, ui: &mut egui::Ui) {
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
    }

    fn show_top_output_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Output");
        if ui.small_button("Default dir").clicked() {
            self.reset_output_paths_to_default_dir();
        }
        if ui.small_button("G-code path").clicked() {
            self.choose_path(FileBrowserTarget::GcodeOutput, ui.ctx().clone());
        }
        if ui
            .add_enabled(!self.gcode.is_empty(), egui::Button::new("Export G-code"))
            .clicked()
        {
            self.export_current(ExportKind::Gcode);
        }
        if self.tool_view.uses_vcarve() {
            if ui
                .add_enabled(
                    !self.secondary_gcode.is_empty(),
                    egui::Button::new("Export cleanup"),
                )
                .clicked()
            {
                self.export_secondary_outputs();
            }
        }
        if ui.small_button("SVG path").clicked() {
            self.choose_path(FileBrowserTarget::SvgOutput, ui.ctx().clone());
        }
        if ui
            .add_enabled(self.svg.is_some(), egui::Button::new("Export SVG"))
            .clicked()
        {
            self.export_current(ExportKind::Svg);
        }
        if ui.small_button("DXF path").clicked() {
            self.choose_path(FileBrowserTarget::DxfOutput, ui.ctx().clone());
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
        if ui
            .add_enabled(self.any_export_available(), egui::Button::new("Export all"))
            .clicked()
        {
            self.export_all_available();
        }
    }

    fn show_preview_controls(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Preview Layers")
            .default_open(false)
            .show(ui, |ui| {
                ui.checkbox(&mut self.show_toolpath, "Toolpath");
                if ui.checkbox(&mut self.show_rapids, "Rapids").changed() {
                    self.save_preferences();
                }
                if ui
                    .add_enabled(
                        !self.preview_cleanup_segments.is_empty(),
                        egui::Checkbox::new(&mut self.show_cleanup, "Cleanup"),
                    )
                    .changed()
                {
                    self.save_preferences();
                }
                ui.checkbox(&mut self.show_bounds, "Bounds");
                ui.checkbox(&mut self.show_axes, "Axes");
                if ui.checkbox(&mut self.show_grid, "Grid").changed() {
                    self.save_preferences();
                }
                ui.label(format!("G-code lines: {}", self.gcode_lines));
                ui.label(format!("Cut moves: {}", self.preview_segments.len()));
                ui.label(format!("Rapid moves: {}", self.preview_rapids.len()));
                if self.tool_view.uses_vcarve() {
                    ui.label(format!(
                        "Cleanup moves: {}",
                        self.preview_cleanup_segments.len()
                    ));
                }
                ui.label(preview_length_readout("Cut length", &self.preview_segments));
                ui.label(preview_length_readout("Rapid length", &self.preview_rapids));
                if self.tool_view.uses_vcarve() {
                    ui.label(preview_length_readout(
                        "Cleanup length",
                        &self.preview_cleanup_segments,
                    ));
                }
                if let Some((size, range)) = preview_bounds_readout(self.preview_bounds) {
                    ui.label(size);
                    ui.monospace(range);
                } else {
                    ui.label("Extents: none");
                }
            });
    }

    fn show_advanced_settings(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Advanced")
            .default_open(false)
            .show(ui, |ui| {
                if self.tool_view.uses_text() {
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
                }
                number_row(ui, "Arc segments", &mut self.controls.segarc, 0.5);
                text_row(ui, "Preamble", &mut self.controls.gpre);
                text_row(ui, "Postamble", &mut self.controls.gpost);
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.controls.recovery_comments, "Recovery comments");
                    ui.checkbox(&mut self.controls.var_dis, "Disable variables");
                    if self.tool_view.uses_text() {
                        ui.checkbox(&mut self.controls.ext_char, "Extended chars");
                    }
                    ui.checkbox(&mut self.controls.show_thick, "Show thickness");
                    if self.tool_view.uses_vcarve() {
                        ui.checkbox(&mut self.controls.show_v_area, "Show V area");
                        ui.checkbox(&mut self.controls.v_pplot, "Plot during V-carve");
                    }
                });
            });
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
            show_rapids: self.show_rapids,
            show_grid: self.show_grid,
            show_cleanup: self.show_cleanup,
            viewport_rotation_degrees: self.transform.viewport_rotation_degrees,
            preview_sample_text: self.preview_sample_text.clone(),
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
        let root_rect = ui.max_rect();
        let top_rect = egui::Rect::from_min_max(
            root_rect.left_top(),
            egui::pos2(
                root_rect.right(),
                (root_rect.top() + TOOLBAR_HEIGHT).min(root_rect.bottom()),
            ),
        );
        let content_rect = egui::Rect::from_min_max(
            egui::pos2(root_rect.left(), top_rect.bottom()),
            root_rect.right_bottom(),
        );
        let left_rect = egui::Rect::from_min_max(
            content_rect.left_top(),
            egui::pos2(
                (content_rect.left() + INPUT_PANEL_WIDTH).min(content_rect.right()),
                content_rect.bottom(),
            ),
        );
        let work_rect = egui::Rect::from_min_max(
            egui::pos2(left_rect.right(), content_rect.top()),
            content_rect.right_bottom(),
        );
        let bottom_rect = egui::Rect::from_min_max(
            egui::pos2(
                work_rect.left(),
                (work_rect.bottom() - STATUS_PANEL_HEIGHT).max(work_rect.top()),
            ),
            work_rect.right_bottom(),
        );
        let preview_rect = egui::Rect::from_min_max(
            work_rect.left_top(),
            egui::pos2(work_rect.right(), bottom_rect.top()),
        );

        paint_panel_background(ui, top_rect);
        paint_panel_background(ui, left_rect);
        paint_panel_background(ui, bottom_rect);

        {
            let mut top_ui = panel_child_ui(ui, "toolbar", top_rect.shrink2(egui::vec2(6.0, 2.0)));
            self.show_toolbar_contents(&mut top_ui);
        }

        {
            let mut left_ui = panel_child_ui(
                ui,
                "input_settings",
                left_rect.shrink2(egui::vec2(6.0, 2.0)),
            );
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(&mut left_ui, |ui| {
                    ui.set_width(left_panel_content_width(ui));
                    self.show_workflow_input_panel(ui);
                    ui.separator();
                    self.show_workflow_settings_panel(ui);
                });
        }

        {
            let mut bottom_ui =
                panel_child_ui(ui, "status_log", bottom_rect.shrink2(egui::vec2(6.0, 2.0)));
            self.show_bottom_panel_contents(&mut bottom_ui);
        }

        {
            let mut preview_ui = panel_child_ui(ui, "preview", preview_rect);
            self.show_preview_panel(&mut preview_ui, preview_rect);
        }

        #[cfg(debug_assertions)]
        if self.debug_layout_overlay {
            draw_debug_layout_overlay(
                ui.ctx(),
                DebugLayoutRects {
                    root: root_rect,
                    top: top_rect,
                    left: left_rect,
                    preview: preview_rect,
                    bottom: bottom_rect,
                },
            );
        }

        self.show_browser(ui.ctx());
    }
}

impl RengraveApp {
    fn show_toolbar_contents(&mut self, ui: &mut egui::Ui) {
        self.show_menu_bar(ui);
        ui.horizontal(|ui| {
            self.show_workbench_selector(ui);
            ui.separator();
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
                if let Some(stale_summary) = self.active_calculation_stale_summary() {
                    ui.colored_label(egui::Color32::from_rgb(225, 176, 84), stale_summary);
                }
                if ui.button("Cancel").clicked() {
                    self.cancel_calculation("Calculation canceled");
                }
            } else if let Some(stale_summary) = self.output_stale_summary() {
                ui.colored_label(egui::Color32::from_rgb(225, 176, 84), stale_summary);
                if ui.button("Recalculate").clicked() {
                    self.start_calculation(ui.ctx().clone());
                }
            }
            ui.separator();
            ui.add_sized(
                [120.0, 20.0],
                egui::Slider::new(&mut self.transform.zoom, 1.0..=500.0)
                    .text("Zoom")
                    .clamping(egui::SliderClamping::Always),
            );
            if ui
                .add_sized(
                    [120.0, 20.0],
                    egui::Slider::new(
                        &mut self.transform.viewport_rotation_degrees,
                        -180.0..=180.0,
                    )
                    .text("View")
                    .clamping(egui::SliderClamping::Always),
                )
                .changed()
            {
                self.save_preferences();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Status");
            ui.monospace(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.show_top_output_controls(ui);
            });
        });
        ui.add_space(4.0);
        self.show_job_summary(ui);
    }

    fn show_bottom_panel_contents(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Status, "Status");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Gcode, "G-code");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Cleanup, "Cleanup");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Svg, "SVG");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Dxf, "DXF");
            ui.separator();
            ui.monospace(&self.status);
            if let Some(stale_summary) = self.output_stale_summary() {
                ui.separator();
                ui.colored_label(egui::Color32::from_rgb(225, 176, 84), stale_summary);
                if self.stale_recalculate_available() && ui.button("Recalculate").clicked() {
                    self.start_calculation(ui.ctx().clone());
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    self.current_bottom_tab_payload().is_some(),
                    egui::Button::new("Copy tab"),
                )
                .clicked()
            {
                self.copy_current_bottom_tab(ui.ctx());
            }
            ui.separator();
            ui.monospace(format!(
                "{} lines, {} cut moves, {} rapid moves, {} cleanup moves",
                self.gcode_lines,
                self.preview_segments.len(),
                self.preview_rapids.len(),
                self.preview_cleanup_segments.len()
            ));
        });
        ui.separator();
        match self.bottom_tab {
            BottomTab::Status => draw_status_log(ui, &self.warnings),
            BottomTab::Gcode => draw_output_preview(ui, Some(&self.gcode), "No G-code generated"),
            BottomTab::Cleanup => {
                let preview = secondary_output_preview_text(&self.secondary_gcode);
                draw_output_preview(ui, preview.as_deref(), "No cleanup G-code generated")
            }
            BottomTab::Svg => draw_output_preview(ui, self.svg.as_deref(), "No SVG generated"),
            BottomTab::Dxf => draw_output_preview(ui, self.dxf.as_deref(), "No DXF generated"),
        }
    }

    fn show_preview_panel(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if self.fit_preview_requested {
            self.fit_preview_to_rect(rect);
            self.fit_preview_requested = false;
        }
        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let hover_pos = response.hover_pos();
        if response.double_clicked() && self.preview_bounds.is_some() {
            self.fit_preview_requested = true;
        }
        if response.dragged() {
            let delta = response.drag_delta();
            self.transform.pan.x += f64::from(delta.x);
            self.transform.pan.y += f64::from(delta.y);
            ui.ctx().request_repaint();
        }
        if response.hovered() {
            let (scroll_y, zoom_delta) =
                ui.input(|input| (input.smooth_scroll_delta().y, input.zoom_delta()));
            let zoom_factor = if (zoom_delta - 1.0).abs() > f32::EPSILON {
                f64::from(zoom_delta)
            } else if scroll_y.abs() > 0.0 {
                2.0_f64.powf(f64::from(scroll_y) / 240.0)
            } else {
                1.0
            };
            if (zoom_factor - 1.0).abs() > f64::EPSILON
                && let Some(anchor) = hover_pos
            {
                zoom_transform_at_screen_point(&mut self.transform, rect, anchor, zoom_factor);
                ui.ctx().request_repaint();
            }
        }

        draw_preview(
            ui.painter(),
            rect,
            self.transform,
            self.controls.units.value(),
            &self.preview_segments,
            &self.preview_rapids,
            &self.preview_cleanup_segments,
            self.preview_bounds,
            self.show_toolpath,
            self.show_rapids,
            self.show_cleanup,
            self.show_bounds,
            self.show_axes,
            self.show_grid,
        );
        if let Some(pos) = hover_pos {
            let cursor = screen_point_to_model(rect, self.transform, pos);
            draw_preview_cursor_readout(ui.painter(), rect, cursor);
        }
    }

    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if menu_action(ui, "Open settings...", true) {
                    self.choose_path(FileBrowserTarget::Settings, ui.ctx().clone());
                }
                if menu_action(ui, "Save settings as...", true) {
                    self.choose_path(FileBrowserTarget::SettingsOutput, ui.ctx().clone());
                }
                if menu_action(ui, "Open input...", true) {
                    self.choose_path(FileBrowserTarget::Input, ui.ctx().clone());
                }
                if menu_action(ui, "Set default directory...", true) {
                    self.choose_path(FileBrowserTarget::DefaultDir, ui.ctx().clone());
                }
                ui.separator();
                if menu_action(ui, "Load", true) {
                    self.reload_document(ui.ctx().clone());
                }
                if menu_action(ui, "Save settings", !self.settings_path.trim().is_empty()) {
                    self.save_current_settings();
                }
                if menu_action(ui, "Reset controls to defaults", true) {
                    self.reset_controls_to_defaults();
                }
                ui.separator();
                if menu_action(ui, "Choose G-code output...", true) {
                    self.choose_path(FileBrowserTarget::GcodeOutput, ui.ctx().clone());
                }
                if menu_action(ui, "Choose SVG output...", true) {
                    self.choose_path(FileBrowserTarget::SvgOutput, ui.ctx().clone());
                }
                if menu_action(ui, "Choose DXF output...", true) {
                    self.choose_path(FileBrowserTarget::DxfOutput, ui.ctx().clone());
                }
                if menu_action(ui, "Use default dir for outputs", true) {
                    self.reset_output_paths_to_default_dir();
                }
                if menu_action(ui, "Export all available", self.any_export_available()) {
                    self.export_all_available();
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
                if menu_action(
                    ui,
                    "Copy current output tab",
                    self.current_bottom_tab_payload().is_some(),
                ) {
                    self.copy_current_bottom_tab(ui.ctx());
                }
            });

            ui.menu_button("Run", |ui| {
                if menu_action(ui, "Calculate", true) {
                    self.start_calculation(ui.ctx().clone());
                }
                if menu_action(ui, "Cancel calculation", self.calculation.is_some()) {
                    self.cancel_calculation("Calculation canceled");
                }
                if let Some(stale_summary) = self.active_calculation_stale_summary() {
                    ui.colored_label(egui::Color32::from_rgb(225, 176, 84), stale_summary);
                }
                ui.separator();
                if menu_action(
                    ui,
                    "Refresh Potrace",
                    self.controls.bitmap_backend == BitmapBackend::PotraceSidecar,
                ) {
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
                    self.save_preferences();
                }
                ui.separator();
                ui.checkbox(&mut self.show_toolpath, "Toolpath layer");
                if ui.checkbox(&mut self.show_rapids, "Rapid layer").changed() {
                    self.save_preferences();
                }
                if ui
                    .add_enabled(
                        !self.preview_cleanup_segments.is_empty(),
                        egui::Checkbox::new(&mut self.show_cleanup, "Cleanup layer"),
                    )
                    .changed()
                {
                    self.save_preferences();
                }
                ui.checkbox(&mut self.show_bounds, "Bounds layer");
                ui.checkbox(&mut self.show_axes, "Axes layer");
                if ui.checkbox(&mut self.show_grid, "Grid layer").changed() {
                    self.save_preferences();
                }
            });

            #[cfg(debug_assertions)]
            self.show_debug_menu(ui);
        });
    }

    #[cfg(debug_assertions)]
    fn show_debug_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Debug", |ui| {
            ui.label("Hover debug is active in debug builds only.");
            ui.label("Enable widget info, then hover the suspicious region.");
            ui.separator();
            ui.checkbox(
                &mut self.debug_layout_overlay,
                "Show R-Engrave layout rectangles",
            );
            ui.separator();

            let mut debug_options = ui.style().debug;
            let previous_options = debug_options;
            debug_options.ui(ui);
            if debug_options != previous_options {
                ui.ctx().all_styles_mut(|style| style.debug = debug_options);
                ui.ctx().request_repaint();
            }
        });
    }

    fn show_job_summary(&self, ui: &mut egui::Ui) {
        let output_state = output_state_summary(
            self.calculation.is_some(),
            self.output_is_stale(),
            !self.gcode.trim().is_empty(),
        );
        ui.horizontal_wrapped(|ui| {
            summary_label(
                ui,
                &input_source_summary(&self.input_path),
                egui::Color32::from_rgb(214, 220, 224),
            );
            summary_separator(ui);
            summary_label(
                ui,
                &tool_summary(&self.controls),
                egui::Color32::from_rgb(214, 220, 224),
            );
            summary_separator(ui);
            summary_label(ui, output_state, output_state_color(output_state));
            summary_separator(ui);
            summary_label(
                ui,
                &artifact_summary(
                    &self.gcode,
                    self.svg.as_deref(),
                    self.dxf.as_deref(),
                    self.secondary_gcode.len(),
                ),
                egui::Color32::from_rgb(214, 220, 224),
            );
            if let Some(warnings) = warning_count_summary(&self.warnings) {
                summary_separator(ui);
                summary_label(ui, &warnings, egui::Color32::from_rgb(225, 176, 84));
            }
            if let Some(vectorizer) = bitmap_vectorizer_summary(
                input_path_is_bitmap(&self.input_path),
                self.controls.bitmap_backend,
                self.potrace_status.available,
            ) {
                summary_separator(ui);
                let color = match self.controls.bitmap_backend {
                    BitmapBackend::NativePotrace => egui::Color32::from_rgb(94, 176, 132),
                    BitmapBackend::PotraceSidecar if self.potrace_status.available => {
                        egui::Color32::from_rgb(94, 176, 132)
                    }
                    BitmapBackend::PotraceSidecar => egui::Color32::from_rgb(225, 176, 84),
                };
                summary_label(ui, vectorizer, color);
            }
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
                self.apply_browser_selection(browser.target, path, ctx.clone());
            }
        }
    }

    fn active_calculation_stale_summary(&self) -> Option<String> {
        let job = self.calculation.as_ref()?;
        let reasons = calculation_stale_reasons(&self.batch_request(true), &job.request);
        (!reasons.is_empty()).then(|| stale_reason_summary("Changed", &reasons))
    }

    fn output_is_stale(&self) -> bool {
        output_request_is_stale(
            &self.batch_request(true),
            self.last_output_request.as_ref(),
            !self.gcode.is_empty(),
        )
    }

    fn output_stale_summary(&self) -> Option<String> {
        let reasons = output_request_stale_reasons(
            &self.batch_request(true),
            self.last_output_request.as_ref(),
            !self.gcode.is_empty(),
        );
        (!reasons.is_empty()).then(|| stale_reason_summary("Output stale", &reasons))
    }

    fn stale_recalculate_available(&self) -> bool {
        stale_recalculate_available(self.output_is_stale(), self.calculation.is_some())
    }

    fn ensure_input_preview(&mut self) {
        let path = path_from_text(&self.input_path);
        let sample_text =
            input_preview_sample_for_path(path.as_deref(), &self.text, &self.preview_sample_text);
        if self.input_preview.path != path || self.input_preview.sample_text != sample_text {
            self.input_preview = InputPreview::load(path, sample_text);
        }
    }

    fn reload_input_preview(&mut self) {
        let path = path_from_text(&self.input_path);
        let sample_text =
            input_preview_sample_for_path(path.as_deref(), &self.text, &self.preview_sample_text);
        self.input_preview = InputPreview::load(path, sample_text);
        self.status = "Input preview refreshed".to_owned();
    }

    fn copy_gcode(&mut self, ctx: &egui::Context) {
        self.copy_text_payload(ctx, "G-code", self.gcode.clone());
    }

    fn copy_current_bottom_tab(&mut self, ctx: &egui::Context) {
        if let Some((label, payload)) = self.current_bottom_tab_payload() {
            self.copy_text_payload(ctx, label, payload);
        }
    }

    fn current_bottom_tab_payload(&self) -> Option<(&'static str, String)> {
        bottom_tab_copy_payload(
            self.bottom_tab,
            &self.warnings,
            &self.gcode,
            &self.secondary_gcode,
            self.svg.as_deref(),
            self.dxf.as_deref(),
        )
    }

    fn copy_text_payload(&mut self, ctx: &egui::Context, label: &str, payload: String) {
        ctx.copy_text(payload);
        self.status = format!("{label} copied");
    }
}

#[derive(Debug, Clone, PartialEq)]
struct UiControls {
    cut_type: CutTypeChoice,
    units: UnitsChoice,
    bit_shape: BitShapeChoice,
    arc_fit: ArcFitChoice,
    height_calc: HeightCalcChoice,
    v_check_all: VCheckScopeChoice,
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
    v_drv_crner: f64,
    v_stp_crner: f64,
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
    bitmap_backend: BitmapBackend,
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
    v_pplot: bool,
    show_thick: bool,
    show_v_area: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolView {
    TextEngrave,
    ImageEngrave,
    TextVCarve,
    ImageVCarve,
}

impl ToolView {
    const ALL: [Self; 4] = [
        Self::TextEngrave,
        Self::ImageEngrave,
        Self::TextVCarve,
        Self::ImageVCarve,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::TextEngrave => "Text Engrave",
            Self::ImageEngrave => "Image Engrave",
            Self::TextVCarve => "Text V-carve",
            Self::ImageVCarve => "Image V-carve",
        }
    }

    fn settings_heading(self) -> &'static str {
        match self {
            Self::TextEngrave => "Text Engraving",
            Self::ImageEngrave => "Image Engraving",
            Self::TextVCarve => "Text V-carving",
            Self::ImageVCarve => "Image V-carving",
        }
    }

    fn uses_text(self) -> bool {
        matches!(self, Self::TextEngrave | Self::TextVCarve)
    }

    fn uses_image(self) -> bool {
        matches!(self, Self::ImageEngrave | Self::ImageVCarve)
    }

    fn uses_vcarve(self) -> bool {
        matches!(self, Self::TextVCarve | Self::ImageVCarve)
    }

    fn cut_type(self) -> CutTypeChoice {
        if self.uses_vcarve() {
            CutTypeChoice::VCarve
        } else {
            CutTypeChoice::Engrave
        }
    }

    fn accepts_kind(self, kind: InputCatalogKind) -> bool {
        match kind {
            InputCatalogKind::CxfFont | InputCatalogKind::TtfFont => self.uses_text(),
            InputCatalogKind::Dxf | InputCatalogKind::Bitmap => self.uses_image(),
        }
    }

    fn with_input_kind(self, kind: InputCatalogKind) -> Self {
        let vcarve = self.uses_vcarve();
        match kind {
            InputCatalogKind::CxfFont | InputCatalogKind::TtfFont => {
                if vcarve {
                    Self::TextVCarve
                } else {
                    Self::TextEngrave
                }
            }
            InputCatalogKind::Dxf | InputCatalogKind::Bitmap => {
                if vcarve {
                    Self::ImageVCarve
                } else {
                    Self::ImageEngrave
                }
            }
        }
    }

    fn from_settings_and_path(settings: &LegacySettings, path: Option<&Path>) -> Self {
        let cut_type = CutTypeChoice::parse(settings.get_last("cut_type").unwrap_or("engrave"));
        let image_input = path
            .and_then(InputCatalogKind::from_path)
            .is_some_and(|kind| matches!(kind, InputCatalogKind::Dxf | InputCatalogKind::Bitmap));
        match (cut_type, image_input) {
            (CutTypeChoice::VCarve, true) => Self::ImageVCarve,
            (CutTypeChoice::VCarve, false) => Self::TextVCarve,
            (CutTypeChoice::Engrave, true) => Self::ImageEngrave,
            (CutTypeChoice::Engrave, false) => Self::TextEngrave,
        }
    }
}

impl UiControls {
    fn from_settings(settings: &LegacySettings) -> Self {
        let explicit_units = settings.get_last("units");
        let source_units = explicit_units
            .map(UnitsChoice::parse)
            .unwrap_or(UnitsChoice::Inch);
        let mut controls = Self {
            cut_type: CutTypeChoice::parse(settings.get_last("cut_type").unwrap_or("engrave")),
            units: source_units,
            bit_shape: BitShapeChoice::parse(settings.get_last("bit_shape").unwrap_or("VBIT")),
            arc_fit: ArcFitChoice::parse(settings.get_last("arc_fit").unwrap_or("none")),
            height_calc: HeightCalcChoice::parse(settings.get_last("H_CALC").unwrap_or("max_use")),
            v_check_all: VCheckScopeChoice::parse(
                settings.get_last("v_check_all").unwrap_or("all"),
            ),
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
            v_drv_crner: setting_f64(settings, "v_drv_crner", 135.0),
            v_stp_crner: setting_f64(settings, "v_stp_crner", 200.0),
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
            bitmap_backend: BitmapBackend::from_settings(settings),
            gpre: settings
                .get_last("gpre")
                .unwrap_or(DEFAULT_GCODE_PREAMBLE)
                .to_owned(),
            gpost: settings
                .get_last("gpost")
                .unwrap_or(DEFAULT_GCODE_POSTAMBLE)
                .to_owned(),
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
            v_pplot: get_legacy_bool(settings, "v_pplot", false),
            show_thick: get_legacy_bool(settings, "show_thick", true),
            show_v_area: get_legacy_bool(settings, "show_v_area", true),
        };
        if explicit_units.is_none() {
            controls.convert_units(UnitsChoice::default_ui());
        }
        controls
    }

    fn convert_units(&mut self, target_units: UnitsChoice) {
        if self.units == target_units {
            return;
        }

        let factor = self.units.conversion_factor_to(target_units);
        self.units = target_units;

        self.yscale *= factor;
        self.text_radius *= factor;
        self.safe_z *= factor;
        self.depth_z *= factor;
        self.stroke_thickness *= factor;
        self.xorigin *= factor;
        self.yorigin *= factor;
        self.accuracy *= factor;
        self.feed *= factor;
        self.plunge *= factor;
        self.boxgap *= factor;
        self.v_bit_dia *= factor;
        self.v_step_len *= factor;
        self.allowance *= factor;
        self.v_max_cut *= factor;
        self.v_rough_stk *= factor;
        self.v_depth_lim *= factor;
        self.clean_dia *= factor;
        self.clean_v *= factor;
    }

    fn overrides(&self) -> Vec<LegacySetting> {
        let mut entries = Vec::new();
        push_setting(&mut entries, "cut_type", self.cut_type.value(), false);
        push_setting(&mut entries, "units", self.units.value(), false);
        push_setting(&mut entries, "bit_shape", self.bit_shape.value(), false);
        push_setting(&mut entries, "arc_fit", self.arc_fit.value(), false);
        push_setting(&mut entries, "H_CALC", self.height_calc.value(), false);
        push_setting(&mut entries, "v_check_all", self.v_check_all.value(), false);
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
            "v_drv_crner",
            format_setting_number(self.v_drv_crner),
            false,
        );
        push_setting(
            &mut entries,
            "v_stp_crner",
            format_setting_number(self.v_stp_crner),
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
        push_setting(
            &mut entries,
            "bitmap_backend",
            self.bitmap_backend.value(),
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
        push_bool(&mut entries, "v_pplot", self.v_pplot);
        push_bool(&mut entries, "show_thick", self.show_thick);
        push_bool(&mut entries, "show_v_area", self.show_v_area);
        entries
    }
}

fn default_ui_controls() -> UiControls {
    let mut controls = UiControls::from_settings(&default_legacy_settings());
    controls.convert_units(UnitsChoice::default_ui());
    controls
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
enum VCheckScopeChoice {
    All,
    Character,
}

impl VCheckScopeChoice {
    const ALL: [Self; 2] = [Self::All, Self::Character];

    fn parse(value: &str) -> Self {
        if value == "chr" {
            Self::Character
        } else {
            Self::All
        }
    }

    fn value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Character => "chr",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Character => "Character",
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
    fn default_ui() -> Self {
        Self::Mm
    }

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

    fn conversion_factor_to(self, target: Self) -> f64 {
        match (self, target) {
            (Self::Inch, Self::Mm) => MM_PER_INCH,
            (Self::Mm, Self::Inch) => 1.0 / MM_PER_INCH,
            _ => 1.0,
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
    SettingsOutput,
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
            Self::SettingsOutput => "settings output",
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
            Self::SettingsOutput => "Save Settings As",
            Self::Input => "Open Input",
            Self::DefaultDir => "Choose Default Directory",
            Self::GcodeOutput => "Choose G-code Output",
            Self::SvgOutput => "Choose SVG Output",
            Self::DxfOutput => "Choose DXF Output",
        }
    }

    fn default_file_name(self) -> Option<&'static str> {
        match self {
            Self::SettingsOutput => Some("rengrave_settings.ngc"),
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
            Self::SettingsOutput | Self::GcodeOutput | Self::SvgOutput | Self::DxfOutput => {
                !path.is_dir()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionFollowup {
    None,
    LoadDocument,
    SaveSettings,
    StartCalculation,
}

fn selection_followup(target: FileBrowserTarget) -> SelectionFollowup {
    match target {
        FileBrowserTarget::Settings => SelectionFollowup::LoadDocument,
        FileBrowserTarget::SettingsOutput => SelectionFollowup::SaveSettings,
        FileBrowserTarget::Input => SelectionFollowup::StartCalculation,
        FileBrowserTarget::DefaultDir
        | FileBrowserTarget::GcodeOutput
        | FileBrowserTarget::SvgOutput
        | FileBrowserTarget::DxfOutput => SelectionFollowup::None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InputCatalogFilter {
    cxf: bool,
    ttf: bool,
    dxf: bool,
    bitmap: bool,
}

impl Default for InputCatalogFilter {
    fn default() -> Self {
        Self {
            cxf: true,
            ttf: true,
            dxf: true,
            bitmap: true,
        }
    }
}

impl InputCatalogFilter {
    fn accepts(self, kind: InputCatalogKind) -> bool {
        match kind {
            InputCatalogKind::CxfFont => self.cxf,
            InputCatalogKind::TtfFont => self.ttf,
            InputCatalogKind::Dxf => self.dxf,
            InputCatalogKind::Bitmap => self.bitmap,
        }
    }
}

struct InputPreview {
    path: Option<PathBuf>,
    sample_text: Option<String>,
    data: InputPreviewData,
    texture: Option<egui::TextureHandle>,
    mask_texture: Option<egui::TextureHandle>,
}

impl Default for InputPreview {
    fn default() -> Self {
        Self {
            path: None,
            sample_text: None,
            data: InputPreviewData::Empty,
            texture: None,
            mask_texture: None,
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
            mask_texture: None,
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
        missing_chars: Vec<char>,
    },
    Bitmap {
        original_width: u32,
        original_height: u32,
        thumbnail_width: usize,
        thumbnail_height: usize,
        rgba: Vec<u8>,
        mask_width: usize,
        mask_height: usize,
        mask_rgba: Vec<u8>,
        trace_stats: BitmapTraceStats,
    },
    Error(String),
}

struct CalculationJob {
    id: u64,
    request: BatchRequest,
    receiver: Receiver<CalculationMessage>,
    cancel_flag: Arc<AtomicBool>,
}

enum CalculationMessage {
    Progress {
        id: u64,
        phase: CalculationPhase,
    },
    Finished {
        id: u64,
        result: Result<BatchOutput, String>,
        canceled: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalculationPhase {
    Queued,
    Batch(BatchProgress),
    Finalizing,
}

impl CalculationPhase {
    fn status_text(self) -> &'static str {
        match self {
            Self::Queued => "Calculation queued",
            Self::Batch(progress) => progress.status_text(),
            Self::Finalizing => "Finalizing output",
        }
    }
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

fn send_calculation_progress(
    sender: &mpsc::Sender<CalculationMessage>,
    ctx: &egui::Context,
    id: u64,
    phase: CalculationPhase,
) {
    let _ = sender.send(CalculationMessage::Progress { id, phase });
    ctx.request_repaint();
}

fn default_output_path(default_dir: &Option<PathBuf>, file_name: &str) -> String {
    default_dir
        .as_ref()
        .map(|dir| dir.join(file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
        .display()
        .to_string()
}

fn default_output_paths(default_dir: &Option<PathBuf>) -> (String, String, String) {
    (
        default_output_path(default_dir, "rengrave_output.ngc"),
        default_output_path(default_dir, "rengrave_output.svg"),
        default_output_path(default_dir, "rengrave_output.dxf"),
    )
}

fn launch_font_or_image_path(
    options: &UiLaunchOptions,
    preferences: &UiPreferences,
) -> Option<PathBuf> {
    options.font_or_image.clone().or_else(|| {
        if options.gcode_file.is_some() {
            None
        } else if let Some(path) = path_from_text(&preferences.input_path) {
            Some(path)
        } else if path_from_text(&preferences.settings_path).is_some() {
            None
        } else {
            bundled_demo_font_path()
        }
    })
}

fn bundled_demo_font_path() -> Option<PathBuf> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/rengrave_demo.cxf");
    path.is_file().then_some(path)
}

fn document_input_path_for_display(
    requested_input: &Option<PathBuf>,
    document: &RengraveDocument,
) -> Option<PathBuf> {
    document
        .input_path
        .clone()
        .or_else(|| requested_input.clone())
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
    !calculation_stale_reasons(current, expected).is_empty()
}

fn calculation_stale_reasons(current: &BatchRequest, expected: &BatchRequest) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if current.batch != expected.batch {
        reasons.push("mode");
    }
    if current.gcode_file != expected.gcode_file {
        reasons.push("settings file");
    }
    if current.font_or_image != expected.font_or_image {
        reasons.push("input file");
    }
    if current.default_dir != expected.default_dir {
        reasons.push("default dir");
    }
    if current.text != expected.text {
        reasons.push("text");
    }
    if current.settings_overrides != expected.settings_overrides {
        reasons.push("controls");
    }
    if current.include_secondary != expected.include_secondary {
        reasons.push("cleanup");
    }
    if current.svg_output.is_some() != expected.svg_output.is_some()
        || current.dxf_output.is_some() != expected.dxf_output.is_some()
    {
        reasons.push("export set");
    }
    reasons
}

fn output_request_is_stale(
    current: &BatchRequest,
    last_output: Option<&BatchRequest>,
    has_output: bool,
) -> bool {
    !output_request_stale_reasons(current, last_output, has_output).is_empty()
}

fn output_request_stale_reasons(
    current: &BatchRequest,
    last_output: Option<&BatchRequest>,
    has_output: bool,
) -> Vec<&'static str> {
    if !has_output {
        return Vec::new();
    }
    last_output
        .map(|last_output| calculation_stale_reasons(current, last_output))
        .unwrap_or_default()
}

fn stale_recalculate_available(output_is_stale: bool, calculation_active: bool) -> bool {
    output_is_stale && !calculation_active
}

fn stale_reason_summary(prefix: &str, reasons: &[&str]) -> String {
    if reasons.is_empty() {
        return prefix.to_owned();
    }
    let max_visible = 3;
    let mut summary = format!(
        "{prefix}: {}",
        reasons
            .iter()
            .take(max_visible)
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    );
    if reasons.len() > max_visible {
        summary.push_str(&format!(", +{} more", reasons.len() - max_visible));
    }
    summary
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

fn initial_potrace_status(backend: BitmapBackend) -> PotraceStatus {
    match backend {
        BitmapBackend::NativePotrace => PotraceStatus::missing("Potrace status not checked"),
        BitmapBackend::PotraceSidecar => detect_potrace(),
    }
}

fn input_path_is_bitmap(path_text: &str) -> bool {
    path_from_text(path_text)
        .as_deref()
        .map(is_bitmap_input)
        .unwrap_or(false)
}

fn vcarve_multipass_enabled(finish_stock: f64) -> bool {
    finish_stock > 0.0
}

fn vcarve_multipass_summary(finish_stock: f64, max_depth_per_pass: f64) -> String {
    if !vcarve_multipass_enabled(finish_stock) {
        "Multipass disabled: finish stock is zero".to_owned()
    } else if max_depth_per_pass >= 0.0 {
        "Multipass configured but max depth/pass should be negative".to_owned()
    } else {
        format!(
            "Multipass enabled: leave {} finish stock, max {} per pass",
            format_setting_number(finish_stock),
            format_setting_number(max_depth_per_pass)
        )
    }
}

fn input_source_summary(path_text: &str) -> String {
    let Some(path) = path_from_text(path_text) else {
        return "Source: none".to_owned();
    };
    let name = path_display_name(&path);
    match InputCatalogKind::from_path(&path) {
        Some(kind) => format!("Source: {} {name}", kind.label()),
        None if path.is_dir() => format!("Source dir: {name}"),
        None => format!("Source: {name}"),
    }
}

fn path_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn tool_summary(controls: &UiControls) -> String {
    format!(
        "Job: {}, {}, {}",
        controls.cut_type.label(),
        controls.bit_shape.label(),
        controls.units.label()
    )
}

fn output_state_summary(
    calculation_active: bool,
    output_stale: bool,
    has_gcode: bool,
) -> &'static str {
    if calculation_active {
        "Output: calculating"
    } else if output_stale {
        "Output: stale"
    } else if has_gcode {
        "Output: ready"
    } else {
        "Output: none"
    }
}

fn output_state_color(output_state: &str) -> egui::Color32 {
    match output_state {
        "Output: ready" => egui::Color32::from_rgb(94, 176, 132),
        "Output: stale" | "Output: calculating" => egui::Color32::from_rgb(225, 176, 84),
        _ => egui::Color32::from_rgb(214, 220, 224),
    }
}

fn artifact_summary(
    gcode: &str,
    svg: Option<&str>,
    dxf: Option<&str>,
    cleanup_count: usize,
) -> String {
    let mut artifacts = Vec::new();
    if !gcode.trim().is_empty() {
        artifacts.push("G-code".to_owned());
    }
    if svg
        .map(|payload| !payload.trim().is_empty())
        .unwrap_or(false)
    {
        artifacts.push("SVG".to_owned());
    }
    if dxf
        .map(|payload| !payload.trim().is_empty())
        .unwrap_or(false)
    {
        artifacts.push("DXF".to_owned());
    }
    if cleanup_count > 0 {
        artifacts.push(format!("cleanup x{cleanup_count}"));
    }

    if artifacts.is_empty() {
        "Artifacts: none".to_owned()
    } else {
        format!("Artifacts: {}", artifacts.join(", "))
    }
}

fn warning_count_summary(warnings: &[String]) -> Option<String> {
    match warnings.len() {
        0 => None,
        1 => Some("Warnings: 1".to_owned()),
        count => Some(format!("Warnings: {count}")),
    }
}

fn bitmap_vectorizer_summary(
    is_bitmap: bool,
    backend: BitmapBackend,
    potrace_available: bool,
) -> Option<&'static str> {
    if !is_bitmap {
        return None;
    }

    match backend {
        BitmapBackend::NativePotrace => Some("Vectorizer: Native Potrace"),
        BitmapBackend::PotraceSidecar if potrace_available => {
            Some("Vectorizer: Potrace sidecar ready")
        }
        BitmapBackend::PotraceSidecar => Some("Vectorizer: Potrace sidecar missing"),
    }
}

fn summary_label(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(text).color(color));
}

fn summary_separator(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("/").color(egui::Color32::from_rgb(120, 130, 136)));
}

fn paint_panel_background(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, ui.visuals().panel_fill);
}

fn panel_child_ui(parent: &mut egui::Ui, id: &'static str, rect: egui::Rect) -> egui::Ui {
    let mut child = parent.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect);
    child.expand_to_include_rect(rect);
    child
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy)]
struct DebugLayoutRects {
    root: egui::Rect,
    top: egui::Rect,
    left: egui::Rect,
    preview: egui::Rect,
    bottom: egui::Rect,
}

#[cfg(debug_assertions)]
fn draw_debug_layout_overlay(ctx: &egui::Context, rects: DebugLayoutRects) {
    let painter = ctx.debug_painter();
    debug_layout_rect(
        &painter,
        "root",
        rects.root,
        egui::Color32::from_rgb(220, 220, 220),
    );
    debug_layout_rect(
        &painter,
        "top",
        rects.top,
        egui::Color32::from_rgb(225, 176, 84),
    );
    debug_layout_rect(
        &painter,
        "left",
        rects.left,
        egui::Color32::from_rgb(104, 166, 200),
    );
    debug_layout_rect(
        &painter,
        "preview",
        rects.preview,
        egui::Color32::from_rgb(94, 176, 132),
    );
    debug_layout_rect(
        &painter,
        "bottom",
        rects.bottom,
        egui::Color32::from_rgb(190, 142, 72),
    );

    if let Some(pointer) = ctx.pointer_hover_pos() {
        painter.circle_filled(pointer, 4.0, egui::Color32::from_rgb(240, 96, 96));
        let mut hits = Vec::new();
        for (name, rect) in [
            ("root", rects.root),
            ("top", rects.top),
            ("left", rects.left),
            ("preview", rects.preview),
            ("bottom", rects.bottom),
        ] {
            if rect.contains(pointer) {
                hits.push(name);
            }
        }
        let hit_text = if hits.is_empty() {
            "none".to_owned()
        } else {
            hits.join(", ")
        };
        painter.text(
            rects.root.left_top() + egui::vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            format!("pointer {:.1},{:.1} in: {}", pointer.x, pointer.y, hit_text),
            egui::FontId::monospace(13.0),
            egui::Color32::WHITE,
        );
    }
}

#[cfg(debug_assertions)]
fn debug_layout_rect(painter: &egui::Painter, name: &str, rect: egui::Rect, color: egui::Color32) {
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(2.0, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_top() + egui::vec2(6.0, 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{} x {:.1}-{:.1} y {:.1}-{:.1} {:.1}x{:.1}",
            name,
            rect.left(),
            rect.right(),
            rect.top(),
            rect.bottom(),
            rect.width(),
            rect.height()
        ),
        egui::FontId::monospace(12.0),
        color,
    );
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PathRowAction {
    browse_clicked: bool,
    value_changed: bool,
}

fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> PathRowAction {
    let mut action = PathRowAction::default();
    ui.horizontal(|ui| {
        row_label(ui, label, 88.0);
        right_aligned_group(ui, PATH_CONTROL_WIDTH, |ui| {
            let text_width = (ui.available_width() - 74.0).max(80.0);
            action.value_changed = ui
                .add_sized(
                    [text_width, 22.0],
                    egui::TextEdit::singleline(value).horizontal_align(egui::Align::RIGHT),
                )
                .changed();
            action.browse_clicked = ui.button("Browse").clicked();
        });
    });
    action
}

fn number_row(ui: &mut egui::Ui, label: &str, value: &mut f64, speed: f64) {
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_sized(
                    [FORM_CONTROL_WIDTH, 22.0],
                    egui::DragValue::new(value).speed(speed).max_decimals(4),
                );
            });
        });
    });
}

fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> PathRowAction {
    let mut action = PathRowAction::default();
    ui.horizontal(|ui| {
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            action.value_changed = ui
                .add_sized(
                    [FORM_CONTROL_WIDTH, 22.0],
                    egui::TextEdit::singleline(value).horizontal_align(egui::Align::RIGHT),
                )
                .changed();
        });
    });
    action
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
        row_label(ui, label, 124.0);
        right_aligned_group(ui, FORM_CONTROL_WIDTH, |ui| {
            egui::ComboBox::from_id_salt(label)
                .selected_text(selected_text)
                .width(FORM_CONTROL_WIDTH)
                .show_ui(ui, body);
        });
    });
}

fn right_aligned_group(ui: &mut egui::Ui, width: f32, body: impl FnOnce(&mut egui::Ui)) {
    let spacing = ui.spacing().item_spacing.x;
    let spacer = (ui.available_width() - width - spacing).max(0.0);
    ui.add_space(spacer);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 22.0),
        egui::Layout::left_to_right(egui::Align::Center),
        body,
    );
}

fn row_label(ui: &mut egui::Ui, label: &str, width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 20.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(label);
        },
    );
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
        FileBrowserTarget::SettingsOutput => dialog
            .set_file_name(output_file_name(current_value, target))
            .add_filter("F-Engrave settings", &["ngc", "nc", "tap"])
            .save_file(),
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

fn visible_input_catalog_entries_for_tool(
    entries: &[InputCatalogEntry],
    filter: InputCatalogFilter,
    tool_view: ToolView,
) -> Vec<InputCatalogEntry> {
    entries
        .iter()
        .filter(|entry| filter.accepts(entry.kind) && tool_view.accepts_kind(entry.kind))
        .cloned()
        .collect()
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

fn input_preview_accepts_sample(path: Option<&Path>) -> bool {
    path.and_then(InputCatalogKind::from_path)
        .is_some_and(|kind| matches!(kind, InputCatalogKind::CxfFont | InputCatalogKind::TtfFont))
}

fn input_preview_sample_for_path(
    path: Option<&Path>,
    text: &str,
    preview_sample_text: &str,
) -> Option<String> {
    let path = path?;
    input_preview_accepts_sample(Some(path)).then(|| {
        if preview_sample_text.trim().is_empty() {
            preview_text_sample(text)
        } else {
            preview_text_sample(preview_sample_text)
        }
    })
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
                let preview = preview_segments_for_font(&font, sample_text);
                vector_input_preview("CXF font", preview.segments, preview.missing_chars)
            }
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::TtfFont) => match read_ttf(path, 5.0, false) {
            Ok(font) => {
                let preview = preview_segments_for_font(&font, sample_text);
                vector_input_preview("TTF font", preview.segments, preview.missing_chars)
            }
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::Dxf) => match read_dxf_font(path, 5.0) {
            Ok(font) => {
                let preview = preview_segments_for_font(&font, None);
                vector_input_preview("DXF artwork", preview.segments, Vec::new())
            }
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::Bitmap) => load_bitmap_preview(path),
        None => InputPreviewData::Error("unsupported input type".to_owned()),
    }
}

fn vector_input_preview(
    label: &str,
    segments: Vec<PreviewSegment>,
    missing_chars: Vec<char>,
) -> InputPreviewData {
    let segment_count = segments.len();
    InputPreviewData::Vector {
        label: label.to_owned(),
        bounds: PreviewBounds::from_segments(&segments),
        segments,
        segment_count,
        missing_chars,
    }
}

fn vector_input_preview_readouts(
    segments: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
) -> Vec<String> {
    let mut readouts = Vec::new();
    readouts.push(preview_length_readout("Stroke length", segments));
    if let Some((size, range)) = preview_bounds_readout(bounds) {
        readouts.push(size);
        readouts.push(range);
    } else {
        readouts.push("Extents: none".to_owned());
    }
    readouts
}

#[derive(Debug, Clone, PartialEq)]
struct FontInputPreview {
    segments: Vec<PreviewSegment>,
    missing_chars: Vec<char>,
}

fn preview_segments_for_font(font: &Font, sample_text: Option<&str>) -> FontInputPreview {
    let mut segments = Vec::new();
    let mut missing_chars = Vec::new();
    let mut cursor_x = 0.0;
    let fallback_advance = font.max_x().max(8.0) * 0.65;
    let sample_text = sample_text.unwrap_or("R-Engrave");

    for ch in sample_text.chars() {
        if ch.is_whitespace() {
            cursor_x += fallback_advance;
            continue;
        }
        let Some(glyph) = font.get_char(ch) else {
            if !missing_chars.contains(&ch) {
                missing_chars.push(ch);
            }
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

    FontInputPreview {
        segments,
        missing_chars,
    }
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
            let (mask_thumbnail, trace_stats) = bitmap_trace_mask_thumbnail_and_stats(&image);
            InputPreviewData::Bitmap {
                original_width,
                original_height,
                thumbnail_width: thumbnail.width() as usize,
                thumbnail_height: thumbnail.height() as usize,
                rgba: thumbnail.into_raw(),
                mask_width: mask_thumbnail.width() as usize,
                mask_height: mask_thumbnail.height() as usize,
                mask_rgba: mask_thumbnail.into_raw(),
                trace_stats,
            }
        }
        Err(err) => InputPreviewData::Error(format!("unable to decode bitmap preview: {err}")),
    }
}

fn bitmap_trace_mask_thumbnail_and_stats(
    image: &image::DynamicImage,
) -> (image::RgbaImage, BitmapTraceStats) {
    let (mask, stats) = bitmap_trace_mask_and_stats(image);
    let (width, height) = mask.dimensions();

    if width <= INPUT_PREVIEW_THUMBNAIL_WIDTH && height <= INPUT_PREVIEW_THUMBNAIL_HEIGHT {
        return (mask, stats);
    }

    let scale = (INPUT_PREVIEW_THUMBNAIL_WIDTH as f64 / width as f64)
        .min(INPUT_PREVIEW_THUMBNAIL_HEIGHT as f64 / height as f64)
        .min(1.0);
    let scaled_width = ((width as f64 * scale).round() as u32).max(1);
    let scaled_height = ((height as f64 * scale).round() as u32).max(1);
    (
        image::imageops::resize(
            &mask,
            scaled_width,
            scaled_height,
            image::imageops::FilterType::Nearest,
        ),
        stats,
    )
}

fn bitmap_trace_stats_readout(stats: BitmapTraceStats) -> String {
    let total = stats.black_pixels + stats.white_pixels;
    if total == 0 {
        return "Trace mask: no pixels".to_owned();
    }
    let black_percent = stats.black_pixels as f64 * 100.0 / total as f64;
    format!(
        "Trace mask: {} black / {} white ({black_percent:.1}% black)",
        stats.black_pixels, stats.white_pixels
    )
}

fn image_preview_model_height(path: Option<&Path>, preview: &InputPreviewData) -> Option<f64> {
    if InputCatalogKind::from_path(path?) != Some(InputCatalogKind::Dxf) {
        return None;
    }

    let InputPreviewData::Vector {
        bounds: Some(bounds),
        ..
    } = preview
    else {
        return None;
    };
    let height = (bounds.max.y - bounds.min.y).abs();
    (height.is_finite() && height > f64::EPSILON).then_some(height)
}

fn convert_image_size_yscale(
    current_yscale: f64,
    enable_image_size: bool,
    image_height: Option<f64>,
) -> Option<f64> {
    let image_height = image_height?;
    if !current_yscale.is_finite() || !image_height.is_finite() || image_height <= f64::EPSILON {
        return None;
    }

    Some(if enable_image_size {
        current_yscale * 100.0 / image_height
    } else {
        current_yscale / 100.0 * image_height
    })
}

fn missing_chars_readout(chars: &[char]) -> String {
    let max_visible = 10;
    let mut text = format!(
        "Missing chars: {}",
        chars
            .iter()
            .take(max_visible)
            .map(|ch| ch.escape_default().to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if chars.len() > max_visible {
        text.push_str(&format!(", +{} more", chars.len() - max_visible));
    }
    text
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
            missing_chars,
        } => {
            ui.label(format!("{label} · {segment_count} segments"));
            if !missing_chars.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(225, 176, 84),
                    missing_chars_readout(missing_chars),
                );
            }
            for readout in vector_input_preview_readouts(segments, *bounds) {
                ui.monospace(readout);
            }
            draw_vector_input_preview(ui, segments, *bounds);
        }
        InputPreviewData::Bitmap {
            original_width,
            original_height,
            thumbnail_width,
            thumbnail_height,
            rgba,
            mask_width,
            mask_height,
            mask_rgba,
            trace_stats,
        } => {
            ui.label(format!("Bitmap · {original_width} x {original_height} px"));
            ui.monospace(bitmap_trace_stats_readout(*trace_stats));
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
            if preview.mask_texture.is_none() {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [*mask_width, *mask_height],
                    mask_rgba,
                );
                let texture =
                    ui.ctx()
                        .load_texture("input-trace-mask", image, egui::TextureOptions::NEAREST);
                preview.mask_texture = Some(texture);
            }
            ui.horizontal_wrapped(|ui| {
                if let Some(texture) = &preview.texture {
                    draw_bitmap_preview_texture(
                        ui,
                        "Original",
                        texture,
                        *thumbnail_width,
                        *thumbnail_height,
                    );
                }
                if let Some(texture) = &preview.mask_texture {
                    draw_bitmap_preview_texture(
                        ui,
                        "Trace mask",
                        texture,
                        *mask_width,
                        *mask_height,
                    );
                }
            });
        }
    }
}

fn draw_bitmap_preview_texture(
    ui: &mut egui::Ui,
    label: &str,
    texture: &egui::TextureHandle,
    width: usize,
    height: usize,
) {
    ui.vertical(|ui| {
        ui.label(label);
        let max_width = ui.available_width().clamp(80.0, 140.0);
        let max_height = INPUT_PREVIEW_THUMBNAIL_HEIGHT as f32;
        let image_width = width as f32;
        let image_height = height as f32;
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
    });
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

fn bottom_tab_copy_payload(
    tab: BottomTab,
    warnings: &[String],
    gcode: &str,
    secondary_gcode: &[SecondaryGcode],
    svg: Option<&str>,
    dxf: Option<&str>,
) -> Option<(&'static str, String)> {
    match tab {
        BottomTab::Status => (!warnings.is_empty()).then(|| ("Status log", warnings.join("\n"))),
        BottomTab::Gcode => non_empty_payload("G-code", gcode),
        BottomTab::Cleanup => {
            secondary_output_preview_text(secondary_gcode).map(|payload| ("Cleanup", payload))
        }
        BottomTab::Svg => svg.and_then(|payload| non_empty_payload("SVG", payload)),
        BottomTab::Dxf => dxf.and_then(|payload| non_empty_payload("DXF", payload)),
    }
}

fn non_empty_payload(label: &'static str, payload: &str) -> Option<(&'static str, String)> {
    (!payload.trim().is_empty()).then(|| (label, payload.to_owned()))
}

fn export_payloads_available(
    gcode: &str,
    svg: Option<&str>,
    dxf: Option<&str>,
    secondary_gcode: &[SecondaryGcode],
) -> bool {
    !gcode.is_empty()
        || svg.is_some_and(|svg| !svg.is_empty())
        || dxf.is_some_and(|dxf| !dxf.is_empty())
        || !secondary_gcode.is_empty()
}

fn left_panel_content_width(ui: &egui::Ui) -> f32 {
    ui.available_width()
        .min(INPUT_PANEL_CONTENT_WIDTH)
        .max(80.0)
}

fn draw_vector_input_preview(
    ui: &mut egui::Ui,
    segments: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
) {
    let desired = egui::vec2(left_panel_content_width(ui), INPUT_PREVIEW_VECTOR_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 30, 32));
    let Some(bounds) = bounds else {
        return;
    };

    let Some(transform) = vector_input_preview_transform(rect, bounds) else {
        return;
    };

    let bounds_points = [
        Point::new(bounds.min.x, bounds.min.y),
        Point::new(bounds.max.x, bounds.min.y),
        Point::new(bounds.max.x, bounds.max.y),
        Point::new(bounds.min.x, bounds.max.y),
        Point::new(bounds.min.x, bounds.min.y),
    ];
    for pair in bounds_points.windows(2) {
        painter.line_segment(
            [transform.to_screen(pair[0]), transform.to_screen(pair[1])],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(72, 82, 88)),
        );
    }

    for axis in vector_input_preview_axis_segments(bounds) {
        painter.line_segment(
            [
                transform.to_screen(axis.start),
                transform.to_screen(axis.end),
            ],
            egui::Stroke::new(0.8, egui::Color32::from_rgb(80, 105, 118)),
        );
    }

    for segment in segments {
        painter.line_segment(
            [
                transform.to_screen(segment.start),
                transform.to_screen(segment.end),
            ],
            egui::Stroke::new(1.2, egui::Color32::from_rgb(94, 176, 132)),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VectorInputPreviewTransform {
    bounds: PreviewBounds,
    scale: f32,
    origin: egui::Pos2,
}

impl VectorInputPreviewTransform {
    fn to_screen(self, point: Point) -> egui::Pos2 {
        egui::pos2(
            self.origin.x + ((point.x - self.bounds.min.x) as f32) * self.scale,
            self.origin.y - ((point.y - self.bounds.min.y) as f32) * self.scale,
        )
    }
}

fn vector_input_preview_transform(
    rect: egui::Rect,
    bounds: PreviewBounds,
) -> Option<VectorInputPreviewTransform> {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return None;
    }
    let width = (bounds.max.x - bounds.min.x).abs().max(0.001) as f32;
    let height = (bounds.max.y - bounds.min.y).abs().max(0.001) as f32;
    let scale = ((rect.width() - 16.0).max(1.0) / width)
        .min((rect.height() - 16.0).max(1.0) / height)
        .max(0.001);
    let preview_width = width * scale;
    let preview_height = height * scale;
    let origin = egui::pos2(
        rect.center().x - preview_width / 2.0,
        rect.center().y + preview_height / 2.0,
    );
    Some(VectorInputPreviewTransform {
        bounds,
        scale,
        origin,
    })
}

fn vector_input_preview_axis_segments(bounds: PreviewBounds) -> Vec<PreviewSegment> {
    let mut axes = Vec::new();
    if bounds.min.y <= 0.0 && bounds.max.y >= 0.0 {
        axes.push(PreviewSegment {
            start: Point::new(bounds.min.x, 0.0),
            end: Point::new(bounds.max.x, 0.0),
        });
    }
    if bounds.min.x <= 0.0 && bounds.max.x >= 0.0 {
        axes.push(PreviewSegment {
            start: Point::new(0.0, bounds.min.y),
            end: Point::new(0.0, bounds.max.y),
        });
    }
    axes
}

#[derive(Debug, Clone, PartialEq)]
struct UiPreferences {
    settings_path: String,
    input_path: String,
    default_dir_path: String,
    gcode_path: String,
    svg_path: String,
    dxf_path: String,
    show_rapids: bool,
    show_grid: bool,
    show_cleanup: bool,
    viewport_rotation_degrees: f64,
    preview_sample_text: String,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            settings_path: String::new(),
            input_path: String::new(),
            default_dir_path: String::new(),
            gcode_path: String::new(),
            svg_path: String::new(),
            dxf_path: String::new(),
            show_rapids: true,
            show_grid: true,
            show_cleanup: true,
            viewport_rotation_degrees: 0.0,
            preview_sample_text: String::new(),
        }
    }
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
                "show_rapids" => preferences.show_rapids = value != "0" && value != "false",
                "show_grid" => preferences.show_grid = value != "0" && value != "false",
                "show_cleanup" => preferences.show_cleanup = value != "0" && value != "false",
                "viewport_rotation_degrees" => {
                    if let Ok(rotation) = value.parse::<f64>() {
                        preferences.viewport_rotation_degrees = rotation.clamp(-180.0, 180.0);
                    }
                }
                "preview_sample_text" => preferences.preview_sample_text = value,
                _ => {}
            }
        }
        preferences
    }

    fn to_text(&self) -> String {
        let viewport_rotation_degrees = format_setting_number(self.viewport_rotation_degrees);
        [
            ("settings_path", self.settings_path.as_str()),
            ("input_path", self.input_path.as_str()),
            ("default_dir_path", self.default_dir_path.as_str()),
            ("gcode_path", self.gcode_path.as_str()),
            ("svg_path", self.svg_path.as_str()),
            ("dxf_path", self.dxf_path.as_str()),
            ("show_rapids", if self.show_rapids { "1" } else { "0" }),
            ("show_grid", if self.show_grid { "1" } else { "0" }),
            ("show_cleanup", if self.show_cleanup { "1" } else { "0" }),
            (
                "viewport_rotation_degrees",
                viewport_rotation_degrees.as_str(),
            ),
            ("preview_sample_text", self.preview_sample_text.as_str()),
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

    fn from_segment_layers(layers: &[&[PreviewSegment]]) -> Option<Self> {
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

fn preview_bounds_readout(bounds: Option<PreviewBounds>) -> Option<(String, String)> {
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

fn preview_length_readout(label: &str, segments: &[PreviewSegment]) -> String {
    format!(
        "{label}: {}",
        format_preview_coord(total_segment_length(segments))
    )
}

fn total_segment_length(segments: &[PreviewSegment]) -> f64 {
    segments
        .iter()
        .map(|segment| {
            let dx = segment.end.x - segment.start.x;
            let dy = segment.end.y - segment.start.y;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

fn format_preview_coord(value: f64) -> String {
    let value = if value.abs() < 0.00005 { 0.0 } else { value };
    format!("{value:.4}")
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

fn zoom_transform_at_screen_point(
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

fn screen_point_to_model(rect: egui::Rect, transform: ViewTransform, screen: egui::Pos2) -> Point {
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

fn draw_preview_cursor_readout(painter: &egui::Painter, rect: egui::Rect, cursor: Point) {
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

fn cleanup_preview_segments(outputs: &[SecondaryGcode]) -> Vec<PreviewSegment> {
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

fn draw_preview(
    painter: &egui::Painter,
    rect: egui::Rect,
    transform: ViewTransform,
    unit_label: &str,
    segments: &[PreviewSegment],
    rapids: &[PreviewSegment],
    cleanup_segments: &[PreviewSegment],
    bounds: Option<PreviewBounds>,
    show_toolpath: bool,
    show_rapids: bool,
    show_cleanup: bool,
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
struct PreviewScaleBar {
    model_length: f64,
    pixel_length: f32,
}

fn preview_scale_bar(zoom: f64) -> Option<PreviewScaleBar> {
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

fn format_scale_bar_label(length: f64, unit_label: &str) -> String {
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
struct PreviewOverlayItem {
    text: String,
    color: egui::Color32,
    swatch: bool,
}

fn preview_overlay_items(
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

fn nice_grid_step(zoom: f64) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

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
    fn preview_bounds_include_cut_rapid_and_cleanup_layers() {
        let cuts = vec![PreviewSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        }];
        let rapids = vec![PreviewSegment {
            start: Point::new(-2.0, 3.0),
            end: Point::new(4.0, -1.0),
        }];
        let cleanup = vec![PreviewSegment {
            start: Point::new(8.0, 2.0),
            end: Point::new(9.0, 5.0),
        }];

        let bounds = PreviewBounds::from_segment_layers(&[&cuts, &rapids, &cleanup]).unwrap();

        assert_eq!(bounds.min, Point::new(-2.0, -1.0));
        assert_eq!(bounds.max, Point::new(9.0, 5.0));
    }

    #[test]
    fn preview_overlay_items_summarize_visible_layers_and_ranges() {
        let cuts = vec![PreviewSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        }];
        let rapids = vec![PreviewSegment {
            start: Point::new(1.0, 1.0),
            end: Point::new(2.0, 2.0),
        }];
        let cleanup = vec![
            PreviewSegment {
                start: Point::new(-1.0, 0.0),
                end: Point::new(-1.0, 1.0),
            },
            PreviewSegment {
                start: Point::new(-2.0, 0.0),
                end: Point::new(-2.0, 1.0),
            },
        ];
        let bounds = PreviewBounds {
            min: Point::new(-2.0, -0.0000001),
            max: Point::new(2.0, 3.25),
        };

        let items = preview_overlay_items(&cuts, &rapids, &cleanup, Some(bounds), true, true, true);

        assert_eq!(
            items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Cut 1",
                "Rapid 1",
                "Cleanup 2",
                "X -2.0000..2.0000",
                "Y 0.0000..3.2500"
            ]
        );
        assert!(items[0].swatch);
        assert!(!items[3].swatch);
    }

    #[test]
    fn preview_overlay_items_skip_hidden_layers() {
        let cuts = vec![PreviewSegment {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        }];
        let cleanup = vec![PreviewSegment {
            start: Point::new(-1.0, 0.0),
            end: Point::new(-1.0, 1.0),
        }];

        let items = preview_overlay_items(&cuts, &[], &cleanup, None, false, true, true);

        assert_eq!(
            items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            vec!["Cleanup 1"]
        );
    }

    #[test]
    fn preview_scale_bar_uses_readable_model_lengths() {
        let normal = preview_scale_bar(80.0).unwrap();
        assert_eq!(normal.model_length, 1.0);
        assert_eq!(normal.pixel_length, 80.0);

        let zoomed_out = preview_scale_bar(20.0).unwrap();
        assert_eq!(zoomed_out.model_length, 5.0);
        assert_eq!(zoomed_out.pixel_length, 100.0);

        let zoomed_in = preview_scale_bar(500.0).unwrap();
        assert_eq!(zoomed_in.model_length, 0.2);
        assert_eq!(zoomed_in.pixel_length, 100.0);

        assert_eq!(preview_scale_bar(0.0), None);
    }

    #[test]
    fn preview_scale_bar_label_uses_active_units_without_noise() {
        assert_eq!(format_scale_bar_label(100.0, "mm"), "100 mm");
        assert_eq!(format_scale_bar_label(10.0, "mm"), "10 mm");
        assert_eq!(format_scale_bar_label(1.0, "in"), "1 in");
        assert_eq!(format_scale_bar_label(0.125, "in"), "0.125 in");
    }

    #[test]
    fn preview_bounds_readout_formats_extents_and_ranges() {
        let bounds = PreviewBounds {
            min: Point::new(-1.25, -0.0000001),
            max: Point::new(2.5, 4.125),
        };

        let (size, range) = preview_bounds_readout(Some(bounds)).unwrap();

        assert_eq!(size, "Extents: 3.7500 x 4.1250");
        assert_eq!(range, "X -1.2500..2.5000  Y 0.0000..4.1250");
        assert_eq!(preview_bounds_readout(None), None);
    }

    #[test]
    fn preview_length_readout_sums_segment_lengths() {
        let segments = vec![
            PreviewSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(3.0, 4.0),
            },
            PreviewSegment {
                start: Point::new(3.0, 4.0),
                end: Point::new(3.0, 5.25),
            },
        ];

        assert_eq!(total_segment_length(&segments), 6.25);
        assert_eq!(
            preview_length_readout("Cut length", &segments),
            "Cut length: 6.2500"
        );
        assert_eq!(
            preview_length_readout("Rapid length", &[]),
            "Rapid length: 0.0000"
        );
    }

    #[test]
    fn preview_grid_step_uses_readable_zoom_scaled_spacing() {
        assert_eq!(nice_grid_step(80.0), 1.0);
        assert_eq!(nice_grid_step(8.0), 10.0);
        assert_eq!(nice_grid_step(320.0), 0.2);
        assert_eq!(nice_grid_step(0.0), 1.0);
    }

    #[test]
    fn vector_input_preview_readouts_include_length_and_bounds() {
        let segments = vec![PreviewSegment {
            start: Point::new(-1.0, 0.0),
            end: Point::new(2.0, 4.0),
        }];
        let bounds = PreviewBounds::from_segments(&segments);

        assert_eq!(
            vector_input_preview_readouts(&segments, bounds),
            vec![
                "Stroke length: 5.0000".to_owned(),
                "Extents: 3.0000 x 4.0000".to_owned(),
                "X -1.0000..2.0000  Y 0.0000..4.0000".to_owned(),
            ]
        );
        assert_eq!(
            vector_input_preview_readouts(&[], None),
            vec![
                "Stroke length: 0.0000".to_owned(),
                "Extents: none".to_owned(),
            ]
        );
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
    fn smoke_layout_keeps_preview_available_at_target_sizes_and_rotations() {
        let bounds = PreviewBounds {
            min: Point::new(-2.0, -1.0),
            max: Point::new(8.0, 4.0),
        };

        for size in [egui::vec2(1280.0, 800.0), egui::vec2(1920.0, 1080.0)] {
            let layout = smoke_layout_for_viewport(size);
            assert_smoke_layout_valid(layout);
            assert!(layout.preview.width() >= 600.0);
            assert!(layout.preview.height() >= 500.0);

            for rotation in [0.0, 45.0, 90.0] {
                let mut transform = ViewTransform {
                    viewport_rotation_degrees: rotation,
                    ..ViewTransform::default()
                };
                fit_transform_to_bounds(&mut transform, Some(bounds), layout.preview);
                assert_fitted_corners_inside(layout.preview, transform, bounds);
            }
        }
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
    fn cursor_zoom_preserves_anchor_model_point() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        let model_point = Point::new(1.5, -0.25);
        let mut transform = ViewTransform {
            zoom: 80.0,
            model_rotation_degrees: 30.0,
            viewport_rotation_degrees: 15.0,
            ..ViewTransform::default()
        };
        let anchor = preview_screen_point(rect, transform, model_point);

        zoom_transform_at_screen_point(&mut transform, rect, anchor, 2.0);

        assert_eq!(transform.zoom, 160.0);
        assert_pos_close(preview_screen_point(rect, transform, model_point), anchor);
    }

    #[test]
    fn cursor_zoom_clamps_to_preview_zoom_limits() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        let anchor = rect.center();
        let mut transform = ViewTransform {
            zoom: 490.0,
            ..ViewTransform::default()
        };

        zoom_transform_at_screen_point(&mut transform, rect, anchor, 10.0);
        assert_eq!(transform.zoom, 500.0);

        zoom_transform_at_screen_point(&mut transform, rect, anchor, 0.001);
        assert_eq!(transform.zoom, 1.0);
    }

    #[test]
    fn screen_point_to_model_inverts_preview_transform() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(400.0, 300.0));
        let model_point = Point::new(3.25, -1.75);
        let transform = ViewTransform {
            pan: Point::new(17.0, -9.0),
            zoom: 73.0,
            model_rotation_degrees: -20.0,
            viewport_rotation_degrees: 65.0,
        };
        let screen = preview_screen_point(rect, transform, model_point);

        let actual = screen_point_to_model(rect, transform, screen);

        assert!((actual.x - model_point.x).abs() < 1e-5);
        assert!((actual.y - model_point.y).abs() < 1e-5);
    }

    #[test]
    fn default_output_paths_use_default_dir_when_present() {
        let dir = Some(PathBuf::from("/tmp/rengrave-ui"));

        assert_eq!(
            default_output_path(&dir, "rengrave_output.ngc"),
            "/tmp/rengrave-ui/rengrave_output.ngc"
        );
        assert_eq!(
            default_output_paths(&dir),
            (
                "/tmp/rengrave-ui/rengrave_output.ngc".to_owned(),
                "/tmp/rengrave-ui/rengrave_output.svg".to_owned(),
                "/tmp/rengrave-ui/rengrave_output.dxf".to_owned()
            )
        );
        assert_eq!(
            default_output_path(&None, "rengrave_output.ngc"),
            "rengrave_output.ngc"
        );
        assert_eq!(
            default_output_paths(&None),
            (
                "rengrave_output.ngc".to_owned(),
                "rengrave_output.svg".to_owned(),
                "rengrave_output.dxf".to_owned()
            )
        );
    }

    #[test]
    fn calculation_phases_have_user_visible_status_text() {
        assert_eq!(CalculationPhase::Queued.status_text(), "Calculation queued");
        assert_eq!(
            CalculationPhase::Batch(BatchProgress::LoadingDocument).status_text(),
            "Loading document"
        );
        assert_eq!(
            CalculationPhase::Batch(BatchProgress::CalculatingVCarve).status_text(),
            "Calculating V-carve"
        );
        assert_eq!(
            CalculationPhase::Finalizing.status_text(),
            "Finalizing output"
        );
    }

    #[test]
    fn launch_font_or_image_ignores_preferences_when_settings_arg_is_explicit() {
        let preferences = UiPreferences {
            input_path: "/tmp/remembered.cxf".to_owned(),
            ..UiPreferences::default()
        };

        assert_eq!(
            launch_font_or_image_path(
                &UiLaunchOptions {
                    gcode_file: Some(PathBuf::from("/tmp/job.ngc")),
                    ..UiLaunchOptions::default()
                },
                &preferences
            ),
            None
        );
        assert_eq!(
            launch_font_or_image_path(&UiLaunchOptions::default(), &preferences),
            Some(PathBuf::from("/tmp/remembered.cxf"))
        );
        assert_eq!(
            launch_font_or_image_path(
                &UiLaunchOptions {
                    gcode_file: Some(PathBuf::from("/tmp/job.ngc")),
                    font_or_image: Some(PathBuf::from("/tmp/explicit.ttf")),
                    ..UiLaunchOptions::default()
                },
                &preferences
            ),
            Some(PathBuf::from("/tmp/explicit.ttf"))
        );
    }

    #[test]
    fn launch_font_or_image_uses_demo_font_for_unconfigured_first_run() {
        let path =
            launch_font_or_image_path(&UiLaunchOptions::default(), &UiPreferences::default())
                .unwrap();

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("rengrave_demo.cxf")
        );
        assert!(path.is_file());
    }

    #[test]
    fn launch_font_or_image_allows_remembered_settings_to_define_input() {
        let preferences = UiPreferences {
            settings_path: "/tmp/job.ngc".to_owned(),
            ..UiPreferences::default()
        };

        assert_eq!(
            launch_font_or_image_path(&UiLaunchOptions::default(), &preferences),
            None
        );
    }

    #[test]
    fn bundled_demo_font_generates_default_text_gcode() {
        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: bundled_demo_font_path(),
            ..BatchRequest::default()
        })
        .unwrap();

        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("( Code generated by r-engrave-"));
        assert!(output.gcode.contains("G1 X"));
        assert!(!output.gcode.contains("settings-only output"));
    }

    #[test]
    fn document_input_path_for_display_prefers_resolved_document_path() {
        let requested = Some(PathBuf::from("/tmp/old-input.cxf"));
        let mut document = RengraveDocument::default();
        document.input_path = Some(PathBuf::from("/tmp/settings/romanc.cxf"));

        assert_eq!(
            document_input_path_for_display(&requested, &document),
            Some(PathBuf::from("/tmp/settings/romanc.cxf"))
        );

        document.input_path = None;
        assert_eq!(
            document_input_path_for_display(&requested, &document),
            requested
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
        assert!(calculation_stale_reasons(&current, &expected).is_empty());
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
        assert_eq!(
            calculation_stale_reasons(&text_changed, &expected),
            vec!["text"]
        );
        assert_eq!(
            calculation_stale_reasons(&settings_changed, &expected),
            vec!["controls"]
        );
        assert_eq!(
            calculation_stale_reasons(&export_toggle_changed, &expected),
            vec!["export set"]
        );
        assert_eq!(
            calculation_stale_reasons(&secondary_toggle_changed, &expected),
            vec!["cleanup"]
        );
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
        assert_eq!(
            output_request_stale_reasons(&current, Some(&expected), true),
            vec!["text"]
        );
        assert!(!output_request_is_stale(
            &output_path_changed,
            Some(&expected),
            true
        ));
        assert!(
            output_request_stale_reasons(&output_path_changed, Some(&expected), true).is_empty()
        );
        assert!(!output_request_is_stale(&current, Some(&expected), false));
        assert!(!output_request_is_stale(&current, None, true));
    }

    #[test]
    fn stale_reason_summary_limits_long_reason_lists() {
        assert_eq!(
            stale_reason_summary("Output stale", &["text", "controls"]),
            "Output stale: text, controls"
        );
        assert_eq!(
            stale_reason_summary(
                "Changed",
                &[
                    "settings file",
                    "input file",
                    "default dir",
                    "text",
                    "controls"
                ],
            ),
            "Changed: settings file, input file, default dir, +2 more"
        );
    }

    #[test]
    fn stale_recalculate_available_only_when_output_is_stale_and_idle() {
        assert!(stale_recalculate_available(true, false));
        assert!(!stale_recalculate_available(true, true));
        assert!(!stale_recalculate_available(false, false));
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
    fn input_path_is_bitmap_only_for_bitmap_files() {
        assert!(input_path_is_bitmap(" /tmp/image.png "));
        assert!(input_path_is_bitmap("/tmp/image.PBM"));
        assert!(!input_path_is_bitmap("/tmp/shape.dxf"));
        assert!(!input_path_is_bitmap("  "));
    }

    #[test]
    fn native_potrace_startup_does_not_require_sidecar_probe() {
        let status = initial_potrace_status(BitmapBackend::NativePotrace);

        assert!(!status.available);
        assert_eq!(status.message, "Potrace status not checked");
    }

    #[test]
    fn vcarve_multipass_summary_matches_f_engrave_control_policy() {
        assert!(!vcarve_multipass_enabled(0.0));
        assert!(!vcarve_multipass_enabled(-0.01));
        assert!(vcarve_multipass_enabled(0.01));
        assert_eq!(
            vcarve_multipass_summary(0.0, -0.1),
            "Multipass disabled: finish stock is zero"
        );
        assert_eq!(
            vcarve_multipass_summary(0.05, 0.1),
            "Multipass configured but max depth/pass should be negative"
        );
        assert_eq!(
            vcarve_multipass_summary(0.05, -0.1),
            "Multipass enabled: leave 0.05 finish stock, max -0.1 per pass"
        );
    }

    #[test]
    fn job_summary_helpers_format_visible_state() {
        let mut controls = UiControls::from_settings(&LegacySettings::default());
        controls.cut_type = CutTypeChoice::VCarve;
        controls.bit_shape = BitShapeChoice::Ball;
        controls.units = UnitsChoice::Mm;

        assert_eq!(
            input_source_summary(" /tmp/fonts/romanc.cxf "),
            "Source: CXF romanc.cxf"
        );
        assert_eq!(
            input_source_summary("/tmp/artwork.dxf"),
            "Source: DXF artwork.dxf"
        );
        assert_eq!(input_source_summary("  "), "Source: none");
        assert_eq!(tool_summary(&controls), "Job: V-carve, Ball, mm");
        assert_eq!(
            output_state_summary(true, false, true),
            "Output: calculating"
        );
        assert_eq!(output_state_summary(false, true, true), "Output: stale");
        assert_eq!(output_state_summary(false, false, true), "Output: ready");
        assert_eq!(output_state_summary(false, false, false), "Output: none");
    }

    #[test]
    fn tool_views_map_to_cut_types_and_input_families() {
        assert_eq!(ToolView::TextEngrave.cut_type(), CutTypeChoice::Engrave);
        assert_eq!(ToolView::ImageEngrave.cut_type(), CutTypeChoice::Engrave);
        assert_eq!(ToolView::TextVCarve.cut_type(), CutTypeChoice::VCarve);
        assert_eq!(ToolView::ImageVCarve.cut_type(), CutTypeChoice::VCarve);
        assert!(ToolView::TextVCarve.accepts_kind(InputCatalogKind::CxfFont));
        assert!(!ToolView::TextVCarve.accepts_kind(InputCatalogKind::Bitmap));
        assert!(ToolView::ImageVCarve.accepts_kind(InputCatalogKind::Dxf));
        assert!(ToolView::ImageVCarve.accepts_kind(InputCatalogKind::Bitmap));
        assert_eq!(
            ToolView::TextVCarve.with_input_kind(InputCatalogKind::Bitmap),
            ToolView::ImageVCarve
        );
        assert_eq!(
            ToolView::ImageEngrave.with_input_kind(InputCatalogKind::TtfFont),
            ToolView::TextEngrave
        );
    }

    #[test]
    fn tool_view_infers_from_settings_and_input_path() {
        let mut settings = LegacySettings::default();
        assert_eq!(
            ToolView::from_settings_and_path(&settings, Some(Path::new("/tmp/font.cxf"))),
            ToolView::TextEngrave
        );
        assert_eq!(
            ToolView::from_settings_and_path(&settings, Some(Path::new("/tmp/image.png"))),
            ToolView::ImageEngrave
        );
        settings.set_or_push("cut_type", "v-carve", false);
        assert_eq!(
            ToolView::from_settings_and_path(&settings, Some(Path::new("/tmp/font.ttf"))),
            ToolView::TextVCarve
        );
        assert_eq!(
            ToolView::from_settings_and_path(&settings, Some(Path::new("/tmp/shape.dxf"))),
            ToolView::ImageVCarve
        );
    }

    #[test]
    fn artifact_and_runtime_summaries_track_export_readiness() {
        assert_eq!(
            artifact_summary("G90\n", Some("<svg/>"), Some("0\nEOF\n"), 2),
            "Artifacts: G-code, SVG, DXF, cleanup x2"
        );
        assert_eq!(artifact_summary(" ", Some(""), None, 0), "Artifacts: none");
        assert_eq!(warning_count_summary(&[]), None);
        assert_eq!(
            warning_count_summary(&["missing potrace".to_owned()]),
            Some("Warnings: 1".to_owned())
        );
        assert_eq!(
            bitmap_vectorizer_summary(false, BitmapBackend::NativePotrace, false),
            None
        );
        assert_eq!(
            bitmap_vectorizer_summary(true, BitmapBackend::NativePotrace, false),
            Some("Vectorizer: Native Potrace")
        );
        assert_eq!(
            bitmap_vectorizer_summary(true, BitmapBackend::PotraceSidecar, false),
            Some("Vectorizer: Potrace sidecar missing")
        );
        assert_eq!(
            bitmap_vectorizer_summary(true, BitmapBackend::PotraceSidecar, true),
            Some("Vectorizer: Potrace sidecar ready")
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
    fn output_file_name_uses_current_name_or_target_default() {
        assert_eq!(
            output_file_name("/tmp/custom.tap", FileBrowserTarget::GcodeOutput),
            "custom.tap"
        );
        assert_eq!(
            output_file_name("  ", FileBrowserTarget::SettingsOutput),
            "rengrave_settings.ngc"
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
    fn browser_selection_followup_matches_user_workflow() {
        assert_eq!(
            selection_followup(FileBrowserTarget::Settings),
            SelectionFollowup::LoadDocument
        );
        assert_eq!(
            selection_followup(FileBrowserTarget::SettingsOutput),
            SelectionFollowup::SaveSettings
        );
        assert_eq!(
            selection_followup(FileBrowserTarget::Input),
            SelectionFollowup::StartCalculation
        );
        assert_eq!(
            selection_followup(FileBrowserTarget::DefaultDir),
            SelectionFollowup::None
        );
        assert_eq!(
            selection_followup(FileBrowserTarget::GcodeOutput),
            SelectionFollowup::None
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
    fn input_catalog_filters_entries_by_workflow() {
        let entries = vec![
            InputCatalogEntry {
                path: PathBuf::from("/tmp/a.cxf"),
                name: "a.cxf".to_owned(),
                kind: InputCatalogKind::CxfFont,
                size_bytes: 10,
            },
            InputCatalogEntry {
                path: PathBuf::from("/tmp/b.ttf"),
                name: "b.ttf".to_owned(),
                kind: InputCatalogKind::TtfFont,
                size_bytes: 20,
            },
            InputCatalogEntry {
                path: PathBuf::from("/tmp/c.dxf"),
                name: "c.dxf".to_owned(),
                kind: InputCatalogKind::Dxf,
                size_bytes: 30,
            },
            InputCatalogEntry {
                path: PathBuf::from("/tmp/d.png"),
                name: "d.png".to_owned(),
                kind: InputCatalogKind::Bitmap,
                size_bytes: 40,
            },
        ];

        let filter = InputCatalogFilter {
            cxf: false,
            ttf: true,
            dxf: false,
            bitmap: true,
        };
        let text_view_entries = visible_input_catalog_entries_for_tool(
            &entries,
            InputCatalogFilter::default(),
            ToolView::TextVCarve,
        );
        assert_eq!(
            text_view_entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a.cxf", "b.ttf"]
        );
        let image_view_entries = visible_input_catalog_entries_for_tool(
            &entries,
            InputCatalogFilter::default(),
            ToolView::ImageVCarve,
        );
        assert_eq!(
            image_view_entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["c.dxf", "d.png"]
        );
        let filtered_entries =
            visible_input_catalog_entries_for_tool(&entries, filter, ToolView::ImageVCarve);
        assert_eq!(
            filtered_entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["d.png"]
        );
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
    fn input_preview_sample_override_applies_only_to_fonts() {
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/font.cxf")), "Generated", "Custom"),
            Some("Custom".to_owned())
        );
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/font.ttf")), "Generated", "  "),
            Some("Generated".to_owned())
        );
        assert_eq!(
            input_preview_sample_for_path(
                Some(Path::new("/tmp/artwork.dxf")),
                "Generated",
                "Custom"
            ),
            None
        );
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/image.png")), "Generated", "Custom"),
            None
        );
    }

    #[test]
    fn input_preview_reports_missing_font_sample_chars() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-ui-cxf-preview-missing-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("font.cxf");
        fs::write(&path, "[A] 1\nL 0,0,1,0\n").unwrap();

        let preview = load_input_preview_data(&path, Some("ABBA"));

        let _ = fs::remove_dir_all(dir);
        match preview {
            InputPreviewData::Vector { missing_chars, .. } => {
                assert_eq!(missing_chars, vec!['B']);
                assert_eq!(missing_chars_readout(&missing_chars), "Missing chars: B");
            }
            other => panic!("unexpected preview: {other:?}"),
        }
    }

    #[test]
    fn missing_chars_readout_limits_long_lists() {
        let chars = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k'];

        assert_eq!(
            missing_chars_readout(&chars),
            "Missing chars: a b c d e f g h i j, +1 more"
        );
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
    fn vector_input_preview_transform_fits_bounds_with_padding() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(220.0, 120.0));
        let bounds = PreviewBounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 10.0),
        };

        let transform = vector_input_preview_transform(rect, bounds).unwrap();

        assert!((transform.scale - 10.4).abs() < 0.0001);
        let min = transform.to_screen(bounds.min);
        let max = transform.to_screen(bounds.max);
        assert!((min.x - 58.0).abs() < 0.0001);
        assert!((min.y - 112.0).abs() < 0.0001);
        assert!((max.x - 162.0).abs() < 0.0001);
        assert!((max.y - 8.0).abs() < 0.0001);
    }

    #[test]
    fn vector_input_preview_axes_only_draw_when_origin_crosses_bounds() {
        let axes = vector_input_preview_axis_segments(PreviewBounds {
            min: Point::new(-2.0, -1.0),
            max: Point::new(3.0, 4.0),
        });

        assert_eq!(
            axes,
            vec![
                PreviewSegment {
                    start: Point::new(-2.0, 0.0),
                    end: Point::new(3.0, 0.0),
                },
                PreviewSegment {
                    start: Point::new(0.0, -1.0),
                    end: Point::new(0.0, 4.0),
                },
            ]
        );
        assert_eq!(
            vector_input_preview_axis_segments(PreviewBounds {
                min: Point::new(1.0, 2.0),
                max: Point::new(3.0, 4.0),
            }),
            Vec::<PreviewSegment>::new()
        );
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
                mask_width,
                mask_height,
                mask_rgba,
                trace_stats,
            } => {
                assert_eq!(original_width, 4);
                assert_eq!(original_height, 2);
                assert!(thumbnail_width <= INPUT_PREVIEW_THUMBNAIL_WIDTH as usize);
                assert!(thumbnail_height <= INPUT_PREVIEW_THUMBNAIL_HEIGHT as usize);
                assert_eq!(rgba.len(), thumbnail_width * thumbnail_height * 4);
                assert!(mask_width <= INPUT_PREVIEW_THUMBNAIL_WIDTH as usize);
                assert!(mask_height <= INPUT_PREVIEW_THUMBNAIL_HEIGHT as usize);
                assert_eq!(mask_rgba.len(), mask_width * mask_height * 4);
                assert_eq!(
                    trace_stats,
                    BitmapTraceStats {
                        black_pixels: 8,
                        white_pixels: 0
                    }
                );
            }
            other => panic!("unexpected preview: {other:?}"),
        }
    }

    #[test]
    fn bitmap_trace_mask_preview_thresholds_like_potrace_input() {
        let mut image = image::RgbaImage::new(3, 1);
        image.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(2, 0, image::Rgba([0, 0, 0, 0]));

        let (mask, _) =
            bitmap_trace_mask_thumbnail_and_stats(&image::DynamicImage::ImageRgba8(image));
        let rgba = mask.into_raw();

        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[255, 255, 255, 255]);
        assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
    }

    #[test]
    fn bitmap_trace_mask_stats_count_full_size_pixels() {
        let mut image = image::RgbaImage::new(3, 1);
        image.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        image.put_pixel(2, 0, image::Rgba([0, 0, 0, 0]));

        let (_, stats) =
            bitmap_trace_mask_thumbnail_and_stats(&image::DynamicImage::ImageRgba8(image));

        assert_eq!(
            stats,
            BitmapTraceStats {
                black_pixels: 1,
                white_pixels: 2
            }
        );
        assert_eq!(
            bitmap_trace_stats_readout(stats),
            "Trace mask: 1 black / 2 white (33.3% black)"
        );
        assert_eq!(
            bitmap_trace_stats_readout(BitmapTraceStats::default()),
            "Trace mask: no pixels"
        );
    }

    #[test]
    fn image_size_toggle_converts_yscale_like_f_engrave() {
        let image_height = Some(10.0);

        let percent_height = convert_image_size_yscale(2.5, true, image_height).unwrap();
        assert!((percent_height - 25.0).abs() < 1e-9);

        let absolute_height =
            convert_image_size_yscale(percent_height, false, image_height).unwrap();
        assert!((absolute_height - 2.5).abs() < 1e-9);

        assert_eq!(convert_image_size_yscale(2.5, true, None), None);
        assert_eq!(convert_image_size_yscale(2.5, true, Some(0.0)), None);
    }

    #[test]
    fn image_size_height_uses_dxf_preview_bounds_only() {
        let preview = InputPreviewData::Vector {
            label: "DXF artwork".to_owned(),
            segments: Vec::new(),
            bounds: Some(PreviewBounds {
                min: Point::new(-1.0, -2.0),
                max: Point::new(3.0, 8.0),
            }),
            segment_count: 0,
            missing_chars: Vec::new(),
        };

        assert_eq!(
            image_preview_model_height(Some(Path::new("part.dxf")), &preview),
            Some(10.0)
        );
        assert_eq!(
            image_preview_model_height(Some(Path::new("font.cxf")), &preview),
            None
        );
        assert_eq!(image_preview_model_height(None, &preview), None);
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
    fn cleanup_preview_segments_parse_secondary_cut_moves() {
        let segments = cleanup_preview_segments(&[
            SecondaryGcode {
                suffix: "clean".to_owned(),
                gcode: "G0 X0 Y0\nG1 X1 Y0\n".to_owned(),
            },
            SecondaryGcode {
                suffix: "v_clean".to_owned(),
                gcode: "G0 X2 Y2\nG1 X2 Y3\n".to_owned(),
            },
        ]);

        assert_eq!(
            segments,
            vec![
                PreviewSegment {
                    start: Point::new(0.0, 0.0),
                    end: Point::new(1.0, 0.0),
                },
                PreviewSegment {
                    start: Point::new(2.0, 2.0),
                    end: Point::new(2.0, 3.0),
                },
            ]
        );
        assert!(cleanup_preview_segments(&[]).is_empty());
    }

    #[test]
    fn bottom_tab_copy_payload_tracks_visible_output_tabs() {
        let warnings = vec!["missing potrace".to_owned(), "fallback output".to_owned()];
        let cleanup = vec![SecondaryGcode {
            suffix: "clean".to_owned(),
            gcode: "G90\nG1 X0\n".to_owned(),
        }];

        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Status, &warnings, "", &[], None, None),
            Some(("Status log", "missing potrace\nfallback output".to_owned()))
        );
        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Gcode, &[], "G90\n", &[], None, None),
            Some(("G-code", "G90\n".to_owned()))
        );
        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Cleanup, &[], "", &cleanup, None, None),
            Some((
                "Cleanup",
                "( cleanup output: _clean )\nG90\nG1 X0\n".to_owned()
            ))
        );
        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Svg, &[], "", &[], Some("<svg/>"), None),
            Some(("SVG", "<svg/>".to_owned()))
        );
        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Dxf, &[], "", &[], None, Some("0\nEOF\n")),
            Some(("DXF", "0\nEOF\n".to_owned()))
        );
        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Gcode, &[], "  ", &[], None, None),
            None
        );
    }

    #[test]
    fn export_payload_availability_tracks_all_output_kinds() {
        assert!(!export_payloads_available("", None, None, &[]));
        assert!(export_payloads_available("G90\n", None, None, &[]));
        assert!(export_payloads_available("", Some("<svg/>"), None, &[]));
        assert!(export_payloads_available("", None, Some("0\nEOF\n"), &[]));
        assert!(export_payloads_available(
            "",
            None,
            None,
            &[SecondaryGcode {
                suffix: "clean".to_owned(),
                gcode: "G90\n".to_owned()
            }]
        ));
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
            show_rapids: false,
            show_grid: false,
            show_cleanup: false,
            viewport_rotation_degrees: 42.5,
            preview_sample_text: "Sample=A".to_owned(),
        };

        let encoded = preferences.to_text();
        let parsed = UiPreferences::parse(&encoded);

        assert_eq!(parsed, preferences);
    }

    #[test]
    fn ui_preferences_default_grid_visible_for_old_files() {
        let preferences = UiPreferences::parse("input_path=/tmp/example.cxf\n");

        assert!(preferences.show_rapids);
        assert!(preferences.show_grid);
        assert!(preferences.show_cleanup);
        assert_eq!(preferences.viewport_rotation_degrees, 0.0);
        assert!(preferences.preview_sample_text.is_empty());
    }

    #[test]
    fn ui_preferences_clamp_viewport_rotation() {
        let preferences = UiPreferences::parse("viewport_rotation_degrees=270\nshow_rapids=0\n");

        assert_eq!(preferences.viewport_rotation_degrees, 180.0);
        assert!(!preferences.show_rapids);
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
    fn ui_controls_map_core_defaults() {
        let settings = default_legacy_settings();
        let controls = UiControls::from_settings(&settings);

        assert_eq!(controls.cut_type, CutTypeChoice::Engrave);
        assert_eq!(controls.units, UnitsChoice::Inch);
        assert_eq!(controls.bit_shape, BitShapeChoice::VBit);
        assert_eq!(controls.arc_fit, ArcFitChoice::NoFit);
        assert_eq!(controls.v_check_all, VCheckScopeChoice::All);
        assert_eq!(controls.origin, OriginChoice::Default);
        assert_eq!(controls.justify, JustifyChoice::Left);
        assert_eq!(controls.v_drv_crner, 135.0);
        assert_eq!(controls.v_stp_crner, 200.0);
        assert_eq!(controls.clean_paths, "1,1,0,1,0,1,0,0");
        assert!(controls.recovery_comments);
        assert!(controls.var_dis);
        assert!(!controls.ext_char);
        assert!(!controls.v_pplot);
        assert!(controls.show_thick);
        assert!(controls.show_v_area);
        assert!(get_legacy_bool(&settings, "show_v_path", false));
        assert!(get_legacy_bool(&settings, "show_box", false));
        assert!(get_legacy_bool(&settings, "show_axis", false));
    }

    #[test]
    fn ui_default_controls_use_millimeters() {
        let controls = default_ui_controls();

        assert_eq!(controls.units, UnitsChoice::Mm);
        assert_close(controls.yscale, 50.8);
        assert_close(controls.safe_z, 6.35);
        assert_close(controls.depth_z, -0.127);
        assert_close(controls.stroke_thickness, 0.254);
        assert_close(controls.accuracy, 0.0254);
        assert_close(controls.feed, 127.0);
        assert_close(controls.boxgap, 6.35);
        assert_close(controls.v_bit_dia, 12.7);
        assert_close(controls.v_step_len, 0.254);
        assert_close(controls.v_max_cut, -25.4);
        assert_close(controls.clean_dia, 6.35);
        assert_close(controls.clean_v, 1.27);
    }

    #[test]
    fn ui_controls_convert_units_bidirectionally() {
        let mut controls = UiControls::from_settings(&default_legacy_settings());
        controls.yscale = 2.0;
        controls.xscale_percent = 111.0;
        controls.line_space = 1.3;
        controls.angle_degrees = 15.0;
        controls.text_radius = 1.5;
        controls.safe_z = 0.25;
        controls.depth_z = -0.125;
        controls.stroke_thickness = 0.02;
        controls.xorigin = 3.0;
        controls.yorigin = 4.0;
        controls.segarc = 7.0;
        controls.accuracy = 0.002;
        controls.feed = 7.5;
        controls.plunge = 1.25;
        controls.boxgap = 0.5;
        controls.v_bit_angle = 55.0;
        controls.v_bit_dia = 0.375;
        controls.v_step_len = 0.02;
        controls.v_drv_crner = 120.0;
        controls.v_stp_crner = 220.0;
        controls.allowance = 0.01;
        controls.v_max_cut = -0.25;
        controls.v_rough_stk = 0.015;
        controls.v_depth_lim = -0.5;
        controls.clean_dia = 0.125;
        controls.clean_step = 45.0;
        controls.clean_v = 0.03;

        controls.convert_units(UnitsChoice::Mm);

        assert_eq!(controls.units, UnitsChoice::Mm);
        assert_close(controls.yscale, 50.8);
        assert_close(controls.text_radius, 38.1);
        assert_close(controls.safe_z, 6.35);
        assert_close(controls.depth_z, -3.175);
        assert_close(controls.stroke_thickness, 0.508);
        assert_close(controls.xorigin, 76.2);
        assert_close(controls.yorigin, 101.6);
        assert_close(controls.accuracy, 0.0508);
        assert_close(controls.feed, 190.5);
        assert_close(controls.plunge, 31.75);
        assert_close(controls.boxgap, 12.7);
        assert_close(controls.v_bit_dia, 9.525);
        assert_close(controls.v_step_len, 0.508);
        assert_close(controls.allowance, 0.254);
        assert_close(controls.v_max_cut, -6.35);
        assert_close(controls.v_rough_stk, 0.381);
        assert_close(controls.v_depth_lim, -12.7);
        assert_close(controls.clean_dia, 3.175);
        assert_close(controls.clean_v, 0.762);
        assert_close(controls.xscale_percent, 111.0);
        assert_close(controls.line_space, 1.3);
        assert_close(controls.angle_degrees, 15.0);
        assert_close(controls.segarc, 7.0);
        assert_close(controls.v_bit_angle, 55.0);
        assert_close(controls.v_drv_crner, 120.0);
        assert_close(controls.v_stp_crner, 220.0);
        assert_close(controls.clean_step, 45.0);

        controls.convert_units(UnitsChoice::Inch);

        assert_eq!(controls.units, UnitsChoice::Inch);
        assert_close(controls.yscale, 2.0);
        assert_close(controls.text_radius, 1.5);
        assert_close(controls.safe_z, 0.25);
        assert_close(controls.depth_z, -0.125);
        assert_close(controls.stroke_thickness, 0.02);
        assert_close(controls.xorigin, 3.0);
        assert_close(controls.yorigin, 4.0);
        assert_close(controls.accuracy, 0.002);
        assert_close(controls.feed, 7.5);
        assert_close(controls.plunge, 1.25);
        assert_close(controls.boxgap, 0.5);
        assert_close(controls.v_bit_dia, 0.375);
        assert_close(controls.v_step_len, 0.02);
        assert_close(controls.allowance, 0.01);
        assert_close(controls.v_max_cut, -0.25);
        assert_close(controls.v_rough_stk, 0.015);
        assert_close(controls.v_depth_lim, -0.5);
        assert_close(controls.clean_dia, 0.125);
        assert_close(controls.clean_v, 0.03);
    }

    #[test]
    fn ui_controls_emit_bitmap_trace_overrides() {
        let mut controls = UiControls::from_settings(&LegacySettings::default());
        assert_eq!(controls.bitmap_backend, BitmapBackend::NativePotrace);
        controls.bmp_turn_policy = BitmapTurnPolicy::Black;
        controls.bmp_turds = 7.0;
        controls.bmp_alpha = 0.75;
        controls.bmp_optto = 0.125;
        controls.bitmap_backend = BitmapBackend::PotraceSidecar;
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
        assert_eq!(value_for("bitmap_backend"), Some("potrace-sidecar"));
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
        settings.set_or_push("v_pplot", "1", false);
        settings.set_or_push("show_thick", "0", false);
        settings.set_or_push("show_v_area", "0", false);
        settings.set_or_push("v_drv_crner", "120", false);
        settings.set_or_push("v_stp_crner", "210", false);
        settings.set_or_push("v_check_all", "chr", false);

        let mut controls = UiControls::from_settings(&settings);
        assert_eq!(controls.height_calc, HeightCalcChoice::MaxAll);
        assert_eq!(controls.gpre, "G17|M3 S12000");
        assert_eq!(controls.clean_paths, "1,0,1,0,1,0,1,1");
        assert!(!controls.recovery_comments);
        assert!(!controls.var_dis);
        assert!(controls.ext_char);
        assert!(controls.v_flop);
        assert!(controls.v_pplot);
        assert!(!controls.show_thick);
        assert!(!controls.show_v_area);
        assert_eq!(controls.v_drv_crner, 120.0);
        assert_eq!(controls.v_stp_crner, 210.0);
        assert_eq!(controls.v_check_all, VCheckScopeChoice::Character);

        controls.height_calc = HeightCalcChoice::MaxUse;
        controls.v_check_all = VCheckScopeChoice::All;
        controls.gpre = " G90|M3 S9000 ".to_owned();
        controls.gpost = " M5|M30 ".to_owned();
        controls.clean_paths = " 0,1,0,1,0,1,0,1 ".to_owned();
        controls.recovery_comments = true;
        controls.var_dis = true;
        controls.ext_char = false;
        controls.v_flop = false;
        controls.v_pplot = false;
        controls.show_thick = true;
        controls.show_v_area = true;
        controls.v_drv_crner = 130.0;
        controls.v_stp_crner = 220.0;

        let overrides = controls.overrides();
        let value_for = |key: &str| {
            overrides
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| entry.value.as_str())
        };

        assert_eq!(value_for("H_CALC"), Some("max_use"));
        assert_eq!(value_for("v_check_all"), Some("all"));
        assert_eq!(value_for("gpre"), Some("G90|M3 S9000"));
        assert_eq!(value_for("gpost"), Some("M5|M30"));
        assert_eq!(value_for("clean_paths"), Some("0,1,0,1,0,1,0,1"));
        assert_eq!(value_for("no_comments"), Some("0"));
        assert_eq!(value_for("var_dis"), Some("1"));
        assert_eq!(value_for("ext_char"), Some("0"));
        assert_eq!(value_for("v_flop"), Some("0"));
        assert_eq!(value_for("v_pplot"), Some("0"));
        assert_eq!(value_for("show_thick"), Some("1"));
        assert_eq!(value_for("show_v_area"), Some("1"));
        assert_eq!(value_for("v_drv_crner"), Some("130"));
        assert_eq!(value_for("v_stp_crner"), Some("220"));
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

    #[derive(Debug, Clone, Copy)]
    struct SmokeLayout {
        top: egui::Rect,
        left: egui::Rect,
        bottom: egui::Rect,
        preview: egui::Rect,
    }

    fn smoke_layout_for_viewport(size: egui::Vec2) -> SmokeLayout {
        let full = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let top = egui::Rect::from_min_max(
            full.left_top(),
            egui::pos2(full.right(), full.top() + TOOLBAR_HEIGHT),
        );
        let content =
            egui::Rect::from_min_max(egui::pos2(full.left(), top.bottom()), full.right_bottom());
        let left = egui::Rect::from_min_max(
            content.left_top(),
            egui::pos2(content.left() + INPUT_PANEL_WIDTH, content.bottom()),
        );
        let center_min_x = left.right();
        let center_max_x = content.right();
        let bottom = egui::Rect::from_min_max(
            egui::pos2(center_min_x, content.bottom() - STATUS_PANEL_HEIGHT),
            egui::pos2(center_max_x, content.bottom()),
        );
        let preview = egui::Rect::from_min_max(
            egui::pos2(center_min_x, content.top()),
            egui::pos2(center_max_x, bottom.top()),
        );

        SmokeLayout {
            top,
            left,
            bottom,
            preview,
        }
    }

    fn assert_smoke_layout_valid(layout: SmokeLayout) {
        for rect in [layout.top, layout.left, layout.bottom, layout.preview] {
            assert!(rect.width() > 0.0, "non-positive width: {rect:?}");
            assert!(rect.height() > 0.0, "non-positive height: {rect:?}");
        }
        let rects = [layout.top, layout.left, layout.bottom, layout.preview];
        for (index, left) in rects.iter().enumerate() {
            for right in rects.iter().skip(index + 1) {
                assert!(
                    !rects_overlap(*left, *right),
                    "layout rects overlap: {left:?} and {right:?}"
                );
            }
        }
    }

    fn rects_overlap(a: egui::Rect, b: egui::Rect) -> bool {
        a.left() < b.right() && a.right() > b.left() && a.top() < b.bottom() && a.bottom() > b.top()
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
