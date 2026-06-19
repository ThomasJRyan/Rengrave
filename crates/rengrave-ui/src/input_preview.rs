use super::*;

pub(crate) struct InputPreview {
    pub(crate) path: Option<PathBuf>,
    pub(crate) sample_text: Option<String>,
    pub(crate) data: InputPreviewData,
    pub(crate) texture: Option<egui::TextureHandle>,
    pub(crate) mask_texture: Option<egui::TextureHandle>,
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
    pub(crate) fn load(path: Option<PathBuf>, sample_text: Option<String>) -> Self {
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
pub(crate) enum InputPreviewData {
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

pub(crate) fn input_preview_accepts_sample(path: Option<&Path>) -> bool {
    path.and_then(InputCatalogKind::from_path)
        .is_some_and(|kind| matches!(kind, InputCatalogKind::CxfFont | InputCatalogKind::TtfFont))
}

pub(crate) fn input_preview_sample_for_path(path: Option<&Path>, text: &str) -> Option<String> {
    let path = path?;
    input_preview_accepts_sample(Some(path)).then(|| preview_text_sample(text))
}

pub(crate) fn preview_text_sample(text: &str) -> String {
    text.lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.chars().take(24).collect::<String>())
        })
        .filter(|sample| !sample.is_empty())
        .unwrap_or_else(|| "R-Engrave".to_owned())
}

pub(crate) fn load_input_preview_data(path: &Path, sample_text: Option<&str>) -> InputPreviewData {
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
        Some(InputCatalogKind::Svg) => match read_svg_font(path) {
            Ok(font) => {
                let preview = preview_segments_for_font(&font, None);
                vector_input_preview("SVG artwork", preview.segments, Vec::new())
            }
            Err(err) => InputPreviewData::Error(err.to_string()),
        },
        Some(InputCatalogKind::Bitmap) => load_bitmap_preview(path),
        None => InputPreviewData::Error("unsupported input type".to_owned()),
    }
}

pub(crate) fn vector_input_preview(
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

pub(crate) fn vector_input_preview_readouts(
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
pub(crate) struct FontInputPreview {
    pub(crate) segments: Vec<PreviewSegment>,
    pub(crate) missing_chars: Vec<char>,
}

pub(crate) fn preview_segments_for_font(
    font: &Font,
    sample_text: Option<&str>,
) -> FontInputPreview {
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

pub(crate) fn append_stroke_segments(
    segments: &mut Vec<PreviewSegment>,
    strokes: &[Stroke],
    offset: Point,
) {
    segments.extend(strokes.iter().map(|stroke| PreviewSegment {
        start: Point::new(stroke.start.x + offset.x, stroke.start.y + offset.y),
        end: Point::new(stroke.end.x + offset.x, stroke.end.y + offset.y),
    }));
}

pub(crate) fn load_bitmap_preview(path: &Path) -> InputPreviewData {
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

pub(crate) fn bitmap_trace_mask_thumbnail_and_stats(
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

pub(crate) fn bitmap_trace_stats_readout(stats: BitmapTraceStats) -> String {
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

pub(crate) fn image_preview_model_height(
    path: Option<&Path>,
    preview: &InputPreviewData,
) -> Option<f64> {
    if !matches!(
        InputCatalogKind::from_path(path?),
        Some(InputCatalogKind::Dxf | InputCatalogKind::Svg)
    ) {
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

pub(crate) fn convert_image_size_yscale(
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

pub(crate) fn missing_chars_readout(chars: &[char]) -> String {
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

pub(crate) fn draw_input_preview(ui: &mut egui::Ui, preview: &mut InputPreview) {
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

pub(crate) fn draw_bitmap_preview_texture(
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

pub(crate) fn draw_vector_input_preview(
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
pub(crate) struct VectorInputPreviewTransform {
    pub(crate) bounds: PreviewBounds,
    pub(crate) scale: f32,
    pub(crate) origin: egui::Pos2,
}

impl VectorInputPreviewTransform {
    pub(crate) fn to_screen(self, point: Point) -> egui::Pos2 {
        egui::pos2(
            self.origin.x + ((point.x - self.bounds.min.x) as f32) * self.scale,
            self.origin.y - ((point.y - self.bounds.min.y) as f32) * self.scale,
        )
    }
}

pub(crate) fn vector_input_preview_transform(
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

pub(crate) fn vector_input_preview_axis_segments(bounds: PreviewBounds) -> Vec<PreviewSegment> {
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
