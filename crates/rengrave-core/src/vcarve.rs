use crate::geometry::Point;
use crate::layout::EngraveSegment;
use crate::settings::{LegacySettings, get_legacy_bool};

const ZERO: f64 = 0.00001;

#[derive(Debug, Clone, PartialEq)]
pub struct VCarveOptions {
    pub step_len: f64,
    pub bit_shape: BitShape,
    pub bit_angle_degrees: f64,
    pub bit_diameter: f64,
    pub depth_limit: f64,
    pub inlay: bool,
    pub allowance: f64,
    pub inlay_depth: f64,
    pub rough_stock: f64,
    pub max_cut: f64,
    pub drive_corner_angle: f64,
    pub step_corner_angle: f64,
    pub check_mode: VCarveCheckMode,
    pub v_flop: bool,
}

impl VCarveOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        let mut step_len = get_f64(settings, "v_step_len", 0.01);
        if settings.get_last("units") == Some("mm") {
            step_len = step_len.max(0.01);
        } else {
            step_len = step_len.max(0.0005);
        }
        let bit_shape = BitShape::parse(settings.get_last("bit_shape").unwrap_or("VBIT"));
        let bit_angle_degrees = get_f64(settings, "v_bit_angle", 60.0);
        let bit_diameter = get_f64(settings, "v_bit_dia", 0.5);
        let depth_limit = get_f64(settings, "v_depth_lim", 0.0);

        Self {
            step_len,
            bit_shape,
            bit_angle_degrees,
            bit_diameter,
            depth_limit,
            inlay: get_legacy_bool(settings, "inlay", false),
            allowance: get_f64(settings, "allowance", 0.0),
            inlay_depth: f_engrave_max_cut_depth(
                bit_shape,
                bit_angle_degrees,
                bit_diameter,
                depth_limit,
            ),
            rough_stock: get_f64(settings, "v_rough_stk", 0.0),
            max_cut: get_f64(settings, "v_max_cut", -1.0),
            drive_corner_angle: get_f64(settings, "v_drv_crner", 135.0),
            step_corner_angle: get_f64(settings, "v_stp_crner", 200.0),
            check_mode: VCarveCheckMode::parse(settings.get_last("v_check_all").unwrap_or("all")),
            v_flop: f_engrave_v_flop(settings),
        }
    }

    pub fn effective_bit_diameter(&self) -> f64 {
        if self.inlay && self.bit_shape == BitShape::VBit {
            let diameter = -2.0 * self.allowance * self.half_angle().tan();
            return diameter.max(0.001);
        }

        if self.depth_limit < 0.0 {
            match self.bit_shape {
                BitShape::VBit => -2.0 * self.depth_limit * self.half_angle().tan(),
                BitShape::Ball => {
                    let radius = self.bit_diameter / 2.0;
                    if self.depth_limit > -radius {
                        2.0 * (radius * radius - (radius + self.depth_limit).powi(2)).sqrt()
                    } else {
                        self.bit_diameter
                    }
                }
                BitShape::Flat => self.bit_diameter,
            }
        } else {
            self.bit_diameter
        }
    }

    pub fn max_radius(&self) -> f64 {
        (self.effective_bit_diameter() / 2.0).max(0.0)
    }

    pub fn depth_for_radius(&self, radius: f64) -> f64 {
        match self.bit_shape {
            BitShape::VBit => {
                let mut depth = -radius / self.half_angle().tan();
                if self.inlay {
                    depth += self.inlay_depth;
                }
                depth
            }
            BitShape::Ball => {
                let bit_radius = (self.bit_diameter / 2.0).max(ZERO);
                let radius = radius.min(bit_radius);
                let theta = (radius / bit_radius).clamp(-1.0, 1.0).acos();
                -bit_radius * (1.0 - theta.sin())
            }
            BitShape::Flat => -self.bit_diameter / 2.0,
        }
    }

    pub fn pass_depth_for_radius(&self, radius: f64, rough_cap: Option<f64>) -> f64 {
        let final_depth = self.depth_for_radius(radius);
        let Some(rough_cap) = rough_cap else {
            return final_depth;
        };

        let rough_depth = (final_depth + self.rough_stock).min(0.0);
        if rough_cap - rough_depth > 0.001 {
            rough_cap
        } else {
            rough_depth
        }
    }

    pub fn rough_pass_caps(&self, max_final_depth: f64) -> Vec<Option<f64>> {
        if self.rough_stock <= 0.0 || self.max_cut >= 0.0 || max_final_depth >= 0.0 {
            return vec![None];
        }

        let rough_target = (max_final_depth + self.rough_stock).min(0.0);
        let mut caps = Vec::new();
        let mut cap = self.max_cut;
        while cap > rough_target {
            caps.push(Some(cap));
            cap += self.max_cut;
        }
        caps.push(Some(cap));
        caps.push(None);
        caps
    }

    fn half_angle(&self) -> f64 {
        (self.bit_angle_degrees / 2.0).to_radians()
    }
}

