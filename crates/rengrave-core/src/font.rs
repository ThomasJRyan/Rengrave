use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

use crate::geometry::Point;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub start: Point,
    pub end: Point,
}

impl Stroke {
    pub fn new(xstart: f64, ystart: f64, xend: f64, yend: f64) -> Self {
        Self {
            start: Point::new(xstart, ystart),
            end: Point::new(xend, yend),
        }
    }

    pub fn xmax(self) -> f64 {
        self.start.x.max(self.end.x)
    }

    pub fn ymax(self) -> f64 {
        self.start.y.max(self.end.y)
    }

    pub fn ymin(self) -> f64 {
        self.start.y.min(self.end.y)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub key: u32,
    pub strokes: Vec<Stroke>,
}

impl Glyph {
    pub fn xmax(&self) -> f64 {
        self.strokes
            .iter()
            .map(|stroke| stroke.xmax())
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    pub fn ymax(&self) -> f64 {
        self.strokes
            .iter()
            .map(|stroke| stroke.ymax())
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    pub fn ymin(&self) -> f64 {
        self.strokes
            .iter()
            .map(|stroke| stroke.ymin())
            .reduce(f64::min)
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Font {
    pub glyphs: BTreeMap<u32, Glyph>,
}

impl Font {
    pub fn get_char(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.get(&(ch as u32))
    }

    pub fn max_x(&self) -> f64 {
        self.glyphs
            .values()
            .map(Glyph::xmax)
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    pub fn max_y(&self) -> f64 {
        self.glyphs
            .values()
            .map(Glyph::ymax)
            .reduce(f64::max)
            .unwrap_or(0.0)
    }

    pub fn min_y(&self) -> f64 {
        self.glyphs
            .values()
            .map(Glyph::ymin)
            .reduce(f64::min)
            .unwrap_or(0.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FontError {
    #[error("unable to read CXF font `{path}`: {source}")]
    ReadCxf {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid line command `{line}`")]
    InvalidLine { line: String },
    #[error("invalid arc command `{line}`")]
    InvalidArc { line: String },
    #[error("unable to read TTF font `{path}`: {source}")]
    ReadTtf {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid TTF font `{path}`")]
    InvalidTtf { path: PathBuf },
    #[error("TTF font `{path}` does not contain outline data for A")]
    MissingScaleGlyph { path: PathBuf },
    #[error("font parsing canceled")]
    Canceled,
}

pub fn read_cxf(path: &Path, segarc_degrees: f64) -> Result<Font, FontError> {
    read_cxf_with_cancel(path, segarc_degrees, &|| false)
}

pub fn read_cxf_with_cancel(
    path: &Path,
    segarc_degrees: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Font, FontError> {
    check_canceled(cancel)?;
    let input = fs::read_to_string(path).map_err(|source| FontError::ReadCxf {
        path: path.to_owned(),
        source,
    })?;
    parse_cxf_with_cancel(&input, segarc_degrees, cancel)
}

pub fn parse_cxf(input: &str, segarc_degrees: f64) -> Result<Font, FontError> {
    parse_cxf_with_cancel(input, segarc_degrees, &|| false)
}

pub fn parse_cxf_with_cancel(
    input: &str,
    segarc_degrees: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Font, FontError> {
    let mut font = Font::default();
    let mut key = None;
    let mut strokes = Vec::new();
    let segarc_degrees = if segarc_degrees > 0.0 {
        segarc_degrees
    } else {
        5.0
    };

    for line in input.lines() {
        check_canceled(cancel)?;
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }

        if let Some(new_key) = parse_character_header(text) {
            if let Some(old_key) = key.replace(new_key) {
                font.glyphs.insert(
                    old_key,
                    Glyph {
                        key: old_key,
                        strokes: std::mem::take(&mut strokes),
                    },
                );
            }
            continue;
        }

        if let Some(coords) = text.strip_prefix("L ") {
            let coords = parse_numbers::<4>(coords, 4).ok_or_else(|| FontError::InvalidLine {
                line: text.to_owned(),
            })?;
            strokes.push(Stroke::new(coords[0], coords[1], coords[2], coords[3]));
            continue;
        }

        if let Some(coords) = text.strip_prefix("A ") {
            let coords = parse_numbers::<5>(coords, 5).ok_or_else(|| FontError::InvalidArc {
                line: text.to_owned(),
            })?;
            append_arc_segments(&mut strokes, coords, segarc_degrees, cancel)?;
        }
    }

    if let Some(old_key) = key {
        font.glyphs.insert(
            old_key,
            Glyph {
                key: old_key,
                strokes,
            },
        );
    }

    Ok(font)
}

pub fn read_ttf(path: &Path, segarc_degrees: f64, extended_chars: bool) -> Result<Font, FontError> {
    read_ttf_with_cancel(path, segarc_degrees, extended_chars, &|| false)
}

pub fn read_ttf_with_cancel(
    path: &Path,
    segarc_degrees: f64,
    extended_chars: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Font, FontError> {
    check_canceled(cancel)?;
    let data = fs::read(path).map_err(|source| FontError::ReadTtf {
        path: path.to_owned(),
        source,
    })?;
    parse_ttf_with_cancel(&data, path, segarc_degrees, extended_chars, cancel)
}

fn parse_ttf_with_cancel(
    data: &[u8],
    path: &Path,
    segarc_degrees: f64,
    extended_chars: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Font, FontError> {
    check_canceled(cancel)?;
    let face = Face::parse(data, 0).map_err(|_| FontError::InvalidTtf {
        path: path.to_owned(),
    })?;
    let scale = ttf_scale_factor(&face).ok_or_else(|| FontError::MissingScaleGlyph {
        path: path.to_owned(),
    })?;
    let max_code = if extended_chars { 0xffff } else { 0xff };
    let mut font = Font::default();

    for code in 0..=max_code {
        check_canceled(cancel)?;
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        let Some(glyph_id) = face.glyph_index(ch) else {
            continue;
        };
        let mut builder = TtfStrokeBuilder::new(scale, segarc_degrees);
        if face.outline_glyph(glyph_id, &mut builder).is_some() && !builder.strokes.is_empty() {
            font.glyphs.insert(
                code,
                Glyph {
                    key: code,
                    strokes: builder.strokes,
                },
            );
        }
    }

    Ok(font)
}

fn check_canceled(cancel: &dyn Fn() -> bool) -> Result<(), FontError> {
    if cancel() {
        Err(FontError::Canceled)
    } else {
        Ok(())
    }
}

fn ttf_scale_factor(face: &Face<'_>) -> Option<f64> {
    let glyph_id = face.glyph_index('A').or(Some(GlyphId(1)))?;
    let mut builder = RawYMaxBuilder::default();
    face.outline_glyph(glyph_id, &mut builder)?;
    let ymax = builder.ymax?;
    (ymax.abs() > f64::EPSILON).then_some(9.0 / ymax)
}

fn parse_character_header(text: &str) -> Option<u32> {
    let rest = text.strip_prefix('[')?;
    let (key, after) = rest.split_once(']')?;
    if !after.starts_with(char::is_whitespace) {
        return None;
    }

    if key.chars().count() == 1 {
        return key.chars().next().map(|ch| ch as u32);
    }

    let hex = if key.len() == 5 { &key[1..] } else { key };
    if hex.len() == 4 {
        u32::from_str_radix(hex, 16).ok()
    } else {
        None
    }
}

fn parse_numbers<const N: usize>(text: &str, expected: usize) -> Option<[f64; N]> {
    debug_assert_eq!(N, expected);
    let mut output = [0.0; N];
    let mut count = 0;
    for token in text.split(',') {
        if count == N {
            return None;
        }
        output[count] = token.trim().parse().ok()?;
        count += 1;
    }
    (count == expected).then_some(output)
}

fn append_arc_segments(
    strokes: &mut Vec<Stroke>,
    coords: [f64; 5],
    segarc_degrees: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<(), FontError> {
    let [xcenter, ycenter, radius, mut start_angle, end_angle] = coords;
    if end_angle < start_angle {
        start_angle -= 360.0;
    }

    let segs = ((end_angle - start_angle) / segarc_degrees).trunc() as usize + 1;
    let angle_increment = (end_angle - start_angle) / segs as f64;
    let mut angle = start_angle;
    let mut xstart = cos_degrees(start_angle) * radius + xcenter;
    let mut ystart = sin_degrees(start_angle) * radius + ycenter;

    for _ in 0..segs {
        check_canceled(cancel)?;
        angle += angle_increment;
        let xend = cos_degrees(angle) * radius + xcenter;
        let yend = sin_degrees(angle) * radius + ycenter;
        strokes.push(Stroke::new(xstart, ystart, xend, yend));
        xstart = xend;
        ystart = yend;
    }
    Ok(())
}

fn sin_degrees(angle: f64) -> f64 {
    (angle * PI / 180.0).sin()
}

fn cos_degrees(angle: f64) -> f64 {
    (angle * PI / 180.0).cos()
}

#[derive(Debug, Default)]
struct RawYMaxBuilder {
    ymax: Option<f64>,
}

impl RawYMaxBuilder {
    fn include(&mut self, y: f32) {
        self.ymax = Some(self.ymax.unwrap_or(f64::NEG_INFINITY).max(f64::from(y)));
    }
}

impl OutlineBuilder for RawYMaxBuilder {
    fn move_to(&mut self, _x: f32, y: f32) {
        self.include(y);
    }

    fn line_to(&mut self, _x: f32, y: f32) {
        self.include(y);
    }

    fn quad_to(&mut self, _x1: f32, y1: f32, _x: f32, y: f32) {
        self.include(y1);
        self.include(y);
    }

    fn curve_to(&mut self, _x1: f32, y1: f32, _x2: f32, y2: f32, _x: f32, y: f32) {
        self.include(y1);
        self.include(y2);
        self.include(y);
    }

    fn close(&mut self) {}
}

#[derive(Debug)]
struct TtfStrokeBuilder {
    strokes: Vec<Stroke>,
    prev: Option<Point>,
    factor: f64,
    segarc_radians: f64,
}

impl TtfStrokeBuilder {
    fn new(factor: f64, segarc_degrees: f64) -> Self {
        let segarc_degrees = if segarc_degrees > 0.0 {
            segarc_degrees
        } else {
            45.0
        };
        Self {
            strokes: Vec::new(),
            prev: None,
            factor,
            segarc_radians: segarc_degrees * PI / 180.0,
        }
    }

    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(f64::from(x), f64::from(y))
    }

    fn push_scaled_line(&mut self, start: Point, end: Point) {
        self.strokes.push(Stroke {
            start: Point::new(start.x * self.factor, start.y * self.factor),
            end: Point::new(end.x * self.factor, end.y * self.factor),
        });
    }

    fn flatten_quad(&mut self, control: Point, to: Point) {
        let Some(from) = self.prev else {
            self.prev = Some(to);
            return;
        };
        let mut t1 = 0.0;
        let mut step = 0.25;
        let mut start = quadratic(from, control, to, t1);

        while t1 < 1.0 {
            let t2 = (t1 + step).min(1.0);
            let end = quadratic(from, control, to, t2);
            let mid_t = (t1 + t2) / 2.0;
            let midpoint = quadratic(from, control, to, mid_t);
            let angle = approximate_arc_angle(start, midpoint, end);

            if angle > self.segarc_radians && step > 1.0e-9 {
                step /= 2.0;
            } else {
                self.push_scaled_line(start, end);
                step *= 2.0;
                t1 = t2;
                start = end;
            }
        }

        self.prev = Some(to);
    }
}

impl OutlineBuilder for TtfStrokeBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.prev = Some(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.point(x, y);
        if let Some(from) = self.prev {
            self.push_scaled_line(from, to);
        }
        self.prev = Some(to);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.flatten_quad(self.point(x1, y1), self.point(x, y));
    }

    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, x: f32, y: f32) {
        let to = self.point(x, y);
        if let Some(from) = self.prev {
            self.push_scaled_line(from, to);
        }
        self.prev = Some(to);
    }

    fn close(&mut self) {}
}

fn quadratic(from: Point, control: Point, to: Point, t: f64) -> Point {
    let one_minus = 1.0 - t;
    Point::new(
        one_minus.powi(2) * from.x + 2.0 * t * one_minus * control.x + t.powi(2) * to.x,
        one_minus.powi(2) * from.y + 2.0 * t * one_minus * control.y + t.powi(2) * to.y,
    )
}

fn approximate_arc_angle(start: Point, midpoint: Point, end: Point) -> f64 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let chord = (dx * dx + dy * dy).sqrt();
    let dx_mid = midpoint.x - (start.x + end.x) / 2.0;
    let dy_mid = midpoint.y - (start.y + end.y) / 2.0;
    let sagitta = (dx_mid * dx_mid + dy_mid * dy_mid).sqrt();
    let l1_squared = (midpoint.x - start.x).powi(2) + (midpoint.y - start.y).powi(2);
    let l2_squared = (end.x - midpoint.x).powi(2) + (end.y - midpoint.y).powi(2);

    if sagitta < 1.0e-6 || l1_squared < 1.0e-6 || l2_squared < 1.0e-6 {
        return 0.0;
    }

    let radius = (chord * chord / 4.0 + sagitta * sagitta) / (2.0 * sagitta);
    let ratio = ((chord / 2.0) / radius).clamp(-1.0, 1.0);
    2.0 * ratio.asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn parses_cxf_line_glyphs() {
        let font = parse_cxf(
            r#"
[A] 3
L 0,0,1,2
L 1,2,2,0
[0042] B
L 0,0,0,2
"#,
            5.0,
        )
        .unwrap();

        assert_eq!(font.glyphs.len(), 2);
        assert_eq!(font.get_char('A').unwrap().xmax(), 2.0);
        assert_eq!(font.get_char('B').unwrap().strokes.len(), 1);
    }

    #[test]
    fn converts_cxf_arcs_to_line_segments() {
        let font = parse_cxf(
            r#"
[O] 1
A 0,0,1,0,90
"#,
            45.0,
        )
        .unwrap();

        let glyph = font.get_char('O').unwrap();
        assert_eq!(glyph.strokes.len(), 3);
        assert!((glyph.strokes[0].start.x - 1.0).abs() < 1e-9);
        assert!((glyph.strokes.last().unwrap().end.y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cxf_parse_can_cancel_inside_arc_expansion() {
        let calls = Cell::new(0usize);
        let err = parse_cxf_with_cancel("[O] 1\nA 0,0,1,0,360\n", 0.01, &|| {
            let next = calls.get() + 1;
            calls.set(next);
            next > 4
        })
        .unwrap_err();

        assert!(matches!(err, FontError::Canceled));
        assert!(calls.get() > 4);
    }

    #[test]
    fn ttf_parse_can_cancel_before_font_walk() {
        let err = parse_ttf_with_cancel(b"not a font", Path::new("bad.ttf"), 5.0, false, &|| true)
            .unwrap_err();

        assert!(matches!(err, FontError::Canceled));
    }

    #[test]
    fn invalid_ttf_data_returns_error() {
        let err = parse_ttf_with_cancel(b"not a font", Path::new("bad.ttf"), 5.0, false, &|| false)
            .unwrap_err();
        assert!(matches!(err, FontError::InvalidTtf { .. }));
    }
}
