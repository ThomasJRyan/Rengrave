use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use image::{DynamicImage, RgbaImage};
use rengrave_potrace::{Bitmap, Options as NativePotraceOptions, TurnPolicy};

use crate::settings::{LegacySettings, get_legacy_bool};

const BITMAP_TRACE_THRESHOLD: u32 = 128;

#[derive(Debug, thiserror::Error)]
pub enum BitmapError {
    #[error("unable to decode bitmap `{path}`: {source}")]
    Decode {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("unable to write temporary PBM `{path}`: {source}")]
    WriteTemp {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unable to run Potrace: {source}")]
    RunPotrace { source: std::io::Error },
    #[error("Potrace failed: {stderr}")]
    PotraceFailed { stderr: String },
    #[error("native Potrace failed: {source}")]
    NativePotraceFailed { source: rengrave_potrace::Error },
    #[error("bitmap vectorization canceled")]
    Canceled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BitmapTraceStats {
    pub black_pixels: usize,
    pub white_pixels: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapBackend {
    NativePotrace,
    PotraceSidecar,
}

impl BitmapBackend {
    pub const ALL: [Self; 2] = [Self::NativePotrace, Self::PotraceSidecar];

    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "potrace-sidecar" | "external-potrace" | "sidecar" => Self::PotraceSidecar,
            _ => Self::NativePotrace,
        }
    }

    pub fn from_settings(settings: &LegacySettings) -> Self {
        settings
            .get_last("bitmap_backend")
            .map(Self::parse)
            .unwrap_or(Self::NativePotrace)
    }

    pub fn value(self) -> &'static str {
        match self {
            Self::NativePotrace => "native-potrace",
            Self::PotraceSidecar => "potrace-sidecar",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NativePotrace => "Native Potrace",
            Self::PotraceSidecar => "Potrace sidecar",
        }
    }

    pub fn requires_potrace(self) -> bool {
        self == Self::PotraceSidecar
    }
}

pub fn bitmap_trace_mask_and_stats(image: &DynamicImage) -> (image::RgbaImage, BitmapTraceStats) {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut mask = image::RgbaImage::new(width, height);
    let mut stats = BitmapTraceStats::default();

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let value = if bitmap_pixel_is_black(pixel.0) {
            stats.black_pixels += 1;
            0
        } else {
            stats.white_pixels += 1;
            255
        };
        mask.put_pixel(x, y, image::Rgba([value, value, value, 255]));
    }

    (mask, stats)
}

pub fn vectorize_bitmap_to_dxf(
    path: &Path,
    settings: &LegacySettings,
) -> Result<String, BitmapError> {
    vectorize_bitmap_to_dxf_with_cancel(path, settings, &|| false)
}

pub fn vectorize_bitmap_to_dxf_with_cancel(
    path: &Path,
    settings: &LegacySettings,
    cancel: &dyn Fn() -> bool,
) -> Result<String, BitmapError> {
    match BitmapBackend::from_settings(settings) {
        BitmapBackend::NativePotrace => {
            vectorize_bitmap_with_native_potrace_to_dxf(path, settings, cancel)
        }
        BitmapBackend::PotraceSidecar => {
            vectorize_bitmap_with_potrace_sidecar_to_dxf(path, settings, cancel)
        }
    }
}

fn vectorize_bitmap_with_native_potrace_to_dxf(
    path: &Path,
    settings: &LegacySettings,
    cancel: &dyn Fn() -> bool,
) -> Result<String, BitmapError> {
    check_canceled(cancel)?;
    let image = image::open(path).map_err(|source| BitmapError::Decode {
        path: path.to_owned(),
        source,
    })?;
    let bitmap = image_to_native_potrace_bitmap(image, cancel)?;
    let options = NativePotraceSettings::from_settings(settings).options();
    rengrave_potrace::trace_bitmap_to_dxf(&bitmap, options)
        .map_err(|source| BitmapError::NativePotraceFailed { source })
}

fn vectorize_bitmap_with_potrace_sidecar_to_dxf(
    path: &Path,
    settings: &LegacySettings,
    cancel: &dyn Fn() -> bool,
) -> Result<String, BitmapError> {
    check_canceled(cancel)?;
    let options = PotraceOptions::from_settings(settings);
    let temp_path = write_temp_pbm(path, cancel)?;
    let input = temp_path.as_path();
    let output = run_potrace(input, &options, cancel);

    let _ = std::fs::remove_file(temp_path);

    output
}

