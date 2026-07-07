use std::path::{Path, PathBuf};

use crate::bitmap::{BitmapError, vectorize_bitmap_to_dxf_with_cancel};
use crate::cleanup::{CleanupBit, CleanupOptions, generate_cleanup_points_with_cancel};
use crate::dxf::{DxfError, dxf_font_from_str_with_cancel, read_dxf_font_with_cancel};
use crate::export::{ExportOptions, write_dxf, write_svg_with_circle};
use crate::external::is_bitmap_input;
use crate::font::{Font, FontError, read_cxf_with_cancel, read_ttf_with_cancel};
use crate::gcode::{
    GcodeOptions, write_cleanup_gcode, write_engrave_gcode, write_engrave_gcode_with_circle,
    write_profile_gcode, write_vcarve_gcode,
};
use crate::geometry::Point;
use crate::layout::{Bounds, EngraveCircle, EngraveSegment, LayoutSettings, layout_text};
use crate::profile::{ProfileOptions, ProfileTool, generate_profile_operations_for_segments};
use crate::project::{DocumentError, DocumentRequest, load_document};
use crate::project::{InputKind, resolve_input_kind};
use crate::settings::{LegacySetting, LegacySettings, get_legacy_bool};
use crate::svg::{is_svg_input, read_svg_font_with_cancel};
use crate::vcarve::{
    VCarveCheckMode, VCarveOptions, generate_vcarve_points_with_cancel,
    sort_image_segments_for_vcarve,
};
use crate::{FENGRAVE_VERSION, RENGRAVE_VERSION};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchRequest {
    pub batch: bool,
    pub gcode_file: Option<PathBuf>,
    pub font_or_image: Option<PathBuf>,
    pub default_dir: Option<PathBuf>,
    pub text: Option<String>,
    pub output: Option<PathBuf>,
    pub svg_output: Option<PathBuf>,
    pub dxf_output: Option<PathBuf>,
    pub include_secondary: bool,
    pub settings_overrides: Vec<LegacySetting>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchOutput {
    pub gcode: String,
    pub warnings: Vec<String>,
    pub secondary_gcode: Vec<SecondaryGcode>,
    pub svg: Option<String>,
    pub dxf: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondaryGcode {
    pub suffix: String,
    pub gcode: String,
}

pub fn secondary_output_path(path: &Path, suffix: &str) -> PathBuf {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchProgress {
    LoadingDocument,
    LoadingTextFont,
    LoadingDxf,
    LoadingSvg,
    VectorizingBitmap,
    LayingOutText,
    LayingOutImage,
    PreparingExports,
    WritingEngrave,
    CalculatingVCarve,
    CalculatingStraightCleanup,
    CalculatingVBitCleanup,
    WritingVCarve,
    WritingProfileChamfer,
    WritingProfileCut,
    RenderingPrimary,
    RenderingSecondary,
    RenderingSettingsOnly,
    Finished,
}

impl BatchProgress {
    pub fn status_text(self) -> &'static str {
        match self {
            Self::LoadingDocument => "Loading document",
            Self::LoadingTextFont => "Loading text font",
            Self::LoadingDxf => "Loading DXF input",
            Self::LoadingSvg => "Loading SVG input",
            Self::VectorizingBitmap => "Vectorizing bitmap",
            Self::LayingOutText => "Laying out text",
            Self::LayingOutImage => "Laying out image",
            Self::PreparingExports => "Preparing exports",
            Self::WritingEngrave => "Writing engrave toolpath",
            Self::CalculatingVCarve => "Calculating V-carve",
            Self::CalculatingStraightCleanup => "Calculating straight cleanup",
            Self::CalculatingVBitCleanup => "Calculating V-bit cleanup",
            Self::WritingVCarve => "Writing V-carve toolpath",
            Self::WritingProfileChamfer => "Writing profile chamfer",
            Self::WritingProfileCut => "Writing profile cut",
            Self::RenderingPrimary => "Rendering primary G-code",
            Self::RenderingSecondary => "Rendering cleanup G-code",
            Self::RenderingSettingsOnly => "Rendering settings-only output",
            Self::Finished => "Calculation complete",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("generation canceled")]
    Canceled,
}

pub fn prepare_batch_output(request: &BatchRequest) -> Result<BatchOutput, BatchError> {
    prepare_batch_output_with_cancel(request, || false)
}

pub fn prepare_batch_output_with_cancel(
    request: &BatchRequest,
    cancel: impl Fn() -> bool,
) -> Result<BatchOutput, BatchError> {
    prepare_batch_output_with_cancel_and_progress(request, cancel, |_| {})
}

pub fn prepare_batch_output_with_cancel_and_progress(
    request: &BatchRequest,
    cancel: impl Fn() -> bool,
    progress: impl Fn(BatchProgress),
) -> Result<BatchOutput, BatchError> {
    check_canceled(&cancel)?;
    progress(BatchProgress::LoadingDocument);
    let document = load_document(&DocumentRequest {
        gcode_file: request.gcode_file.clone(),
        font_or_image: request.font_or_image.clone(),
        default_dir: request.default_dir.clone(),
        text: request.text.clone(),
        settings_overrides: request.settings_overrides.clone(),
    })?;
    check_canceled(&cancel)?;

    let mut warnings = document.warnings;
    let generated_gcode = generate_engrave_gcode(
        &document.settings,
        &document.text,
        request.output.is_some() || request.include_secondary,
        request.svg_output.is_some(),
        request.dxf_output.is_some(),
        &mut warnings,
        &cancel,
        &progress,
    )?;
    check_canceled(&cancel)?;
    let gcode = if let Some(gcode_lines) = generated_gcode {
        check_canceled(&cancel)?;
        progress(BatchProgress::RenderingPrimary);
        let primary = render_gcode(&document.settings, &document.text, &gcode_lines.primary);
        check_canceled(&cancel)?;
        let mut secondary_gcode = Vec::new();
        for secondary in gcode_lines.secondary {
            progress(BatchProgress::RenderingSecondary);
            secondary_gcode.push(SecondaryGcode {
                suffix: secondary.suffix,
                gcode: render_secondary_gcode(&document.settings, &document.text, &secondary.lines),
            });
        }
        progress(BatchProgress::Finished);
        return Ok(BatchOutput {
            gcode: primary,
            warnings,
            secondary_gcode,
            svg: gcode_lines.svg,
            dxf: gcode_lines.dxf,
        });
    } else {
        warnings.push(
            "settings-only output generated because no toolpath could be produced".to_owned(),
        );
        progress(BatchProgress::RenderingSettingsOnly);
        render_settings_only_gcode(&document.settings, &document.text)
    };

    progress(BatchProgress::Finished);
    Ok(BatchOutput {
        gcode,
        warnings,
        secondary_gcode: Vec::new(),
        svg: None,
        dxf: None,
    })
}

/// The laid-out input text outline in final engraving coordinates (real
/// mm/inch units), matching the coordinate space of the generated toolpath.
#[derive(Debug, Clone, PartialEq)]
pub struct TextOutlinePreview {
    pub segments: Vec<EngraveSegment>,
    pub bounds: Option<Bounds>,
}

/// Lays out the request's input with the same font/vectorization and settings
/// the toolpath uses, returning the outline in final engraving coordinates.
/// Returns `Ok(None)` when no input font/segments can be produced.
///
/// For text inputs this lays out the document text; for image inputs (bitmaps
/// or DXF artwork) it vectorizes the same way the toolpath does and lays out
/// the resulting artwork, so the overlay aligns with the generated toolpath in
/// both cases.
///
/// This reuses the exact document-load and layout path of the batch pipeline,
/// so the returned segments align with the generated toolpath and can be
/// overlaid directly on a toolpath preview.
pub fn layout_text_outline(
    request: &BatchRequest,
) -> Result<Option<TextOutlinePreview>, BatchError> {
    let document = load_document(&DocumentRequest {
        gcode_file: request.gcode_file.clone(),
        font_or_image: request.font_or_image.clone(),
        default_dir: request.default_dir.clone(),
        text: request.text.clone(),
        settings_overrides: request.settings_overrides.clone(),
    })?;
    let settings = &document.settings;

    let segarc = settings
        .get_last("segarc")
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);

    let (font, layout_input) = if settings.get_last("input_type") == Some("image") {
        let InputKind::Image(path) = resolve_input_kind(settings) else {
            return Ok(None);
        };
        let is_dxf = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("dxf"))
            .unwrap_or(false);
        let font = if is_dxf {
            match read_dxf_font_with_cancel(&path, segarc, &|| false) {
                Ok(font) => font,
                Err(_) => return Ok(None),
            }
        } else if is_svg_input(&path) {
            match read_svg_font_with_cancel(&path, &|| false) {
                Ok(font) => font,
                Err(_) => return Ok(None),
            }
        } else if is_bitmap_input(&path) {
            match vectorize_bitmap_to_dxf_with_cancel(&path, settings, &|| false) {
                Ok(dxf) => match bitmap_font_from_dxf_with_cancel(&dxf, segarc, &|| false) {
                    Ok(font) => font,
                    Err(_) => return Ok(None),
                },
                Err(_) => return Ok(None),
            }
        } else {
            return Ok(None);
        };
        (font, "F".to_owned())
    } else {
        let font = match resolve_input_kind(settings) {
            InputKind::CxfFont(path) => match read_cxf_with_cancel(&path, segarc, &|| false) {
                Ok(font) => font,
                Err(_) => return Ok(None),
            },
            InputKind::TtfFont(path) => {
                let extended_chars = get_legacy_bool(settings, "ext_char", false);
                match read_ttf_with_cancel(&path, segarc, extended_chars, &|| false) {
                    Ok(font) => font,
                    Err(_) => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
        (font, document.text.clone())
    };

    let layout_settings = LayoutSettings::from_legacy(settings);
    let layout = layout_text(&font, &layout_input, &layout_settings);
    if layout.segments.is_empty() {
        return Ok(None);
    }
    Ok(Some(TextOutlinePreview {
        segments: layout.segments,
        bounds: layout.bounds,
    }))
}

fn generate_engrave_gcode(
    settings: &LegacySettings,
    text: &str,
    include_secondary: bool,
    include_svg: bool,
    include_dxf: bool,
    warnings: &mut Vec<String>,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(BatchProgress),
) -> Result<Option<GeneratedToolpaths>, BatchError> {
    check_canceled(cancel)?;
    if settings.get_last("input_type") == Some("image") {
        return generate_dxf_engrave_gcode(
            settings,
            include_secondary,
            include_svg,
            include_dxf,
            warnings,
            cancel,
            progress,
        );
    }

    generate_text_engrave_gcode(
        settings,
        text,
        include_secondary,
        include_svg,
        include_dxf,
        warnings,
        cancel,
        progress,
    )
}

fn generate_text_engrave_gcode(
    settings: &LegacySettings,
    text: &str,
    include_secondary: bool,
    include_svg: bool,
    include_dxf: bool,
    warnings: &mut Vec<String>,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(BatchProgress),
) -> Result<Option<GeneratedToolpaths>, BatchError> {
    check_canceled(cancel)?;
    let input = resolve_input_kind(settings);
    let segarc = settings
        .get_last("segarc")
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);
    progress(BatchProgress::LoadingTextFont);
    let font = match input {
        InputKind::CxfFont(path) => match read_cxf_with_cancel(&path, segarc, cancel) {
            Ok(font) => font,
            Err(FontError::Canceled) => return Err(BatchError::Canceled),
            Err(err) => {
                warnings.push(err.to_string());
                return Ok(None);
            }
        },
        InputKind::TtfFont(path) => {
            let extended_chars = get_legacy_bool(settings, "ext_char", false);
            match read_ttf_with_cancel(&path, segarc, extended_chars, cancel) {
                Ok(font) => font,
                Err(FontError::Canceled) => return Err(BatchError::Canceled),
                Err(err) => {
                    warnings.push(err.to_string());
                    return Ok(None);
                }
            }
        }
        _ => {
            warnings.push(
                "only CXF and TTF text fonts are currently generated in Rust batch mode".to_owned(),
            );
            return Ok(None);
        }
    };

    check_canceled(cancel)?;
    progress(BatchProgress::LayingOutText);
    let layout_settings = LayoutSettings::from_legacy(settings);
    let layout = layout_text(&font, text, &layout_settings);
    check_canceled(cancel)?;
    if !layout.missing_chars.is_empty() {
        let missing: String = layout.missing_chars.iter().collect();
        warnings.push(format!("characters not found in font file: {missing}"));
    }
    if layout.segments.is_empty() {
        warnings.push("no engraveable text segments were generated".to_owned());
        return Ok(None);
    }

    write_layout_gcode(
        settings,
        &layout.segments,
        layout.bounds,
        layout.circle_border,
        include_secondary,
        include_svg,
        include_dxf,
        warnings,
        cancel,
        progress,
    )
}

fn generate_dxf_engrave_gcode(
    settings: &LegacySettings,
    include_secondary: bool,
    include_svg: bool,
    include_dxf: bool,
    warnings: &mut Vec<String>,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(BatchProgress),
) -> Result<Option<GeneratedToolpaths>, BatchError> {
    check_canceled(cancel)?;
    let InputKind::Image(path) = resolve_input_kind(settings) else {
        warnings.push("image input path is missing".to_owned());
        return Ok(None);
    };

    let is_dxf = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("dxf"))
        .unwrap_or(false);
    let segarc = settings
        .get_last("segarc")
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);
    let font = if is_dxf {
        progress(BatchProgress::LoadingDxf);
        match read_dxf_font_with_cancel(&path, segarc, cancel) {
            Ok(font) => font,
            Err(DxfError::Canceled) => return Err(BatchError::Canceled),
            Err(err) => {
                warnings.push(err.to_string());
                return Ok(None);
            }
        }
    } else if is_svg_input(&path) {
        progress(BatchProgress::LoadingSvg);
        match read_svg_font_with_cancel(&path, cancel) {
            Ok(font) => font,
            Err(crate::svg::SvgError::Canceled) => return Err(BatchError::Canceled),
            Err(err) => {
                warnings.push(err.to_string());
                return Ok(None);
            }
        }
    } else if is_bitmap_input(&path) {
        progress(BatchProgress::VectorizingBitmap);
        match vectorize_bitmap_to_dxf_with_cancel(&path, settings, cancel) {
            Ok(dxf) => match bitmap_font_from_dxf_with_cancel(&dxf, segarc, cancel) {
                Ok(font) => font,
                Err(DxfError::Canceled) => return Err(BatchError::Canceled),
                Err(err) => {
                    warnings.push(err.to_string());
                    return Ok(None);
                }
            },
            Err(BitmapError::Canceled) => return Err(BatchError::Canceled),
            Err(err) => {
                warnings.push(err.to_string());
                return Ok(None);
            }
        }
    } else {
        warnings.push("unsupported image input format".to_owned());
        return Ok(None);
    };
    check_canceled(cancel)?;
    progress(BatchProgress::LayingOutImage);
    let layout = layout_text(&font, "F", &LayoutSettings::from_legacy(settings));
    check_canceled(cancel)?;
    if layout.segments.is_empty() {
        warnings.push("DXF contained no engraveable segments".to_owned());
        return Ok(None);
    }

    write_layout_gcode(
        settings,
        &layout.segments,
        layout.bounds,
        layout.circle_border,
        include_secondary,
        include_svg,
        include_dxf,
        warnings,
        cancel,
        progress,
    )
}

fn bitmap_font_from_dxf_with_cancel(
    dxf: &str,
    segarc: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Font, DxfError> {
    let mut font = dxf_font_from_str_with_cancel(dxf, segarc, cancel)?;
    normalize_bitmap_font_to_content_origin(&mut font);
    Ok(font)
}

fn normalize_bitmap_font_to_content_origin(font: &mut Font) {
    let Some(min) = font_min_point(font) else {
        return;
    };

    if min.x.abs() <= f64::EPSILON && min.y.abs() <= f64::EPSILON {
        return;
    }

    for glyph in font.glyphs.values_mut() {
        for stroke in &mut glyph.strokes {
            stroke.start.x -= min.x;
            stroke.end.x -= min.x;
            stroke.start.y -= min.y;
            stroke.end.y -= min.y;
        }
    }
}

fn font_min_point(font: &Font) -> Option<Point> {
    let mut min: Option<Point> = None;

    for glyph in font.glyphs.values() {
        for stroke in &glyph.strokes {
            for point in [stroke.start, stroke.end] {
                if !point.x.is_finite() || !point.y.is_finite() {
                    continue;
                }
                min = Some(match min {
                    Some(current) => Point::new(current.x.min(point.x), current.y.min(point.y)),
                    None => point,
                });
            }
        }
    }

    min
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedToolpaths {
    primary: Vec<String>,
    secondary: Vec<GeneratedSecondary>,
    svg: Option<String>,
    dxf: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedSecondary {
    suffix: String,
    lines: Vec<String>,
}

fn write_layout_gcode(
    settings: &LegacySettings,
    segments: &[crate::layout::EngraveSegment],
    bounds: Option<Bounds>,
    circle: Option<EngraveCircle>,
    include_secondary: bool,
    include_svg: bool,
    include_dxf: bool,
    warnings: &mut Vec<String>,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(BatchProgress),
) -> Result<Option<GeneratedToolpaths>, BatchError> {
    check_canceled(cancel)?;
    let gcode_options = GcodeOptions::from_legacy(settings);
    progress(BatchProgress::PreparingExports);
    let exports = build_exports(settings, segments, circle, include_svg, include_dxf);
    check_canceled(cancel)?;
    if settings.get_last("cut_type") != Some("v-carve") {
        progress(BatchProgress::WritingEngrave);
        let secondary = profile_secondary_gcode(
            settings,
            &gcode_options,
            bounds,
            segments,
            include_secondary,
            warnings,
            cancel,
            progress,
        )?;
        return Ok(Some(GeneratedToolpaths {
            primary: write_engrave_gcode_with_circle(segments, circle, &gcode_options),
            secondary,
            svg: exports.svg,
            dxf: exports.dxf,
        }));
    }

    let mut vcarve_options = VCarveOptions::from_legacy(settings);
    if settings.get_last("input_type") == Some("image") {
        vcarve_options.check_mode = VCarveCheckMode::All;
    }
    if vcarve_options.bit_shape == crate::vcarve::BitShape::Flat {
        progress(BatchProgress::WritingEngrave);
        let secondary = profile_secondary_gcode(
            settings,
            &gcode_options,
            bounds,
            segments,
            include_secondary,
            warnings,
            cancel,
            progress,
        )?;
        return Ok(Some(GeneratedToolpaths {
            primary: write_engrave_gcode(segments, &gcode_options),
            secondary,
            svg: exports.svg,
            dxf: exports.dxf,
        }));
    }

    check_canceled(cancel)?;
    progress(BatchProgress::CalculatingVCarve);
    let sorted_image_segments;
    let vcarve_segments = if settings.get_last("input_type") == Some("image") {
        sorted_image_segments = sort_image_segments_for_vcarve(segments, gcode_options.accuracy);
        &sorted_image_segments
    } else {
        segments
    };
    let points = generate_vcarve_points_with_cancel(
        vcarve_segments,
        &vcarve_options,
        gcode_options.accuracy,
        cancel,
    )
    .map_err(|_| BatchError::Canceled)?;
    check_canceled(cancel)?;
    if points.is_empty() {
        warnings.push("v-carve generated no toolpath points".to_owned());
        return Ok(None);
    }

    let mut secondary = Vec::new();
    if include_secondary {
        let cleanup_options = CleanupOptions::from_legacy(settings);
        for bit in [CleanupBit::Straight, CleanupBit::VBit] {
            check_canceled(cancel)?;
            progress(match bit {
                CleanupBit::Straight => BatchProgress::CalculatingStraightCleanup,
                CleanupBit::VBit => BatchProgress::CalculatingVBitCleanup,
            });
            let cleanup_points = generate_cleanup_points_with_cancel(
                vcarve_segments,
                &cleanup_options,
                &vcarve_options,
                bit,
                gcode_options.accuracy,
                cancel,
            )
            .map_err(|_| BatchError::Canceled)?;
            check_canceled(cancel)?;
            if cleanup_points.is_empty() {
                continue;
            }
            secondary.push(GeneratedSecondary {
                suffix: bit.suffix().to_owned(),
                lines: write_cleanup_gcode(
                    &cleanup_points,
                    &gcode_options,
                    &cleanup_options,
                    &vcarve_options,
                    bit,
                ),
            });
        }
    }
    secondary.extend(profile_secondary_gcode(
        settings,
        &gcode_options,
        bounds,
        segments,
        include_secondary,
        warnings,
        cancel,
        progress,
    )?);

    check_canceled(cancel)?;
    progress(BatchProgress::WritingVCarve);
    Ok(Some(GeneratedToolpaths {
        primary: write_vcarve_gcode(&points, &gcode_options, &vcarve_options),
        secondary,
        svg: exports.svg,
        dxf: exports.dxf,
    }))
}

fn check_canceled(cancel: &dyn Fn() -> bool) -> Result<(), BatchError> {
    if cancel() {
        Err(BatchError::Canceled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GeneratedExports {
    svg: Option<String>,
    dxf: Option<String>,
}

fn build_exports(
    settings: &LegacySettings,
    segments: &[crate::layout::EngraveSegment],
    circle: Option<EngraveCircle>,
    include_svg: bool,
    include_dxf: bool,
) -> GeneratedExports {
    GeneratedExports {
        svg: include_svg.then(|| {
            write_svg_with_circle(segments, circle, &ExportOptions::from_legacy(settings))
        }),
        dxf: include_dxf.then(|| write_dxf(segments)),
    }
}

fn profile_secondary_gcode(
    settings: &LegacySettings,
    gcode_options: &GcodeOptions,
    bounds: Option<Bounds>,
    segments: &[crate::layout::EngraveSegment],
    include_secondary: bool,
    warnings: &mut Vec<String>,
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(BatchProgress),
) -> Result<Vec<GeneratedSecondary>, BatchError> {
    if !include_secondary {
        return Ok(Vec::new());
    }

    let profile_options = ProfileOptions::from_legacy(settings);
    if !profile_options.enabled {
        return Ok(Vec::new());
    }
    if !profile_options.is_usable() {
        warnings.push(
            "profile cut skipped because material thickness and endmill diameter must be positive"
                .to_owned(),
        );
        return Ok(Vec::new());
    }
    let Some(bounds) = bounds else {
        warnings.push("profile cut skipped because project bounds are unavailable".to_owned());
        return Ok(Vec::new());
    };

    let operations = generate_profile_operations_for_segments(
        bounds,
        segments,
        &profile_options,
        gcode_options.accuracy,
    );
    if operations.is_empty() {
        warnings.push("profile cut skipped because no profile path could be generated".to_owned());
        return Ok(Vec::new());
    }

    let mut secondary = Vec::new();
    for operation in operations {
        check_canceled(cancel)?;
        progress(match operation.tool {
            ProfileTool::VBitChamfer => BatchProgress::WritingProfileChamfer,
            ProfileTool::StraightEndmill => BatchProgress::WritingProfileCut,
        });
        let lines = write_profile_gcode(&operation, gcode_options);
        if lines.is_empty() {
            continue;
        }
        secondary.push(GeneratedSecondary {
            suffix: operation.tool.suffix().to_owned(),
            lines,
        });
    }
    Ok(secondary)
}

fn render_settings_only_gcode(settings: &LegacySettings, text: &str) -> String {
    let mut lines = render_settings_header(settings, text);
    lines.push("( R-Engrave settings-only output: no toolpath was generated )".to_owned());
    lines.push(String::new());
    lines.join("\n")
}

fn render_gcode(settings: &LegacySettings, text: &str, gcode_lines: &[String]) -> String {
    let mut lines = render_settings_header(settings, text);
    lines.extend(gcode_lines.iter().cloned());
    lines.push(String::new());
    lines.join("\n")
}

fn render_secondary_gcode(settings: &LegacySettings, text: &str, gcode_lines: &[String]) -> String {
    let mut lines = render_settings_header(settings, text);
    lines.extend(gcode_lines.iter().cloned());
    lines.push(String::new());
    lines.join("\n")
}

fn render_settings_header(settings: &LegacySettings, text: &str) -> Vec<String> {
    if get_legacy_bool(settings, "no_comments", false) {
        return Vec::new();
    }

    let mut lines = vec![
        format!("( Code generated by r-engrave-{RENGRAVE_VERSION} )"),
        format!("( Compatibility target: f-engrave-{FENGRAVE_VERSION}.py )"),
        "(Settings used in f-engrave when this file was created)".to_owned(),
    ];
    if settings.get_last("input_type") == Some("text") {
        lines.push(format!("(Engrave Text:{} )", sanitized_text_comment(text)));
    }
    lines.push("(=========================================================)".to_owned());

    lines.extend(settings.to_comments());
    lines.push("(#########################################################)".to_owned());
    lines
}

fn sanitized_text_comment(text: &str) -> String {
    let mut output = String::new();
    for ch in text.chars().take(40) {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
        } else {
            output.push(' ');
        }
    }
    if text.chars().count() > 40 {
        output.push_str("___");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::fs;

    #[test]
    fn batch_cancel_hook_stops_before_document_load() {
        let err = prepare_batch_output_with_cancel(&BatchRequest::default(), || true).unwrap_err();

        assert_eq!(err.to_string(), "generation canceled");
    }

    #[test]
    fn batch_cancel_hook_stops_at_stage_boundaries() {
        let calls = Cell::new(0usize);
        let err = prepare_batch_output_with_cancel(&BatchRequest::default(), || {
            let next = calls.get() + 1;
            calls.set(next);
            next >= 2
        })
        .unwrap_err();

        assert_eq!(err.to_string(), "generation canceled");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn batch_cancel_hook_allows_normal_generation_when_clear() {
        let calls = Cell::new(0usize);
        let output = prepare_batch_output_with_cancel(
            &BatchRequest {
                batch: true,
                ..BatchRequest::default()
            },
            || {
                calls.set(calls.get() + 1);
                false
            },
        )
        .unwrap();

        assert!(output.gcode.contains("settings-only output"));
        assert!(calls.get() > 2);
    }

    #[test]
    fn batch_progress_reports_text_generation_stages() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-progress-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 1\nL 0,0,0,10\n").unwrap();
        let events = RefCell::new(Vec::new());

        let output = prepare_batch_output_with_cancel_and_progress(
            &BatchRequest {
                batch: true,
                font_or_image: Some(path.clone()),
                text: Some("A".to_owned()),
                ..BatchRequest::default()
            },
            || false,
            |event| events.borrow_mut().push(event),
        )
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert_eq!(
            events.into_inner(),
            vec![
                BatchProgress::LoadingDocument,
                BatchProgress::LoadingTextFont,
                BatchProgress::LayingOutText,
                BatchProgress::PreparingExports,
                BatchProgress::WritingEngrave,
                BatchProgress::RenderingPrimary,
                BatchProgress::Finished,
            ]
        );
        assert_eq!(
            BatchProgress::CalculatingVCarve.status_text(),
            "Calculating V-carve"
        );
    }

    #[test]
    fn batch_cancel_hook_stops_during_vcarve_sampling() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-vcarve-cancel-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 4\nL 0,0,2,0\nL 2,0,2,2\nL 2,2,0,2\nL 0,2,0,0\n").unwrap();
        let in_vcarve = Cell::new(false);
        let vcarve_checks = Cell::new(0usize);

        let err = prepare_batch_output_with_cancel_and_progress(
            &BatchRequest {
                batch: true,
                font_or_image: Some(path.clone()),
                text: Some("A".to_owned()),
                settings_overrides: vec![
                    LegacySetting::new("cut_type", "v-carve", false),
                    LegacySetting::new("v_step_len", "0.01", false),
                ],
                ..BatchRequest::default()
            },
            || {
                if in_vcarve.get() {
                    let next = vcarve_checks.get() + 1;
                    vcarve_checks.set(next);
                    next > 4
                } else {
                    false
                }
            },
            |event| {
                if event == BatchProgress::CalculatingVCarve {
                    in_vcarve.set(true);
                }
            },
        )
        .unwrap_err();

        let _ = fs::remove_file(path);
        assert_eq!(err.to_string(), "generation canceled");
        assert!(vcarve_checks.get() > 4);
    }

    #[test]
    fn batch_cancel_hook_stops_during_cleanup_scanlines() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-cancel-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 4\nL 0,0,2,0\nL 2,0,2,2\nL 2,2,0,2\nL 0,2,0,0\n").unwrap();
        let in_cleanup = Cell::new(false);
        let cleanup_checks = Cell::new(0usize);

        let err = prepare_batch_output_with_cancel_and_progress(
            &BatchRequest {
                batch: true,
                font_or_image: Some(path.clone()),
                text: Some("A".to_owned()),
                include_secondary: true,
                settings_overrides: vec![
                    LegacySetting::new("cut_type", "v-carve", false),
                    LegacySetting::new("v_step_len", "0.5", false),
                    LegacySetting::new("clean_paths", "0,1,0,0,0,0,0,0", false),
                    LegacySetting::new("clean_dia", "0.01", false),
                    LegacySetting::new("clean_step", "10", false),
                ],
                ..BatchRequest::default()
            },
            || {
                if in_cleanup.get() {
                    let next = cleanup_checks.get() + 1;
                    cleanup_checks.set(next);
                    next > 4
                } else {
                    false
                }
            },
            |event| {
                if event == BatchProgress::CalculatingStraightCleanup {
                    in_cleanup.set(true);
                }
            },
        )
        .unwrap_err();

        let _ = fs::remove_file(path);
        assert_eq!(err.to_string(), "generation canceled");
        assert!(cleanup_checks.get() > 4);
    }

    #[test]
    fn batch_cancel_hook_stops_before_bitmap_vectorization_work() {
        let in_vectorization = Cell::new(false);

        let err = prepare_batch_output_with_cancel_and_progress(
            &BatchRequest {
                batch: true,
                font_or_image: Some(PathBuf::from("/tmp/rengrave-cancel-image.png")),
                ..BatchRequest::default()
            },
            || in_vectorization.get(),
            |event| {
                if event == BatchProgress::VectorizingBitmap {
                    in_vectorization.set(true);
                }
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "generation canceled");
        assert!(in_vectorization.get());
    }

    #[test]
    fn batch_cancel_hook_stops_during_cxf_font_parse() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-cxf-cancel-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let mut contents = String::from("[O] 1\n");
        for _ in 0..200 {
            contents.push_str("A 0,0,1,0,360\n");
        }
        fs::write(&path, contents).unwrap();
        let in_font_load = Cell::new(false);
        let font_checks = Cell::new(0usize);

        let err = prepare_batch_output_with_cancel_and_progress(
            &BatchRequest {
                batch: true,
                font_or_image: Some(path.clone()),
                text: Some("O".to_owned()),
                settings_overrides: vec![LegacySetting::new("segarc", "0.01", false)],
                ..BatchRequest::default()
            },
            || {
                if in_font_load.get() {
                    let next = font_checks.get() + 1;
                    font_checks.set(next);
                    next > 4
                } else {
                    false
                }
            },
            |event| {
                if event == BatchProgress::LoadingTextFont {
                    in_font_load.set(true);
                }
            },
        )
        .unwrap_err();

        let _ = fs::remove_file(path);
        assert_eq!(err.to_string(), "generation canceled");
        assert!(font_checks.get() > 4);
    }

    #[test]
    fn batch_cancel_hook_stops_during_dxf_parse() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-dxf-cancel-{}-{}.dxf",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &path,
            "0\nSECTION\n2\nENTITIES\n0\nARC\n10\n0\n20\n0\n40\n1\n50\n0\n51\n360\n0\nENDSEC\n0\nEOF\n",
        )
        .unwrap();
        let in_dxf = Cell::new(false);
        let dxf_checks = Cell::new(0usize);

        let err = prepare_batch_output_with_cancel_and_progress(
            &BatchRequest {
                batch: true,
                font_or_image: Some(path.clone()),
                settings_overrides: vec![LegacySetting::new("segarc", "1.0", false)],
                ..BatchRequest::default()
            },
            || {
                if in_dxf.get() {
                    let next = dxf_checks.get() + 1;
                    dxf_checks.set(next);
                    next > 50
                } else {
                    false
                }
            },
            |event| {
                if event == BatchProgress::LoadingDxf {
                    in_dxf.set(true);
                }
            },
        )
        .unwrap_err();

        let _ = fs::remove_file(path);
        assert_eq!(err.to_string(), "generation canceled");
        assert!(dxf_checks.get() > 50);
    }

    #[test]
    fn batch_text_uses_legacy_pipe_newline_convention() {
        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            text: Some("Line 1|Line 2".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        assert!(
            output.gcode.contains(
                "(fengrave_set TCODE       076 105 110 101 032 049 010 076 105 110 101 )"
            )
        );
        assert!(output.gcode.contains("(fengrave_set TCODE       032 050 )"));
    }

    #[test]
    fn font_path_updates_text_input_settings() {
        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(PathBuf::from("/tmp/example.ttf")),
            ..BatchRequest::default()
        })
        .unwrap();

        assert!(
            output
                .gcode
                .contains("(fengrave_set fontfile    \"example.ttf\" )")
        );
        assert!(output.gcode.contains("(fengrave_set input_type  text )"));
    }

    #[test]
    fn batch_generates_basic_gcode_for_cxf_text() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-basic-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 1\nL 0,0,0,10\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("G90\nG20\nG17 M3 S3000"));
        assert!(!output.gcode.contains("G64"));
        assert!(output.gcode.contains("G1 Z-0.0050"));
        assert!(output.gcode.contains("G1 X0.0000 Y1.9900"));
    }

    #[test]
    fn batch_honors_legacy_no_comments_setting() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-no-comments-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-no-comments-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 1\nL 0,0,0,10\n").unwrap();
        fs::write(&settings_path, "(fengrave_set no_comments 1 )\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert!(!output.gcode.contains("fengrave_set"));
        assert!(!output.gcode.contains("Settings used in f-engrave"));
        assert!(output.gcode.starts_with("G90\nG20\n"));
    }

    #[test]
    fn batch_applies_settings_overrides() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-overrides-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 2\nL 0,0,10,0\nL 0,0,0,10\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            text: Some("A".to_owned()),
            settings_overrides: vec![
                LegacySetting::new("YSCALE", "3.0", false),
                LegacySetting::new("plotbox", "1", false),
                LegacySetting::new("boxgap", "0.5", false),
            ],
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("(fengrave_set YSCALE      3.0 )"));
        assert!(output.gcode.contains("(fengrave_set plotbox     1 )"));
        assert!(output.gcode.contains("G1 X3.5000 Y-0.5100"));
    }

    #[test]
    fn batch_generates_basic_gcode_for_dxf_image() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-basic-{}-{}.dxf",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &path,
            "0\nSECTION\n0\nLINE\n10\n0\n20\n0\n11\n0\n21\n10\n0\nENDSEC\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("(fengrave_set input_type  image )"));
        assert!(!output.gcode.contains("(Engrave Text:"));
        assert!(output.gcode.contains("G1 X0.0000 Y1.9900"));
    }

    #[test]
    fn batch_generates_basic_gcode_for_svg_image() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-basic-{}-{}.svg",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &path,
            r#"<svg viewBox="0 0 100 100"><path d="M 50 20 L 50 30 L 60 30 Z"/></svg>"#,
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("(fengrave_set input_type  image )"));
        assert!(!output.gcode.contains("(Engrave Text:"));
        assert!(output.gcode.contains("G1 X"));
    }

    #[test]
    fn bitmap_dxf_font_normalizes_residual_trace_offset() {
        let dxf = "0\nSECTION\n0\nLINE\n10\n100\n20\n50\n11\n120\n21\n80\n0\nENDSEC\n";

        let font = bitmap_font_from_dxf_with_cancel(dxf, 5.0, &|| false).unwrap();
        let stroke = font.get_char('F').unwrap().strokes[0];

        assert!((stroke.start.x - 0.0).abs() < 1e-9);
        assert!((stroke.start.y - 0.0).abs() < 1e-9);
        assert!((stroke.end.x - 20.0).abs() < 1e-9);
        assert!((stroke.end.y - 30.0).abs() < 1e-9);
    }

    #[test]
    fn bitmap_layout_uses_trimmed_content_origin() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-trimmed-origin-{}-{}.png",
            std::process::id(),
            "batch"
        ));
        let mut image = image::RgbaImage::from_pixel(48, 24, image::Rgba([255, 255, 255, 255]));
        for y in 8..18 {
            for x in 34..42 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        image.save(&path).unwrap();

        let outline = layout_text_outline(&BatchRequest {
            font_or_image: Some(path.clone()),
            settings_overrides: vec![LegacySetting::new("cut_type", "v-carve", false)],
            ..BatchRequest::default()
        })
        .unwrap()
        .unwrap();

        let _ = fs::remove_file(path);
        let bounds = outline.bounds.unwrap();
        assert!(
            bounds.min.x.abs() < 1.0e-9,
            "expected bitmap content min X at origin, got {}",
            bounds.min.x
        );
        assert!(
            bounds.min.y.abs() < 1.0e-9,
            "expected bitmap content min Y at origin, got {}",
            bounds.min.y
        );
    }

    #[test]
    fn svg_layout_uses_content_origin() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-svg-origin-{}-{}.svg",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &path,
            r#"<svg viewBox="0 0 100 100"><path d="M 50 20 L 60 20 L 60 35 Z"/></svg>"#,
        )
        .unwrap();

        let outline = layout_text_outline(&BatchRequest {
            font_or_image: Some(path.clone()),
            ..BatchRequest::default()
        })
        .unwrap()
        .unwrap();

        let _ = fs::remove_file(path);
        let bounds = outline.bounds.unwrap();
        assert!(
            bounds.min.x.abs() < 1.0e-2,
            "expected SVG content min X at origin, got {}",
            bounds.min.x
        );
        assert!(
            bounds.min.y.abs() < 1.0e-2,
            "expected SVG content min Y at origin, got {}",
            bounds.min.y
        );
    }

    #[test]
    fn batch_image_vcarve_sorts_loop_winding_before_sampling() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-image-vcarve-sort-{}-{}.dxf",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &path,
            "0\nSECTION\n0\nLWPOLYLINE\n70\n1\n10\n0\n20\n0\n10\n1\n20\n0\n10\n1\n20\n1\n10\n0\n20\n1\n0\nENDSEC\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            settings_overrides: vec![
                LegacySetting::new("cut_type", "v-carve", false),
                LegacySetting::new("YSCALE", "0.1", false),
                LegacySetting::new("v_step_len", "0.01", false),
            ],
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        let min_z = output
            .gcode
            .lines()
            .flat_map(|line| line.split_whitespace())
            .filter_map(|word| word.strip_prefix('Z'))
            .filter_map(|value| value.parse::<f64>().ok())
            .fold(0.0_f64, f64::min);

        assert!(output.warnings.is_empty());
        assert!(min_z > -0.2, "unexpected outside-cut depth: {min_z}");
    }

    #[test]
    fn batch_recovers_stale_image_path_from_ngc_dir() {
        let dir = std::env::temp_dir().join(format!(
            "rengrave-recover-image-{}-{}",
            std::process::id(),
            "batch"
        ));
        fs::create_dir_all(&dir).unwrap();
        let dxf_path = dir.join("part.dxf");
        let settings_path = dir.join("settings.ngc");
        fs::write(
            &dxf_path,
            "0\nSECTION\n0\nLINE\n10\n0\n20\n0\n11\n0\n21\n10\n0\nENDSEC\n",
        )
        .unwrap();
        fs::write(
            &settings_path,
            format!(
                "(fengrave_set input_type  image )\n(fengrave_set imagefile   \"/missing/original/part.dxf\" )\n(fengrave_set NGC_DIR     \"{}\" )\n",
                dir.display()
            ),
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_dir_all(dir);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("(fengrave_set input_type  image )"));
        assert!(output.gcode.contains("G1 X0.0000 Y1.9900"));
    }

    #[test]
    fn batch_reads_legacy_plotbox_box_alias() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-plotbox-alias-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-plotbox-alias-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 2\nL 0,0,10,0\nL 0,0,0,10\n").unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set plotbox   box )\n(fengrave_set boxgap    0.25 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("(fengrave_set plotbox     box )"));
        assert!(output.gcode.contains("G1 X2.2500 Y-0.2600"));
    }

    #[test]
    fn batch_generates_add_circle_for_text_on_circle() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-add-circle-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-add-circle-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 2\nL 0,0,10,0\nL 0,0,0,10\n").unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set TRADIUS    5 )\n(fengrave_set plotbox    1 )\n(fengrave_set boxgap     0.25 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            svg_output: Some(std::env::temp_dir().join("rengrave-add-circle.svg")),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("G0 X-7.2550 Y0.0000"));
        assert!(output.gcode.contains("G2 I7.2550 J0.0000"));
        assert!(
            output
                .svg
                .as_deref()
                .unwrap()
                .contains("<circle cx=\"726.000000\" cy=\"726.000000\" r=\"725.500000\"")
        );
        assert!(!output.gcode.contains("G1 X7.2500 Y-7.2500"));
    }

    #[test]
    fn batch_generates_vcarve_gcode_for_closed_cxf_text() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-vcarve-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-vcarve-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &font_path,
            "[A] 4\nL 0,0,10,0\nL 10,0,10,10\nL 10,10,0,10\nL 0,10,0,0\n",
        )
        .unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set cut_type   v-carve )\n(fengrave_set v_step_len  0.5 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert!(output.gcode.contains("cut_type"));
        assert!(output.gcode.contains("v-carve"));
        assert!(output.gcode.contains("G1 X"));
        assert!(output.gcode.contains(" Z-"));
        assert!(
            !output
                .gcode
                .contains("v-carve generation is not ported yet")
        );
        assert!(output.secondary_gcode.is_empty());
    }

    #[test]
    fn batch_prepares_cleanup_companion_gcode_when_output_path_is_set() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        let output_path = std::env::temp_dir().join(format!(
            "rengrave-cleanup-{}-{}.out.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &font_path,
            "[A] 4\nL 0,0,10,0\nL 10,0,10,10\nL 10,10,0,10\nL 0,10,0,0\n",
        )
        .unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set cut_type   v-carve )\n(fengrave_set clean_paths  1,0,0,0,0,0,0,0 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            output: Some(output_path),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert_eq!(output.secondary_gcode.len(), 1);
        assert_eq!(output.secondary_gcode[0].suffix, "clean");
        assert!(
            output.secondary_gcode[0]
                .gcode
                .contains("secondary cleanup operation")
        );
    }

    #[test]
    fn batch_prepares_cleanup_companion_gcode_when_explicitly_requested() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-ui-cleanup-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let settings_path = std::env::temp_dir().join(format!(
            "rengrave-ui-cleanup-{}-{}.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(
            &font_path,
            "[A] 4\nL 0,0,10,0\nL 10,0,10,10\nL 10,10,0,10\nL 0,10,0,0\n",
        )
        .unwrap();
        fs::write(
            &settings_path,
            "(fengrave_set cut_type   v-carve )\n(fengrave_set clean_paths  1,0,0,0,0,0,0,0 )\n",
        )
        .unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            gcode_file: Some(settings_path.clone()),
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            include_secondary: true,
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        let _ = fs::remove_file(settings_path);
        assert!(output.warnings.is_empty());
        assert_eq!(output.secondary_gcode.len(), 1);
        assert_eq!(output.secondary_gcode[0].suffix, "clean");
    }

    #[test]
    fn batch_prepares_profile_companion_gcode_for_engrave_jobs() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-profile-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let output_path = std::env::temp_dir().join(format!(
            "rengrave-profile-{}-{}.out.ngc",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 1\nL 0,0,10,0\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            output: Some(output_path),
            settings_overrides: vec![
                LegacySetting::new("profile_cut", "1", false),
                LegacySetting::new("profile_margin", "0", false),
                LegacySetting::new("profile_depth", "0.2", false),
                LegacySetting::new("profile_steps", "2", false),
                LegacySetting::new("profile_endmill_dia", "0.25", false),
                LegacySetting::new("profile_tabs", "2", false),
                LegacySetting::new("profile_tab_height", "0.025", false),
                LegacySetting::new("profile_tab_width", "0.5", false),
            ],
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        assert!(output.warnings.is_empty());
        assert_eq!(output.secondary_gcode.len(), 1);
        assert_eq!(output.secondary_gcode[0].suffix, "profile");
        assert!(output.secondary_gcode[0].gcode.contains("profile cut"));
        assert!(output.secondary_gcode[0].gcode.contains("Profile tabs: 2"));
        assert!(
            output.secondary_gcode[0]
                .gcode
                .contains("Profile tab height: 0.0250")
        );
        assert!(output.secondary_gcode[0].gcode.contains("G1 Z-0.1000"));
        assert!(output.secondary_gcode[0].gcode.contains("G1 Z-0.2000"));
        assert!(
            output.secondary_gcode[0]
                .gcode
                .lines()
                .any(|line| line.contains(" Z-0.1750"))
        );
    }

    #[test]
    fn profile_cut_participates_in_origin_alignment() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-profile-origin-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 2\nL 0,0,10,0\nL 0,0,10,10\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            include_secondary: true,
            settings_overrides: vec![
                LegacySetting::new("origin", "Top-Left", false),
                LegacySetting::new("profile_cut", "1", false),
                LegacySetting::new("profile_margin", "0", false),
                LegacySetting::new("profile_depth", "0.2", false),
                LegacySetting::new("profile_steps", "1", false),
                LegacySetting::new("profile_endmill_dia", "0.25", false),
            ],
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        assert!(output.warnings.is_empty());
        assert_eq!(output.secondary_gcode.len(), 1);
        assert_eq!(output.secondary_gcode[0].suffix, "profile");
        let points = profile_xy_points(&output.secondary_gcode[0].gcode);
        let min_x = points.iter().map(|point| point.x).reduce(f64::min).unwrap();
        let max_y = points.iter().map(|point| point.y).reduce(f64::max).unwrap();
        assert!(min_x.abs() < 1.0e-9, "profile min X was {min_x}");
        assert!(max_y.abs() < 1.0e-9, "profile max Y was {max_y}");
    }

    fn profile_xy_points(gcode: &str) -> Vec<Point> {
        gcode
            .lines()
            .filter_map(|line| {
                let mut x = None;
                let mut y = None;
                for token in line.split_whitespace() {
                    if let Some(value) = token.strip_prefix('X') {
                        x = value.parse().ok();
                    } else if let Some(value) = token.strip_prefix('Y') {
                        y = value.parse().ok();
                    }
                }
                Some(Point::new(x?, y?))
            })
            .collect()
    }

    #[test]
    fn batch_splits_profile_chamfer_and_straight_profile_files() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-profile-chamfer-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 1\nL 0,0,10,0\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            include_secondary: true,
            settings_overrides: vec![
                LegacySetting::new("profile_cut", "1", false),
                LegacySetting::new("profile_margin", "0", false),
                LegacySetting::new("profile_depth", "0.2", false),
                LegacySetting::new("profile_steps", "1", false),
                LegacySetting::new("profile_endmill_dia", "0.25", false),
                LegacySetting::new("profile_chamfer", "1", false),
                LegacySetting::new("profile_chamfer_depth", "0.05", false),
                LegacySetting::new("profile_chamfer_angle", "90", false),
            ],
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        assert!(output.warnings.is_empty());
        assert_eq!(
            output
                .secondary_gcode
                .iter()
                .map(|secondary| secondary.suffix.as_str())
                .collect::<Vec<_>>(),
            vec!["profile_chamfer", "profile"]
        );
        assert!(
            output.secondary_gcode[0]
                .gcode
                .contains("profile chamfer operation")
        );
        assert!(
            output.secondary_gcode[1]
                .gcode
                .contains("straight profile cut operation")
        );
    }

    #[test]
    fn batch_prepares_svg_and_dxf_exports_when_requested() {
        let font_path = std::env::temp_dir().join(format!(
            "rengrave-export-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        let svg_path = std::env::temp_dir().join(format!(
            "rengrave-export-{}-{}.svg",
            std::process::id(),
            "batch"
        ));
        let dxf_path = std::env::temp_dir().join(format!(
            "rengrave-export-{}-{}.dxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&font_path, "[A] 1\nL 0,0,10,0\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(font_path.clone()),
            text: Some("A".to_owned()),
            svg_output: Some(svg_path),
            dxf_output: Some(dxf_path),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(font_path);
        assert!(output.warnings.is_empty());
        assert!(output.svg.as_deref().unwrap().contains("<svg"));
        assert!(output.svg.as_deref().unwrap().contains("<path d=\"M"));
        assert!(
            output
                .dxf
                .as_deref()
                .unwrap()
                .contains("SECTION\n2\nENTITIES")
        );
        assert!(output.dxf.as_deref().unwrap().contains("LINE\n  5\n30"));
    }

    #[test]
    fn batch_skips_exports_when_not_requested() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-no-export-{}-{}.cxf",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, "[A] 1\nL 0,0,10,0\n").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            text: Some("A".to_owned()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(output.svg.is_none());
        assert!(output.dxf.is_none());
    }

    #[test]
    fn bitmap_decode_failure_falls_back_to_settings_only_output() {
        let path = std::env::temp_dir().join(format!(
            "rengrave-invalid-bitmap-{}-{}.png",
            std::process::id(),
            "batch"
        ));
        fs::write(&path, b"not a png").unwrap();

        let output = prepare_batch_output(&BatchRequest {
            batch: true,
            font_or_image: Some(path.clone()),
            ..BatchRequest::default()
        })
        .unwrap();

        let _ = fs::remove_file(path);
        assert!(
            output
                .warnings
                .iter()
                .any(|warning| warning.contains("unable to decode bitmap"))
        );
        assert!(output.warnings.iter().any(|warning| {
            warning.contains("settings-only output generated because no toolpath could be produced")
        }));
        assert!(
            output
                .gcode
                .contains("( R-Engrave settings-only output: no toolpath was generated )")
        );
        assert!(!output.gcode.contains("not implemented"));
    }
}
