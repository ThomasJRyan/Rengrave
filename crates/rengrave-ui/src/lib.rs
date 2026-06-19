use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
#[cfg(test)]
use rengrave_core::batch::prepare_batch_output;
use rengrave_core::batch::{
    BatchOutput, BatchProgress, BatchRequest, SecondaryGcode, layout_text_outline,
    prepare_batch_output_with_cancel_and_progress,
};
use rengrave_core::bitmap::{BitmapBackend, BitmapTraceStats, bitmap_trace_mask_and_stats};
use rengrave_core::dxf::read_dxf_font;
use rengrave_core::external::is_bitmap_input;
use rengrave_core::font::{Font, Stroke, read_cxf, read_ttf};
use rengrave_core::geometry::{Point, ViewTransform};
use rengrave_core::project::{
    DocumentRequest, RENGRAVE_PROJECT_FORMAT_VERSION, RengraveDocument, RengraveProjectFile,
    RengraveProjectOutputs, is_rengrave_project_path, load_document, read_rengrave_project,
    write_rengrave_project,
};
use rengrave_core::settings::{
    DEFAULT_GCODE_POSTAMBLE, DEFAULT_GCODE_PREAMBLE, LegacySetting, LegacySettings,
    default_legacy_settings, get_legacy_bool, legacy_bool_value,
};
use rengrave_core::svg::read_svg_font;
use rfd::FileDialog;

mod browser;
mod catalog;
mod controls;
mod debug_overlay;
mod input_preview;
mod preferences;
mod preview;
mod widgets;

pub(crate) use browser::*;
pub(crate) use catalog::*;
pub(crate) use controls::*;
pub(crate) use debug_overlay::*;
pub(crate) use input_preview::*;
pub(crate) use preferences::*;
pub(crate) use preview::*;
pub(crate) use widgets::*;

