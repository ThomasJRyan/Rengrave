use std::path::Path;

pub fn is_bitmap_input(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "bmp" | "gif" | "jpg" | "jpeg" | "png" | "tif" | "tiff" | "pbm" | "ppm" | "pgm" | "pnm",
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_bitmap_inputs() {
        assert!(is_bitmap_input(Path::new("input.png")));
        assert!(is_bitmap_input(Path::new("input.gif")));
        assert!(is_bitmap_input(Path::new("input.jpeg")));
        assert!(is_bitmap_input(Path::new("input.PBM")));
        assert!(!is_bitmap_input(Path::new("input.dxf")));
        assert!(!is_bitmap_input(Path::new("input.cxf")));
    }
}
