use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewTransform {
    pub pan: Point,
    pub zoom: f64,
    pub model_rotation_degrees: f64,
    pub viewport_rotation_degrees: f64,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            pan: Point::default(),
            zoom: 1.0,
            model_rotation_degrees: 0.0,
            viewport_rotation_degrees: 0.0,
        }
    }
}

impl ViewTransform {
    pub fn total_rotation_radians(&self) -> f64 {
        (self.model_rotation_degrees + self.viewport_rotation_degrees).to_radians()
    }

    pub fn apply(&self, point: Point) -> Point {
        let (sin, cos) = self.total_rotation_radians().sin_cos();
        let rotated = Point {
            x: point.x * cos - point.y * sin,
            y: point.x * sin + point.y * cos,
        };

        Point {
            x: rotated.x * self.zoom + self.pan.x,
            y: rotated.y * self.zoom + self.pan.y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_model_and_viewport_rotation_before_zoom_and_pan() {
        let transform = ViewTransform {
            pan: Point::new(10.0, -3.0),
            zoom: 2.0,
            model_rotation_degrees: 45.0,
            viewport_rotation_degrees: 45.0,
        };

        let point = transform.apply(Point::new(1.0, 0.0));

        assert!((point.x - 10.0).abs() < 1e-9);
        assert!((point.y - -1.0).abs() < 1e-9);
    }
}