const DEFAULT_PREVIEW_ZOOM: f64 = 80.0;
const MM_PER_INCH: f64 = 25.4;
const PREVIEW_FIT_PADDING: f32 = 24.0;
const OUTPUT_PREVIEW_CHARS: usize = 8000;
const INPUT_PREVIEW_VECTOR_HEIGHT: f32 = 180.0;
const INPUT_PREVIEW_THUMBNAIL_WIDTH: u32 = 300;
const INPUT_PREVIEW_THUMBNAIL_HEIGHT: u32 = 180;
const DEFAULT_WINDOW_SIZE: [f32; 2] = [1280.0, 800.0];
const AUTO_RECALC_DEBOUNCE: Duration = Duration::from_millis(400);
#[allow(dead_code)] // referenced by the layout smoke test only
const TOOLBAR_HEIGHT: f32 = 104.0;
const INPUT_PANEL_WIDTH: f32 = 380.0;
const RIGHT_PANEL_WIDTH: f32 = 320.0;
const STATUS_PANEL_HEIGHT: f32 = 150.0;
const STATUS_STRIP_HEIGHT: f32 = 26.0;
const FORM_CONTROL_WIDTH: f32 = 170.0;
const PATH_CONTROL_WIDTH: f32 = 244.0;
const LOGO_SIZE: f32 = 40.0;

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
    project_path: String,
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
    gcode_arc_count: usize,
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
    show_input_overlay: bool,
    input_overlay_outline: Vec<PreviewSegment>,
    browser: Option<FileBrowser>,
    input_catalog: InputCatalog,
    catalog_font_registry: CatalogFontRegistry,
    input_catalog_filter: InputCatalogFilter,
    input_catalog_search: String,
    input_preview: InputPreview,
    show_new_project_modal: bool,
    preferences_path: Option<PathBuf>,
    calculation: Option<CalculationJob>,
    next_calculation_id: u64,
    warnings: Vec<String>,
    fit_preview_requested: bool,
    last_output_request: Option<BatchRequest>,
    auto_recalculate: bool,
    auto_recalc_signature: Option<BatchRequest>,
    auto_recalc_changed_at: Option<Instant>,
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
        let input_catalog = default_input_catalog_for_tool_view(
            tool_view,
            display_input_path.as_ref(),
            default_dir.as_ref(),
        );
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
            project_path: String::new(),
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
            gcode_arc_count: 0,
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
            show_input_overlay: preferences.show_input_overlay,
            input_overlay_outline: Vec::new(),
            browser: None,
            input_catalog,
            catalog_font_registry: CatalogFontRegistry::default(),
            input_catalog_filter: InputCatalogFilter::default(),
            input_catalog_search: String::new(),
            input_preview: InputPreview::default(),
            show_new_project_modal: false,
            preferences_path,
            calculation: None,
            next_calculation_id: 1,
            warnings: document.warnings,
            fit_preview_requested: false,
            last_output_request: None,
            auto_recalculate: preferences.auto_recalculate,
            auto_recalc_signature: None,
            auto_recalc_changed_at: None,
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

    fn current_settings_snapshot(&self) -> Result<(LegacySettings, Vec<String>), String> {
        let document =
            load_document(&self.settings_request_for_save()).map_err(|err| err.to_string())?;
        Ok((document.settings, document.warnings))
    }

    fn current_project_file(&self) -> Result<(RengraveProjectFile, Vec<String>), String> {
        let (settings, warnings) = self.current_settings_snapshot()?;
        Ok((
            RengraveProjectFile {
                format_version: RENGRAVE_PROJECT_FORMAT_VERSION,
                application_version: rengrave_core::RENGRAVE_VERSION.to_owned(),
                text: self.text.clone(),
                settings,
                input_path: path_from_text(&self.input_path),
                default_dir: path_from_text(&self.default_dir_path),
                legacy_settings_path: path_from_text(&self.settings_path),
                workbench: self.tool_view.value().to_owned(),
                outputs: RengraveProjectOutputs {
                    gcode_path: path_from_text(&self.gcode_path),
                    svg_path: path_from_text(&self.svg_path),
                    dxf_path: path_from_text(&self.dxf_path),
                },
            },
            warnings,
        ))
    }

    fn load_project_or_legacy_settings(&mut self, path: PathBuf, ctx: egui::Context) {
        if is_rengrave_project_path(&path) {
            self.load_project(path, ctx);
        } else {
            self.project_path.clear();
            self.settings_path = path.display().to_string();
            self.reload_document(ctx);
        }
    }

    fn load_project(&mut self, path: PathBuf, ctx: egui::Context) {
        match read_rengrave_project(&path) {
            Ok(project) => {
                self.apply_project_file(project, path, ctx);
            }
            Err(err) => {
                self.cancel_calculation("Project load failed");
                self.status = "Project load failed".to_owned();
                self.warnings = vec![err.to_string()];
            }
        }
    }

    fn apply_project_file(
        &mut self,
        project: RengraveProjectFile,
        path: PathBuf,
        ctx: egui::Context,
    ) {
        self.cancel_calculation("Project loaded");
        self.project_path = path.display().to_string();
        self.settings_path = path_to_text(&project.legacy_settings_path);
        self.input_path = path_to_text(&project.input_path);
        self.default_dir_path = path_to_text(&project.default_dir);
        self.gcode_path = path_to_text(&project.outputs.gcode_path);
        self.svg_path = path_to_text(&project.outputs.svg_path);
        self.dxf_path = path_to_text(&project.outputs.dxf_path);
        self.text = project.text;
        self.controls = UiControls::from_settings(&project.settings);
        let inferred = ToolView::from_settings_and_path(
            &project.settings,
            path_from_text(&self.input_path).as_deref(),
        );
        self.tool_view = ToolView::parse(&project.workbench).unwrap_or(inferred);
        self.controls.cut_type = self.tool_view.cut_type();
        self.show_toolpath = get_legacy_bool(&project.settings, "show_v_path", true);
        self.show_bounds = get_legacy_bool(&project.settings, "show_box", true);
        self.show_axes = get_legacy_bool(&project.settings, "show_axis", true);
        self.settings_count = project.settings.entries.len();
        self.warnings.clear();
        self.status = format!("Project loaded: {}", path.display());
        self.refresh_input_catalog();
        self.save_preferences();
        self.start_calculation(ctx);
    }

    fn save_current_project(&mut self) {
        let Some(path) = path_from_text(&self.project_path).map(project_path_with_extension) else {
            self.status = "Project path is empty".to_owned();
            return;
        };

        match self.current_project_file() {
            Ok((project, warnings)) => match write_rengrave_project(&path, &project) {
                Ok(()) => {
                    self.project_path = path.display().to_string();
                    self.status = format!("Project saved: {}", path.display());
                    self.warnings = warnings;
                    self.save_preferences();
                }
                Err(err) => {
                    self.status = "Project save failed".to_owned();
                    self.warnings.push(err.to_string());
                }
            },
            Err(err) => {
                self.status = "Project save failed".to_owned();
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

    fn start_new_project(&mut self, tool_view: ToolView) {
        self.cancel_calculation("New project");
        self.project_path.clear();
        self.settings_path.clear();
        self.input_path.clear();
        self.text = RengraveDocument::default().text;
        self.reset_controls_to_defaults();
        self.set_tool_view(tool_view);
        self.gcode.clear();
        self.svg = None;
        self.dxf = None;
        self.secondary_gcode.clear();
        self.gcode_lines = 0;
        self.gcode_arc_count = 0;
        self.preview_segments.clear();
        self.preview_rapids.clear();
        self.preview_cleanup_segments.clear();
        self.preview_bounds = None;
        self.input_overlay_outline.clear();
        self.last_output_request = None;
        self.auto_recalc_signature = None;
        self.auto_recalc_changed_at = None;
        self.show_new_project_modal = false;
        self.refresh_default_catalog_for_tool_view();
        self.status = format!("New {} project", tool_view.label());
        self.save_preferences();
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
                self.input_overlay_outline = input_outline_segments(&job.request);
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
                self.gcode_arc_count = 0;
                self.preview_segments.clear();
                self.preview_rapids.clear();
                self.preview_cleanup_segments.clear();
                self.preview_bounds = None;
                self.input_overlay_outline.clear();
                self.last_output_request = None;
            }
        }
    }

    fn apply_batch_output(&mut self, output: BatchOutput) {
        self.gcode_lines = output.gcode.lines().count();
        self.gcode_arc_count = count_arc_moves(&output.gcode);
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

    fn export_gcode(&mut self) {
        if self.gcode.is_empty() {
            self.status = "G-code export unavailable".to_owned();
            return;
        }

        match write_text_file(&self.gcode_path, &self.gcode) {
            Ok(path) => {
                self.status = format!("G-code exported: {}", path.display());
                self.save_preferences();
            }
            Err(err) => {
                self.status = "G-code export failed".to_owned();
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

    fn choose_path(&mut self, target: FileBrowserTarget, ctx: egui::Context) {
        if let Some(path) = choose_native_path(
            target,
            self.browser_value(target),
            path_from_text(&self.default_dir_path),
            self.input_dialog_filter(),
        ) {
            self.apply_browser_selection(target, path, ctx);
        } else {
            self.open_browser(target);
            self.status = "Using in-app browser".to_owned();
        }
    }

    /// Chooses which file-type filter to apply to the input picker based on the
    /// active workbench, so fonts and images are surfaced directly.
    fn input_dialog_filter(&self) -> InputDialogFilter {
        if self.tool_view.uses_image() {
            InputDialogFilter::Images
        } else if self.tool_view.uses_text() {
            InputDialogFilter::Fonts
        } else {
            InputDialogFilter::All
        }
    }

    fn browser_value(&self, target: FileBrowserTarget) -> &str {
        match target {
            FileBrowserTarget::Project | FileBrowserTarget::ProjectOutput => &self.project_path,
            FileBrowserTarget::Input => &self.input_path,
            FileBrowserTarget::DefaultDir => &self.default_dir_path,
            FileBrowserTarget::GcodeOutput => &self.gcode_path,
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
            FileBrowserTarget::Project => {
                if is_rengrave_project_path(&path) {
                    self.project_path = text;
                } else {
                    self.settings_path = text;
                    self.project_path.clear();
                }
            }
            FileBrowserTarget::ProjectOutput => {
                self.project_path = project_path_with_extension(path.clone())
                    .display()
                    .to_string()
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
        }
        self.status = format!("Selected {}", target.label());
        self.save_preferences();
        match selection_followup(target) {
            SelectionFollowup::None => {}
            SelectionFollowup::LoadProject => self.load_project_or_legacy_settings(path, ctx),
            SelectionFollowup::SaveProject => self.save_current_project(),
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

    fn refresh_system_font_catalog(&mut self) {
        self.input_catalog = InputCatalog::scan_system_fonts();
        self.status = format!("Found {} system font(s)", self.input_catalog.entries.len());
    }

    fn refresh_default_catalog_for_tool_view(&mut self) {
        if self.tool_view.uses_text() {
            self.refresh_system_font_catalog();
        } else {
            self.refresh_input_catalog();
        }
        self.input_catalog_search.clear();
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

    fn show_workflow_input_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Input")
            .default_open(true)
            .show(ui, |ui| {
                self.show_input_paths(ui);
            });

        if self.tool_view.uses_text() {
            egui::CollapsingHeader::new("Text")
                .default_open(true)
                .show(ui, |ui| {
                    self.show_text_input_panel(ui);
                });
        }

        egui::CollapsingHeader::new("Catalog")
            .default_open(false)
            .show(ui, |ui| {
                self.show_input_catalog_panel(ui);
            });

        ui.label(format!("Legacy keys: {}", self.settings_count));
    }

    fn show_input_paths(&mut self, ui: &mut egui::Ui) {
        if self.tool_view.uses_image() {
            if full_width_button(ui, "Open image\u{2026}", true) {
                self.choose_path(FileBrowserTarget::Input, ui.ctx().clone());
            }
        } else if self.tool_view.uses_text()
            && full_width_button(ui, "Open font file\u{2026}", true)
        {
            self.choose_path(FileBrowserTarget::Input, ui.ctx().clone());
        }
    }

    fn show_text_input_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_sized(
            [ui.available_width(), 120.0],
            egui::TextEdit::multiline(&mut self.text),
        );
    }

    fn show_input_catalog_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Scan folder").clicked() {
                self.refresh_input_catalog();
            }
            if self.tool_view.uses_text() && ui.button("System fonts").clicked() {
                self.refresh_system_font_catalog();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Source");
            let source = self.input_catalog.source_label();
            let compact = compact_text_middle(&source, 72);
            let response = ui.monospace(compact.as_str());
            if compact != source {
                response.on_hover_text(source);
            }
        });
        if let Some(error) = &self.input_catalog.error {
            ui.colored_label(egui::Color32::from_rgb(225, 176, 84), error);
        }
        if self.input_catalog.entries.is_empty() {
            ui.label("No supported files found");
            return;
        }

        let _ = text_row(ui, "Search", &mut self.input_catalog_search);

        let visible_entries = visible_input_catalog_entries_for_tool(
            &self.input_catalog.entries,
            self.input_catalog_filter,
            self.tool_view,
        );
        let query = self.input_catalog_search.trim().to_lowercase();
        let filtered: Vec<InputCatalogEntry> = visible_entries
            .into_iter()
            .filter(|entry| query.is_empty() || entry.name.to_lowercase().contains(&query))
            .collect();
        if filtered.is_empty() {
            ui.label("No compatible files found");
            return;
        }

        self.catalog_font_registry.refresh(ui.ctx(), &filtered);
        let total = filtered.len();
        let shown = total.min(CATALOG_DISPLAY_LIMIT);
        let selected_input = path_from_text(&self.input_path);
        let mut chosen = None;
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for entry in filtered.into_iter().take(CATALOG_DISPLAY_LIMIT) {
                    let selected = selected_input.as_ref() == Some(&entry.path);
                    let label = format!(
                        "{}  {}  {}",
                        entry.kind.label(),
                        entry.name,
                        format_bytes(entry.size_bytes)
                    );
                    let mut label = egui::RichText::new(label);
                    if let Some(family) = self.catalog_font_registry.family_for_path(&entry.path) {
                        label = label.font(egui::FontId::new(13.0, family));
                    }
                    if ui.selectable_label(selected, label).clicked() {
                        chosen = Some(entry.path.clone());
                    }
                }
            });
        if total > shown {
            ui.label(
                egui::RichText::new(format!(
                    "Showing {shown} of {total}. Refine the search to narrow results."
                ))
                .small()
                .color(egui::Color32::from_rgb(150, 158, 164)),
            );
        }
        if let Some(path) = chosen {
            self.select_input_catalog_entry(path, ui.ctx().clone());
        }
    }

    fn show_input_preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Preview");
            if ui.button("Refresh").clicked() {
                self.reload_input_preview();
            }
        });
        self.ensure_input_preview();
        draw_input_preview(ui, &mut self.input_preview);
    }

    fn show_workflow_settings_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Layout")
            .default_open(true)
            .show(ui, |ui| {
                self.show_layout_settings(ui);
            });

        egui::CollapsingHeader::new("Cut")
            .default_open(true)
            .show(ui, |ui| {
                self.show_machine_settings(ui);
            });

        if self.tool_view.uses_image() && input_path_is_bitmap(&self.input_path) {
            egui::CollapsingHeader::new("Bitmap trace")
                .default_open(false)
                .show(ui, |ui| {
                    self.show_bitmap_settings(ui);
                });
        }

        if self.tool_view.uses_vcarve() {
            egui::CollapsingHeader::new("V-carve")
                .default_open(false)
                .show(ui, |ui| {
                    self.show_vcarve_settings(ui);
                });
            egui::CollapsingHeader::new("Multipass")
                .default_open(false)
                .show(ui, |ui| {
                    self.show_multipass_settings(ui);
                });
            egui::CollapsingHeader::new("Cleanup")
                .default_open(false)
                .show(ui, |ui| {
                    self.show_cleanup_settings(ui);
                });
        }

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
            if ui.button("Reset to defaults").clicked() {
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
        combo_row(
            ui,
            "Turn policy",
            self.controls.bmp_turn_policy.label(),
            |ui| {
                for value in BitmapTurnPolicy::ALL {
                    ui.selectable_value(&mut self.controls.bmp_turn_policy, value, value.label());
                }
            },
        );
        number_row(ui, "Turd size", &mut self.controls.bmp_turds, 1.0);
        number_row(ui, "Alpha max", &mut self.controls.bmp_alpha, 0.05);
        number_row(ui, "Opt tolerance", &mut self.controls.bmp_optto, 0.01);
        ui.checkbox(&mut self.controls.bmp_long, "Long curves");
    }

    fn show_vcarve_settings(&mut self, ui: &mut egui::Ui) {
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

    fn show_canvas_panel(&mut self, ui: &mut egui::Ui) -> egui::Rect {
        self.show_canvas_toolbar(ui);
        let rect = ui.available_rect_before_wrap();
        self.show_preview_panel(ui, rect);
        rect
    }

    fn show_canvas_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Fit").clicked() {
                self.fit_preview_requested = true;
            }
            if ui.button("Reset view").clicked() {
                self.reset_preview_pan_zoom();
            }
            if ui.small_button("Zoom -").clicked() {
                self.transform.zoom = (self.transform.zoom / 1.25).clamp(1.0, 500.0);
            }
            ui.add_sized(
                [130.0, 18.0],
                egui::Slider::new(&mut self.transform.zoom, 1.0..=500.0)
                    .text("Zoom")
                    .clamping(egui::SliderClamping::Always),
            );
            if ui.small_button("Zoom +").clicked() {
                self.transform.zoom = (self.transform.zoom * 1.25).clamp(1.0, 500.0);
            }
            ui.separator();
            if ui
                .add_sized(
                    [130.0, 18.0],
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.toggle_value(&mut self.show_grid, "Grid").changed() {
                    self.save_preferences();
                }
                ui.toggle_value(&mut self.show_axes, "Axes");
                ui.toggle_value(&mut self.show_bounds, "Bounds");
            });
        });
        ui.separator();
    }

    fn show_right_sidebar(&mut self, ui: &mut egui::Ui) {
        self.show_input_preview_panel(ui);

        ui.separator();
        ui.heading("Layers");
        if ui
            .checkbox(&mut self.show_rapids, "Show travel moves")
            .changed()
        {
            self.save_preferences();
        }
        if ui
            .add_enabled(
                !self.preview_cleanup_segments.is_empty(),
                egui::Checkbox::new(&mut self.show_cleanup, "Show cleanup moves"),
            )
            .changed()
        {
            self.save_preferences();
        }
        ui.checkbox(&mut self.show_bounds, "Show bounding box");
        ui.checkbox(&mut self.show_toolpath, "Show toolpath");
        if ui
            .add_enabled(
                self.has_input_overlay(),
                egui::Checkbox::new(&mut self.show_input_overlay, "Show input outline"),
            )
            .on_hover_text("Overlay the input vector on top of the toolpath")
            .changed()
        {
            self.save_preferences();
        }

        ui.separator();
        ui.heading("Statistics");
        let stats = self.job_statistics();
        stat_row(ui, "Width", &stats.width);
        stat_row(ui, "Height", &stats.height);
        stat_row(ui, "Total paths", &stats.total_paths);
        stat_row(ui, "Total length", &stats.total_length);
        stat_row(ui, "Estimated time", &stats.estimated_time);
        stat_row(ui, "Rapid moves", &stats.rapid_percent);
        stat_row(ui, "G-code lines", &stats.gcode_lines);
        stat_row(ui, "Arc moves", &stats.arc_moves);

        ui.separator();
        ui.heading("Export");
        self.show_units_row(ui);
        let gcode_path_action = path_row(ui, "G-code", &mut self.gcode_path);
        if gcode_path_action.browse_clicked {
            self.choose_path(FileBrowserTarget::GcodeOutput, ui.ctx().clone());
        }
        if gcode_path_action.value_changed {
            self.save_preferences();
        }
        ui.add_space(4.0);
        if full_width_button(ui, "Generate G-code", true) {
            self.start_calculation(ui.ctx().clone());
        }
        if full_width_button(ui, "Copy to clipboard", !self.gcode.is_empty()) {
            self.copy_gcode(ui.ctx());
        }
        if full_width_button(ui, "Save to file", !self.gcode.is_empty()) {
            self.export_gcode();
        }
    }

    fn show_bottom_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let ready = self.calculation.is_none();
            let color = if ready {
                egui::Color32::from_rgb(94, 176, 132)
            } else {
                egui::Color32::from_rgb(225, 176, 84)
            };
            ui.colored_label(color, "\u{25CF}");
            ui.label(if ready { "Ready" } else { "Working" });
            ui.separator();
            ui.monospace(format!("Lines: {}", self.gcode_lines));
            ui.separator();
            ui.monospace(format!("Arcs: {}", self.gcode_arc_count));
            ui.separator();
            let unit = self.controls.units.value();
            ui.monospace(format!(
                "Length: {}",
                format_length(total_segment_length(&self.preview_segments), unit)
            ));
            ui.separator();
            let minutes = estimated_cut_minutes(
                total_segment_length(&self.preview_segments),
                self.controls.feed,
            );
            ui.monospace(format!("Est: {}", format_duration_minutes(minutes)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.monospace(format!("Zoom: {:.0}%", self.transform.zoom));
            });
        });
    }

    fn job_statistics(&self) -> JobStatistics {
        let unit = self.controls.units.value();
        let (width, height) = self
            .preview_bounds
            .map(|bounds| {
                (
                    (bounds.max.x - bounds.min.x).abs(),
                    (bounds.max.y - bounds.min.y).abs(),
                )
            })
            .unwrap_or((0.0, 0.0));
        let cut_length = total_segment_length(&self.preview_segments);
        let rapid_length = total_segment_length(&self.preview_rapids);
        let total = cut_length + rapid_length;
        let rapid_percent = if total > 0.0 {
            rapid_length / total * 100.0
        } else {
            0.0
        };
        let minutes = estimated_cut_minutes(cut_length, self.controls.feed);
        JobStatistics {
            width: format_measurement(width, unit),
            height: format_measurement(height, unit),
            total_paths: self.preview_segments.len().to_string(),
            total_length: format_length(cut_length, unit),
            estimated_time: format_duration_minutes(minutes),
            rapid_percent: format!("{rapid_percent:.1} %"),
            gcode_lines: self.gcode_lines.to_string(),
            arc_moves: self.gcode_arc_count.to_string(),
        }
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
            auto_recalculate: self.auto_recalculate,
            show_input_overlay: self.show_input_overlay,
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
        self.maybe_auto_recalculate(ui.ctx());
        let root_rect = ui.max_rect();

        let top_rect = egui::Panel::top("toolbar")
            .resizable(false)
            .show_inside(ui, |ui| {
                self.show_toolbar_contents(ui);
            })
            .response
            .rect;

        // Full-width status strip pinned to the very bottom. It is added before
        // the side panels so it spans the entire window width.
        egui::Panel::bottom("status_strip")
            .resizable(false)
            .exact_size(STATUS_STRIP_HEIGHT)
            .show_inside(ui, |ui| {
                self.show_bottom_status_bar(ui);
            });

        let left_rect = egui::Panel::left("input_settings")
            .resizable(true)
            .default_size(INPUT_PANEL_WIDTH)
            .size_range(450.0..=640.0)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.show_workflow_input_panel(ui);
                        ui.separator();
                        self.show_workflow_settings_panel(ui);
                    });
            })
            .response
            .rect;

        let _right_rect = egui::Panel::right("output_panel")
            .resizable(true)
            .default_size(RIGHT_PANEL_WIDTH)
            .size_range(280.0..=460.0)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.show_right_sidebar(ui);
                    });
            })
            .response
            .rect;

        let bottom_rect = egui::Panel::bottom("status_log")
            .resizable(true)
            .default_size(STATUS_PANEL_HEIGHT)
            .show_inside(ui, |ui| {
                self.show_bottom_panel_contents(ui);
            })
            .response
            .rect;

        let preview_rect = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| self.show_canvas_panel(ui))
            .inner;

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
        self.show_new_project_modal(ui.ctx());
    }
}

