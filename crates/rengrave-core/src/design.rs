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
    pub fn distance_to_path(self, point_mm: Point) -> f64 {
        let dx = point_mm.x - self.center_mm.x;
        let dy = point_mm.y - self.center_mm.y;
        (dx.hypot(dy) - self.radius_mm).abs()
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
    #[error("design object {0} does not exist")]
    ObjectNotFound(u64),
}

impl VectorDocument {
    pub fn objects(&self) -> &[DesignObject] {
        &self.objects
    }

    pub fn object(&self, id: DesignObjectId) -> Option<&DesignObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn add_circle(
        &mut self,
        center_mm: Point,
        radius_mm: f64,
    ) -> Result<DesignObjectId, DesignError> {
        validate_circle(center_mm, radius_mm)?;

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

    pub fn update_circle(
        &mut self,
        id: DesignObjectId,
        center_mm: Point,
        radius_mm: f64,
    ) -> Result<(), DesignError> {
        validate_circle(center_mm, radius_mm)?;
        let object = self
            .objects
            .iter_mut()
            .find(|object| object.id == id)
            .ok_or(DesignError::ObjectNotFound(id.get()))?;
        object.geometry = DesignGeometry::Circle(DesignCircle {
            center_mm,
            radius_mm,
        });
        Ok(())
    }

    pub fn hit_test(&self, point_mm: Point, tolerance_mm: f64) -> Option<DesignObjectId> {
        let tolerance_mm = tolerance_mm.max(0.0);
        let mut nearest = None;
        for object in self.objects.iter().rev() {
            let distance = match object.geometry {
                DesignGeometry::Circle(circle) => circle.distance_to_path(point_mm),
            };
            if distance <= tolerance_mm
                && nearest.is_none_or(|(_, nearest_distance)| distance < nearest_distance)
            {
                nearest = Some((object.id, distance));
            }
        }
        nearest.map(|(id, _)| id)
    }
}

fn validate_circle(center_mm: Point, radius_mm: f64) -> Result<(), DesignError> {
    if !center_mm.x.is_finite() || !center_mm.y.is_finite() {
        return Err(DesignError::NonFiniteCenter);
    }
    if !radius_mm.is_finite() || radius_mm <= 0.0 {
        return Err(DesignError::InvalidRadius);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circles_receive_stable_ids_and_hit_test_the_nearest_path() {
        let mut document = VectorDocument::default();
        let first = document
            .add_circle(Point::new(0.0, 0.0), 10.0)
            .expect("valid first circle");
        let second = document
            .add_circle(Point::new(5.0, 0.0), 4.0)
            .expect("valid second circle");

        assert_eq!(first.get(), 1);
        assert_eq!(second.get(), 2);
        assert_eq!(document.hit_test(Point::new(9.0, 0.0), 0.0), Some(second));
        assert_eq!(document.hit_test(Point::new(-10.0, 0.0), 0.0), Some(first));
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
    fn concentric_circles_are_selected_by_the_path_nearest_the_pointer() {
        let mut document = VectorDocument::default();
        let small = document
            .add_circle(Point::new(0.0, 0.0), 5.0)
            .expect("valid small circle");
        let medium = document
            .add_circle(Point::new(0.0, 0.0), 10.0)
            .expect("valid medium circle");
        let large = document
            .add_circle(Point::new(0.0, 0.0), 15.0)
            .expect("valid large circle");

        assert_eq!(document.hit_test(Point::new(5.0, 0.0), 0.5), Some(small));
        assert_eq!(document.hit_test(Point::new(10.0, 0.0), 0.5), Some(medium));
        assert_eq!(document.hit_test(Point::new(15.0, 0.0), 0.5), Some(large));
        assert_eq!(document.hit_test(Point::new(0.0, 0.0), 0.5), None);
        assert_eq!(document.hit_test(Point::new(5.4, 0.0), 0.5), Some(small));
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

    #[test]
    fn updating_a_circle_preserves_identity_and_rejects_invalid_changes() {
        let mut document = VectorDocument::default();
        let id = document
            .add_circle(Point::new(0.0, 0.0), 5.0)
            .expect("valid circle");

        document
            .update_circle(id, Point::new(12.0, -3.0), 8.0)
            .expect("valid update");
        assert_eq!(document.object(id).expect("updated object").id, id);
        assert_eq!(
            document.object(id).expect("updated object").geometry,
            DesignGeometry::Circle(DesignCircle {
                center_mm: Point::new(12.0, -3.0),
                radius_mm: 8.0,
            })
        );

        assert_eq!(
            document.update_circle(id, Point::new(99.0, 99.0), 0.0),
            Err(DesignError::InvalidRadius)
        );
        assert_eq!(
            document.object(id).expect("unchanged object").geometry,
            DesignGeometry::Circle(DesignCircle {
                center_mm: Point::new(12.0, -3.0),
                radius_mm: 8.0,
            })
        );
        assert_eq!(
            document.update_circle(DesignObjectId(999), Point::default(), 1.0),
            Err(DesignError::ObjectNotFound(999))
        );
    }
}