fn f_engrave_max_cut_depth(
    bit_shape: BitShape,
    bit_angle_degrees: f64,
    bit_diameter: f64,
    depth_limit: f64,
) -> f64 {
    let bit_depth = match bit_shape {
        BitShape::VBit => {
            let tangent = (bit_angle_degrees / 2.0).to_radians().tan();
            if tangent == 0.0 {
                f64::NAN
            } else {
                -bit_diameter / 2.0 / tangent
            }
        }
        BitShape::Ball | BitShape::Flat => -bit_diameter / 2.0,
    };
    let depth = if bit_shape != BitShape::Flat {
        if depth_limit < 0.0 {
            bit_depth.max(depth_limit)
        } else {
            bit_depth
        }
    } else if depth_limit < 0.0 {
        depth_limit
    } else {
        bit_depth
    };
    if depth.is_finite() {
        format!("{depth:.3}").parse().unwrap_or(0.0)
    } else {
        0.0
    }
}

fn f_engrave_v_flop(settings: &LegacySettings) -> bool {
    let mut v_flop = get_legacy_bool(settings, "v_flop", false);
    if settings.get_last("input_type") == Some("text") {
        for key in ["plotbox", "mirror", "flip"] {
            if get_legacy_bool(settings, key, false) {
                v_flop = !v_flop;
            }
        }
    }
    v_flop
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitShape {
    VBit,
    Ball,
    Flat,
}

impl BitShape {
    fn parse(value: &str) -> Self {
        match value {
            "BALL" => Self::Ball,
            "FLAT" => Self::Flat,
            _ => Self::VBit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VCarveCheckMode {
    All,
    Character,
}

impl VCarveCheckMode {
    fn parse(value: &str) -> Self {
        if value == "chr" {
            Self::Character
        } else {
            Self::All
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VCarvePoint {
    pub position: Point,
    pub radius: f64,
    pub loop_id: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VCarveCanceled;

pub fn generate_vcarve_points(
    segments: &[EngraveSegment],
    options: &VCarveOptions,
    accuracy: f64,
) -> Vec<VCarvePoint> {
    generate_vcarve_points_with_cancel(segments, options, accuracy, &|| false)
        .expect("non-canceling V-carve generation should not cancel")
}

pub fn generate_vcarve_points_with_cancel(
    segments: &[EngraveSegment],
    options: &VCarveOptions,
    accuracy: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<VCarvePoint>, VCarveCanceled> {
    let Some(grid) = PartitionGrid::new(segments, options.max_radius(), options.step_len) else {
        return Ok(Vec::new());
    };

    let mut output = Vec::new();
    let drive_corner_angle = if options.inlay {
        360.0 - options.step_corner_angle
    } else {
        options.drive_corner_angle
    };
    let max_radius = options.max_radius();
    let mut delta_angle = (options.step_len / max_radius.max(ZERO)).to_degrees();
    if delta_angle < 2.0 {
        delta_angle = 2.0;
    }
    let not_ball_carve = options.bit_shape != BitShape::Ball;
    let bit_angle_enabled = options.bit_angle_degrees != 0.0;

    let mut xa = 9999.0;
    let mut ya = 9999.0;
    let mut xb = 9999.0;
    let mut yb = 9999.0;
    let mut x0 = 9999.0;
    let mut y0 = 9999.0;
    let mut previous_seg_sin = 2.0;
    let mut previous_seg_cos = 2.0;
    let mut previous_char_num = None;
    let mut theta = 9999.0;
    let mut loop_id = 0usize;

    for line_index in 0..segments.len() {
        check_canceled(cancel)?;
        let segment = traversal_segment(segments, line_index, options.v_flop);
        let start = segment.start;
        let end = segment.end;
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length = (dx * dx + dy * dy).sqrt();
        if length < ZERO {
            continue;
        }

        let char_num = segment.loop_id;
        let mut new_loop = false;
        let seg_sin = dy / length;
        let seg_cos = -dx / length;
        let phi_degrees = legacy_angle(seg_sin, seg_cos);

        if (start.x - x0).abs() > ZERO
            || (start.y - y0).abs() > ZERO
            || previous_char_num != Some(char_num)
        {
            new_loop = true;
            loop_id += 1;
            xa = start.x;
            ya = start.y;
            xb = end.x;
            yb = end.y;
            theta = 9999.0;
            previous_seg_sin = 2.0;
            previous_seg_cos = 2.0;
        }

        let delta = corner_delta(dx, dy, previous_seg_sin, previous_seg_cos);
        if delta < drive_corner_angle && bit_angle_enabled && not_ball_carve {
            output.push(VCarvePoint {
                position: start,
                radius: 0.0,
                loop_id,
            });
        }

        if delta > options.step_corner_angle {
            let phi_steps = (((delta - 180.0) / delta_angle).floor() as usize).max(2);
            let step_phi = (delta - 180.0) / phi_steps as f64;
            for step in 1..phi_steps {
                check_canceled(cancel)?;
                let sub_phi = (-(step as f64) * step_phi + theta).to_radians();
                let sub_seg_cos = sub_phi.cos();
                let sub_seg_sin = sub_phi.sin();
                let radius = grid.find_max_circle(
                    start,
                    max_radius,
                    char_num,
                    sub_seg_sin,
                    sub_seg_cos,
                    true,
                    options.check_mode,
                );
                record_vcarve_point(&mut output, start, sub_phi, radius, loop_id);
            }
        }

        theta = phi_degrees;
        x0 = end.x;
        y0 = end.y;
        previous_seg_sin = seg_sin;
        previous_seg_cos = seg_cos;
        previous_char_num = Some(char_num);

        let steps = ((length / options.step_len).floor() as usize).max(2);
        let step_dx = dx / steps as f64;
        let step_dy = dy / steps as f64;
        let phi_radians = legacy_angle(seg_sin, seg_cos).to_radians();
        let mut saved_first_cut = None;
        let mut step = if new_loop && bit_angle_enabled && not_ball_carve {
            -1isize
        } else {
            0isize
        };
        while step < steps as isize - 1 {
            check_canceled(cancel)?;
            step += 1;
            let outline = Point::new(
                start.x + step_dx * step as f64,
                start.y + step_dy * step as f64,
            );
            let mut radius = grid.find_max_circle(
                outline,
                max_radius,
                char_num,
                seg_sin,
                seg_cos,
                false,
                options.check_mode,
            );
            if step == 0 && not_ball_carve {
                radius = 0.0;
            }
            record_vcarve_point(&mut output, outline, phi_radians, radius, loop_id);

            if new_loop && step == 1 {
                saved_first_cut = Some((outline, phi_radians, radius));
            }
        }

        if (end.x - xa).abs() < ZERO && (end.y - ya).abs() < ZERO {
            let close_dx = xb - xa;
            let close_dy = yb - ya;
            let close_delta = corner_delta(close_dx, close_dy, previous_seg_sin, previous_seg_cos);
            let first_point = saved_first_cut.unwrap_or((start, phi_radians, 0.0));

            if close_delta < drive_corner_angle {
                output.push(VCarvePoint {
                    position: Point::new(xa, ya),
                    radius: 0.0,
                    loop_id,
                });
            } else if close_delta > options.step_corner_angle {
                let phi_steps = (((close_delta - 180.0) / delta_angle).floor() as usize).max(2);
                let step_phi = (close_delta - 180.0) / phi_steps as f64;
                for step in 1..phi_steps {
                    check_canceled(cancel)?;
                    let sub_phi = (-(step as f64) * step_phi + theta).to_radians();
                    let sub_seg_cos = sub_phi.cos();
                    let sub_seg_sin = sub_phi.sin();
                    let radius = grid.find_max_circle(
                        Point::new(xa, ya),
                        max_radius,
                        char_num,
                        sub_seg_sin,
                        sub_seg_cos,
                        true,
                        options.check_mode,
                    );
                    record_vcarve_point(&mut output, Point::new(xa, ya), sub_phi, radius, loop_id);
                }
                record_vcarve_point(
                    &mut output,
                    first_point.0,
                    first_point.1,
                    first_point.2,
                    loop_id,
                );
            } else {
                record_vcarve_point(
                    &mut output,
                    first_point.0,
                    first_point.1,
                    first_point.2,
                    loop_id,
                );
            }
        }
    }

    Ok(reorder_loops(output, accuracy))
}

pub fn sort_image_segments_for_vcarve(
    segments: &[EngraveSegment],
    accuracy: f64,
) -> Vec<EngraveSegment> {
    if segments.is_empty() {
        return Vec::new();
    }

    let mut ecoords = Vec::new();
    let mut loop_begins = Vec::new();
    let mut loop_ends = Vec::new();
    let mut old_end = segments[0].end;
    let mut current_index = 0usize;

    for (index, segment) in segments.iter().enumerate() {
        if index == 0 {
            ecoords.push(segment.start);
            loop_begins.push(current_index);
            current_index += 1;
            ecoords.push(segment.end);
            old_end = segment.end;
            continue;
        }

        if point_distance(old_end, segment.start) > ZERO {
            loop_ends.push(current_index);
            current_index += 1;
            ecoords.push(segment.start);
            loop_begins.push(current_index);
        }
        current_index += 1;
        ecoords.push(segment.end);
        old_end = segment.end;
    }
    loop_ends.push(current_index);

    let mut open_begins = Vec::new();
    let mut open_ends = Vec::new();
    let mut index = 0usize;
    while index < loop_begins.len() {
        let start = ecoords[loop_begins[index]];
        let end = ecoords[loop_ends[index]];
        if point_distance(start, end) <= ZERO {
            ecoords[loop_ends[index]] = start;
            index += 1;
        } else {
            open_begins.push(loop_begins.remove(index));
            open_ends.push(loop_ends.remove(index));
        }
    }

    let mut new_begins = Vec::new();
    let mut new_ends = Vec::new();
    let mut new_loop_ids = Vec::new();
    let mut loop_id = 0usize;
    while !open_begins.is_empty() {
        let start = open_begins.remove(0);
        let mut end = open_ends.remove(0);
        loop_id += 1;
        new_loop_ids.push(loop_id);
        new_begins.push(start);
        new_ends.push(end);
        let original_start = ecoords[start];

        let mut open = true;
        while open && !open_begins.is_empty() {
            let current_end = ecoords[end];
            let mut best_begin_distance = point_distance(current_end, original_start);
            let mut best_end_distance = best_begin_distance;
            let mut best_begin = None;
            let mut best_end = None;

            for candidate in 0..open_begins.len() {
                let candidate_start = ecoords[open_begins[candidate]];
                let candidate_end = ecoords[open_ends[candidate]];
                let begin_distance = point_distance(current_end, candidate_start);
                let end_distance = point_distance(current_end, candidate_end);

                if begin_distance < best_begin_distance {
                    best_begin_distance = begin_distance;
                    best_begin = Some(candidate);
                }
                if end_distance < best_end_distance {
                    best_end_distance = end_distance;
                    best_end = Some(candidate);
                }
            }

            if best_begin.is_none() && best_end.is_none() {
                ecoords.push(ecoords[end]);
                ecoords.push(original_start);
                new_loop_ids.push(loop_id);
                new_begins.push(ecoords.len() - 2);
                new_ends.push(ecoords.len() - 1);
                open = false;
            } else if best_end_distance < best_begin_distance {
                let candidate = best_end.unwrap();
                let next_end = open_begins.remove(candidate);
                let next_begin = open_ends.remove(candidate);

                ecoords.push(ecoords[end]);
                ecoords.push(ecoords[next_begin]);
                new_loop_ids.push(loop_id);
                new_begins.push(ecoords.len() - 2);
                new_ends.push(ecoords.len() - 1);
                new_loop_ids.push(loop_id);
                new_begins.push(next_begin);
                new_ends.push(next_end);
                end = next_end;
            } else {
                let candidate = best_begin.unwrap();
                let next_begin = open_begins.remove(candidate);
                let next_end = open_ends.remove(candidate);

                ecoords.push(ecoords[end]);
                ecoords.push(ecoords[next_begin]);
                new_loop_ids.push(loop_id);
                new_begins.push(ecoords.len() - 2);
                new_ends.push(ecoords.len() - 1);
                new_loop_ids.push(loop_id);
                new_begins.push(next_begin);
                new_ends.push(next_end);
                end = next_end;
            }
        }

        if open && open_begins.is_empty() {
            ecoords.push(ecoords[end]);
            ecoords.push(original_start);
            new_loop_ids.push(loop_id);
            new_begins.push(ecoords.len() - 2);
            new_ends.push(ecoords.len() - 1);
        }
    }

    for range_index in 0..loop_begins.len() {
        let start = loop_begins[range_index];
        let end = loop_ends[range_index];
        let mut previous = ecoords[start];
        for point_index in start + 1..=end {
            let point = ecoords[point_index];
            if point_distance(previous, point) >= accuracy {
                previous = point;
            } else if point_index != end {
                ecoords[point_index] = previous;
            } else {
                ecoords[end] = ecoords[start];
            }
        }
    }

    let mut last_new_loop = None;
    for range_index in 0..new_begins.len() {
        let start = new_begins[range_index];
        let end = new_ends[range_index];
        let current_loop = new_loop_ids[range_index];
        if last_new_loop != Some(current_loop) {
            loop_begins.push(ecoords.len());
            if last_new_loop.is_some() {
                loop_ends.push(ecoords.len() - 1);
            }
            last_new_loop = Some(current_loop);
        }

        append_points_in_range(&mut ecoords, start, end);
    }
    if loop_begins.len() > loop_ends.len() {
        loop_ends.push(ecoords.len() - 1);
    }

    let mut loop_flips = loop_begins
        .iter()
        .zip(&loop_ends)
        .map(|(&start, &end)| signed_area(&ecoords, start, end) <= 0.0)
        .collect::<Vec<_>>();

    for outer in 0..loop_begins.len() {
        let poly_start = loop_begins[outer];
        let poly_end = loop_ends[outer];
        if poly_start >= poly_end {
            continue;
        }
        let polygon = &ecoords[poly_start..poly_end];
        for inner in 0..loop_begins.len() {
            if inner == outer {
                continue;
            }
            let point = ecoords[loop_begins[inner]];
            if point_inside_polygon(point, polygon) > 0 {
                loop_flips[inner] = !loop_flips[inner];
            }
        }
    }

    let mut loop_numbers = (0..loop_begins.len()).collect::<Vec<_>>();
    let mut order = Vec::new();
    if !loop_flips.is_empty() {
        if loop_flips[0] {
            order.push((loop_ends[0], loop_begins[0], loop_numbers[0]));
        } else {
            order.push((loop_begins[0], loop_ends[0], loop_numbers[0]));
        }
    }

    let mut next_index = 0usize;
    let total = loop_begins.len();
    for _ in 0..total.saturating_sub(1) {
        loop_begins.remove(next_index);
        let current_end = loop_ends.remove(next_index);
        loop_flips.remove(next_index);
        loop_numbers.remove(next_index);

        if loop_begins.is_empty() {
            break;
        }

        let current = ecoords[current_end];
        next_index = 0;
        let mut best_distance = point_distance_squared(current, ecoords[loop_begins[0]]);
        for candidate in 1..loop_begins.len() {
            let distance = point_distance_squared(current, ecoords[loop_begins[candidate]]);
            if distance < best_distance {
                best_distance = distance;
                next_index = candidate;
            }
        }

        if loop_flips[next_index] {
            order.push((
                loop_ends[next_index],
                loop_begins[next_index],
                loop_numbers[next_index],
            ));
        } else {
            order.push((
                loop_begins[next_index],
                loop_ends[next_index],
                loop_numbers[next_index],
            ));
        }
    }

    let mut sorted = Vec::new();
    for (start, end, loop_number) in order {
        append_segments_in_range(&ecoords, start, end, loop_number, accuracy, &mut sorted);
    }

    remove_tiny_segment_loops(sorted)
}

fn append_points_in_range(points: &mut Vec<Point>, start: usize, end: usize) {
    if start <= end {
        for index in start..=end {
            points.push(points[index]);
        }
    } else {
        for index in (end..=start).rev() {
            points.push(points[index]);
        }
    }
}

fn signed_area(points: &[Point], start: usize, end: usize) -> f64 {
    let mut area = 0.0;
    let mut previous = points[start];
    for point in points.iter().take(end + 1).skip(start + 1) {
        area += (point.x - previous.x) * (point.y + previous.y);
        previous = *point;
    }
    area
}

fn point_inside_polygon(point: Point, polygon: &[Point]) -> i32 {
    if polygon.is_empty() {
        return -1;
    }

    let mut inside = -1;
    let mut previous = polygon[0];
    for index in 0..=polygon.len() {
        let current = polygon[index % polygon.len()];
        if point.y > previous.y.min(current.y)
            && point.y <= previous.y.max(current.y)
            && point.x <= previous.x.max(current.x)
        {
            let mut x_intersection = previous.x;
            if (previous.y - current.y).abs() > ZERO {
                x_intersection = (point.y - previous.y) * (current.x - previous.x)
                    / (current.y - previous.y)
                    + previous.x;
            }
            if (previous.x - current.x).abs() < ZERO || point.x <= x_intersection {
                inside *= -1;
            }
        }
        previous = current;
    }
    inside
}

fn append_segments_in_range(
    points: &[Point],
    start: usize,
    end: usize,
    loop_number: usize,
    accuracy: f64,
    output: &mut Vec<EngraveSegment>,
) {
    let step = if start > end { -1isize } else { 1isize };
    let first = points[start];
    let mut collapsed_start = None;
    let mut index = start as isize + step;

    while (step > 0 && index <= end as isize) || (step < 0 && index >= end as isize) {
        let previous_index = (index - step) as usize;
        let segment_start = collapsed_start.unwrap_or(points[previous_index]);
        let segment_end = points[index as usize];

        if point_distance(segment_start, segment_end) >= ZERO {
            output.push(EngraveSegment {
                start: segment_start,
                end: segment_end,
                loop_id: loop_number,
            });
            collapsed_start = None;
        } else {
            collapsed_start = Some(segment_start);
        }

        index += step;
    }

    if let Some(segment_start) = collapsed_start {
        let last_distance = point_distance(segment_start, first);
        if last_distance <= accuracy
            && output
                .last()
                .map(|segment| segment.loop_id == loop_number)
                .unwrap_or(false)
        {
            if let Some(last) = output.last_mut() {
                last.end = first;
            }
        } else {
            output.push(EngraveSegment {
                start: segment_start,
                end: first,
                loop_id: loop_number,
            });
        }
    }
}

fn remove_tiny_segment_loops(segments: Vec<EngraveSegment>) -> Vec<EngraveSegment> {
    let mut output = Vec::new();
    let mut start = 0usize;
    while start < segments.len() {
        let loop_id = segments[start].loop_id;
        let mut end = start + 1;
        while end < segments.len() && segments[end].loop_id == loop_id {
            end += 1;
        }
        if end - start >= 3 {
            output.extend_from_slice(&segments[start..end]);
        }
        start = end;
    }
    output
}

#[derive(Debug, Clone, Copy)]
struct PartitionLine {
    start: Point,
    end: Point,
    char_num: usize,
    center: Point,
    reach: f64,
}

#[derive(Debug, Clone)]
struct PartitionGrid {
    min: Point,
    x_len: f64,
    y_len: f64,
    x_count: usize,
    y_count: usize,
    cells: Vec<Vec<PartitionLine>>,
}

impl PartitionGrid {
    fn new(segments: &[EngraveSegment], max_radius: f64, step_len: f64) -> Option<Self> {
        if segments.is_empty() {
            return None;
        }

        let mut min = Point::new(f64::INFINITY, f64::INFINITY);
        let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for segment in segments {
            for point in [segment.start, segment.end] {
                min.x = min.x.min(point.x);
                min.y = min.y.min(point.y);
                max.x = max.x.max(point.x);
                max.y = max.y.max(point.y);
            }
        }
        if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
            return None;
        }

        let width = max.x - min.x;
        let height = max.y - min.y;
        let partition_size = (2.0 * max_radius + step_len) * 1.1;
        let x_count_minus_1 = ((width / partition_size) as usize).max(1);
        let y_count_minus_1 = ((height / partition_size) as usize).max(1);
        let mut x_len = width / x_count_minus_1 as f64;
        let mut y_len = height / y_count_minus_1 as f64;
        if x_len < ZERO {
            x_len = 1.0;
        }
        if y_len < ZERO {
            y_len = 1.0;
        }
        let x_count = x_count_minus_1 + 1;
        let y_count = y_count_minus_1 + 1;
        let mut grid = Self {
            min,
            x_len,
            y_len,
            x_count,
            y_count,
            cells: vec![Vec::new(); x_count * y_count],
        };

        for segment in segments {
            let dx = segment.end.x - segment.start.x;
            let dy = segment.end.y - segment.start.y;
            let length = (dx * dx + dy * dy).sqrt();
            let line = PartitionLine {
                start: segment.start,
                end: segment.end,
                char_num: segment.loop_id,
                center: Point::new(
                    (segment.start.x + segment.end.x) / 2.0,
                    (segment.start.y + segment.end.y) / 2.0,
                ),
                reach: length / 2.0 + max_radius,
            };

            for index in grid.active_indices(segment.start, segment.end) {
                grid.cells[index].push(line);
            }
        }

        Some(grid)
    }

    fn find_max_circle(
        &self,
        point: Point,
        mut radius: f64,
        char_num: usize,
        seg_sin: f64,
        seg_cos: f64,
        corner: bool,
        check_mode: VCarveCheckMode,
    ) -> f64 {
        let x_index = self.x_index(point.x);
        let y_index = self.y_index(point.y);
        let candidate_reach = radius.abs();
        let nearby: Vec<_> = self.cells[self.cell_index(x_index, y_index)]
            .iter()
            .copied()
            .filter(|line| {
                point_distance(line.center, point) < (candidate_reach + line.reach).abs()
            })
            .collect();

        for line in nearby {
            let x_max = line.start.x.max(line.end.x) + radius * 2.0;
            let x_min = line.start.x.min(line.end.x) - radius * 2.0;
            let y_max = line.start.y.max(line.end.y) + radius * 2.0;
            let y_min = line.start.y.min(line.end.y) - radius * 2.0;
            if point.x < x_min || point.x > x_max || point.y < y_min || point.y > y_max {
                continue;
            }
            if check_mode == VCarveCheckMode::Character && char_num != line.char_num {
                continue;
            }
            if corner
                && ((point.x - line.start.x).abs() <= ZERO
                    && (point.y - line.start.y).abs() <= ZERO
                    || (point.x - line.end.x).abs() <= ZERO && (point.y - line.end.y).abs() <= ZERO)
            {
                continue;
            }

            let xc1 = (line.start.x - point.x) * seg_cos - (line.start.y - point.y) * seg_sin;
            let yc1 = (line.start.x - point.x) * seg_sin + (line.start.y - point.y) * seg_cos;
            let xc2 = (line.end.x - point.x) * seg_cos - (line.end.y - point.y) * seg_sin;
            let yc2 = (line.end.x - point.x) * seg_sin + (line.end.y - point.y) * seg_cos;

            if (xc2 - xc1).abs() < ZERO && (yc2 - yc1).abs() > ZERO {
                let candidate = xc1.abs();
                if yc1.max(yc2) >= candidate && yc1.min(yc2) <= candidate {
                    radius = radius.min(candidate);
                }
            } else if (yc2 - yc1).abs() < ZERO
                && (xc2 - xc1).abs() > ZERO
                && xc1.max(xc2) >= 0.0
                && xc1.min(xc2) <= 0.0
                && yc1 > ZERO
            {
                radius = radius.min(yc1 / 2.0);
            }

            if (yc2 - yc1).abs() > ZERO && (xc2 - xc1).abs() > ZERO {
                let m = (yc2 - yc1) / (xc2 - xc1);
                if m.abs() > ZERO {
                    let b = yc1 - m * xc1;
                    let sq = m + 1.0 / m;
                    let a = 1.0 + m * m - 2.0 * m * sq;
                    let bb = -2.0 * b * sq;
                    let c = -b * b;
                    let discriminant = bb * bb - 4.0 * a * c;
                    if discriminant >= 0.0 && a.abs() > ZERO {
                        let root = discriminant.sqrt();
                        for xq in [(-bb + root) / (2.0 * a), (-bb - root) / (2.0 * a)] {
                            if xq >= xc1.min(xc2) && xq <= xc1.max(xc2) {
                                let candidate = xq * sq + b;
                                if candidate >= 0.0 {
                                    radius = radius.min(candidate);
                                }
                            }
                        }
                    }
                }
            }

            if yc1 > ZERO {
                radius = radius.min((xc1 * xc1 + yc1 * yc1) / (2.0 * yc1));
            }
            if yc2 > ZERO {
                radius = radius.min((xc2 * xc2 + yc2 * yc2) / (2.0 * yc2));
            }

            if yc1.abs() < ZERO && xc1.abs() < ZERO && yc2 > ZERO {
                radius = 0.0;
            }
            if yc2.abs() < ZERO && xc2.abs() < ZERO && yc1 > ZERO {
                radius = 0.0;
            }
        }

        radius.max(0.0)
    }

    fn active_indices(&self, start: Point, end: Point) -> Vec<usize> {
        let x1_g = start.x - self.min.x;
        let y1_g = start.y - self.min.y;
        let x2_g = end.x - self.min.x;
        let y2_g = end.y - self.min.y;

        let x1_i = self.local_x_index(x1_g);
        let x2_i = self.local_x_index(x2_g);
        let y1_i = self.local_y_index(y1_g);
        let y2_i = self.local_y_index(y2_g);

        let x_min = x1_i.min(x2_i);
        let x_max = x1_i.max(x2_i);
        let y_min = y1_i.min(y2_i);
        let y_max = y1_i.max(y2_i);

        let mut check_points = Vec::new();
        if x_max > x_min && (x2_g - x1_g).abs() > ZERO {
            if y_max > y_min && (y2_g - y1_g).abs() > ZERO {
                check_points.push((x1_i, y1_i));
                check_points.push((x2_i, y2_i));
                let slope = (y2_g - y1_g) / (x2_g - x1_g);
                let intercept = y1_g - slope * x1_g;
                for x_index in x_min + 1..x_max {
                    let x_value = x_index as f64 * self.x_len;
                    let y_value = slope * x_value + intercept;
                    check_points.push((x_index, self.local_y_index(y_value)));
                }
                for y_index in y_min + 1..y_max {
                    let y_value = y_index as f64 * self.y_len;
                    let x_value = (y_value - intercept) / slope;
                    check_points.push((self.local_x_index(x_value), y_index));
                }
            } else {
                for x_index in x_min..=x_max {
                    check_points.push((x_index, y_min));
                }
            }
        } else {
            for y_index in y_min..=y_max {
                check_points.push((x_min, y_index));
            }
        }

        let mut indices = Vec::new();
        for (x_index, y_index) in check_points {
            let x_start = x_index.saturating_sub(1);
            let x_end = (x_index + 2).min(self.x_count);
            let y_start = y_index.saturating_sub(1);
            let y_end = (y_index + 2).min(self.y_count);
            for x in x_start..x_end {
                for y in y_start..y_end {
                    indices.push(self.cell_index(x, y));
                }
            }
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    fn x_index(&self, x: f64) -> usize {
        self.local_x_index(x - self.min.x)
    }

    fn y_index(&self, y: f64) -> usize {
        self.local_y_index(y - self.min.y)
    }

    fn local_x_index(&self, x: f64) -> usize {
        trunc_index(x / self.x_len, self.x_count)
    }

    fn local_y_index(&self, y: f64) -> usize {
        trunc_index(y / self.y_len, self.y_count)
    }

    fn cell_index(&self, x: usize, y: usize) -> usize {
        x + y * self.x_count
    }
}

fn traversal_segment(
    segments: &[EngraveSegment],
    line_index: usize,
    v_flop: bool,
) -> EngraveSegment {
    if !v_flop {
        segments[line_index]
    } else {
        let segment = segments[segments.len() - 1 - line_index];
        EngraveSegment {
            start: segment.end,
            end: segment.start,
            loop_id: segment.loop_id,
        }
    }
}

fn record_vcarve_point(
    output: &mut Vec<VCarvePoint>,
    outline: Point,
    phi: f64,
    radius: f64,
    loop_id: usize,
) {
    let (offset_x, offset_y) = transform(0.0, radius, -phi);
    output.push(VCarvePoint {
        position: Point::new(outline.x + offset_x, outline.y + offset_y),
        radius,
        loop_id,
    });
}

fn transform(x: f64, y: f64, angle: f64) -> (f64, f64) {
    (
        x * angle.cos() - y * angle.sin(),
        x * angle.sin() + y * angle.cos(),
    )
}

fn corner_delta(dx: f64, dy: f64, previous_seg_sin: f64, previous_seg_cos: f64) -> f64 {
    if previous_seg_cos > 1.0 {
        180.0
    } else {
        let x_tmp = dx * previous_seg_cos - dy * previous_seg_sin;
        let y_tmp = dx * previous_seg_sin + dy * previous_seg_cos;
        let length = (x_tmp * x_tmp + y_tmp * y_tmp).sqrt();
        if length < ZERO {
            180.0
        } else {
            legacy_angle(y_tmp / length, x_tmp / length)
        }
    }
}

fn legacy_angle(sin: f64, cos: f64) -> f64 {
    let angle = cos.clamp(-1.0, 1.0).acos().to_degrees();
    let mut angle = if sin >= 0.0 { angle } else { 360.0 - angle };
    if angle < 0.001 && sin < 0.0 {
        angle = 360.0;
    }
    if angle > 359.999 && sin >= 0.0 {
        angle = 0.0;
    }
    angle
}

fn trunc_index(value: f64, count: usize) -> usize {
    if !value.is_finite() || value <= 0.0 {
        0
    } else {
        (value as usize).min(count.saturating_sub(1))
    }
}

fn reorder_loops(points: Vec<VCarvePoint>, accuracy: f64) -> Vec<VCarvePoint> {
    if points.len() < 2 {
        return points;
    }

    let mut loops = Vec::new();
    let mut start = 0usize;
    for index in 1..points.len() {
        if points[index].loop_id != points[index - 1].loop_id {
            loops.push((start, index - 1));
            start = index;
        }
    }
    loops.push((start, points.len() - 1));
    if loops.len() < 2 {
        return points;
    }

    let mut ordered = Vec::with_capacity(loops.len());
    ordered.push(loops.remove(0));
    while !loops.is_empty() {
        let (_, current_end) = *ordered.last().unwrap();
        let current = points[current_end].position;
        let mut nearest = 0usize;
        let mut nearest_distance = point_distance_squared(current, points[loops[0].0].position);
        for (index, (loop_start, _)) in loops.iter().enumerate().skip(1) {
            let distance = point_distance_squared(current, points[*loop_start].position);
            if distance + accuracy * accuracy < nearest_distance {
                nearest_distance = distance;
                nearest = index;
            }
        }
        ordered.push(loops.remove(nearest));
    }

    let mut output = Vec::with_capacity(points.len());
    for (start, end) in ordered {
        output.extend(points[start..=end].iter().copied());
    }
    output
}

fn point_distance_squared(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

fn check_canceled(cancel: &dyn Fn() -> bool) -> Result<(), VCarveCanceled> {
    if cancel() {
        Err(VCarveCanceled)
    } else {
        Ok(())
    }
}

fn point_distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::default_legacy_settings;
    use std::cell::Cell;

    #[test]
    fn vbit_depth_uses_included_angle() {
        let options = VCarveOptions::from_legacy(&default_legacy_settings());

        assert!((options.depth_for_radius(0.5) + 0.8660254038).abs() < 1e-9);
    }

    #[test]
    fn depth_limit_reduces_effective_vbit_diameter() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("v_depth_lim", "-0.2", false);
        let options = VCarveOptions::from_legacy(&settings);

        assert!((options.effective_bit_diameter() - 0.2309401077).abs() < 1e-9);
    }

    #[test]
    fn inlay_vbit_diameter_uses_allowance() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("inlay", "1", false);
        settings.set_or_push("allowance", "-0.1", false);
        let options = VCarveOptions::from_legacy(&settings);

        assert!((options.effective_bit_diameter() - 0.1154700538).abs() < 1e-9);
    }

    #[test]
    fn inlay_depth_uses_f_engrave_rounded_maxcut() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("inlay", "1", false);
        settings.set_or_push("allowance", "-0.1", false);
        let options = VCarveOptions::from_legacy(&settings);

        assert!((options.inlay_depth + 0.433).abs() < 1e-9);
        assert!((options.depth_for_radius(0.0) + 0.433).abs() < 1e-9);
        assert!((options.depth_for_radius(options.max_radius()) + 0.533).abs() < 1e-9);
    }

    #[test]
    fn inlay_depth_honors_f_engrave_depth_limit_maxcut() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("inlay", "1", false);
        settings.set_or_push("v_depth_lim", "-0.2", false);
        let options = VCarveOptions::from_legacy(&settings);

        assert!((options.inlay_depth + 0.2).abs() < 1e-9);
        assert!((options.depth_for_radius(0.0) + 0.2).abs() < 1e-9);
    }

    #[test]
    fn v_flop_matches_f_engrave_text_transform_rules() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("input_type", "text", false);
        assert!(!VCarveOptions::from_legacy(&settings).v_flop);

        settings.set_or_push("plotbox", "1", false);
        assert!(VCarveOptions::from_legacy(&settings).v_flop);

        settings.set_or_push("mirror", "1", false);
        assert!(!VCarveOptions::from_legacy(&settings).v_flop);

        settings.set_or_push("v_flop", "1", false);
        assert!(VCarveOptions::from_legacy(&settings).v_flop);

        settings.set_or_push("input_type", "image", false);
        assert!(VCarveOptions::from_legacy(&settings).v_flop);
    }

    #[test]
    fn rough_pass_caps_include_final_pass() {
        let mut settings = default_legacy_settings();
        settings.set_or_push("v_rough_stk", "0.1", false);
        settings.set_or_push("v_max_cut", "-0.2", false);
        let options = VCarveOptions::from_legacy(&settings);

        let caps = options.rough_pass_caps(-0.55);
        assert_eq!(caps.len(), 4);
        assert!((caps[0].unwrap() + 0.2).abs() < 1e-9);
        assert!((caps[1].unwrap() + 0.4).abs() < 1e-9);
        assert!((caps[2].unwrap() + 0.6).abs() < 1e-9);
        assert_eq!(caps[3], None);
        assert!((options.pass_depth_for_radius(0.3, Some(-0.2)) + 0.2).abs() < 1e-9);
        assert!((options.pass_depth_for_radius(0.3, None) + 0.5196152423).abs() < 1e-9);
    }

    #[test]
    fn generates_variable_radius_points_for_closed_square() {
        let segments = square_segments();
        let mut settings = default_legacy_settings();
        settings.set_or_push("v_step_len", "0.5", false);
        let options = VCarveOptions::from_legacy(&settings);

        let points = generate_vcarve_points(&segments, &options, 0.001);

        assert!(points.iter().any(|point| point.radius == 0.0));
        assert!(points.iter().any(|point| point.radius > 0.24));
        assert!(points.iter().any(|point| point.position.y > 0.2));
    }

    #[test]
    fn image_sort_reverses_counter_clockwise_outer_loop() {
        let sorted = sort_image_segments_for_vcarve(&square_segments(), 0.001);

        assert_eq!(sorted[0].start, Point::new(0.0, 0.0));
        assert_eq!(sorted[0].end, Point::new(0.0, 2.0));
        assert_eq!(sorted[0].loop_id, sorted[1].loop_id);
        assert!(point_distance(sorted[0].end, sorted[1].start) < ZERO);
    }

    #[test]
    fn vcarve_generation_can_cancel_inside_sampling_loop() {
        let segments = square_segments();
        let mut settings = default_legacy_settings();
        settings.set_or_push("v_step_len", "0.01", false);
        let options = VCarveOptions::from_legacy(&settings);
        let calls = Cell::new(0usize);

        let err = generate_vcarve_points_with_cancel(&segments, &options, 0.001, &|| {
            let next = calls.get() + 1;
            calls.set(next);
            next > 8
        })
        .unwrap_err();

        assert_eq!(err, VCarveCanceled);
        assert!(calls.get() > 8);
    }

    fn square_segments() -> Vec<EngraveSegment> {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
            Point::new(0.0, 0.0),
        ];
        points
            .windows(2)
            .map(|pair| EngraveSegment {
                start: pair[0],
                end: pair[1],
                loop_id: 1,
            })
            .collect()
    }
}
