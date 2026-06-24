//! Input catalog: scanning directories and system font folders for usable
//! inputs (CXF/TTF fonts, DXF, bitmaps), the catalog data model, kind/filter
//! classification, and tool-aware visibility filtering.

use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InputCatalog {
    pub(crate) dir: Option<PathBuf>,
    pub(crate) is_system_fonts: bool,
    pub(crate) entries: Vec<InputCatalogEntry>,
    pub(crate) error: Option<String>,
}

impl InputCatalog {
    pub(crate) fn scan(dir: PathBuf) -> Self {
        match read_input_catalog_entries(&dir) {
            Ok(entries) => Self {
                dir: Some(dir),
                is_system_fonts: false,
                entries,
                error: None,
            },
            Err(err) => Self {
                dir: Some(dir),
                is_system_fonts: false,
                entries: Vec::new(),
                error: Some(err),
            },
        }
    }

    pub(crate) fn scan_system_fonts() -> Self {
        let entries = read_system_font_entries();
        let error = entries
            .is_empty()
            .then(|| "No system fonts were found in the standard font directories".to_owned());
        Self {
            dir: None,
            is_system_fonts: true,
            entries,
            error,
        }
    }

    pub(crate) fn source_label(&self) -> String {
        if self.is_system_fonts {
            format!("System fonts ({})", self.entries.len())
        } else if let Some(dir) = &self.dir {
            dir.display().to_string()
        } else {
            "No source scanned".to_owned()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputCatalogEntry {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) kind: InputCatalogKind,
    pub(crate) size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputCatalogKind {
    CxfFont,
    TtfFont,
    Dxf,
    Svg,
    Bitmap,
}

impl InputCatalogKind {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("cxf") => Some(Self::CxfFont),
            Some("ttf") => Some(Self::TtfFont),
            Some("dxf") => Some(Self::Dxf),
            Some("svg") => Some(Self::Svg),
            Some(
                "bmp" | "gif" | "jpg" | "jpeg" | "png" | "tif" | "tiff" | "pbm" | "ppm" | "pgm"
                | "pnm",
            ) => Some(Self::Bitmap),
            _ => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CxfFont => "CXF",
            Self::TtfFont => "TTF",
            Self::Dxf => "DXF",
            Self::Svg => "SVG",
            Self::Bitmap => "Bitmap",
        }
    }

    pub(crate) fn sort_rank(self) -> u8 {
        match self {
            Self::CxfFont => 0,
            Self::TtfFont => 1,
            Self::Dxf => 2,
            Self::Svg => 3,
            Self::Bitmap => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputCatalogFilter {
    pub(crate) cxf: bool,
    pub(crate) ttf: bool,
    pub(crate) dxf: bool,
    pub(crate) svg: bool,
    pub(crate) bitmap: bool,
}

impl Default for InputCatalogFilter {
    fn default() -> Self {
        Self {
            cxf: true,
            ttf: true,
            dxf: true,
            svg: true,
            bitmap: true,
        }
    }
}

impl InputCatalogFilter {
    pub(crate) fn accepts(self, kind: InputCatalogKind) -> bool {
        match kind {
            InputCatalogKind::CxfFont => self.cxf,
            InputCatalogKind::TtfFont => self.ttf,
            InputCatalogKind::Dxf => self.dxf,
            InputCatalogKind::Svg => self.svg,
            InputCatalogKind::Bitmap => self.bitmap,
        }
    }
}

pub(crate) fn input_catalog_start_dir(
    input: &Option<PathBuf>,
    default_dir: &Option<PathBuf>,
) -> PathBuf {
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

/// Standard font directories for the current operating system. Only existing
/// directories are returned so callers can scan them directly.
pub(crate) fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(windir) = env::var_os("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = user_home_dir() {
            dirs.push(home.join("Library").join("Fonts"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(data) = env::var_os("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(data).join("fonts"));
        }
        if let Some(home) = user_home_dir() {
            dirs.push(home.join(".fonts"));
            dirs.push(home.join(".local").join("share").join("fonts"));
        }
    }
    dirs.retain(|dir| dir.is_dir());
    dirs.dedup();
    dirs
}

/// Maximum directory depth to recurse when scanning system font folders. Font
/// directories nest by family/foundry on some platforms, but very deep trees
/// are unusual and bounded recursion keeps the scan responsive.
const SYSTEM_FONT_SCAN_MAX_DEPTH: usize = 6;

/// Maximum number of catalog entries rendered at once. Scanning the system font
/// directories can yield thousands of fonts, so the list is capped to keep the
/// UI responsive while the search field narrows results.
pub(crate) const CATALOG_DISPLAY_LIMIT: usize = 250;
const CATALOG_FONT_RENDER_LIMIT: usize = 128;

#[derive(Debug, Default)]
pub(crate) struct CatalogFontRegistry {
    signature: Vec<PathBuf>,
    families_by_path: std::collections::BTreeMap<PathBuf, egui::FontFamily>,
}

impl CatalogFontRegistry {
    pub(crate) fn refresh(&mut self, ctx: &egui::Context, entries: &[InputCatalogEntry]) -> bool {
        let signature = catalog_ttf_font_paths(entries, CATALOG_FONT_RENDER_LIMIT);
        if signature == self.signature {
            return false;
        }

        let mut definitions = egui::FontDefinitions::default();
        let mut families_by_path = std::collections::BTreeMap::new();
        for (index, path) in signature.iter().enumerate() {
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            if ttf_parser::Face::parse(&bytes, 0).is_err() {
                continue;
            }

            let name = format!("rengrave-catalog-font-{index}");
            definitions.font_data.insert(
                name.clone(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            let family = egui::FontFamily::Name(name.clone().into());
            definitions.families.insert(family.clone(), vec![name]);
            families_by_path.insert(path.clone(), family);
        }

        ctx.set_fonts(definitions);
        self.signature = signature;
        self.families_by_path = families_by_path;
        true
    }

    pub(crate) fn family_for_path(&self, path: &Path) -> Option<egui::FontFamily> {
        self.families_by_path.get(path).cloned()
    }
}

fn catalog_ttf_font_paths(entries: &[InputCatalogEntry], limit: usize) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|entry| entry.kind == InputCatalogKind::TtfFont)
        .take(limit)
        .map(|entry| entry.path.clone())
        .collect()
}

/// Recursively collects CXF/TTF fonts from the platform font directories.
pub(crate) fn read_system_font_entries() -> Vec<InputCatalogEntry> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in system_font_dirs() {
        collect_font_entries(&dir, 0, &mut entries, &mut seen);
    }
    entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    entries
}

fn collect_font_entries(
    dir: &Path,
    depth: usize,
    entries: &mut Vec<InputCatalogEntry>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    if depth > SYSTEM_FONT_SCAN_MAX_DEPTH {
        return;
    }
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_font_entries(&path, depth + 1, entries, seen);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(kind @ (InputCatalogKind::CxfFont | InputCatalogKind::TtfFont)) =
            InputCatalogKind::from_path(&path)
        else {
            continue;
        };
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let size_bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        entries.push(InputCatalogEntry {
            path,
            name,
            kind,
            size_bytes,
        });
    }
}

pub(crate) fn read_input_catalog_entries(dir: &Path) -> Result<Vec<InputCatalogEntry>, String> {
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

pub(crate) fn visible_input_catalog_entries_for_tool(
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

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_font_registry_tracks_ttf_entries_only_with_limit() {
        let entries = [
            InputCatalogEntry {
                path: PathBuf::from("/tmp/a.cxf"),
                name: "a.cxf".to_owned(),
                kind: InputCatalogKind::CxfFont,
                size_bytes: 1,
            },
            InputCatalogEntry {
                path: PathBuf::from("/tmp/b.ttf"),
                name: "b.ttf".to_owned(),
                kind: InputCatalogKind::TtfFont,
                size_bytes: 1,
            },
            InputCatalogEntry {
                path: PathBuf::from("/tmp/c.ttf"),
                name: "c.ttf".to_owned(),
                kind: InputCatalogKind::TtfFont,
                size_bytes: 1,
            },
        ];

        assert_eq!(
            catalog_ttf_font_paths(&entries, 1),
            vec![PathBuf::from("/tmp/b.ttf")]
        );
        assert_eq!(
            catalog_ttf_font_paths(&entries, 8),
            vec![PathBuf::from("/tmp/b.ttf"), PathBuf::from("/tmp/c.ttf")]
        );
    }
}
