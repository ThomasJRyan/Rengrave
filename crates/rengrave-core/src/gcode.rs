use crate::layout::EngraveSegment;
use crate::settings::LegacySettings;

#[derive(Debug, Clone, PartialEq)]
pub struct GcodeOptions {
    pub safe_z: f64,
    pub depth_z: f64,
    pub feed: f64,
    pub plunge: f64,
    pub accuracy: f64,
    pub units: Units,
    pub preamble: String,
    pub postamble: String,
    pub variables_disabled: bool,
}

impl GcodeOptions {
    pub fn from_legacy(settings: &LegacySettings) -> Self {
        Self {
            safe_z: get_f64(settings, "ZSAFE", 0.25),
            depth_z: get_f64(settings, "ZCUT", -0.005),
            feed: get_f64(settings, "FEED", 5.0),
            plunge: get_f64(settings, "PLUNGE", 0.0),
            accuracy: get_f64(settings, "accuracy", 0.001),
            units: Units::parse(settings.get_last("units").unwrap_or("in")),
            preamble: settings
                .get_last("gpre")
                .unwrap_or("G17 G64 P0.001 M3 S3000")
                .to_owned(),
            postamble: settings.get_last("gpost").unwrap_or("M5|M2").to_owned(),
            variables_disabled: get_bool(settings, "var_dis", true),
        }
    }

    fn coord_digits(&self) -> usize {
        match self.units {
            Units::Inch => 4,
            Units::Mm => 3,
        }
    }

    fn feed_digits(&self) -> usize {
        match self.units {
            Units::Inch => 2,
            Units::Mm => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    Inch,
    Mm,
}

impl Units {
    fn parse(value: &str) -> Self {
        if value == "mm" { Self::Mm } else { Self::Inch }
    }

    fn gcode(self) -> &'static str {
        match self {
            Self::Inch => "G20",
            Self::Mm => "G21",
        }
    }
}

pub fn write_engrave_gcode(segments: &[EngraveSegment], options: &GcodeOptions) -> Vec<String> {
    let dp = options.coord_digits();
    let dpfeed = options.feed_digits();
    let safe_value = format_number(options.safe_z, dp);
    let depth_value = format_number(options.depth_z, dp);
    let feed = format_number(options.feed, dpfeed);
    let mut plunge = format_number(options.plunge, dpfeed);
    let zero_feed = format_number(0.0, dpfeed);
    if plunge == zero_feed {
        plunge = feed.clone();
    }

    let mut lines = Vec::new();
    if !options.variables_disabled {
        lines.push(format!("#1 = {}  ( Safe Z )", safe_value));
        lines.push(format!("#2 = {}  ( Engraving Depth Z )", depth_value));
    }
    lines.push("G90".to_owned());
    lines.push(options.units.gcode().to_owned());
    lines.extend(split_gcode_lines(&options.preamble));
    lines.push(format!("F{feed}"));

    for path in order_paths(segments, options.accuracy) {
        let Some(first) = path.first() else {
            continue;
        };
        lines.push(format!("G0 Z{safe_value}"));
        lines.push(format!(
            "G0 X{} Y{}",
            format_number(first.x, dp),
            format_number(first.y, dp)
        ));
        if plunge == feed {
            lines.push(format!("G1 Z{depth_value}"));
        } else {
            lines.push(format!("G1 Z{depth_value} F{plunge}"));
        }

        for point in path {
            lines.push(format!(
                "G1 X{} Y{}",
                format_number(point.x, dp),
                format_number(point.y, dp)
            ));
        }
    }

    lines.push(format!("G0 Z{safe_value}"));
    lines.extend(split_gcode_lines(&options.postamble));
    lines
}

fn split_gcode_lines(value: &str) -> impl Iterator<Item = String> + '_ {
    value.split('|').map(str::to_owned)
}

fn order_paths(segments: &[EngraveSegment], accuracy: f64) -> Vec<Vec<crate::geometry::Point>> {
    let mut paths: Vec<Vec<crate::geometry::Point>> = Vec::new();
    let mut last_end = None;
    let mut current_loop = None;

    for segment in segments {
        let starts_new = current_loop != Some(segment.loop_id)
            || last_end
                .map(|last: crate::geometry::Point| distance(last, segment.start) > accuracy)
                .unwrap_or(true);
        if starts_new {
            paths.push(vec![segment.start]);
        }
        paths.last_mut().unwrap().push(segment.end);
        last_end = Some(segment.end);
        current_loop = Some(segment.loop_id);
    }

    sort_paths(paths)
}

fn sort_paths(mut paths: Vec<Vec<crate::geometry::Point>>) -> Vec<Vec<crate::geometry::Point>> {
    if paths.is_empty() {
        return paths;
    }

    let mut ordered = vec![paths.remove(0)];
    while !paths.is_empty() {
        let current = *ordered.last().and_then(|path| path.last()).unwrap();
        let mut best_index = 0;
        let mut best_reverse = false;
        let mut best_distance = distance(current, paths[0][0]);
        let mut best_end_distance = distance(current, *paths[0].last().unwrap());

        for (idx, path) in paths.iter().enumerate().skip(1) {
            let begin_distance = distance(current, path[0]);
            if begin_distance < best_distance {
                best_distance = begin_distance;
                best_index = idx;
                best_reverse = false;
            }
            let end_distance = distance(current, *path.last().unwrap());
            if end_distance < best_end_distance {
                best_end_distance = end_distance;
                best_index = idx;
                best_reverse = true;
            }
        }

        let mut next = paths.remove(best_index);
        if best_reverse {
            next.reverse();
        }
        ordered.push(next);
    }

    ordered
}

fn distance(a: crate::geometry::Point, b: crate::geometry::Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn format_number(value: f64, digits: usize) -> String {
    format!("{value:.digits$}")
}

fn get_f64(settings: &LegacySettings, key: &str, default: f64) -> f64 {
    settings
        .get_last(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_bool(settings: &LegacySettings, key: &str, default: bool) -> bool {
    settings
        .get_last(key)
        .map(|value| matches!(value, "1" | "true" | "True"))
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    #[test]
    fn writes_basic_engrave_moves() {
        let options = GcodeOptions {
            safe_z: 0.25,
            depth_z: -0.005,
            feed: 5.0,
            plunge: 0.0,
            accuracy: 0.001,
            units: Units::Inch,
            preamble: "G17 G64 P0.001 M3 S3000".to_owned(),
            postamble: "M5|M2".to_owned(),
            variables_disabled: true,
        };
        let lines = write_engrave_gcode(
            &[EngraveSegment {
                start: Point::new(0.0, 0.0),
                end: Point::new(1.0, 0.0),
                loop_id: 1,
            }],
            &options,
        );

        assert!(lines.contains(&"G90".to_owned()));
        assert!(lines.contains(&"G20".to_owned()));
        assert!(lines.contains(&"G1 Z-0.0050".to_owned()));
        assert!(lines.contains(&"G1 X1.0000 Y0.0000".to_owned()));
    }
}
