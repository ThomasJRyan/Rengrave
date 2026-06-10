use std::collections::BTreeSet;

use crate::font::Font;
use crate::geometry::Point;
use crate::settings::LegacySettings;

const ZERO: f64 = 0.00001;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngraveSegment {
    pub start: Point,
    pub end: Point,
    pub loop_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutOutput {
    pub segments: Vec<EngraveSegment>,
    pub bounds: Option<Bounds>,
    pub missing_chars: Vec<char>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSettings {
    pub yscale: f64,
    pub xscale_percent: f64,
    pub line_space: f64,
    pub char_space_percent: f64,
    pub word_space_percent: f64,
    pub angle_degrees: f64,
    pub text_radius: f64,
    pub stroke_thickness: f64,
    pub xorigin: f64,
    pub yorigin: f64,
    pub height_calc: HeightCalc,
    pub origin: Origin,
    pub justify: Justify,
    pub mirror: bool,
    pub flip: bool,
    pub use_image_size: bool,
    pub input_type: InputType,
    pub cut_type: CutType,
    pub outer: bool,
    pub upper: bool,
}

impl LayoutSettings {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        Self {
            yscale: get_f64(settings, "YSCALE", 2.0),
            xscale_percent: get_f64(settings, "XSCALE", 100.0),
            line_space: get_f64(settings, "LSPACE", 1.1),
            char_space_percent: get_f64(settings, "CSPACE", 25.0),
            word_space_percent: get_f64(settings, "WSPACE", 100.0),
            angle_degrees: get_f64(settings, "TANGLE", 0.0),
            text_radius: get_f64(settings, "TRADIUS", 0.0),
            stroke_thickness: get_f64(settings, "STHICK", 0.01),
            xorigin: get_f64(settings, "xorigin", 0.0),
            yorigin: get_f64(settings, "yorigin", 0.0),
            height_calc: HeightCalc::parse(settings.get_last("H_CALC").unwrap_or("max_use")),
            origin: Origin::parse(settings.get_last("origin").unwrap_or("Default")),
            justify: Justify::parse(settings.get_last("justify").unwrap_or("Left")),
            mirror: get_bool(settings, "mirror", false),
            flip: get_bool(settings, "flip", false),
            use_image_size: get_bool(settings, "useIMGsize", false),
            input_type: InputType::parse(settings.get_last("input_type").unwrap_or("text")),
            cut_type: CutType::parse(settings.get_last("cut_type").unwrap_or("engrave")),
            outer: get_bool(settings, "outer", true),
            upper: get_bool(settings, "upper", true),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightCalc {
    MaxAll,
    MaxUse,
}

impl HeightCalc {
    fn parse(value: &str) -> Self {
        match value {
            "max_all" => Self::MaxAll,
            _ => Self::MaxUse,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Left,
    Center,
    Right,
}

impl Justify {
    fn parse(value: &str) -> Self {
        match value {
            "Center" => Self::Center,
            "Right" => Self::Right,
            _ => Self::Left,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
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

impl Origin {
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

    fn zero_point(self, bounds: Bounds) -> Point {
        let midx = (bounds.min.x + bounds.max.x) / 2.0;
        let midy = (bounds.min.y + bounds.max.y) / 2.0;
        match self {
            Self::TopLeft => Point::new(bounds.min.x, bounds.max.y),
            Self::TopCenter => Point::new(midx, bounds.max.y),
            Self::TopRight => Point::new(bounds.max.x, bounds.max.y),
            Self::MidLeft => Point::new(bounds.min.x, midy),
            Self::MidCenter => Point::new(midx, midy),
            Self::MidRight => Point::new(bounds.max.x, midy),
            Self::BotLeft => Point::new(bounds.min.x, bounds.min.y),
            Self::BotCenter => Point::new(midx, bounds.min.y),
            Self::BotRight => Point::new(bounds.max.x, bounds.min.y),
            Self::ArcCenter | Self::Default => Point::new(0.0, 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    Text,
    Image,
}

impl InputType {
    fn parse(value: &str) -> Self {
        if value == "image" {
            Self::Image
        } else {
            Self::Text
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutType {
    Engrave,
    VCarve,
}

impl CutType {
    fn parse(value: &str) -> Self {
        if value == "v-carve" {
            Self::VCarve
        } else {
            Self::Engrave
        }
    }
}

pub fn layout_text(font: &Font, text: &str, settings: &LayoutSettings) -> LayoutOutput {
    let mut font_used_height = f64::NEG_INFINITY;
    let mut font_used_depth = f64::INFINITY;
    let mut missing = BTreeSet::new();

    for ch in text.chars() {
        if let Some(glyph) = font.get_char(ch) {
            font_used_height = font_used_height.max(glyph.ymax());
            font_used_depth = font_used_depth.min(glyph.ymin());
        }
    }

    let (font_line_height, font_line_depth) = match settings.height_calc {
        HeightCalc::MaxAll => (font.max_y(), font.min_y()),
        HeightCalc::MaxUse => {
            if font_used_height.is_finite() {
                (font_used_height, font_used_depth)
            } else {
                return LayoutOutput {
                    segments: Vec::new(),
                    bounds: None,
                    missing_chars: text.chars().collect(),
                };
            }
        }
    };

    let thick = if settings.cut_type == CutType::VCarve {
        0.0
    } else {
        settings.stroke_thickness
    };
    let mut yscale = if settings.use_image_size && settings.input_type == InputType::Image {
        settings.yscale / 100.0
    } else {
        (settings.yscale - thick) / (font_line_height - font_line_depth)
    };
    if yscale <= ZERO || !yscale.is_finite() {
        yscale = 0.1;
    }

    let font_char_width = font.max_x();
    let font_word_space = font_char_width * (settings.word_space_percent / 100.0);
    let xscale = settings.xscale_percent * yscale / 100.0;
    let font_char_space = font_char_width * (settings.char_space_percent / 100.0);
    let font_line_space =
        (font_line_height - font_line_depth + thick / yscale) * settings.line_space;

    let mut raw_segments = Vec::new();
    let mut line_bounds = Vec::new();
    let mut line_state = MutableBounds::empty();
    let mut xposition = 0.0;
    let mut yposition = 0.0;
    let mut line_id = 0usize;
    let mut char_count = 0usize;

    for ch in text.chars() {
        char_count += 1;
        match ch {
            ' ' => {
                xposition += font_word_space;
                continue;
            }
            '\t' => {
                xposition += 3.0 * font_word_space;
                continue;
            }
            '\n' => {
                line_bounds.push(line_state.take());
                xposition = 0.0;
                yposition += font_line_space;
                line_id += 1;
                continue;
            }
            _ => {}
        }

        let Some(glyph) = font.get_char(ch) else {
            missing.insert(ch);
            continue;
        };

        for stroke in &glyph.strokes {
            let start = scale_point(
                Point::new(stroke.start.x + xposition, stroke.start.y - yposition),
                xscale,
                yscale,
            );
            let end = scale_point(
                Point::new(stroke.end.x + xposition, stroke.end.y - yposition),
                xscale,
                yscale,
            );
            line_state.include(start);
            line_state.include(end);
            raw_segments.push((start, end, line_id, char_count));
        }

        xposition += font_char_space + glyph.xmax();
    }

    line_bounds.push(line_state.take());
    let max_line_width = line_bounds
        .iter()
        .flatten()
        .map(|bounds| bounds.max.x)
        .reduce(f64::max)
        .unwrap_or(0.0);

    let mut segments: Vec<EngraveSegment> = raw_segments
        .into_iter()
        .map(|(mut start, mut end, line_id, char_count)| {
            let line_max = line_bounds
                .get(line_id)
                .and_then(|bounds| *bounds)
                .map(|bounds| bounds.max.x)
                .unwrap_or(0.0);
            let offset = match settings.justify {
                Justify::Left => 0.0,
                Justify::Center => (max_line_width - line_max) / 2.0,
                Justify::Right => max_line_width - line_max,
            };
            start.x += offset;
            end.x += offset;
            EngraveSegment {
                start,
                end,
                loop_id: char_count,
            }
        })
        .collect();

    let circle_radius =
        text_circle_radius(settings, font_line_height, font_line_depth, thick, yscale);
    if circle_radius != 0.0 {
        apply_text_circle(
            &mut segments,
            circle_radius,
            settings.justify,
            settings.upper,
        );
    }

    let angle = settings.angle_degrees.to_radians();
    let mut transformed_bounds = MutableBounds::empty();
    for segment in &mut segments {
        if settings.angle_degrees != 0.0 {
            segment.start = rotate(segment.start, angle);
            segment.end = rotate(segment.end, angle);
        }
        if settings.mirror {
            segment.start.x = -segment.start.x;
            segment.end.x = -segment.end.x;
        }
        if settings.flip {
            segment.start.y = -segment.start.y;
            segment.end.y = -segment.end.y;
        }
        transformed_bounds.include(segment.start);
        transformed_bounds.include(segment.end);
    }

    let Some(mut bounds) = transformed_bounds.take() else {
        return LayoutOutput {
            segments,
            bounds: None,
            missing_chars: missing.into_iter().collect(),
        };
    };

    bounds.min.x -= thick / 2.0;
    bounds.max.x += thick / 2.0;
    bounds.min.y -= thick / 2.0;
    bounds.max.y += thick / 2.0;

    let zero = settings.origin.zero_point(bounds);
    for segment in &mut segments {
        segment.start.x = segment.start.x - zero.x + settings.xorigin;
        segment.start.y = segment.start.y - zero.y + settings.yorigin;
        segment.end.x = segment.end.x - zero.x + settings.xorigin;
        segment.end.y = segment.end.y - zero.y + settings.yorigin;
    }

    bounds.min.x = bounds.min.x - zero.x + settings.xorigin;
    bounds.max.x = bounds.max.x - zero.x + settings.xorigin;
    bounds.min.y = bounds.min.y - zero.y + settings.yorigin;
    bounds.max.y = bounds.max.y - zero.y + settings.yorigin;

    LayoutOutput {
        segments,
        bounds: Some(bounds),
        missing_chars: missing.into_iter().collect(),
    }
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_bool(settings: &LegacySettings, key: &str, default: bool) -> bool {
    settings
        .get_last(key)
        .map(|value| matches!(value, "1" | "true" | "True"))
        .unwrap_or(default)
}

fn scale_point(point: Point, xscale: f64, yscale: f64) -> Point {
    Point::new(point.x * xscale, point.y * yscale)
}

fn rotate(point: Point, angle: f64) -> Point {
    Point::new(
        point.x * angle.cos() - point.y * angle.sin(),
        point.x * angle.sin() + point.y * angle.cos(),
    )
}

fn text_circle_radius(
    settings: &LayoutSettings,
    font_line_height: f64,
    font_line_depth: f64,
    thick: f64,
    yscale: f64,
) -> f64 {
    if settings.input_type != InputType::Text || settings.text_radius == 0.0 {
        return 0.0;
    }

    if settings.outer {
        if settings.upper {
            settings.text_radius + thick / 2.0 + yscale * (-font_line_depth)
        } else {
            -settings.text_radius - thick / 2.0 - yscale * font_line_height
        }
    } else if settings.upper {
        settings.text_radius - thick / 2.0 - yscale * font_line_height
    } else {
        -settings.text_radius + thick / 2.0 + yscale * (-font_line_depth)
    }
}

fn apply_text_circle(segments: &mut [EngraveSegment], radius: f64, justify: Justify, upper: bool) {
    let mut min_angle = f64::INFINITY;
    let mut max_angle = f64::NEG_INFINITY;

    for segment in segments.iter_mut() {
        let (start, start_angle) = bend_point_to_circle(segment.start, radius);
        let (end, end_angle) = bend_point_to_circle(segment.end, radius);
        segment.start = start;
        segment.end = end;
        min_angle = min_angle.min(start_angle).min(end_angle);
        max_angle = max_angle.max(start_angle).max(end_angle);
    }

    let rotation = match justify {
        Justify::Left => 0.0,
        Justify::Center => (min_angle + max_angle) / 2.0,
        Justify::Right if upper => max_angle,
        Justify::Right => min_angle,
    };
    if rotation == 0.0 {
        return;
    }

    for segment in segments {
        segment.start = rotate(segment.start, rotation);
        segment.end = rotate(segment.end, rotation);
    }
}

fn bend_point_to_circle(point: Point, radius: f64) -> (Point, f64) {
    let alpha = point.x / radius;
    (
        Point::new(
            (radius + point.y) * alpha.sin(),
            (radius + point.y) * alpha.cos(),
        ),
        alpha,
    )
}

#[derive(Debug, Clone, Copy)]
struct MutableBounds {
    min: Point,
    max: Point,
    empty: bool,
}

impl MutableBounds {
    fn empty() -> Self {
        Self {
            min: Point::new(0.0, 0.0),
            max: Point::new(0.0, 0.0),
            empty: true,
        }
    }

    fn include(&mut self, point: Point) {
        if self.empty {
            self.min = point;
            self.max = point;
            self.empty = false;
        } else {
            self.min.x = self.min.x.min(point.x);
            self.min.y = self.min.y.min(point.y);
            self.max.x = self.max.x.max(point.x);
            self.max.y = self.max.y.max(point.y);
        }
    }

    fn take(&mut self) -> Option<Bounds> {
        if self.empty {
            None
        } else {
            let bounds = Bounds {
                min: self.min,
                max: self.max,
            };
            *self = Self::empty();
            Some(bounds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::parse_cxf;
    use crate::settings::default_legacy_settings;

    #[test]
    fn lays_out_simple_cxf_text_with_default_scale() {
        let font = parse_cxf("[A] 1\nL 0,0,0,10\n", 5.0).unwrap();
        let settings = LayoutSettings::from_legacy(&default_legacy_settings());
        let output = layout_text(&font, "A", &settings);

        assert_eq!(output.segments.len(), 1);
        assert!((output.segments[0].end.y - 1.99).abs() < 1e-9);
    }

    #[test]
    fn applies_bottom_left_origin() {
        let font = parse_cxf("[A] 1\nL 1,1,2,3\n", 5.0).unwrap();
        let mut legacy = default_legacy_settings();
        legacy.set_or_push("origin", "Bot-Left", false);
        let settings = LayoutSettings::from_legacy(&legacy);
        let output = layout_text(&font, "A", &settings);

        assert!(output.bounds.unwrap().min.x.abs() < 1e-9);
        assert!(output.bounds.unwrap().min.y.abs() < 1e-9);
    }

    #[test]
    fn bends_text_onto_upper_outside_circle() {
        let font = parse_cxf("[A] 2\nL 0,0,10,0\nL 0,0,0,10\n", 5.0).unwrap();
        let mut legacy = default_legacy_settings();
        legacy.set_or_push("TRADIUS", "5", false);
        let settings = LayoutSettings::from_legacy(&legacy);
        let output = layout_text(&font, "A", &settings);
        let segment = output.segments[0];

        assert!(segment.start.x.abs() < 1e-9);
        assert!((segment.start.y - 5.005).abs() < 1e-9);
        assert!(segment.end.x > 1.8);
        assert!(segment.end.y < segment.start.y);
    }

    #[test]
    fn bends_text_onto_lower_outside_circle() {
        let font = parse_cxf("[A] 2\nL 0,0,10,0\nL 0,0,0,10\n", 5.0).unwrap();
        let mut legacy = default_legacy_settings();
        legacy.set_or_push("TRADIUS", "5", false);
        legacy.set_or_push("upper", "0", false);
        let settings = LayoutSettings::from_legacy(&legacy);
        let output = layout_text(&font, "A", &settings);
        let segment = output.segments[0];

        assert!(segment.start.x.abs() < 1e-9);
        assert!((segment.start.y + 6.995).abs() < 1e-9);
        assert!(segment.end.x > 1.8);
        assert!(segment.end.y > segment.start.y);
    }
}