fn run_potrace(
    input: &Path,
    options: &PotraceOptions,
    cancel: &dyn Fn() -> bool,
) -> Result<String, BitmapError> {
    check_canceled(cancel)?;
    let mut child = Command::new("potrace")
        .args(options.args(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| BitmapError::RunPotrace { source })?;
    let mut stdout = child.stdout.take().ok_or_else(pipe_error)?;
    let mut stderr = child.stderr.take().ok_or_else(pipe_error)?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });

    let status = loop {
        if cancel() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_reader(stdout_reader);
            let _ = join_reader(stderr_reader);
            return Err(BitmapError::Canceled);
        }
        if let Some(status) = child.try_wait().map_err(|source| {
            let _ = child.kill();
            BitmapError::RunPotrace { source }
        })? {
            break status;
        }
        thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;

    if !status.success() {
        return Err(BitmapError::PotraceFailed {
            stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
        });
    }

    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

fn join_reader(
    reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, BitmapError> {
    reader
        .join()
        .map_err(|_| BitmapError::RunPotrace {
            source: std::io::Error::new(
                std::io::ErrorKind::Other,
                "Potrace pipe reader thread stopped",
            ),
        })?
        .map_err(|source| BitmapError::RunPotrace { source })
}

fn pipe_error() -> BitmapError {
    BitmapError::RunPotrace {
        source: std::io::Error::new(std::io::ErrorKind::Other, "Potrace pipe was not available"),
    }
}

fn write_temp_pbm(path: &Path, cancel: &dyn Fn() -> bool) -> Result<PathBuf, BitmapError> {
    check_canceled(cancel)?;
    let image = image::open(path).map_err(|source| BitmapError::Decode {
        path: path.to_owned(),
        source,
    })?;
    let bytes = image_to_pbm_bytes_with_cancel(image, cancel)?;
    let temp_path = std::env::temp_dir().join(format!(
        "rengrave-potrace-{}-{}.pbm",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&temp_path, bytes).map_err(|source| BitmapError::WriteTemp {
        path: temp_path.clone(),
        source,
    })?;
    Ok(temp_path)
}

fn image_to_pbm_bytes_with_cancel(
    image: DynamicImage,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<u8>, BitmapError> {
    let rgba = image.to_rgba8();
    let bounds = trace_content_bounds(&rgba, cancel)?;
    let (width, height) = bounds
        .map(|bounds| (bounds.width(), bounds.height()))
        .unwrap_or((1, 1));
    let mut output = format!("P4\n{width} {height}\n").into_bytes();

    let Some(bounds) = bounds else {
        output.push(0);
        return Ok(output);
    };

    for y in bounds.min_y..=bounds.max_y {
        check_canceled(cancel)?;
        let mut byte = 0u8;
        let mut bit = 0;
        for x in bounds.min_x..=bounds.max_x {
            let pixel = rgba.get_pixel(x, y).0;
            if bitmap_pixel_is_black(pixel) {
                byte |= 0x80 >> bit;
            }
            bit += 1;
            if bit == 8 {
                output.push(byte);
                byte = 0;
                bit = 0;
            }
        }
        if bit != 0 {
            output.push(byte);
        }
    }

    Ok(output)
}

fn image_to_native_potrace_bitmap(
    image: DynamicImage,
    cancel: &dyn Fn() -> bool,
) -> Result<Bitmap, BitmapError> {
    let rgba = image.to_rgba8();
    let bounds = trace_content_bounds(&rgba, cancel)?;
    let (width, height) = bounds
        .map(|bounds| (bounds.width(), bounds.height()))
        .unwrap_or((1, 1));
    let mut bits = Vec::with_capacity((width * height) as usize);

    let Some(bounds) = bounds else {
        bits.push(false);
        return Bitmap::from_bits(1, 1, bits)
            .map_err(|source| BitmapError::NativePotraceFailed { source });
    };

    for y in (bounds.min_y..=bounds.max_y).rev() {
        check_canceled(cancel)?;
        for x in bounds.min_x..=bounds.max_x {
            let pixel = rgba.get_pixel(x, y).0;
            bits.push(bitmap_pixel_is_black(pixel));
        }
    }

    Bitmap::from_bits(width as i32, height as i32, bits)
        .map_err(|source| BitmapError::NativePotraceFailed { source })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TraceContentBounds {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

impl TraceContentBounds {
    fn width(self) -> u32 {
        self.max_x - self.min_x + 1
    }

    fn height(self) -> u32 {
        self.max_y - self.min_y + 1
    }
}

fn trace_content_bounds(
    rgba: &RgbaImage,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<TraceContentBounds>, BitmapError> {
    let (width, height) = rgba.dimensions();
    let mut bounds: Option<TraceContentBounds> = None;

    for y in 0..height {
        check_canceled(cancel)?;
        for x in 0..width {
            if !bitmap_pixel_is_black(rgba.get_pixel(x, y).0) {
                continue;
            }

            if let Some(bounds) = &mut bounds {
                bounds.min_x = bounds.min_x.min(x);
                bounds.min_y = bounds.min_y.min(y);
                bounds.max_x = bounds.max_x.max(x);
                bounds.max_y = bounds.max_y.max(y);
            } else {
                bounds = Some(TraceContentBounds {
                    min_x: x,
                    min_y: y,
                    max_x: x,
                    max_y: y,
                });
            }
        }
    }

    Ok(bounds)
}

fn bitmap_pixel_is_black(pixel: [u8; 4]) -> bool {
    let alpha = pixel[3] as u32;
    let red = composite_over_white(pixel[0] as u32, alpha);
    let green = composite_over_white(pixel[1] as u32, alpha);
    let blue = composite_over_white(pixel[2] as u32, alpha);
    let luma = (299 * red + 587 * green + 114 * blue) / 1000;
    luma < BITMAP_TRACE_THRESHOLD
}

fn check_canceled(cancel: &dyn Fn() -> bool) -> Result<(), BitmapError> {
    if cancel() {
        Err(BitmapError::Canceled)
    } else {
        Ok(())
    }
}

fn composite_over_white(channel: u32, alpha: u32) -> u32 {
    (channel * alpha + 255 * (255 - alpha)) / 255
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NativePotraceSettings {
    turn_policy: TurnPolicy,
    turd_size: i32,
    alpha_max: f64,
    opti_curve: bool,
    opt_tolerance: f64,
}

impl NativePotraceSettings {
    fn from_settings(settings: &LegacySettings) -> Self {
        Self {
            turn_policy: TurnPolicy::parse(settings.get_last("bmp_turnp").unwrap_or("minority")),
            turd_size: settings
                .get_last("bmp_turds")
                .and_then(|value| value.parse().ok())
                .unwrap_or(2),
            alpha_max: get_f64(settings, "bmp_alpha", 1.0),
            opti_curve: get_legacy_bool(settings, "bmp_long", true),
            opt_tolerance: get_f64(settings, "bmp_optto", 0.2),
        }
    }

    fn options(self) -> NativePotraceOptions {
        NativePotraceOptions {
            turd_size: self.turd_size,
            turn_policy: self.turn_policy,
            alpha_max: self.alpha_max,
            opti_curve: self.opti_curve,
            opt_tolerance: self.opt_tolerance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PotraceOptions {
    turn_policy: String,
    turd_size: String,
    alpha_max: String,
    opt_tolerance: String,
    long_curve: bool,
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

impl PotraceOptions {
    fn from_settings(settings: &LegacySettings) -> Self {
        Self {
            turn_policy: settings
                .get_last("bmp_turnp")
                .unwrap_or("minority")
                .to_owned(),
            turd_size: settings.get_last("bmp_turds").unwrap_or("2").to_owned(),
            alpha_max: settings.get_last("bmp_alpha").unwrap_or("1").to_owned(),
            opt_tolerance: settings.get_last("bmp_optto").unwrap_or("0.2").to_owned(),
            long_curve: get_legacy_bool(settings, "bmp_long", true),
        }
    }

    fn args(&self, input: &Path) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("-z"),
            OsString::from(&self.turn_policy),
            OsString::from("-t"),
            OsString::from(&self.turd_size),
            OsString::from("-a"),
            OsString::from(&self.alpha_max),
        ];
        if self.long_curve {
            args.push(OsString::from("-O"));
            args.push(OsString::from(&self.opt_tolerance));
        } else {
            args.push(OsString::from("-n"));
        }
        args.extend([
            OsString::from("-b"),
            OsString::from("dxf"),
            input.as_os_str().to_owned(),
            OsString::from("-o"),
            OsString::from("-"),
        ]);
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};
    use std::cell::Cell;

    #[test]
    fn trace_mask_matches_pbm_threshold_and_counts_full_image() {
        let mut image = RgbaImage::new(3, 1);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        image.put_pixel(1, 0, Rgba([255, 255, 255, 255]));
        image.put_pixel(2, 0, Rgba([0, 0, 0, 0]));

        let (mask, stats) = bitmap_trace_mask_and_stats(&DynamicImage::ImageRgba8(image));
        let rgba = mask.into_raw();

        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[255, 255, 255, 255]);
        assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
        assert_eq!(
            stats,
            BitmapTraceStats {
                black_pixels: 1,
                white_pixels: 2
            }
        );
    }

    #[test]
    fn bitmap_conversion_trims_white_border_and_writes_packed_pbm_bits() {
        let mut image = RgbaImage::from_pixel(10, 3, Rgba([255, 255, 255, 255]));
        for x in 2..5 {
            let value = if x < 4 { 0 } else { 255 };
            image.put_pixel(x, 0, Rgba([value, value, value, 255]));
        }
        image.put_pixel(2, 1, Rgba([0, 0, 0, 255]));

        let bytes =
            image_to_pbm_bytes_with_cancel(DynamicImage::ImageRgba8(image), &|| false).unwrap();

        assert_eq!(&bytes[..7], b"P4\n2 2\n");
        assert_eq!(bytes[7], 0b1100_0000);
        assert_eq!(bytes[8], 0b1000_0000);
    }

    #[test]
    fn fully_white_bitmap_converts_to_minimal_empty_pbm() {
        let image = RgbaImage::from_pixel(4, 3, Rgba([255, 255, 255, 255]));

        let bytes =
            image_to_pbm_bytes_with_cancel(DynamicImage::ImageRgba8(image), &|| false).unwrap();

        assert_eq!(&bytes, b"P4\n1 1\n\0");
    }

    #[test]
    fn native_potrace_bitmap_trims_to_content_bounds() {
        let mut image = RgbaImage::from_pixel(6, 5, Rgba([255, 255, 255, 255]));
        image.put_pixel(2, 1, Rgba([0, 0, 0, 255]));
        image.put_pixel(4, 3, Rgba([0, 0, 0, 255]));

        let bitmap =
            image_to_native_potrace_bitmap(DynamicImage::ImageRgba8(image), &|| false).unwrap();

        assert_eq!(bitmap.width(), 3);
        assert_eq!(bitmap.height(), 3);
    }

    #[test]
    fn transparent_pixels_composite_to_white_for_pbm_conversion() {
        let mut image = RgbaImage::new(1, 1);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));

        let bytes =
            image_to_pbm_bytes_with_cancel(DynamicImage::ImageRgba8(image), &|| false).unwrap();

        assert_eq!(bytes.last(), Some(&0));
    }

    #[test]
    fn pbm_conversion_can_cancel_between_rows() {
        let image = RgbaImage::from_pixel(1, 3, Rgba([0, 0, 0, 255]));
        let calls = Cell::new(0usize);

        let err = image_to_pbm_bytes_with_cancel(DynamicImage::ImageRgba8(image), &|| {
            let next = calls.get() + 1;
            calls.set(next);
            next > 1
        })
        .unwrap_err();

        assert!(matches!(err, BitmapError::Canceled));
        assert!(calls.get() > 1);
    }

    #[test]
    fn gif_inputs_decode_for_pbm_conversion() {
        let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
        let image = image::load_from_memory(gif).unwrap();

        let bytes = image_to_pbm_bytes_with_cancel(image, &|| false).unwrap();

        assert_eq!(&bytes[..7], b"P4\n1 1\n");
        assert_eq!(bytes[7], 0b1000_0000);
    }

    #[test]
    fn bitmap_backend_defaults_to_native_potrace_and_accepts_sidecar() {
        let settings = crate::settings::default_legacy_settings();
        assert_eq!(
            BitmapBackend::from_settings(&settings),
            BitmapBackend::NativePotrace
        );

        let mut settings = settings;
        settings.set_or_push("bitmap_backend", "potrace-sidecar", false);
        assert_eq!(
            BitmapBackend::from_settings(&settings),
            BitmapBackend::PotraceSidecar
        );
    }

    #[test]
    fn native_potrace_backend_vectorizes_bitmap_without_sidecar() {
        let mut image = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
        for y in 8..24 {
            for x in 8..24 {
                image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }

        let path = std::env::temp_dir().join(format!(
            "rengrave-native-potrace-test-{}-{}.png",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        DynamicImage::ImageRgba8(image).save(&path).unwrap();

        let dxf = vectorize_bitmap_to_dxf(&path, &crate::settings::default_legacy_settings())
            .expect("native Potrace bitmap vectorization should produce DXF");

        let _ = std::fs::remove_file(&path);
        assert!(dxf.contains("POLYLINE"));
        assert!(dxf.contains("VERTEX"));
        assert!(dxf.ends_with("  0\nEOF\n"));
    }

    #[test]
    fn potrace_args_match_f_engrave_long_curve_mode() {
        let settings = crate::settings::default_legacy_settings();
        let args = PotraceOptions::from_settings(&settings).args(Path::new("input.pbm"));
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            args,
            [
                "-z",
                "minority",
                "-t",
                "2",
                "-a",
                "1",
                "-O",
                "0.2",
                "-b",
                "dxf",
                "input.pbm",
                "-o",
                "-"
            ]
        );
    }

    #[test]
    fn potrace_args_match_f_engrave_polygon_mode() {
        let mut settings = crate::settings::default_legacy_settings();
        settings.set_or_push("bmp_long", "0", false);

        let args = PotraceOptions::from_settings(&settings).args(Path::new("input.pbm"));
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(args.contains(&"-n".to_owned()));
        assert!(!args.contains(&"-O".to_owned()));
    }
}
