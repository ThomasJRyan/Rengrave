//! Native pocketing operations for the General workbench.
//!
//! Pocketing consumes the same closed exterior boundary adapter as profiling.
//! The pattern generators operate on the compensated interior, keeping
//! authored geometry separate from derived CAM motion.

use serde::{Deserialize, Serialize};

use crate::design::{DesignObject, DesignObjectId};
use crate::general_profile::{ProfileBoundary, ProfileToolpathContour, ProfileToolpathPoint};
use crate::general_toolbit::{GeneralSpindleDirection, GeneralToolbit};
use crate::geometry::Point;

const MINIMUM_DIMENSION_MM: f64 = 0.0001;
const CIRCLE_SEGMENTS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocketPathPattern {
    ZigZag,
    Offset,
    ZigZagOffset,
    Grid,
    Line,
}

impl PocketPathPattern {
    pub const ALL: [Self; 5] = [
        Self::ZigZag,
        Self::Offset,
        Self::ZigZagOffset,
        Self::Grid,
        Self::Line,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ZigZag => "ZigZag",
            Self::Offset => "Offset",
            Self::ZigZagOffset => "ZigZag + Offset",
            Self::Grid => "Grid",
            Self::Line => "Line",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PocketCutMode {
    #[default]
    Climb,
    Conventional,
}

impl PocketCutMode {
    pub const ALL: [Self; 2] = [Self::Climb, Self::Conventional];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Climb => "Climb",
            Self::Conventional => "Conventional",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PocketParameters {
    pub pattern: PocketPathPattern,
    pub cut_depth_mm: f64,
    pub step_down_mm: f64,
    pub start_height_mm: f64,
    pub safe_height_mm: f64,
    pub step_over_mm: f64,
    pub cut_mode: PocketCutMode,
    pub feed_mm_min: f64,
    pub plunge_mm_min: f64,
    #[serde(default)]
    pub rest_machining: bool,
}

impl Default for PocketParameters {
    fn default() -> Self {
        Self {
            pattern: PocketPathPattern::Offset,
            cut_depth_mm: 3.0,
            step_down_mm: 1.0,
            start_height_mm: 0.0,
            safe_height_mm: 3.0,
            step_over_mm: 1.0,
            cut_mode: PocketCutMode::Climb,
            feed_mm_min: 0.0,
            plunge_mm_min: 0.0,
            rest_machining: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneralPocketOperation {
    pub source_object_id: DesignObjectId,
    pub tool: GeneralToolbit,
    pub parameters: PocketParameters,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedPocket {
    pub contours: Vec<ProfileToolpathContour>,
    pub gcode: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PocketGenerationError {
    #[error("the pocket operation does not reference the supplied vector")]
    SourceMismatch,
    #[error("the selected toolbit is not suitable for pocketing")]
    UnsupportedTool,
    #[error("the selected vector has no closed interior boundary")]
    NotClosed,
    #[error("the selected toolbit does not fit inside the vector")]
    ToolDoesNotFit,
    #[error("pocket parameters must contain finite values")]
    NonFiniteParameter,
    #[error("cut depth, step down, step over, and safe height must be greater than zero")]
    InvalidDepth,
    #[error("tool feed and plunge rates must be greater than zero")]
    InvalidFeed,
    #[error("cut depth exceeds the toolbit cutting edge height")]
    CutDepthExceedsTool,
}

pub fn generate_pocket(
    operation: &GeneralPocketOperation,
    source: &DesignObject,
    surface_z_mm: f64,
) -> Result<GeneratedPocket, PocketGenerationError> {
    if operation.source_object_id != source.id {
        return Err(PocketGenerationError::SourceMismatch);
    }
    validate_operation(operation, surface_z_mm)?;
    let boundary = source
        .geometry
        .profile_exteriors()
        .into_iter()
        .next()
        .ok_or(PocketGenerationError::NotClosed)?;
    let radius = match boundary {
        ProfileBoundary::Circle { radius_mm, .. } => radius_mm,
        ProfileBoundary::ClosedContour { .. } => {
            return Err(PocketGenerationError::NotClosed);
        }
    };
    let usable_radius = radius - operation.tool.diameter_mm / 2.0;
    if usable_radius <= MINIMUM_DIMENSION_MM {
        return Err(PocketGenerationError::ToolDoesNotFit);
    }

    let depths = pass_depths(
        operation.parameters.cut_depth_mm,
        operation.parameters.step_down_mm,
    );
    let contours = circle_pattern(boundary, usable_radius, operation.parameters, &depths);
    if contours.is_empty() {
        return Err(PocketGenerationError::ToolDoesNotFit);
    }
    Ok(GeneratedPocket {
        gcode: write_pocket_gcode(operation, &contours, &depths, surface_z_mm),
        contours,
    })
}

fn validate_operation(
    operation: &GeneralPocketOperation,
    surface_z_mm: f64,
) -> Result<(), PocketGenerationError> {
    if !operation.tool.supports_profile_cut() || !operation.tool.validate().is_empty() {
        return Err(PocketGenerationError::UnsupportedTool);
    }
    let p = operation.parameters;
    if !surface_z_mm.is_finite()
        || !p.cut_depth_mm.is_finite()
        || !p.step_down_mm.is_finite()
        || !p.start_height_mm.is_finite()
        || !p.safe_height_mm.is_finite()
        || !p.step_over_mm.is_finite()
        || !p.feed_mm_min.is_finite()
        || !p.plunge_mm_min.is_finite()
    {
        return Err(PocketGenerationError::NonFiniteParameter);
    }
    if p.cut_depth_mm <= 0.0
        || p.step_down_mm <= 0.0
        || p.step_over_mm <= 0.0
        || p.safe_height_mm <= 0.0
    {
        return Err(PocketGenerationError::InvalidDepth);
    }
    if p.feed_mm_min <= 0.0 || p.plunge_mm_min <= 0.0 {
        return Err(PocketGenerationError::InvalidFeed);
    }
    if p.cut_depth_mm > operation.tool.cutting_edge_height_mm {
        return Err(PocketGenerationError::CutDepthExceedsTool);
    }
    Ok(())
}

fn pass_depths(cut_depth_mm: f64, step_down_mm: f64) -> Vec<f64> {
    let count = (cut_depth_mm / step_down_mm).ceil() as usize;
    (1..=count)
        .map(|pass| (pass as f64 * step_down_mm).min(cut_depth_mm))
        .collect()
}

fn circle_pattern(
    boundary: ProfileBoundary,
    usable_radius: f64,
    parameters: PocketParameters,
    depths: &[f64],
) -> Vec<ProfileToolpathContour> {
    let ProfileBoundary::Circle { center_mm, .. } = boundary else {
        return Vec::new();
    };
    let spacing = parameters
        .step_over_mm
        .min(usable_radius)
        .max(MINIMUM_DIMENSION_MM);
    let mut contours = Vec::new();
    for depth in depths {
        let rings = matches!(
            parameters.pattern,
            PocketPathPattern::Offset | PocketPathPattern::ZigZagOffset | PocketPathPattern::Grid
        );
        if rings {
            let mut radius = usable_radius;
            while radius > MINIMUM_DIMENSION_MM {
                contours.push(circle_contour(
                    center_mm,
                    radius,
                    -*depth,
                    parameters.cut_mode,
                ));
                radius -= spacing;
            }
        }
        if matches!(
            parameters.pattern,
            PocketPathPattern::ZigZag
                | PocketPathPattern::ZigZagOffset
                | PocketPathPattern::Grid
                | PocketPathPattern::Line
        ) {
            let mut y = if parameters.pattern == PocketPathPattern::Line {
                0.0
            } else {
                -usable_radius
            };
            let mut reverse = false;
            while y <= usable_radius + MINIMUM_DIMENSION_MM {
                let half_width = (usable_radius * usable_radius - y * y).max(0.0).sqrt();
                if half_width > MINIMUM_DIMENSION_MM {
                    let left = Point::new(center_mm.x - half_width, center_mm.y + y);
                    let right = Point::new(center_mm.x + half_width, center_mm.y + y);
                    let (start, end) = if reverse {
                        (right, left)
                    } else {
                        (left, right)
                    };
                    contours.push(ProfileToolpathContour {
                        points: vec![point(start, -*depth), point(end, -*depth)],
                        closed: false,
                    });
                    reverse = !reverse;
                }
                if matches!(parameters.pattern, PocketPathPattern::Line) {
                    break;
                }
                y += spacing;
            }
            if parameters.pattern == PocketPathPattern::Grid {
                let mut x = -usable_radius;
                while x <= usable_radius + MINIMUM_DIMENSION_MM {
                    let half_height = (usable_radius * usable_radius - x * x).max(0.0).sqrt();
                    if half_height > MINIMUM_DIMENSION_MM {
                        contours.push(ProfileToolpathContour {
                            points: vec![
                                point(
                                    Point::new(center_mm.x + x, center_mm.y - half_height),
                                    -*depth,
                                ),
                                point(
                                    Point::new(center_mm.x + x, center_mm.y + half_height),
                                    -*depth,
                                ),
                            ],
                            closed: false,
                        });
                    }
                    x += spacing;
                }
            }
        }
    }
    contours
}

fn circle_contour(
    center: Point,
    radius: f64,
    z: f64,
    mode: PocketCutMode,
) -> ProfileToolpathContour {
    let mut points = (0..CIRCLE_SEGMENTS)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / CIRCLE_SEGMENTS as f64;
            point(
                Point::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                ),
                z,
            )
        })
        .collect::<Vec<_>>();
    if mode == PocketCutMode::Conventional {
        points.reverse();
    }
    ProfileToolpathContour {
        points,
        closed: true,
    }
}

fn point(point: Point, z_mm: f64) -> ProfileToolpathPoint {
    ProfileToolpathPoint {
        x_mm: point.x,
        y_mm: point.y,
        z_mm,
    }
}

fn write_pocket_gcode(
    operation: &GeneralPocketOperation,
    contours: &[ProfileToolpathContour],
    depths: &[f64],
    surface_z_mm: f64,
) -> String {
    let p = operation.parameters;
    let safe_z = surface_z_mm + p.safe_height_mm;
    let start_z = surface_z_mm + p.start_height_mm;
    let approach_z = safe_z.max(start_z);
    let spindle = match operation.tool.spindle_direction {
        GeneralSpindleDirection::Forward => "M3",
        GeneralSpindleDirection::Reverse => "M4",
    };
    let mut lines = vec![
        "(R-Engrave General Pocket)".to_owned(),
        format!("(Pattern: {})", p.pattern.label()),
        format!("(Cut mode: {})", p.cut_mode.label()),
        format!(
            "(Rest machining: {})",
            if p.rest_machining { "on" } else { "off" }
        ),
        "G90".to_owned(),
        "G21".to_owned(),
        format!("T{} M6", operation.tool.tool_number),
        spindle.to_owned(),
        format!("G0 Z{}", format_mm(approach_z)),
    ];
    for depth in depths {
        let z = surface_z_mm - depth;
        for contour in contours.iter().filter(|contour| {
            contour
                .points
                .first()
                .map_or(false, |point| (point.z_mm + depth).abs() < 0.0001)
        }) {
            let Some(first) = contour.points.first() else {
                continue;
            };
            lines.push(format!(
                "G0 X{} Y{}",
                format_mm(first.x_mm),
                format_mm(first.y_mm)
            ));
            lines.push(format!("G0 Z{}", format_mm(start_z)));
            lines.push(format!(
                "G1 Z{} F{}",
                format_mm(z),
                format_feed(p.plunge_mm_min)
            ));
            for point in contour.points.iter().skip(1) {
                lines.push(format!(
                    "G1 X{} Y{} F{}",
                    format_mm(point.x_mm),
                    format_mm(point.y_mm),
                    format_feed(p.feed_mm_min)
                ));
            }
            if contour.closed {
                lines.push(format!(
                    "G1 X{} Y{} F{}",
                    format_mm(first.x_mm),
                    format_mm(first.y_mm),
                    format_feed(p.feed_mm_min)
                ));
            }
            lines.push(format!("G0 Z{}", format_mm(safe_z)));
        }
    }
    lines.extend(["M5".to_owned(), "M2".to_owned()]);
    lines.join("\n") + "\n"
}

fn format_mm(value: f64) -> String {
    format!("{value:.3}")
}
fn format_feed(value: f64) -> String {
    format!("{value:.1}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{DesignCircle, DesignGeometry, VectorDocument};
    use crate::general_toolbit::{GeneralToolbitKind, general_toolbit_presets};

    fn operation(id: DesignObjectId) -> GeneralPocketOperation {
        let mut tool = general_toolbit_presets()
            .into_iter()
            .find(|tool| tool.kind == GeneralToolbitKind::Endmill)
            .unwrap();
        tool.feed_mm_min = 600.0;
        tool.plunge_mm_min = 200.0;
        GeneralPocketOperation {
            source_object_id: id,
            tool,
            parameters: PocketParameters {
                cut_depth_mm: 2.5,
                step_down_mm: 1.0,
                step_over_mm: 1.0,
                safe_height_mm: 3.0,
                feed_mm_min: 600.0,
                plunge_mm_min: 200.0,
                ..PocketParameters::default()
            },
        }
    }

    #[test]
    fn offset_pocket_creates_inner_rings_at_each_depth() {
        let mut document = VectorDocument::default();
        let id = document.add_circle(Point::default(), 10.0).unwrap();
        let generated = generate_pocket(&operation(id), document.object(id).unwrap(), 0.0).unwrap();
        assert_eq!(generated.contours.len(), 27);
        assert!(generated.gcode.starts_with("(R-Engrave General Pocket)\n"));
        assert!(generated.gcode.contains("(Pattern: Offset)"));
    }

    #[test]
    fn pocket_rejects_a_cutter_larger_than_the_closed_vector() {
        let mut document = VectorDocument::default();
        let id = document.add_circle(Point::default(), 2.0).unwrap();
        let mut op = operation(id);
        op.tool.diameter_mm = 5.0;
        assert_eq!(
            generate_pocket(&op, document.object(id).unwrap(), 0.0),
            Err(PocketGenerationError::ToolDoesNotFit)
        );
    }

    #[test]
    fn pocket_requires_matching_source_and_valid_rates() {
        let mut document = VectorDocument::default();
        let id = document.add_circle(Point::default(), 10.0).unwrap();
        let other = document.add_circle(Point::new(20.0, 0.0), 10.0).unwrap();
        assert_eq!(
            generate_pocket(&operation(id), document.object(other).unwrap(), 0.0),
            Err(PocketGenerationError::SourceMismatch)
        );
        let mut op = operation(id);
        op.parameters.feed_mm_min = 0.0;
        assert_eq!(
            generate_pocket(&op, document.object(id).unwrap(), 0.0),
            Err(PocketGenerationError::InvalidFeed)
        );
    }

    #[test]
    fn every_path_pattern_produces_a_compensated_pocket() {
        let mut document = VectorDocument::default();
        let id = document.add_circle(Point::default(), 10.0).unwrap();
        for pattern in PocketPathPattern::ALL {
            let mut op = operation(id);
            op.parameters.pattern = pattern;
            let generated = generate_pocket(&op, document.object(id).unwrap(), 0.0)
                .expect("pattern should generate");
            assert!(
                !generated.contours.is_empty(),
                "{pattern:?} produced no paths"
            );
            assert!(
                generated
                    .gcode
                    .contains(&format!("(Pattern: {})", pattern.label()))
            );
        }
    }

    #[test]
    fn profile_exterior_adapter_remains_the_pocket_boundary_seam() {
        let geometry = DesignGeometry::Circle(DesignCircle {
            center_mm: Point::default(),
            radius_mm: 4.0,
        });
        assert!(matches!(
            geometry.profile_exteriors().as_slice(),
            [ProfileBoundary::Circle { .. }]
        ));
    }
}
