//! Native profile-cut operations for the General workbench.
//!
//! Authored geometry enters through [`ProfileBoundary`]. The generated paths
//! and G-code are derived CAM output and never replace the source vectors.

use serde::{Deserialize, Serialize};

use crate::design::{DesignGeometry, DesignObject, DesignObjectId};
use crate::general_toolbit::{GeneralSpindleDirection, GeneralToolbit};
use crate::geometry::Point;

const PROFILE_FLATTEN_SEGMENTS: usize = 128;
const MINIMUM_DIMENSION_MM: f64 = 0.0001;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileBoundary {
    Circle {
        center_mm: Point,
        radius_mm: f64,
    },
    /// A closed exterior contour. The closing point is implicit.
    ClosedContour {
        points_mm: Vec<Point>,
    },
}

impl DesignGeometry {
    /// Returns only closed exterior boundaries suitable for profile cutting.
    ///
    /// This deliberately differs from an engraving/stroke adapter. Future
    /// compound vector objects (including imported SVGs) should expose their
    /// exterior silhouettes here, not every authored line in the object.
    pub fn profile_exteriors(self) -> Vec<ProfileBoundary> {
        match self {
            Self::Circle(circle) => vec![ProfileBoundary::Circle {
                center_mm: circle.center_mm,
                radius_mm: circle.radius_mm,
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileCutSide {
    #[default]
    Outside,
    OnLine,
    Inside,
}

impl ProfileCutSide {
    pub const ALL: [Self; 3] = [Self::Outside, Self::OnLine, Self::Inside];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Outside => "Outside",
            Self::OnLine => "On the line",
            Self::Inside => "Inside",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProfileParameters {
    pub cut_side: ProfileCutSide,
    /// Extra distance on the selected side. For on-line cuts this is signed:
    /// positive is outward and negative is inward.
    pub additional_offset_mm: f64,
    /// Positive depth measured down from the material surface.
    pub cut_depth_mm: f64,
    pub pass_depth_mm: f64,
    pub safe_height_mm: f64,
    /// Per-operation feed override, initialized from the selected toolbit.
    #[serde(default)]
    pub feed_mm_min: f64,
    /// Per-operation plunge override, initialized from the selected toolbit.
    #[serde(default)]
    pub plunge_mm_min: f64,
}

impl Default for ProfileParameters {
    fn default() -> Self {
        Self {
            cut_side: ProfileCutSide::Outside,
            additional_offset_mm: 0.0,
            cut_depth_mm: 3.0,
            pass_depth_mm: 1.0,
            safe_height_mm: 3.0,
            feed_mm_min: 0.0,
            plunge_mm_min: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneralProfileOperation {
    pub source_object_id: DesignObjectId,
    pub tool: GeneralToolbit,
    pub parameters: ProfileParameters,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProfileToolpathPoint {
    pub x_mm: f64,
    pub y_mm: f64,
    /// Negative below the material surface.
    pub z_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileToolpathContour {
    pub points: Vec<ProfileToolpathPoint>,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedProfile {
    pub contours: Vec<ProfileToolpathContour>,
    pub gcode: String,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProfileGenerationError {
    #[error("the profile operation does not reference the supplied vector")]
    SourceMismatch,
    #[error("the selected toolbit is not suitable for profile cutting")]
    UnsupportedTool,
    #[error("profile dimensions must contain finite values")]
    NonFiniteParameter,
    #[error("cut depth, pass depth, and safe height must be greater than zero")]
    InvalidDepth,
    #[error("tool feed and plunge rates must be greater than zero")]
    InvalidFeed,
    #[error("cut depth exceeds the toolbit cutting edge height")]
    CutDepthExceedsTool,
    #[error("the selected side and offset collapse the profile boundary")]
    CollapsedBoundary,
    #[error("the selected vector has no closed exterior boundary")]
    NoExteriorBoundary,
}

pub fn generate_profile(
    operation: &GeneralProfileOperation,
    source: &DesignObject,
    surface_z_mm: f64,
) -> Result<GeneratedProfile, ProfileGenerationError> {
    if operation.source_object_id != source.id {
        return Err(ProfileGenerationError::SourceMismatch);
    }
    validate_operation(operation, surface_z_mm)?;
    let cutter_radius = operation.tool.diameter_mm / 2.0;
    let offset = effective_offset(operation.parameters, cutter_radius);
    let boundaries = source.geometry.profile_exteriors();
    if boundaries.is_empty() {
        return Err(ProfileGenerationError::NoExteriorBoundary);
    }

    let mut offset_boundaries = Vec::new();
    for boundary in boundaries {
        offset_boundaries.extend(offset_boundary(&boundary, offset)?);
    }
    if offset_boundaries.is_empty() {
        return Err(ProfileGenerationError::CollapsedBoundary);
    }

    let depths = pass_depths(
        operation.parameters.cut_depth_mm,
        operation.parameters.pass_depth_mm,
    );
    let mut contours = Vec::new();
    for depth in &depths {
        for boundary in &offset_boundaries {
            contours.push(flatten_boundary(boundary, -*depth));
        }
    }

    Ok(GeneratedProfile {
        gcode: write_profile_gcode(operation, &offset_boundaries, &depths, surface_z_mm),
        contours,
    })
}

fn validate_operation(
    operation: &GeneralProfileOperation,
    surface_z_mm: f64,
) -> Result<(), ProfileGenerationError> {
    if !operation.tool.supports_profile_cut() || !operation.tool.validate().is_empty() {
        return Err(ProfileGenerationError::UnsupportedTool);
    }
    let parameters = operation.parameters;
    if !surface_z_mm.is_finite()
        || !parameters.additional_offset_mm.is_finite()
        || !parameters.cut_depth_mm.is_finite()
        || !parameters.pass_depth_mm.is_finite()
        || !parameters.safe_height_mm.is_finite()
        || !parameters.feed_mm_min.is_finite()
        || !parameters.plunge_mm_min.is_finite()
    {
        return Err(ProfileGenerationError::NonFiniteParameter);
    }
    if parameters.cut_depth_mm <= 0.0
        || parameters.pass_depth_mm <= 0.0
        || parameters.safe_height_mm <= 0.0
    {
        return Err(ProfileGenerationError::InvalidDepth);
    }
    if parameters.feed_mm_min <= 0.0 || parameters.plunge_mm_min <= 0.0 {
        return Err(ProfileGenerationError::InvalidFeed);
    }
    if parameters.cut_depth_mm > operation.tool.cutting_edge_height_mm {
        return Err(ProfileGenerationError::CutDepthExceedsTool);
    }
    if matches!(
        parameters.cut_side,
        ProfileCutSide::Outside | ProfileCutSide::Inside
    ) && operation.tool.diameter_mm / 2.0 + parameters.additional_offset_mm < 0.0
    {
        return Err(ProfileGenerationError::CollapsedBoundary);
    }
    Ok(())
}

fn effective_offset(parameters: ProfileParameters, cutter_radius: f64) -> f64 {
    match parameters.cut_side {
        ProfileCutSide::Outside => cutter_radius + parameters.additional_offset_mm,
        ProfileCutSide::OnLine => parameters.additional_offset_mm,
        ProfileCutSide::Inside => -(cutter_radius + parameters.additional_offset_mm),
    }
}

fn pass_depths(cut_depth_mm: f64, pass_depth_mm: f64) -> Vec<f64> {
    let pass_count = (cut_depth_mm / pass_depth_mm).ceil() as usize;
    (1..=pass_count)
        .map(|pass| (pass as f64 * pass_depth_mm).min(cut_depth_mm))
        .collect()
}

fn offset_boundary(
    boundary: &ProfileBoundary,
    offset_mm: f64,
) -> Result<Vec<ProfileBoundary>, ProfileGenerationError> {
    match boundary {
        ProfileBoundary::Circle {
            center_mm,
            radius_mm,
        } => {
            let radius_mm = radius_mm + offset_mm;
            if !radius_mm.is_finite() || radius_mm <= MINIMUM_DIMENSION_MM {
                return Err(ProfileGenerationError::CollapsedBoundary);
            }
            Ok(vec![ProfileBoundary::Circle {
                center_mm: *center_mm,
                radius_mm,
            }])
        }
        ProfileBoundary::ClosedContour { points_mm } => {
            if points_mm.len() < 3 {
                return Err(ProfileGenerationError::NoExteriorBoundary);
            }
            if offset_mm.abs() <= MINIMUM_DIMENSION_MM {
                return Ok(vec![boundary.clone()]);
            }
            let mut points = points_mm.clone();
            if signed_area(&points) < 0.0 {
                points.reverse();
            }
            let paths: clipper2::Paths<ProfileScaler> = vec![
                points
                    .iter()
                    .map(|point| (point.x, point.y))
                    .collect::<Vec<_>>(),
            ]
            .into();
            let output: Vec<Vec<(f64, f64)>> = paths
                .inflate(
                    offset_mm,
                    clipper2::JoinType::Round,
                    clipper2::EndType::Polygon,
                    0.0,
                )
                .into();
            let output = output
                .into_iter()
                .filter(|path| path.len() >= 3)
                .map(|path| ProfileBoundary::ClosedContour {
                    points_mm: path.into_iter().map(|(x, y)| Point::new(x, y)).collect(),
                })
                .collect::<Vec<_>>();
            if output.is_empty() {
                Err(ProfileGenerationError::CollapsedBoundary)
            } else {
                Ok(output)
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
struct ProfileScaler;

impl clipper2::PointScaler for ProfileScaler {
    const MULTIPLIER: f64 = 10000.0;
}

fn signed_area(points: &[Point]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        / 2.0
}

fn flatten_boundary(boundary: &ProfileBoundary, z_mm: f64) -> ProfileToolpathContour {
    let points = match boundary {
        ProfileBoundary::Circle {
            center_mm,
            radius_mm,
        } => (0..PROFILE_FLATTEN_SEGMENTS)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / PROFILE_FLATTEN_SEGMENTS as f64;
                ProfileToolpathPoint {
                    x_mm: center_mm.x + radius_mm * angle.cos(),
                    y_mm: center_mm.y + radius_mm * angle.sin(),
                    z_mm,
                }
            })
            .collect(),
        ProfileBoundary::ClosedContour { points_mm } => points_mm
            .iter()
            .map(|point| ProfileToolpathPoint {
                x_mm: point.x,
                y_mm: point.y,
                z_mm,
            })
            .collect(),
    };
    ProfileToolpathContour {
        points,
        closed: true,
    }
}

fn write_profile_gcode(
    operation: &GeneralProfileOperation,
    boundaries: &[ProfileBoundary],
    depths: &[f64],
    surface_z_mm: f64,
) -> String {
    let parameters = operation.parameters;
    let safe_z = surface_z_mm + parameters.safe_height_mm;
    let spindle = match operation.tool.spindle_direction {
        GeneralSpindleDirection::Forward => "M3",
        GeneralSpindleDirection::Reverse => "M4",
    };
    let mut lines = vec![
        "(R-Engrave General Profile)".to_owned(),
        format!("(Cut side: {})", parameters.cut_side.label()),
        "G90".to_owned(),
        "G21".to_owned(),
        format!("T{} M6", operation.tool.tool_number),
        spindle.to_owned(),
        format!("G0 Z{}", format_mm(safe_z)),
    ];

    for depth in depths {
        let z = surface_z_mm - depth;
        for boundary in boundaries {
            let start = boundary_start(boundary);
            lines.push(format!(
                "G0 X{} Y{}",
                format_mm(start.x),
                format_mm(start.y)
            ));
            lines.push(format!(
                "G1 Z{} F{}",
                format_mm(z),
                format_feed(parameters.plunge_mm_min)
            ));
            match boundary {
                ProfileBoundary::Circle { radius_mm, .. } => lines.push(format!(
                    "G2 X{} Y{} I{} J0.000 F{}",
                    format_mm(start.x),
                    format_mm(start.y),
                    format_mm(-radius_mm),
                    format_feed(parameters.feed_mm_min)
                )),
                ProfileBoundary::ClosedContour { points_mm } => {
                    for point in points_mm.iter().skip(1).chain(points_mm.first()) {
                        lines.push(format!(
                            "G1 X{} Y{} F{}",
                            format_mm(point.x),
                            format_mm(point.y),
                            format_feed(parameters.feed_mm_min)
                        ));
                    }
                }
            }
            lines.push(format!("G0 Z{}", format_mm(safe_z)));
        }
    }
    lines.extend(["M5".to_owned(), "M2".to_owned()]);
    lines.join("\n") + "\n"
}

fn boundary_start(boundary: &ProfileBoundary) -> Point {
    match boundary {
        ProfileBoundary::Circle {
            center_mm,
            radius_mm,
        } => Point::new(center_mm.x + radius_mm, center_mm.y),
        ProfileBoundary::ClosedContour { points_mm } => points_mm[0],
    }
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
    use crate::design::VectorDocument;
    use crate::general_toolbit::{GeneralToolbitKind, general_toolbit_presets};

    fn circle() -> (VectorDocument, DesignObjectId) {
        let mut document = VectorDocument::default();
        let id = document
            .add_circle(Point::new(2.0, -3.0), 10.0)
            .expect("valid circle");
        (document, id)
    }

    fn tool() -> GeneralToolbit {
        let mut tool = general_toolbit_presets()
            .into_iter()
            .find(|tool| tool.kind == GeneralToolbitKind::Endmill)
            .expect("endmill preset");
        tool.feed_mm_min = 600.0;
        tool.plunge_mm_min = 200.0;
        tool
    }

    fn operation(
        source_object_id: DesignObjectId,
        side: ProfileCutSide,
    ) -> GeneralProfileOperation {
        GeneralProfileOperation {
            source_object_id,
            tool: tool(),
            parameters: ProfileParameters {
                cut_side: side,
                cut_depth_mm: 2.5,
                pass_depth_mm: 1.0,
                safe_height_mm: 3.0,
                feed_mm_min: 600.0,
                plunge_mm_min: 200.0,
                additional_offset_mm: 0.5,
            },
        }
    }

    #[test]
    fn circle_profiles_apply_cutter_compensation_on_the_selected_side() {
        let (document, id) = circle();
        let source = document.object(id).expect("circle object");
        let outside = generate_profile(&operation(id, ProfileCutSide::Outside), source, 0.0)
            .expect("outside profile");
        let inside = generate_profile(&operation(id, ProfileCutSide::Inside), source, 0.0)
            .expect("inside profile");
        let on_line = generate_profile(&operation(id, ProfileCutSide::OnLine), source, 0.0)
            .expect("on-line profile");

        let radius = |profile: &GeneratedProfile| {
            let point = profile.contours[0].points[0];
            point.x_mm - 2.0
        };
        let cutter_radius = tool().diameter_mm / 2.0;
        assert!((radius(&outside) - (10.0 + cutter_radius + 0.5)).abs() < 1e-6);
        assert!((radius(&inside) - (10.0 - cutter_radius - 0.5)).abs() < 1e-6);
        assert!((radius(&on_line) - 10.5).abs() < 1e-6);
    }

    #[test]
    fn profile_exteriors_expose_closed_geometry_instead_of_display_strokes() {
        let circle = DesignGeometry::Circle(crate::design::DesignCircle {
            center_mm: Point::new(4.0, -6.0),
            radius_mm: 3.0,
        });
        assert_eq!(
            circle.profile_exteriors(),
            vec![ProfileBoundary::Circle {
                center_mm: Point::new(4.0, -6.0),
                radius_mm: 3.0,
            }]
        );
    }

    #[test]
    fn profile_depths_create_deterministic_passes_and_buffered_gcode() {
        let (document, id) = circle();
        let generated = generate_profile(
            &operation(id, ProfileCutSide::Outside),
            document.object(id).expect("circle object"),
            10.0,
        )
        .expect("profile");
        assert_eq!(generated.contours.len(), 3);
        assert_eq!(
            generated
                .contours
                .iter()
                .map(|contour| contour.points[0].z_mm)
                .collect::<Vec<_>>(),
            vec![-1.0, -2.0, -2.5]
        );
        assert!(generated.gcode.starts_with("(R-Engrave General Profile)\n"));
        assert!(generated.gcode.contains("F600.0"));
        assert!(generated.gcode.contains("G0 Z13.000"));
        assert!(generated.gcode.contains("G1 Z7.500"));
        assert_eq!(generated.gcode.matches("\nG2 ").count(), 3);
        assert!(generated.gcode.ends_with("M5\nM2\n"));
    }

    #[test]
    fn operation_feed_and_plunge_override_toolbit_defaults_in_gcode() {
        let (document, id) = circle();
        let mut operation = operation(id, ProfileCutSide::Outside);
        operation.tool.feed_mm_min = 50.0;
        operation.tool.plunge_mm_min = 25.0;
        operation.parameters.feed_mm_min = 321.0;
        operation.parameters.plunge_mm_min = 123.0;
        let generated =
            generate_profile(&operation, document.object(id).expect("circle object"), 0.0)
                .expect("profile");

        assert!(generated.gcode.contains("G1 Z-1.000 F123.0"));
        assert!(generated.gcode.contains("G2 ") && generated.gcode.contains("F321.0"));
        assert!(!generated.gcode.contains("F50.0"));
        assert!(!generated.gcode.contains("F25.0"));
    }

    #[test]
    fn invalid_or_collapsed_profiles_are_rejected() {
        let (document, id) = circle();
        let source = document.object(id).expect("circle object");
        let mut invalid_tool = operation(id, ProfileCutSide::Outside);
        invalid_tool.tool.kind = GeneralToolbitKind::Probe;
        assert_eq!(
            generate_profile(&invalid_tool, source, 0.0),
            Err(ProfileGenerationError::UnsupportedTool)
        );

        let mut collapsed = operation(id, ProfileCutSide::Inside);
        collapsed.tool.diameter_mm = 25.0;
        assert_eq!(
            generate_profile(&collapsed, source, 0.0),
            Err(ProfileGenerationError::CollapsedBoundary)
        );

        let mut no_feed = operation(id, ProfileCutSide::Outside);
        no_feed.parameters.feed_mm_min = 0.0;
        assert_eq!(
            generate_profile(&no_feed, source, 0.0),
            Err(ProfileGenerationError::InvalidFeed)
        );

        let mut too_deep = operation(id, ProfileCutSide::Outside);
        too_deep.parameters.cut_depth_mm = too_deep.tool.cutting_edge_height_mm + 0.1;
        assert_eq!(
            generate_profile(&too_deep, source, 0.0),
            Err(ProfileGenerationError::CutDepthExceedsTool)
        );
    }

    #[test]
    fn operation_must_match_the_supplied_source_identity() {
        let (mut document, id) = circle();
        let other = document
            .add_circle(Point::new(20.0, 20.0), 2.0)
            .expect("other circle");
        assert_eq!(
            generate_profile(
                &operation(id, ProfileCutSide::Outside),
                document.object(other).expect("other object"),
                0.0,
            ),
            Err(ProfileGenerationError::SourceMismatch)
        );
    }

    #[test]
    fn closed_contours_use_the_same_generic_profile_boundary() {
        let boundary = ProfileBoundary::ClosedContour {
            points_mm: vec![
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 10.0),
            ],
        };
        let expanded = offset_boundary(&boundary, 1.0).expect("expanded contour");
        let ProfileBoundary::ClosedContour { points_mm } = &expanded[0] else {
            panic!("expected closed contour");
        };
        assert!(points_mm.iter().any(|point| point.x < -0.9));
        assert!(points_mm.iter().any(|point| point.y < -0.9));
        assert!(points_mm.iter().any(|point| point.x > 10.9));
        assert!(points_mm.iter().any(|point| point.y > 10.9));
    }
}
