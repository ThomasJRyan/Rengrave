//! Authored vector geometry for the General Purpose workbench.
//!
//! This model is intentionally independent from generated toolpaths and
//! preview segments. CAM operations can reference stable object IDs later
//! without making the editor depend on G-code output geometry.

use serde::{Deserialize, Serialize};

use crate::geometry::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DesignObjectId(u64);

impl DesignObjectId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DesignCircle {
    pub center_mm: Point,
    pub radius_mm: f64,
}

impl DesignCircle {
    pub fn contains(self, point_mm: Point, tolerance_mm: f64) -> bool {
        let dx = point_mm.x - self.center_mm.x;
        let dy = point_mm.y - self.center_mm.y;
        let selection_radius = self.radius_mm + tolerance_mm.max(0.0);
        dx * dx + dy * dy <= selection_radius * selection_radius
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DesignGeometry {
    Circle(DesignCircle),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DesignObject {
    pub id: DesignObjectId,
    pub geometry: DesignGeometry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorDocument {
    objects: Vec<DesignObject>,
    next_id: u64,
}

impl Default for VectorDocument {
    fn default() -> Self {
        Self {
            objects: Vec::new(),
            next_id: 1,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DesignError {
    #[error("circle center must contain finite coordinates")]
    NonFiniteCenter,
    #[error("circle radius must be finite and greater than zero")]
    InvalidRadius,
}

impl VectorDocument {
    pub fn objects(&self) -> &[DesignObject] {
        &self.objects
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn add_circle(
        &mut self,
        center_mm: Point,
        radius_mm: f64,
    ) -> Result<DesignObjectId, DesignError> {
        if !center_mm.x.is_finite() || !center_mm.y.is_finite() {
            return Err(DesignError::NonFiniteCenter);
        }
        if !radius_mm.is_finite() || radius_mm <= 0.0 {
            return Err(DesignError::InvalidRadius);
        }

        let id = DesignObjectId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.objects.push(DesignObject {
            id,
            geometry: DesignGeometry::Circle(DesignCircle {
                center_mm,
                radius_mm,
            }),
        });
        Ok(id)
    }

    pub fn remove_object(&mut self, id: DesignObjectId) -> bool {
        let original_len = self.objects.len();
        self.objects.retain(|object| object.id != id);
        self.objects.len() != original_len
    }

    /// Returns every object containing the point, ordered from the smallest
    /// enclosing geometry to the largest. Equal-sized objects retain visual
    /// stacking order with the newest object first.
    pub fn hit_candidates(&self, point_mm: Point, tolerance_mm: f64) -> Vec<DesignObjectId> {
        let mut candidates = self
            .objects
            .iter()
            .rev()
            .filter_map(|object| {
                let (hit, selection_size) = match object.geometry {
                    DesignGeometry::Circle(circle) => {
                        (circle.contains(point_mm, tolerance_mm), circle.radius_mm)
                    }
                };
                hit.then_some((object.id, selection_size))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.1.total_cmp(&right.1));
        candidates.into_iter().map(|(id, _)| id).collect()
    }

    pub fn hit_test(&self, point_mm: Point, tolerance_mm: f64) -> Option<DesignObjectId> {
        self.hit_candidates(point_mm, tolerance_mm)
            .into_iter()
            .next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circles_receive_stable_ids_and_hit_test_topmost_first() {
        let mut document = VectorDocument::default();
        let first = document
            .add_circle(Point::new(0.0, 0.0), 10.0)
            .expect("valid first circle");
        let second = document
            .add_circle(Point::new(5.0, 0.0), 4.0)
            .expect("valid second circle");

        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert_eq!(document.hit_test(Point::new(5.0, 0.0), 0.0), Some(second));
        assert_eq!(document.hit_test(Point::new(-9.0, 0.0), 0.0), Some(first));
        assert_eq!(document.hit_test(Point::new(20.0, 0.0), 0.0), None);
    }

    #[test]
    fn invalid_circles_are_rejected_without_consuming_ids() {
        let mut document = VectorDocument::default();
        assert_eq!(
            document.add_circle(Point::new(f64::NAN, 0.0), 2.0),
            Err(DesignError::NonFiniteCenter)
        );
        assert_eq!(
            document.add_circle(Point::new(0.0, 0.0), 0.0),
            Err(DesignError::InvalidRadius)
        );
        assert_eq!(
            document
                .add_circle(Point::new(0.0, 0.0), 2.0)
                .expect("valid circle")
                .get(),
            1
        );
    }

    #[test]
    fn nested_circle_hits_prefer_the_smallest_and_expose_all_candidates() {
        let mut document = VectorDocument::default();
        let small = document
            .add_circle(Point::new(0.0, 0.0), 5.0)
            .expect("valid small circle");
        let large = document
            .add_circle(Point::new(0.0, 0.0), 10.0)
            .expect("valid large circle");

        assert_eq!(
            document.hit_candidates(Point::new(0.0, 0.0), 0.0),
            vec![small, large]
        );
        assert_eq!(document.hit_test(Point::new(0.0, 0.0), 0.0), Some(small));
        assert_eq!(document.hit_test(Point::new(8.0, 0.0), 0.0), Some(large));
    }

    #[test]
    fn removing_an_object_preserves_other_ids_and_future_id_allocation() {
        let mut document = VectorDocument::default();
        let first = document
            .add_circle(Point::new(0.0, 0.0), 5.0)
            .expect("valid first circle");
        let second = document
            .add_circle(Point::new(10.0, 0.0), 5.0)
            .expect("valid second circle");

        assert!(document.remove_object(first));
        assert!(!document.remove_object(first));
        assert_eq!(document.objects().len(), 1);
        assert_eq!(document.objects()[0].id, second);
        assert_eq!(
            document
                .add_circle(Point::new(20.0, 0.0), 5.0)
                .expect("valid third circle")
                .get(),
            3
        );
    }
}
