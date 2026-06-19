//! File browsing: the native/in-app file picker targets, the embedded
//! `FileBrowser` directory navigator, input dialog filters, and directory
//! listing helpers.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileBrowserTarget {
    Settings,
    SettingsOutput,
    Input,
    DefaultDir,
    GcodeOutput,
}

impl FileBrowserTarget {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::SettingsOutput => "settings output",
            Self::Input => "input",
            Self::DefaultDir => "default directory",
            Self::GcodeOutput => "G-code output",
        }
    }

    pub(crate) fn dialog_title(self) -> &'static str {
        match self {
            Self::Settings => "Open Settings",
            Self::SettingsOutput => "Save Settings As",
            Self::Input => "Open Input",
            Self::DefaultDir => "Choose Default Directory",
            Self::GcodeOutput => "Choose G-code Output",
        }
    }

    pub(crate) fn default_file_name(self) -> Option<&'static str> {
        match self {
            Self::SettingsOutput => Some("rengrave_settings.ngc"),
            Self::GcodeOutput => Some("rengrave_output.ngc"),
            _ => None,
        }
    }

    pub(crate) fn can_select(self, path: &Path) -> bool {
        match self {
            Self::DefaultDir => path.is_dir(),
            Self::Settings => path.is_file(),
            Self::Input => path.is_file() || path.is_dir(),
            Self::SettingsOutput | Self::GcodeOutput => !path.is_dir(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionFollowup {
    None,
    LoadDocument,
    SaveSettings,
    StartCalculation,
}

pub(crate) fn selection_followup(target: FileBrowserTarget) -> SelectionFollowup {
    match target {
        FileBrowserTarget::Settings => SelectionFollowup::LoadDocument,
        FileBrowserTarget::SettingsOutput => SelectionFollowup::SaveSettings,
        FileBrowserTarget::Input => SelectionFollowup::StartCalculation,
        FileBrowserTarget::DefaultDir | FileBrowserTarget::GcodeOutput => SelectionFollowup::None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileBrowser {
    pub(crate) target: FileBrowserTarget,
    pub(crate) current_dir: PathBuf,
    pub(crate) selected_path: Option<PathBuf>,
    pub(crate) entries: Vec<BrowserEntry>,
    pub(crate) error: Option<String>,
}

impl FileBrowser {
    pub(crate) fn new(target: FileBrowserTarget, current_dir: PathBuf) -> Self {
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

    pub(crate) fn refresh(&mut self) {
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

    pub(crate) fn set_dir(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.current_dir = path;
            self.selected_path = None;
            self.refresh();
        } else {
            self.error = Some(format!("not a directory: {}", path.display()));
        }
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) -> BrowserAction {
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
pub(crate) enum BrowserAction {
    Keep,
    Close,
    Select(PathBuf),
}

pub(crate) fn browser_start_dir(
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

/// The file-type filter applied to the input picker, chosen by workbench so a
/// font workbench shows fonts and an image workbench shows images directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputDialogFilter {
    Fonts,
    Images,
    All,
}

impl InputDialogFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Fonts => "Fonts (CXF, TTF)",
            Self::Images => "Images",
            Self::All => "R-Engrave inputs",
        }
    }

    pub(crate) fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Fonts => &["cxf", "ttf"],
            Self::Images => &[
                "dxf", "bmp", "gif", "jpg", "jpeg", "png", "tif", "tiff", "pbm", "ppm", "pgm",
                "pnm",
            ],
            Self::All => &[
                "cxf", "ttf", "dxf", "bmp", "gif", "jpg", "jpeg", "png", "tif", "tiff", "pbm",
                "ppm", "pgm", "pnm",
            ],
        }
    }
}

pub(crate) fn choose_native_path(
    target: FileBrowserTarget,
    current_value: &str,
    default_dir: Option<PathBuf>,
    input_filter: InputDialogFilter,
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
            .add_filter(input_filter.label(), input_filter.extensions())
            .add_filter("All files", &["*"])
            .pick_file(),
        FileBrowserTarget::GcodeOutput => dialog
            .set_file_name(output_file_name(current_value, target))
            .add_filter("G-code", &["ngc", "nc", "tap"])
            .save_file(),
    }
}

pub(crate) fn output_file_name(current_value: &str, target: FileBrowserTarget) -> String {
    path_from_text(current_value)
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .or_else(|| target.default_file_name().map(str::to_owned))
        .unwrap_or_else(|| "rengrave_output".to_owned())
}

pub(crate) fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

pub(crate) fn read_browser_entries(dir: &Path) -> Result<Vec<BrowserEntry>, String> {
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
