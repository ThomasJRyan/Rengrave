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
    pub(crate) fn loading_system_fonts() -> Self {
        Self {
            dir: None,
            is_system_fonts: true,
            entries: Vec::new(),
            error: None,
        }
    }

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
            if self.entries.is_empty() && self.error.is_none() {
                "System fonts loading".to_owned()
            } else {
                format!("System fonts ({})", self.entries.len())
            }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FontStyleChoice {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

impl FontStyleChoice {
    pub(crate) fn toggled_bold(self) -> Self {
        Self {
            bold: !self.bold,
            italic: self.italic,
        }
    }

    pub(crate) fn toggled_italic(self) -> Self {
        Self {
            bold: self.bold,
            italic: !self.italic,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FontStyleVariants {
    pub(crate) regular: PathBuf,
    pub(crate) bold: Option<PathBuf>,
    pub(crate) italic: Option<PathBuf>,
    pub(crate) bold_italic: Option<PathBuf>,
}

impl FontStyleVariants {
    pub(crate) fn path_for(&self, style: FontStyleChoice) -> Option<&PathBuf> {
        match (style.bold, style.italic) {
            (false, false) => Some(&self.regular),
            (true, false) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (true, true) => self.bold_italic.as_ref(),
        }
    }

    pub(crate) fn contains_path(&self, path: &Path) -> bool {
        self.style_for_path(path).is_some()
    }

    pub(crate) fn style_for_path(&self, path: &Path) -> Option<FontStyleChoice> {
        if self.regular == path {
            return Some(FontStyleChoice::default());
        }
        if self.bold.as_deref() == Some(path) {
            return Some(FontStyleChoice {
                bold: true,
                italic: false,
            });
        }
        if self.italic.as_deref() == Some(path) {
            return Some(FontStyleChoice {
                bold: false,
                italic: true,
            });
        }
        if self.bold_italic.as_deref() == Some(path) {
            return Some(FontStyleChoice {
                bold: true,
                italic: true,
            });
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontCatalogRow {
    pub(crate) entry: InputCatalogEntry,
    pub(crate) display_name: String,
    pub(crate) variants: FontStyleVariants,
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

/// Maximum filtered rows that may be rendered in their own TTF face. Loading
/// hundreds of font files on first expansion makes the catalog feel blocked;
/// the normal UI font is faster and clearer until search narrows the list.
pub(crate) const CATALOG_FONT_PREVIEW_THRESHOLD: usize = 32;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedFontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Other,
}

#[derive(Debug, Default)]
struct TtfFamilyBuilder {
    display_name: String,
    regular: Option<InputCatalogEntry>,
    bold: Option<PathBuf>,
    italic: Option<PathBuf>,
    bold_italic: Option<PathBuf>,
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
        if kind == InputCatalogKind::TtfFont && !is_valid_catalog_ttf(&path) {
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
        if kind == InputCatalogKind::TtfFont && !is_valid_catalog_ttf(&path) {
            continue;
        }
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

fn is_valid_catalog_ttf(path: &Path) -> bool {
    read_ttf(path, 5.0, false)
        .map(|font| !font.glyphs.is_empty() && "Text".chars().all(|ch| font.get_char(ch).is_some()))
        .unwrap_or(false)
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

pub(crate) fn font_catalog_rows_for_tool(
    entries: &[InputCatalogEntry],
    filter: InputCatalogFilter,
    tool_view: ToolView,
) -> Vec<FontCatalogRow> {
    let mut rows = Vec::new();
    let mut families = std::collections::BTreeMap::<String, TtfFamilyBuilder>::new();

    for entry in visible_input_catalog_entries_for_tool(entries, filter, tool_view) {
        match entry.kind {
            InputCatalogKind::CxfFont => {
                let display_name = catalog_font_stem(&entry);
                rows.push(FontCatalogRow {
                    variants: FontStyleVariants {
                        regular: entry.path.clone(),
                        bold: None,
                        italic: None,
                        bold_italic: None,
                    },
                    entry,
                    display_name,
                });
            }
            InputCatalogKind::TtfFont => {
                let (display_name, style) = parse_font_style(&catalog_font_stem(&entry));
                let key = display_name.to_lowercase();
                let family = families.entry(key).or_insert_with(|| TtfFamilyBuilder {
                    display_name,
                    ..TtfFamilyBuilder::default()
                });
                match style {
                    ParsedFontStyle::Regular => {
                        if family.regular.is_none() {
                            family.regular = Some(entry);
                        }
                    }
                    ParsedFontStyle::Bold => {
                        family.bold.get_or_insert(entry.path);
                    }
                    ParsedFontStyle::Italic => {
                        family.italic.get_or_insert(entry.path);
                    }
                    ParsedFontStyle::BoldItalic => {
                        family.bold_italic.get_or_insert(entry.path);
                    }
                    ParsedFontStyle::Other => {}
                }
            }
            _ => {}
        }
    }

    rows.extend(families.into_values().filter_map(|family| {
        let entry = family.regular?;
        Some(FontCatalogRow {
            display_name: family.display_name,
            variants: FontStyleVariants {
                regular: entry.path.clone(),
                bold: family.bold,
                italic: family.italic,
                bold_italic: family.bold_italic,
            },
            entry,
        })
    }));
    rows.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
    });
    rows
}

pub(crate) fn font_style_for_selected_path(
    entries: &[InputCatalogEntry],
    selected_path: Option<&Path>,
) -> Option<(FontStyleChoice, FontStyleVariants)> {
    let selected_path = selected_path?;
    font_catalog_rows_for_tool(
        entries,
        InputCatalogFilter::default(),
        ToolView::TextEngrave,
    )
    .into_iter()
    .find_map(|row| {
        row.variants
            .style_for_path(selected_path)
            .map(|style| (style, row.variants))
    })
}

fn catalog_font_stem(entry: &InputCatalogEntry) -> String {
    entry
        .path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| entry.name.clone())
}

fn parse_font_style(stem: &str) -> (String, ParsedFontStyle) {
    for (suffixes, style) in [
        (
            &[
                "BoldItalic",
                "Bold Italic",
                "Bold-Italic",
                "Bold_Italic",
                "BoldOblique",
                "Bold Oblique",
                "Bold-Oblique",
                "Bold_Oblique",
            ][..],
            ParsedFontStyle::BoldItalic,
        ),
        (&["Italic", "Oblique"][..], ParsedFontStyle::Italic),
        (&["Bold"][..], ParsedFontStyle::Bold),
        (
            &["Regular", "Roman", "Normal", "Book"][..],
            ParsedFontStyle::Regular,
        ),
        (
            &[
                "ThinItalic",
                "ThinOblique",
                "ExtraLightItalic",
                "ExtraLightOblique",
                "UltraLightItalic",
                "UltraLightOblique",
                "LightItalic",
                "LightOblique",
                "MediumItalic",
                "MediumOblique",
                "SemiBoldItalic",
                "SemiBoldOblique",
                "DemiBoldItalic",
                "DemiBoldOblique",
                "ExtraBoldItalic",
                "ExtraBoldOblique",
                "UltraBoldItalic",
                "UltraBoldOblique",
                "BlackItalic",
                "BlackOblique",
                "HeavyItalic",
                "HeavyOblique",
                "Thin",
                "ExtraLight",
                "UltraLight",
                "Light",
                "Medium",
                "SemiBold",
                "DemiBold",
                "ExtraBold",
                "UltraBold",
                "Black",
                "Heavy",
            ][..],
            ParsedFontStyle::Other,
        ),
    ] {
        for suffix in suffixes {
            if let Some(base) = strip_font_style_suffix(stem, suffix) {
                return (base, style);
            }
        }
    }
    (stem.to_owned(), ParsedFontStyle::Regular)
}

fn strip_font_style_suffix(stem: &str, suffix: &str) -> Option<String> {
    let start = stem.len().checked_sub(suffix.len())?;
    if !stem[start..].eq_ignore_ascii_case(suffix) {
        return None;
    }
    if start > 0 {
        let previous = stem[..start].chars().next_back()?;
        if !matches!(previous, '-' | '_' | ' ') {
            return None;
        }
    }
    let base = stem[..start].trim_end_matches(['-', '_', ' ']).trim();
    (!base.is_empty()).then(|| base.to_owned())
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

    fn catalog_entry(path: &str, kind: InputCatalogKind) -> InputCatalogEntry {
        InputCatalogEntry {
            path: PathBuf::from(path),
            name: Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path)
                .to_owned(),
            kind,
            size_bytes: 1,
        }
    }

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

    #[test]
    fn font_catalog_rows_group_ttf_style_variants_under_regular_face() {
        let entries = vec![
            catalog_entry("/fonts/AdwaitaMono-Regular.ttf", InputCatalogKind::TtfFont),
            catalog_entry("/fonts/AdwaitaMono-Bold.ttf", InputCatalogKind::TtfFont),
            catalog_entry("/fonts/AdwaitaMono-Italic.ttf", InputCatalogKind::TtfFont),
            catalog_entry(
                "/fonts/AdwaitaMono-BoldItalic.ttf",
                InputCatalogKind::TtfFont,
            ),
            catalog_entry(
                "/fonts/AdwaitaMono-ExtraLight.ttf",
                InputCatalogKind::TtfFont,
            ),
            catalog_entry(
                "/fonts/AdwaitaMono-LightItalic.ttf",
                InputCatalogKind::TtfFont,
            ),
            catalog_entry("/fonts/Solo.ttf", InputCatalogKind::TtfFont),
            catalog_entry("/fonts/romanc.cxf", InputCatalogKind::CxfFont),
        ];

        let rows = font_catalog_rows_for_tool(
            &entries,
            InputCatalogFilter::default(),
            ToolView::TextEngrave,
        );

        assert_eq!(
            rows.iter()
                .map(|row| row.display_name.as_str())
                .collect::<Vec<_>>(),
            vec!["AdwaitaMono", "romanc", "Solo"]
        );
        let adwaita = rows
            .iter()
            .find(|row| row.display_name == "AdwaitaMono")
            .unwrap();
        assert_eq!(
            adwaita.entry.path,
            PathBuf::from("/fonts/AdwaitaMono-Regular.ttf")
        );
        assert_eq!(
            adwaita.variants.path_for(FontStyleChoice {
                bold: true,
                italic: false,
            }),
            Some(&PathBuf::from("/fonts/AdwaitaMono-Bold.ttf"))
        );
        assert_eq!(
            adwaita.variants.path_for(FontStyleChoice {
                bold: false,
                italic: true,
            }),
            Some(&PathBuf::from("/fonts/AdwaitaMono-Italic.ttf"))
        );
        assert_eq!(
            adwaita.variants.path_for(FontStyleChoice {
                bold: true,
                italic: true,
            }),
            Some(&PathBuf::from("/fonts/AdwaitaMono-BoldItalic.ttf"))
        );
        assert!(
            rows.iter()
                .all(|row| row.display_name != "AdwaitaMono-ExtraLight")
        );
    }

    #[test]
    fn font_style_for_selected_path_finds_family_variant_state() {
        let entries = vec![
            catalog_entry("/fonts/Example-Regular.ttf", InputCatalogKind::TtfFont),
            catalog_entry("/fonts/Example-BoldItalic.ttf", InputCatalogKind::TtfFont),
        ];

        let (style, variants) = font_style_for_selected_path(
            &entries,
            Some(Path::new("/fonts/Example-BoldItalic.ttf")),
        )
        .unwrap();

        assert_eq!(
            style,
            FontStyleChoice {
                bold: true,
                italic: true,
            }
        );
        assert_eq!(
            variants.regular,
            PathBuf::from("/fonts/Example-Regular.ttf")
        );
        assert!(variants.path_for(style.toggled_italic()).is_none());
    }
}
