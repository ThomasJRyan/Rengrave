use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use image::DynamicImage;

use crate::settings::{LegacySettings, get_legacy_bool};

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
}

pub fn vectorize_bitmap_to_dxf(
    path: &Path,
    settings: &LegacySettings,
) -> Result<String, BitmapError> {
    let options = PotraceOptions::from_settings(settings);
    let temp_path = if needs_image_conversion(path) {
        Some(write_temp_pbm(path)?)
    } else {
        None
    };
    let input = temp_path.as_deref().unwrap_or(path);
    let output = run_potrace(input, &options);

    if let Some(path) = temp_path {
        let _ = std::fs::remove_file(path);
    }

    output
}

fn run_potrace(input: &Path, options: &PotraceOptions) -> Result<String, BitmapError> {
    let output = Command::new("potrace")
        .args(options.args(input))
        .output()
        .map_err(|source| BitmapError::RunPotrace { source })?;

    if !output.status.success() {
        return Err(BitmapError::PotraceFailed {
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn write_temp_pbm(path: &Path) -> Result<PathBuf, BitmapError> {
    let image = image::open(path).map_err(|source| BitmapError::Decode {
        path: path.to_owned(),
        source,
    })?;
    let bytes = image_to_pbm_bytes(image);
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

fn image_to_pbm_bytes(image: DynamicImage) -> Vec<u8> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut output = format!("P4\n{width} {height}\n").into_bytes();

    for y in 0..height {
        let mut byte = 0u8;
        let mut bit = 0;
        for x in 0..width {
            let pixel = rgba.get_pixel(x, y).0;
            let alpha = pixel[3] as u32;
            let red = composite_over_white(pixel[0] as u32, alpha);
            let green = composite_over_white(pixel[1] as u32, alpha);
            let blue = composite_over_white(pixel[2] as u32, alpha);
            let luma = (299 * red + 587 * green + 114 * blue) / 1000;
            if luma < 128 {
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

    output
}

fn composite_over_white(channel: u32, alpha: u32) -> u32 {
    (channel * alpha + 255 * (255 - alpha)) / 255
}

fn needs_image_conversion(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("gif" | "jpg" | "jpeg" | "png" | "tif" | "tiff")
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PotraceOptions {
    turn_policy: String,
    turd_size: String,
    alpha_max: String,
    opt_tolerance: String,
    long_curve: bool,
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

    #[test]
    fn bitmap_conversion_writes_packed_pbm_bits() {
        let mut image = RgbaImage::new(10, 1);
        for x in 0..10 {
            let value = if x < 3 { 0 } else { 255 };
            image.put_pixel(x, 0, Rgba([value, value, value, 255]));
        }

        let bytes = image_to_pbm_bytes(DynamicImage::ImageRgba8(image));

        assert_eq!(&bytes[..8], b"P4\n10 1\n");
        assert_eq!(bytes[8], 0b1110_0000);
        assert_eq!(bytes[9], 0);
    }

    #[test]
    fn transparent_pixels_composite_to_white_for_pbm_conversion() {
        let mut image = RgbaImage::new(1, 1);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));

        let bytes = image_to_pbm_bytes(DynamicImage::ImageRgba8(image));

        assert_eq!(bytes.last(), Some(&0));
    }

    #[test]
    fn gif_inputs_decode_for_pbm_conversion() {
        let gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";
        let image = image::load_from_memory(gif).unwrap();

        let bytes = image_to_pbm_bytes(image);

        assert_eq!(&bytes[..7], b"P4\n1 1\n");
        assert_eq!(bytes[7], 0b1000_0000);
        assert!(needs_image_conversion(Path::new("input.gif")));
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