impl RengraveApp {
    fn show_toolbar_contents(&mut self, ui: &mut egui::Ui) {
        self.show_menu_bar(ui);
        ui.horizontal(|ui| {
            draw_rengrave_logo(ui);
            ui.separator();
            ui.label("Workbench");
            ui.strong(self.tool_view.label());
            if self.calculation.is_some() {
                ui.separator();
                ui.spinner();
                ui.label("Calculating");
                if ui.button("Cancel").clicked() {
                    self.cancel_calculation("Calculation canceled");
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.gcode.is_empty(), egui::Button::new("Export G-code"))
                    .clicked()
                {
                    self.export_gcode();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Status");
            ui.monospace(&self.status);
            if let Some(stale_summary) = self.output_stale_summary() {
                ui.colored_label(egui::Color32::from_rgb(225, 176, 84), stale_summary);
                if self.stale_recalculate_available() && ui.button("Recalculate").clicked() {
                    self.start_calculation(ui.ctx().clone());
                }
            }
            if ui
                .toggle_value(&mut self.auto_recalculate, "Auto")
                .on_hover_text("Automatically recalculate when a setting changes")
                .changed()
            {
                self.auto_recalc_signature = None;
                self.auto_recalc_changed_at = None;
                self.save_preferences();
            }
        });
        ui.add_space(2.0);
        self.show_job_summary(ui);
    }

    fn show_bottom_panel_contents(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
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
                        if self.stale_recalculate_available() && ui.button("Recalculate").clicked()
                        {
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

        self.ensure_input_preview();

        draw_preview(
            ui.painter(),
            rect,
            self.transform,
            self.controls.units.value(),
            &self.preview_segments,
            &self.preview_rapids,
            &self.preview_cleanup_segments,
            &self.input_overlay_outline,
            self.preview_bounds,
            self.show_toolpath,
            self.show_rapids,
            self.show_cleanup,
            self.show_input_overlay && !self.input_overlay_outline.is_empty(),
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
                if menu_action(ui, "New", true) {
                    self.show_new_project_modal = true;
                }

                if menu_action(ui, "Open", true) {
                    self.choose_path(FileBrowserTarget::Project, ui.ctx().clone());
                }

                if menu_action(ui, "Save", !self.project_path.trim().is_empty()) {
                    self.save_current_project();
                }

                if menu_action(ui, "Save As", true) {
                    self.choose_path(FileBrowserTarget::ProjectOutput, ui.ctx().clone());
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
            if let Some(warnings) = warning_count_summary(&self.warnings) {
                summary_separator(ui);
                summary_label(ui, &warnings, egui::Color32::from_rgb(225, 176, 84));
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

    fn show_new_project_modal(&mut self, ctx: &egui::Context) {
        if !self.show_new_project_modal {
            return;
        }

        let mut selected = None;
        let response = egui::Modal::new(egui::Id::new("new_project_modal")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("New project");
            ui.add_space(6.0);
            for category in ["Text generation", "Image generation"] {
                ui.label(egui::RichText::new(category).strong());
                ui.add_space(2.0);
                for tool_view in ToolView::ALL
                    .into_iter()
                    .filter(|tool_view| tool_view.category_label() == category)
                {
                    if full_width_button(ui, tool_view.label(), true) {
                        selected = Some(tool_view);
                    }
                }
                ui.add_space(8.0);
            }
            ui.separator();
            if full_width_button(ui, "Cancel", true) {
                ui.close();
            }
        });

        if let Some(tool_view) = selected {
            self.start_new_project(tool_view);
        } else if response.should_close() {
            self.show_new_project_modal = false;
        }
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

    /// Triggers a recalculation automatically when auto-recalc is enabled and the
    /// engraving settings have been stable for a short debounce window. The
    /// debounce avoids restarting the worker on every frame while a slider is
    /// being dragged or text is being typed.
    fn maybe_auto_recalculate(&mut self, ctx: &egui::Context) {
        if !self.auto_recalculate {
            self.auto_recalc_signature = None;
            self.auto_recalc_changed_at = None;
            return;
        }

        let request = self.batch_request(true);
        let changed = self
            .auto_recalc_signature
            .as_ref()
            .map(|previous| !calculation_stale_reasons(&request, previous).is_empty())
            .unwrap_or(true);
        if changed {
            self.auto_recalc_signature = Some(request);
            self.auto_recalc_changed_at = Some(Instant::now());
            ctx.request_repaint_after(AUTO_RECALC_DEBOUNCE);
            return;
        }

        if self.calculation.is_some() || !self.output_is_stale() {
            self.auto_recalc_changed_at = None;
            return;
        }

        let debounce_elapsed = self
            .auto_recalc_changed_at
            .map(|since| since.elapsed() >= AUTO_RECALC_DEBOUNCE)
            .unwrap_or(true);
        if debounce_elapsed {
            self.auto_recalc_changed_at = None;
            self.start_calculation(ctx.clone());
        } else {
            ctx.request_repaint_after(AUTO_RECALC_DEBOUNCE);
        }
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

    /// Returns true when an input outline overlay is available for the current
    /// toolpath (text/vector inputs, not bitmaps).
    fn has_input_overlay(&self) -> bool {
        !self.input_overlay_outline.is_empty()
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

fn default_input_catalog_for_tool_view(
    tool_view: ToolView,
    input: Option<&PathBuf>,
    default_dir: Option<&PathBuf>,
) -> InputCatalog {
    if tool_view_uses_system_font_catalog_by_default(tool_view) {
        InputCatalog::scan_system_fonts()
    } else {
        InputCatalog::scan(input_catalog_start_dir(
            &input.cloned(),
            &default_dir.cloned(),
        ))
    }
}

fn tool_view_uses_system_font_catalog_by_default(tool_view: ToolView) -> bool {
    tool_view.uses_text()
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
    path_from_text(path_text).filter(|path| path.is_file() && !is_rengrave_project_path(path))
}

#[cfg(test)]
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

fn project_path_with_extension(path: PathBuf) -> PathBuf {
    if is_rengrave_project_path(&path) {
        path
    } else {
        path.with_extension(rengrave_core::project::RENGRAVE_PROJECT_EXTENSION)
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

fn warning_count_summary(warnings: &[String]) -> Option<String> {
    match warnings.len() {
        0 => None,
        1 => Some("Warnings: 1".to_owned()),
        count => Some(format!("Warnings: {count}")),
    }
}

fn summary_label(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(text).color(color));
}

fn summary_separator(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("/").color(egui::Color32::from_rgb(120, 130, 136)));
}

struct JobStatistics {
    width: String,
    height: String,
    total_paths: String,
    total_length: String,
    estimated_time: String,
    rapid_percent: String,
    gcode_lines: String,
    arc_moves: String,
}

fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.monospace(value);
        });
    });
}

fn draw_rengrave_logo(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(LOGO_SIZE, LOGO_SIZE), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, egui::Color32::from_rgb(47, 158, 99));
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.5, egui::Color32::from_rgb(29, 95, 62)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "R",
        egui::FontId::proportional(28.0),
        egui::Color32::from_rgb(244, 251, 247),
    );
    response.on_hover_text("R-Engrave")
}

/// Shortens long labels while preserving both start and end context.
fn compact_text_middle(value: &str, max_chars: usize) -> String {
    if max_chars <= 1 {
        return "…".to_owned();
    }
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_owned();
    }
    let head_len = max_chars / 2;
    let tail_len = max_chars.saturating_sub(head_len + 1);
    let head: String = value.chars().take(head_len).collect();
    let tail: String = value
        .chars()
        .rev()
        .take(tail_len)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

fn full_width_button(ui: &mut egui::Ui, label: &str, enabled: bool) -> bool {
    let width = ui.available_width();
    ui.add_enabled(
        enabled,
        egui::Button::new(label).min_size(egui::vec2(width, 26.0)),
    )
    .clicked()
}

fn count_arc_moves(gcode: &str) -> usize {
    gcode
        .lines()
        .filter(|line| {
            matches!(
                line.trim().split_whitespace().next().unwrap_or_default(),
                "G2" | "G02" | "G3" | "G03"
            )
        })
        .count()
}

fn estimated_cut_minutes(cut_length: f64, feed: f64) -> f64 {
    if feed > 0.0 && cut_length.is_finite() {
        cut_length / feed
    } else {
        0.0
    }
}

fn format_measurement(value: f64, unit: &str) -> String {
    format!("{value:.2} {unit}")
}

fn format_length(value: f64, unit: &str) -> String {
    if unit == "mm" && value >= 1000.0 {
        format!("{:.2} m", value / 1000.0)
    } else if unit == "in" && value >= 12.0 {
        format!("{:.2} ft", value / 12.0)
    } else {
        format!("{value:.2} {unit}")
    }
}

fn format_duration_minutes(minutes: f64) -> String {
    if !minutes.is_finite() || minutes <= 0.0 {
        return "\u{2014}".to_owned();
    }
    let total_seconds = (minutes * 60.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total_seconds / 3600,
        (total_seconds % 3600) / 60,
        total_seconds % 60
    )
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

fn left_panel_content_width(ui: &egui::Ui) -> f32 {
    ui.available_width().max(80.0)
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
    fn statistics_helpers_format_job_summary_values() {
        assert_eq!(format_measurement(153.0, "mm"), "153.00 mm");
        assert_eq!(format_length(1920.0, "mm"), "1.92 m");
        assert_eq!(format_length(500.0, "mm"), "500.00 mm");
        assert_eq!(format_length(24.0, "in"), "2.00 ft");
        assert_eq!(format_duration_minutes(3.75), "00:03:45");
        assert_eq!(format_duration_minutes(0.0), "\u{2014}");
        assert!((estimated_cut_minutes(1920.0, 1200.0) - 1.6).abs() < 1e-9);
        assert_eq!(estimated_cut_minutes(100.0, 0.0), 0.0);
        assert_eq!(
            count_arc_moves("G1 X0\nG2 X1 Y1 I1 J0\nG03 X2\n(comment)\n"),
            2
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
            CalculationPhase::Batch(BatchProgress::LoadingSvg).status_text(),
            "Loading SVG input"
        );
        assert_eq!(
            CalculationPhase::Finalizing.status_text(),
            "Finalizing output"
        );
    }

    #[test]
    fn text_workbenches_use_system_font_catalog_by_default() {
        assert!(tool_view_uses_system_font_catalog_by_default(
            ToolView::TextEngrave
        ));
        assert!(tool_view_uses_system_font_catalog_by_default(
            ToolView::TextVCarve
        ));
        assert!(!tool_view_uses_system_font_catalog_by_default(
            ToolView::ImageEngrave
        ));
        assert!(!tool_view_uses_system_font_catalog_by_default(
            ToolView::ImageVCarve
        ));
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
    fn project_paths_default_to_rgrv_extension() {
        assert_eq!(
            project_path_with_extension(PathBuf::from("/tmp/job")).as_path(),
            Path::new("/tmp/job.rgrv")
        );
        assert_eq!(
            project_path_with_extension(PathBuf::from("/tmp/job.RGRV")).as_path(),
            Path::new("/tmp/job.RGRV")
        );
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
        assert_eq!(
            input_source_summary("/tmp/artwork.svg"),
            "Source: SVG artwork.svg"
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
    fn logo_svg_asset_is_green_boxed_r() {
        let svg = include_str!("../../../assets/logo/rengrave-r.svg");

        assert!(svg.contains("<svg"));
        assert!(svg.contains("#2f9e63"));
        assert!(svg.contains("R-Engrave"));
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
        assert!(ToolView::ImageVCarve.accepts_kind(InputCatalogKind::Svg));
        assert!(ToolView::ImageVCarve.accepts_kind(InputCatalogKind::Bitmap));
        assert_eq!(ToolView::TextEngrave.category_label(), "Text generation");
        assert_eq!(ToolView::ImageVCarve.category_label(), "Image generation");
        assert_eq!(ToolView::ImageVCarve.value(), "image-v-carve");
        assert_eq!(
            ToolView::parse("image-v-carve"),
            Some(ToolView::ImageVCarve)
        );
        assert_eq!(ToolView::parse("unknown"), None);
        assert_eq!(
            ToolView::TextVCarve.with_input_kind(InputCatalogKind::Bitmap),
            ToolView::ImageVCarve
        );
        assert_eq!(
            ToolView::TextEngrave.with_input_kind(InputCatalogKind::Svg),
            ToolView::ImageEngrave
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
        assert_eq!(
            ToolView::from_settings_and_path(&settings, Some(Path::new("/tmp/shape.svg"))),
            ToolView::ImageVCarve
        );
    }

    #[test]
    fn warning_summary_tracks_visible_warning_count() {
        assert_eq!(warning_count_summary(&[]), None);
        assert_eq!(
            warning_count_summary(&["missing input".to_owned()]),
            Some("Warnings: 1".to_owned())
        );
        assert_eq!(
            warning_count_summary(&["one".to_owned(), "two".to_owned()]),
            Some("Warnings: 2".to_owned())
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
                FileBrowserTarget::Project,
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
            output_file_name("  ", FileBrowserTarget::ProjectOutput),
            "rengrave_project.rgrv"
        );
    }

    #[test]
    fn browser_selection_followup_matches_user_workflow() {
        assert_eq!(
            selection_followup(FileBrowserTarget::Project),
            SelectionFollowup::LoadProject
        );
        assert_eq!(
            selection_followup(FileBrowserTarget::ProjectOutput),
            SelectionFollowup::SaveProject
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
        fs::write(dir.join("vector.svg"), "<svg/>").unwrap();
        fs::write(dir.join("image.PNG"), b"not really png").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let entries = read_input_catalog_entries(&dir).unwrap();

        let _ = fs::remove_dir_all(dir);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, InputCatalogKind::CxfFont);
        assert_eq!(entries[1].kind, InputCatalogKind::Dxf);
        assert_eq!(entries[2].kind, InputCatalogKind::Svg);
        assert_eq!(entries[3].kind, InputCatalogKind::Bitmap);
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
            InputCatalogEntry {
                path: PathBuf::from("/tmp/e.svg"),
                name: "e.svg".to_owned(),
                kind: InputCatalogKind::Svg,
                size_bytes: 50,
            },
        ];

        let filter = InputCatalogFilter {
            cxf: false,
            ttf: true,
            dxf: false,
            svg: false,
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
            vec!["c.dxf", "d.png", "e.svg"]
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
    fn input_preview_sample_uses_engraving_text_for_fonts_only() {
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/font.cxf")), "Generated"),
            Some("Generated".to_owned())
        );
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/font.ttf")), "Generated"),
            Some("Generated".to_owned())
        );
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/artwork.dxf")), "Generated"),
            None
        );
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/artwork.svg")), "Generated"),
            None
        );
        assert_eq!(
            input_preview_sample_for_path(Some(Path::new("/tmp/image.png")), "Generated"),
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
    fn input_preview_loads_svg_vector_segments() {
        let dir =
            std::env::temp_dir().join(format!("rengrave-ui-svg-preview-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shape.svg");
        fs::write(&path, r#"<svg><path d="M 10 20 L 14 20 L 14 24 Z"/></svg>"#).unwrap();

        let preview = load_input_preview_data(&path, None);

        let _ = fs::remove_dir_all(dir);
        match preview {
            InputPreviewData::Vector {
                label,
                segment_count,
                bounds,
                ..
            } => {
                assert_eq!(label, "SVG artwork");
                assert_eq!(segment_count, 3);
                assert_eq!(bounds.unwrap().min, Point::new(0.0, 0.0));
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
    fn image_size_height_uses_vector_image_preview_bounds_only() {
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
            image_preview_model_height(Some(Path::new("part.svg")), &preview),
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
        let warnings = vec!["missing input".to_owned(), "fallback output".to_owned()];
        let cleanup = vec![SecondaryGcode {
            suffix: "clean".to_owned(),
            gcode: "G90\nG1 X0\n".to_owned(),
        }];

        assert_eq!(
            bottom_tab_copy_payload(BottomTab::Status, &warnings, "", &[], None, None),
            Some(("Status log", "missing input\nfallback output".to_owned()))
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
            auto_recalculate: true,
            show_input_overlay: false,
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
        assert!(!preferences.auto_recalculate);
        assert!(preferences.show_input_overlay);
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
        assert_eq!(value_for("bitmap_backend"), Some("native-potrace"));
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
