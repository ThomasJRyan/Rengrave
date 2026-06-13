// Native Rust port-in-progress of the Potrace bitmap-to-vector path used by R-Engrave.
// The algorithm structure follows Potrace 1.16 by Peter Selinger.

use std::fmt::Write as _;

const WORD_BITS: i32 = u64::BITS as i32;
const ALL_BITS: u64 = !0;
const COS179: f64 = -0.999_847_695_156;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bitmap dimensions must be non-negative")]
    InvalidBitmapDimensions,
    #[error("bitmap data length does not match dimensions")]
    InvalidBitmapData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPolicy {
    Black,
    White,
    Left,
    Right,
    Minority,
    Majority,
    Random,
}

impl Default for TurnPolicy {
    fn default() -> Self {
        Self::Minority
    }
}

impl TurnPolicy {
    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "black" => Self::Black,
            "white" => Self::White,
            "left" => Self::Left,
            "right" => Self::Right,
            "majority" => Self::Majority,
            "random" => Self::Random,
            _ => Self::Minority,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub turd_size: i32,
    pub turn_policy: TurnPolicy,
    pub alpha_max: f64,
    pub opti_curve: bool,
    pub opt_tolerance: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            turd_size: 2,
            turn_policy: TurnPolicy::Minority,
            alpha_max: 1.0,
            opti_curve: true,
            opt_tolerance: 0.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    width: i32,
    height: i32,
    dy: i32,
    map: Vec<u64>,
}

impl Bitmap {
    pub fn new(width: i32, height: i32) -> Result<Self, Error> {
        if width < 0 || height < 0 {
            return Err(Error::InvalidBitmapDimensions);
        }
        let dy = if width == 0 {
            0
        } else {
            (width - 1) / WORD_BITS + 1
        };
        let map_len = dy.saturating_mul(height) as usize;
        Ok(Self {
            width,
            height,
            dy,
            map: vec![0; map_len],
        })
    }

    pub fn from_bits(
        width: i32,
        height: i32,
        bits: impl IntoIterator<Item = bool>,
    ) -> Result<Self, Error> {
        let mut bitmap = Self::new(width, height)?;
        if width == 0 || height == 0 {
            if bits.into_iter().next().is_some() {
                return Err(Error::InvalidBitmapData);
            }
            return Ok(bitmap);
        }
        let mut count = 0usize;
        for (index, bit) in bits.into_iter().enumerate() {
            let x = (index as i32) % width;
            let y = (index as i32) / width;
            if y >= height {
                return Err(Error::InvalidBitmapData);
            }
            if bit {
                bitmap.set(x, y, true);
            }
            count += 1;
        }
        if count != (width * height) as usize {
            return Err(Error::InvalidBitmapData);
        }
        Ok(bitmap)
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn get(&self, x: i32, y: i32) -> bool {
        if !self.safe(x, y) {
            return false;
        }
        self.get_unchecked(x, y)
    }

    pub fn set(&mut self, x: i32, y: i32, value: bool) {
        if !self.safe(x, y) {
            return;
        }
        let index = self.word_index(x, y);
        let mask = word_mask(x);
        if value {
            self.map[index] |= mask;
        } else {
            self.map[index] &= !mask;
        }
    }

    fn get_unchecked(&self, x: i32, y: i32) -> bool {
        (self.map[self.word_index(x, y)] & word_mask(x)) != 0
    }

    fn word_index(&self, x: i32, y: i32) -> usize {
        (y * self.dy + x / WORD_BITS) as usize
    }

    fn safe(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width && y >= 0 && y < self.height
    }

    fn clear_excess(&mut self) {
        let extra = self.width % WORD_BITS;
        if extra == 0 || self.width == 0 {
            return;
        }
        let mask = ALL_BITS << (WORD_BITS - extra);
        for y in 0..self.height {
            let index = self.word_index(self.width, y);
            self.map[index] &= mask;
        }
    }

    fn scanline_has_bits_from(&self, x0: i32, y: i32) -> Option<i32> {
        let mut x = x0 & !(WORD_BITS - 1);
        while x < self.width && x >= 0 {
            if self.map[self.word_index(x, y)] != 0 {
                while !self.get(x, y) {
                    x += 1;
                }
                return Some(x);
            }
            x += WORD_BITS;
        }
        None
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct IPoint {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct DPoint {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveTag {
    CurveTo,
    Corner,
}

#[derive(Debug, Clone, PartialEq)]
struct Curve {
    tag: Vec<CurveTag>,
    c: Vec<[DPoint; 3]>,
    vertex: Vec<DPoint>,
    alpha: Vec<f64>,
    alpha0: Vec<f64>,
    beta: Vec<f64>,
    alpha_curve: bool,
}

impl Curve {
    fn new(n: usize) -> Self {
        Self {
            tag: vec![CurveTag::Corner; n],
            c: vec![[DPoint::default(); 3]; n],
            vertex: vec![DPoint::default(); n],
            alpha: vec![0.0; n],
            alpha0: vec![0.0; n],
            beta: vec![0.0; n],
            alpha_curve: false,
        }
    }

    fn len(&self) -> usize {
        self.tag.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Path {
    points: Vec<IPoint>,
    area: i32,
    sign: char,
    curve: Curve,
    x0: i32,
    y0: i32,
    sums: Vec<Sums>,
    lon: Vec<usize>,
    po: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Sums {
    x: f64,
    y: f64,
    x2: f64,
    xy: f64,
    y2: f64,
}

pub fn trace_bitmap_to_dxf(bitmap: &Bitmap, options: Options) -> Result<String, Error> {
    let paths = trace_bitmap(bitmap, options)?;
    Ok(write_dxf(
        bitmap.width() as f64,
        bitmap.height() as f64,
        &paths,
    ))
}

fn trace_bitmap(bitmap: &Bitmap, options: Options) -> Result<Vec<Path>, Error> {
    let mut paths = decompose_bitmap(bitmap, options)?;
    for path in &mut paths {
        calc_sums(path);
        calc_lon(path);
        best_polygon(path);
        adjust_vertices(path);
        if path.sign == '-' {
            reverse_curve_vertices(&mut path.curve);
        }
        smooth(&mut path.curve, options.alpha_max);
        if options.opti_curve {
            path.curve = opticurve(path, options.opt_tolerance);
        }
    }
    Ok(paths)
}

fn decompose_bitmap(bitmap: &Bitmap, options: Options) -> Result<Vec<Path>, Error> {
    let mut bm = bitmap.clone();
    bm.clear_excess();

    let mut paths = Vec::new();
    let mut x = 0;
    let mut y = bm.height - 1;
    while let Some((next_x, next_y)) = find_next(&bm, x, y) {
        x = next_x;
        y = next_y;
        let sign = if bitmap.get(x, y) { '+' } else { '-' };
        let path = find_path(&bm, x, y + 1, sign, options.turn_policy);
        xor_path(&mut bm, &path);
        if path.area > options.turd_size {
            paths.push(path);
        }
    }

    Ok(paths)
}

fn find_next(bitmap: &Bitmap, mut x: i32, mut y: i32) -> Option<(i32, i32)> {
    let mut x0 = x & !(WORD_BITS - 1);
    while y >= 0 {
        if let Some(found_x) = bitmap.scanline_has_bits_from(x0, y) {
            x = found_x;
            return Some((x, y));
        }
        x0 = 0;
        y -= 1;
    }
    None
}

fn find_path(bitmap: &Bitmap, x0: i32, y0: i32, sign: char, turn_policy: TurnPolicy) -> Path {
    let mut x = x0;
    let mut y = y0;
    let mut dirx = 0;
    let mut diry = -1;
    let mut points = Vec::new();
    let mut area: i64 = 0;

    loop {
        points.push(IPoint { x, y });
        x += dirx;
        y += diry;
        area += i64::from(x * diry);
        if x == x0 && y == y0 {
            break;
        }

        let c = bitmap.get(x + (dirx + diry - 1) / 2, y + (diry - dirx - 1) / 2);
        let d = bitmap.get(x + (dirx - diry - 1) / 2, y + (diry + dirx - 1) / 2);

        if c && !d {
            if should_turn_right(bitmap, x, y, sign, turn_policy) {
                let tmp = dirx;
                dirx = diry;
                diry = -tmp;
            } else {
                let tmp = dirx;
                dirx = -diry;
                diry = tmp;
            }
        } else if c {
            let tmp = dirx;
            dirx = diry;
            diry = -tmp;
        } else if !d {
            let tmp = dirx;
            dirx = -diry;
            diry = tmp;
        }
    }

    Path {
        points,
        area: area.min(i64::from(i32::MAX)) as i32,
        sign,
        curve: Curve::new(0),
        x0: 0,
        y0: 0,
        sums: Vec::new(),
        lon: Vec::new(),
        po: Vec::new(),
    }
}

fn should_turn_right(bitmap: &Bitmap, x: i32, y: i32, sign: char, turn_policy: TurnPolicy) -> bool {
    match turn_policy {
        TurnPolicy::Right => true,
        TurnPolicy::Black => sign == '+',
        TurnPolicy::White => sign == '-',
        TurnPolicy::Random => detrand(x, y),
        TurnPolicy::Majority => majority(bitmap, x, y),
        TurnPolicy::Minority => !majority(bitmap, x, y),
        TurnPolicy::Left => false,
    }
}

fn detrand(x: i32, y: i32) -> bool {
    const TABLE: [u8; 256] = [
        0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0,
        0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1,
        1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0,
        0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0,
        0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0,
        0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1,
        1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1,
        0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1,
        1, 1, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0,
    ];
    let z = (((0x04b3_e375u32).wrapping_mul(x as u32)) ^ (y as u32)).wrapping_mul(0x05a8_ef93);
    let bit = TABLE[(z & 0xff) as usize]
        ^ TABLE[((z >> 8) & 0xff) as usize]
        ^ TABLE[((z >> 16) & 0xff) as usize]
        ^ TABLE[((z >> 24) & 0xff) as usize];
    bit != 0
}

fn majority(bitmap: &Bitmap, x: i32, y: i32) -> bool {
    for radius in 2..5 {
        let mut count = 0;
        for a in -radius + 1..=radius - 1 {
            count += if bitmap.get(x + a, y + radius - 1) {
                1
            } else {
                -1
            };
            count += if bitmap.get(x + radius - 1, y + a - 1) {
                1
            } else {
                -1
            };
            count += if bitmap.get(x + a - 1, y - radius) {
                1
            } else {
                -1
            };
            count += if bitmap.get(x - radius, y + a) { 1 } else { -1 };
        }
        if count > 0 {
            return true;
        }
        if count < 0 {
            return false;
        }
    }
    false
}

fn xor_path(bitmap: &mut Bitmap, path: &Path) {
    if path.points.is_empty() {
        return;
    }
    let mut y1 = path.points[path.points.len() - 1].y;
    let xa = path.points[0].x & !(WORD_BITS - 1);
    for point in &path.points {
        let x = point.x;
        let y = point.y;
        if y != y1 {
            xor_to_ref(bitmap, x, y.min(y1), xa);
            y1 = y;
        }
    }
}

fn xor_to_ref(bitmap: &mut Bitmap, x: i32, y: i32, xa: i32) {
    let xhi = x & !(WORD_BITS - 1);
    let xlo = x & (WORD_BITS - 1);
    if xhi < xa {
        let mut i = xhi;
        while i < xa {
            let index = bitmap.word_index(i, y);
            bitmap.map[index] ^= ALL_BITS;
            i += WORD_BITS;
        }
    } else {
        let mut i = xa;
        while i < xhi {
            let index = bitmap.word_index(i, y);
            bitmap.map[index] ^= ALL_BITS;
            i += WORD_BITS;
        }
    }
    if xlo != 0 {
        let index = bitmap.word_index(xhi, y);
        bitmap.map[index] ^= ALL_BITS << (WORD_BITS - xlo);
    }
}

fn calc_sums(path: &mut Path) {
    let n = path.points.len();
    path.sums = vec![Sums::default(); n + 1];
    path.x0 = path.points[0].x;
    path.y0 = path.points[0].y;
    for i in 0..n {
        let x = f64::from(path.points[i].x - path.x0);
        let y = f64::from(path.points[i].y - path.y0);
        path.sums[i + 1].x = path.sums[i].x + x;
        path.sums[i + 1].y = path.sums[i].y + y;
        path.sums[i + 1].x2 = path.sums[i].x2 + x * x;
        path.sums[i + 1].xy = path.sums[i].xy + x * y;
        path.sums[i + 1].y2 = path.sums[i].y2 + y * y;
    }
}

fn calc_lon(path: &mut Path) {
    let pt = &path.points;
    let n = pt.len();
    let mut pivk = vec![0usize; n];
    let mut nc = vec![0usize; n];

    let mut k = 0usize;
    for i in (0..n).rev() {
        if pt[i].x != pt[k].x && pt[i].y != pt[k].y {
            k = i + 1;
        }
        nc[i] = k;
    }

    path.lon = vec![0usize; n];

    for i in (0..n).rev() {
        let mut ct = [0i32; 4];
        let dir = (3
            + 3 * (pt[mod_index(i as i32 + 1, n)].x - pt[i].x)
            + (pt[mod_index(i as i32 + 1, n)].y - pt[i].y))
            / 2;
        ct[dir as usize] += 1;

        let mut constraint = [IPoint::default(), IPoint::default()];
        k = nc[i];
        let mut k1 = i;

        loop {
            let dir = (3 + 3 * sign(pt[k].x - pt[k1].x) + sign(pt[k].y - pt[k1].y)) / 2;
            ct[dir as usize] += 1;

            if ct.iter().all(|count| *count != 0) {
                pivk[i] = k1;
                break;
            }

            let cur = IPoint {
                x: pt[k].x - pt[i].x,
                y: pt[k].y - pt[i].y,
            };

            if xprod_i(constraint[0], cur) < 0 || xprod_i(constraint[1], cur) > 0 {
                let dk = IPoint {
                    x: sign(pt[k].x - pt[k1].x),
                    y: sign(pt[k].y - pt[k1].y),
                };
                let cur = IPoint {
                    x: pt[k1].x - pt[i].x,
                    y: pt[k1].y - pt[i].y,
                };
                let a = xprod_i(constraint[0], cur);
                let b = xprod_i(constraint[0], dk);
                let c = xprod_i(constraint[1], cur);
                let d = xprod_i(constraint[1], dk);
                let mut j = i32::MAX;
                if b < 0 {
                    j = floor_div(a, -b);
                }
                if d > 0 {
                    j = j.min(floor_div(-c, d));
                }
                pivk[i] = mod_index(k1 as i32 + j, n);
                break;
            }

            if cur.x.abs() <= 1 && cur.y.abs() <= 1 {
                // No constraint.
            } else {
                let off = IPoint {
                    x: cur.x
                        + if cur.y >= 0 && (cur.y > 0 || cur.x < 0) {
                            1
                        } else {
                            -1
                        },
                    y: cur.y
                        + if cur.x <= 0 && (cur.x < 0 || cur.y < 0) {
                            1
                        } else {
                            -1
                        },
                };
                if xprod_i(constraint[0], off) >= 0 {
                    constraint[0] = off;
                }
                let off = IPoint {
                    x: cur.x
                        + if cur.y <= 0 && (cur.y < 0 || cur.x < 0) {
                            1
                        } else {
                            -1
                        },
                    y: cur.y
                        + if cur.x >= 0 && (cur.x > 0 || cur.y < 0) {
                            1
                        } else {
                            -1
                        },
                };
                if xprod_i(constraint[1], off) <= 0 {
                    constraint[1] = off;
                }
            }

            k1 = k;
            k = nc[k1];
            if !cyclic(k, i, k1, n) {
                break;
            }
        }
    }

    let mut j = pivk[n - 1];
    path.lon[n - 1] = j;
    for i in (0..n - 1).rev() {
        if cyclic(i + 1, pivk[i], j, n) {
            j = pivk[i];
        }
        path.lon[i] = j;
    }

    let mut i = n - 1;
    while cyclic(mod_index(i as i32 + 1, n), j, path.lon[i], n) {
        path.lon[i] = j;
        if i == 0 {
            break;
        }
        i -= 1;
    }
}

fn penalty3(path: &Path, i: usize, mut j: usize) -> f64 {
    let n = path.points.len();
    let pt = &path.points;
    let sums = &path.sums;
    let mut r = 0usize;
    if j >= n {
        j -= n;
        r = 1;
    }

    let (x, y, x2, xy, y2, k) = if r == 0 {
        (
            sums[j + 1].x - sums[i].x,
            sums[j + 1].y - sums[i].y,
            sums[j + 1].x2 - sums[i].x2,
            sums[j + 1].xy - sums[i].xy,
            sums[j + 1].y2 - sums[i].y2,
            (j + 1 - i) as f64,
        )
    } else {
        (
            sums[j + 1].x - sums[i].x + sums[n].x,
            sums[j + 1].y - sums[i].y + sums[n].y,
            sums[j + 1].x2 - sums[i].x2 + sums[n].x2,
            sums[j + 1].xy - sums[i].xy + sums[n].xy,
            sums[j + 1].y2 - sums[i].y2 + sums[n].y2,
            f64::from(j as i32 + 1 - i as i32 + n as i32),
        )
    };

    let px = f64::from(pt[i].x + pt[j].x) / 2.0 - f64::from(pt[0].x);
    let py = f64::from(pt[i].y + pt[j].y) / 2.0 - f64::from(pt[0].y);
    let ey = f64::from(pt[j].x - pt[i].x);
    let ex = -f64::from(pt[j].y - pt[i].y);

    let a = (x2 - 2.0 * x * px) / k + px * px;
    let b = (xy - x * py - y * px) / k + px * py;
    let c = (y2 - 2.0 * y * py) / k + py * py;
    let s = ex * ex * a + 2.0 * ex * ey * b + ey * ey * c;
    s.sqrt()
}

fn best_polygon(path: &mut Path) {
    let n = path.points.len();
    let mut pen = vec![0.0; n + 1];
    let mut prev = vec![0usize; n + 1];
    let mut clip0 = vec![0usize; n];
    let mut clip1 = vec![0usize; n + 1];
    let mut seg0 = vec![0usize; n + 1];
    let mut seg1 = vec![0usize; n + 1];

    for i in 0..n {
        let mut c = mod_index(path.lon[mod_index(i as i32 - 1, n)] as i32 - 1, n);
        if c == i {
            c = mod_index(i as i32 + 1, n);
        }
        clip0[i] = if c < i { n } else { c };
    }

    let mut j = 1usize;
    for (i, clip) in clip0.iter().enumerate() {
        while j <= *clip {
            clip1[j] = i;
            j += 1;
        }
    }

    let mut i = 0usize;
    j = 0;
    while i < n {
        seg0[j] = i;
        i = clip0[i];
        j += 1;
    }
    seg0[j] = n;
    let m = j;

    i = n;
    for j in (1..=m).rev() {
        seg1[j] = i;
        i = clip1[i];
    }
    seg1[0] = 0;

    pen[0] = 0.0;
    for j in 1..=m {
        for i in seg1[j]..=seg0[j] {
            let mut best = -1.0;
            for k in (clip1[i]..=seg0[j - 1]).rev() {
                let this_pen = penalty3(path, k, i) + pen[k];
                if best < 0.0 || this_pen < best {
                    prev[i] = k;
                    best = this_pen;
                }
            }
            pen[i] = best;
        }
    }

    path.po = vec![0usize; m];
    i = n;
    for j in (0..m).rev() {
        i = prev[i];
        path.po[j] = i;
    }
}

type QuadForm = [[f64; 3]; 3];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Opti {
    pen: f64,
    c: [DPoint; 2],
    t: f64,
    s: f64,
    alpha: f64,
}

fn adjust_vertices(path: &mut Path) {
    let m = path.po.len();
    let n = path.points.len();
    if m == 0 || n == 0 {
        path.curve = Curve::new(0);
        return;
    }

    let mut ctr = vec![DPoint::default(); m];
    let mut dir = vec![DPoint::default(); m];
    let mut q = vec![[[0.0; 3]; 3]; m];
    path.curve = Curve::new(m);

    for i in 0..m {
        let mut j = path.po[mod_index(i as i32 + 1, m)] as i32;
        j = mod_index(j - path.po[i] as i32, n) as i32 + path.po[i] as i32;
        (ctr[i], dir[i]) = pointslope(path, path.po[i] as i32, j);
    }

    for i in 0..m {
        let d = sq(dir[i].x) + sq(dir[i].y);
        if d == 0.0 {
            continue;
        }
        let v = [
            dir[i].y,
            -dir[i].x,
            dir[i].x * ctr[i].y - dir[i].y * ctr[i].x,
        ];
        for l in 0..3 {
            for k in 0..3 {
                q[i][l][k] = v[l] * v[k] / d;
            }
        }
    }

    for i in 0..m {
        let s = DPoint {
            x: f64::from(path.points[path.po[i]].x - path.x0),
            y: f64::from(path.points[path.po[i]].y - path.y0),
        };
        let j = mod_index(i as i32 - 1, m);
        let mut qsum = [[0.0; 3]; 3];
        for l in 0..3 {
            for k in 0..3 {
                qsum[l][k] = q[j][l][k] + q[i][l][k];
            }
        }

        let mut w;
        loop {
            let det = qsum[0][0] * qsum[1][1] - qsum[0][1] * qsum[1][0];
            if det != 0.0 {
                w = DPoint {
                    x: (-qsum[0][2] * qsum[1][1] + qsum[1][2] * qsum[0][1]) / det,
                    y: (qsum[0][2] * qsum[1][0] - qsum[1][2] * qsum[0][0]) / det,
                };
                break;
            }

            let mut v = [0.0; 3];
            if qsum[0][0] > qsum[1][1] {
                v[0] = -qsum[0][1];
                v[1] = qsum[0][0];
            } else if qsum[1][1] != 0.0 {
                v[0] = -qsum[1][1];
                v[1] = qsum[1][0];
            } else {
                v[0] = 1.0;
                v[1] = 0.0;
            }
            let d = sq(v[0]) + sq(v[1]);
            v[2] = -v[1] * s.y - v[0] * s.x;
            for l in 0..3 {
                for k in 0..3 {
                    qsum[l][k] += v[l] * v[k] / d;
                }
            }
        }

        let dx = (w.x - s.x).abs();
        let dy = (w.y - s.y).abs();
        if dx <= 0.5 && dy <= 0.5 {
            path.curve.vertex[i] = DPoint {
                x: w.x + f64::from(path.x0),
                y: w.y + f64::from(path.y0),
            };
            continue;
        }

        let mut min = quadform(qsum, s);
        let mut xmin = s.x;
        let mut ymin = s.y;

        if qsum[0][0] != 0.0 {
            for z in 0..2 {
                w = DPoint {
                    x: 0.0,
                    y: s.y - 0.5 + f64::from(z),
                };
                w.x = -(qsum[0][1] * w.y + qsum[0][2]) / qsum[0][0];
                let dx = (w.x - s.x).abs();
                let cand = quadform(qsum, w);
                if dx <= 0.5 && cand < min {
                    min = cand;
                    xmin = w.x;
                    ymin = w.y;
                }
            }
        }

        if qsum[1][1] != 0.0 {
            for z in 0..2 {
                w = DPoint {
                    x: s.x - 0.5 + f64::from(z),
                    y: 0.0,
                };
                w.y = -(qsum[1][0] * w.x + qsum[1][2]) / qsum[1][1];
                let dy = (w.y - s.y).abs();
                let cand = quadform(qsum, w);
                if dy <= 0.5 && cand < min {
                    min = cand;
                    xmin = w.x;
                    ymin = w.y;
                }
            }
        }

        for l in 0..2 {
            for k in 0..2 {
                w = DPoint {
                    x: s.x - 0.5 + f64::from(l),
                    y: s.y - 0.5 + f64::from(k),
                };
                let cand = quadform(qsum, w);
                if cand < min {
                    min = cand;
                    xmin = w.x;
                    ymin = w.y;
                }
            }
        }

        path.curve.vertex[i] = DPoint {
            x: xmin + f64::from(path.x0),
            y: ymin + f64::from(path.y0),
        };
    }
}

fn reverse_curve_vertices(curve: &mut Curve) {
    curve.vertex.reverse();
}

fn smooth(curve: &mut Curve, alpha_max: f64) {
    let m = curve.len();
    if m == 0 {
        return;
    }

    for i in 0..m {
        let j = mod_index(i as i32 + 1, m);
        let k = mod_index(i as i32 + 2, m);
        let p4 = interval(0.5, curve.vertex[k], curve.vertex[j]);

        let denom = ddenom(curve.vertex[i], curve.vertex[k]);
        let mut alpha = if denom != 0.0 {
            let dd = (dpara(curve.vertex[i], curve.vertex[j], curve.vertex[k]) / denom).abs();
            let alpha = if dd > 1.0 { 1.0 - 1.0 / dd } else { 0.0 };
            alpha / 0.75
        } else {
            4.0 / 3.0
        };
        curve.alpha0[j] = alpha;

        if alpha >= alpha_max {
            curve.tag[j] = CurveTag::Corner;
            curve.c[j][1] = curve.vertex[j];
            curve.c[j][2] = p4;
        } else {
            alpha = alpha.clamp(0.55, 1.0);
            let p2 = interval(0.5 + 0.5 * alpha, curve.vertex[i], curve.vertex[j]);
            let p3 = interval(0.5 + 0.5 * alpha, curve.vertex[k], curve.vertex[j]);
            curve.tag[j] = CurveTag::CurveTo;
            curve.c[j][0] = p2;
            curve.c[j][1] = p3;
            curve.c[j][2] = p4;
        }
        curve.alpha[j] = alpha;
        curve.beta[j] = 0.5;
    }
    curve.alpha_curve = true;
}

fn opticurve(path: &Path, opt_tolerance: f64) -> Curve {
    let curve = &path.curve;
    let m = curve.len();
    if m == 0 {
        return Curve::new(0);
    }

    let mut convc = vec![0; m];
    for (i, conv) in convc.iter_mut().enumerate() {
        if curve.tag[i] == CurveTag::CurveTo {
            *conv = sign_f64(dpara(
                curve.vertex[mod_index(i as i32 - 1, m)],
                curve.vertex[i],
                curve.vertex[mod_index(i as i32 + 1, m)],
            ));
        }
    }

    let mut area = 0.0;
    let mut areac = vec![0.0; m + 1];
    let p0 = curve.vertex[0];
    for i in 0..m {
        let i1 = mod_index(i as i32 + 1, m);
        if curve.tag[i1] == CurveTag::CurveTo {
            let alpha = curve.alpha[i1];
            area += 0.3
                * alpha
                * (4.0 - alpha)
                * dpara(curve.c[i][2], curve.vertex[i1], curve.c[i1][2])
                / 2.0;
            area += dpara(p0, curve.c[i][2], curve.c[i1][2]) / 2.0;
        }
        areac[i + 1] = area;
    }

    let mut pt = vec![-1isize; m + 1];
    let mut pen = vec![0.0; m + 1];
    let mut len = vec![0usize; m + 1];
    let mut opt = vec![Opti::default(); m + 1];

    for j in 1..=m {
        pt[j] = j as isize - 1;
        pen[j] = pen[j - 1];
        len[j] = len[j - 1] + 1;

        if j >= 2 {
            for i in (0..=j - 2).rev() {
                let Some(o) = opti_penalty(
                    path,
                    i,
                    mod_index(j as i32, m),
                    opt_tolerance,
                    &convc,
                    &areac,
                ) else {
                    break;
                };
                if len[j] > len[i] + 1 || (len[j] == len[i] + 1 && pen[j] > pen[i] + o.pen) {
                    pt[j] = i as isize;
                    pen[j] = pen[i] + o.pen;
                    len[j] = len[i] + 1;
                    opt[j] = o;
                }
            }
        }
    }

    let om = len[m];
    let mut ocurve = Curve::new(om);
    let mut s = vec![0.0; om];
    let mut t = vec![0.0; om];

    let mut j = m;
    for i in (0..om).rev() {
        if pt[j] == j as isize - 1 {
            let idx = mod_index(j as i32, m);
            ocurve.tag[i] = curve.tag[idx];
            ocurve.c[i] = curve.c[idx];
            ocurve.vertex[i] = curve.vertex[idx];
            ocurve.alpha[i] = curve.alpha[idx];
            ocurve.alpha0[i] = curve.alpha0[idx];
            ocurve.beta[i] = curve.beta[idx];
            s[i] = 1.0;
            t[i] = 1.0;
        } else {
            let o = opt[j];
            let idx = mod_index(j as i32, m);
            ocurve.tag[i] = CurveTag::CurveTo;
            ocurve.c[i][0] = o.c[0];
            ocurve.c[i][1] = o.c[1];
            ocurve.c[i][2] = curve.c[idx][2];
            ocurve.vertex[i] = interval(o.s, curve.c[idx][2], curve.vertex[idx]);
            ocurve.alpha[i] = o.alpha;
            ocurve.alpha0[i] = o.alpha;
            s[i] = o.s;
            t[i] = o.t;
        }
        j = pt[j] as usize;
    }

    for i in 0..om {
        let i1 = mod_index(i as i32 + 1, om);
        ocurve.beta[i] = s[i] / (s[i] + t[i1]);
    }
    ocurve.alpha_curve = true;
    ocurve
}

fn opti_penalty(
    path: &Path,
    i: usize,
    j: usize,
    opt_tolerance: f64,
    convc: &[i32],
    areac: &[f64],
) -> Option<Opti> {
    let curve = &path.curve;
    let m = curve.len();

    if i == j {
        return None;
    }

    let i1 = mod_index(i as i32 + 1, m);
    let mut k1 = i1;
    let conv = convc[k1];
    if conv == 0 {
        return None;
    }
    let d = ddist(curve.vertex[i], curve.vertex[i1]);
    let mut k = k1;
    while k != j {
        k1 = mod_index(k as i32 + 1, m);
        let k2 = mod_index(k as i32 + 2, m);
        if convc[k1] != conv {
            return None;
        }
        if sign_f64(cprod(
            curve.vertex[i],
            curve.vertex[i1],
            curve.vertex[k1],
            curve.vertex[k2],
        )) != conv
        {
            return None;
        }
        if iprod4(
            curve.vertex[i],
            curve.vertex[i1],
            curve.vertex[k1],
            curve.vertex[k2],
        ) < d * ddist(curve.vertex[k1], curve.vertex[k2]) * COS179
        {
            return None;
        }
        k = k1;
    }

    let p0 = curve.c[mod_index(i as i32, m)][2];
    let p1 = curve.vertex[i1];
    let p2 = curve.vertex[mod_index(j as i32, m)];
    let p3 = curve.c[mod_index(j as i32, m)][2];

    let mut area = areac[j] - areac[i];
    area -= dpara(curve.vertex[0], curve.c[i][2], curve.c[j][2]) / 2.0;
    if i >= j {
        area += areac[m];
    }

    let a1 = dpara(p0, p1, p2);
    let a2 = dpara(p0, p1, p3);
    let a3 = dpara(p0, p2, p3);
    let a4 = a1 + a3 - a2;
    if a2 == a1 {
        return None;
    }

    let t = a3 / (a3 - a4);
    let s = a2 / (a2 - a1);
    let a = a2 * t / 2.0;
    if a == 0.0 {
        return None;
    }

    let r = area / a;
    let alpha = 2.0 - (4.0 - r / 0.3).sqrt();
    let mut res = Opti {
        c: [interval(t * alpha, p0, p1), interval(s * alpha, p3, p2)],
        alpha,
        t,
        s,
        ..Opti::default()
    };
    let p1 = res.c[0];
    let p2 = res.c[1];

    k = mod_index(i as i32 + 1, m);
    while k != j {
        k1 = mod_index(k as i32 + 1, m);
        let t = tangent(p0, p1, p2, p3, curve.vertex[k], curve.vertex[k1]);
        if t < -0.5 {
            return None;
        }
        let pt = bezier(t, p0, p1, p2, p3);
        let d = ddist(curve.vertex[k], curve.vertex[k1]);
        if d == 0.0 {
            return None;
        }
        let d1 = dpara(curve.vertex[k], curve.vertex[k1], pt) / d;
        if d1.abs() > opt_tolerance {
            return None;
        }
        if iprod3(curve.vertex[k], curve.vertex[k1], pt) < 0.0
            || iprod3(curve.vertex[k1], curve.vertex[k], pt) < 0.0
        {
            return None;
        }
        res.pen += sq(d1);
        k = k1;
    }

    k = i;
    while k != j {
        k1 = mod_index(k as i32 + 1, m);
        let t = tangent(p0, p1, p2, p3, curve.c[k][2], curve.c[k1][2]);
        if t < -0.5 {
            return None;
        }
        let pt = bezier(t, p0, p1, p2, p3);
        let d = ddist(curve.c[k][2], curve.c[k1][2]);
        if d == 0.0 {
            return None;
        }
        let mut d1 = dpara(curve.c[k][2], curve.c[k1][2], pt) / d;
        let mut d2 = dpara(curve.c[k][2], curve.c[k1][2], curve.vertex[k1]) / d;
        d2 *= 0.75 * curve.alpha[k1];
        if d2 < 0.0 {
            d1 = -d1;
            d2 = -d2;
        }
        if d1 < d2 - opt_tolerance {
            return None;
        }
        if d1 < d2 {
            res.pen += sq(d1 - d2);
        }
        k = k1;
    }

    Some(res)
}

fn pointslope(path: &Path, mut i: i32, mut j: i32) -> (DPoint, DPoint) {
    let n = path.points.len() as i32;
    let mut r = 0;

    while j >= n {
        j -= n;
        r += 1;
    }
    while i >= n {
        i -= n;
        r -= 1;
    }
    while j < 0 {
        j += n;
        r -= 1;
    }
    while i < 0 {
        i += n;
        r += 1;
    }

    let i = i as usize;
    let j = j as usize;
    let r = f64::from(r);
    let sums = &path.sums;
    let nsum = sums[path.points.len()];

    let x = sums[j + 1].x - sums[i].x + r * nsum.x;
    let y = sums[j + 1].y - sums[i].y + r * nsum.y;
    let x2 = sums[j + 1].x2 - sums[i].x2 + r * nsum.x2;
    let xy = sums[j + 1].xy - sums[i].xy + r * nsum.xy;
    let y2 = sums[j + 1].y2 - sums[i].y2 + r * nsum.y2;
    let k = (j + 1) as f64 - i as f64 + r * path.points.len() as f64;

    let ctr = DPoint { x: x / k, y: y / k };

    let mut a = (x2 - x * x / k) / k;
    let b = (xy - x * y / k) / k;
    let mut c = (y2 - y * y / k) / k;
    let lambda2 = (a + c + ((a - c) * (a - c) + 4.0 * b * b).sqrt()) / 2.0;

    a -= lambda2;
    c -= lambda2;

    let (dir, l) = if a.abs() >= c.abs() {
        let l = (a * a + b * b).sqrt();
        let dir = if l != 0.0 {
            DPoint {
                x: -b / l,
                y: a / l,
            }
        } else {
            DPoint::default()
        };
        (dir, l)
    } else {
        let l = (c * c + b * b).sqrt();
        let dir = if l != 0.0 {
            DPoint {
                x: -c / l,
                y: b / l,
            }
        } else {
            DPoint::default()
        };
        (dir, l)
    };

    if l == 0.0 {
        (ctr, DPoint::default())
    } else {
        (ctr, dir)
    }
}

fn write_dxf(width: f64, height: f64, paths: &[Path]) -> String {
    let mut out = String::new();
    ship_comment(
        &mut out,
        "DXF data, created by R-Engrave native Potrace port-in-progress",
    );
    ship_section(&mut out, "HEADER");
    ship(&mut out, 9, "$ACADVER");
    ship(&mut out, 1, "AC1006");
    ship(&mut out, 9, "$EXTMIN");
    ship_f64(&mut out, 10, 0.0);
    ship_f64(&mut out, 20, 0.0);
    ship_f64(&mut out, 30, 0.0);
    ship(&mut out, 9, "$EXTMAX");
    ship_f64(&mut out, 10, width);
    ship_f64(&mut out, 20, height);
    ship_f64(&mut out, 30, 0.0);
    ship_endsec(&mut out);

    ship_section(&mut out, "ENTITIES");
    for path in paths {
        write_dxf_path(&mut out, "0", &path.curve);
    }
    ship_endsec(&mut out);
    ship(&mut out, 0, "EOF");
    out
}

fn write_dxf_path(out: &mut String, layer: &str, curve: &Curve) {
    if curve.len() < 2 {
        return;
    }
    ship(out, 0, "POLYLINE");
    ship(out, 8, layer);
    ship_i32(out, 66, 1);
    ship_i32(out, 70, 1);
    for i in 0..curve.len() {
        let c = curve.c[i];
        let prev = curve.c[mod_index(i as i32 - 1, curve.len())];
        match curve.tag[i] {
            CurveTag::Corner => {
                ship_vertex(out, layer, prev[2], 0.0);
                ship_vertex(out, layer, c[1], 0.0);
            }
            CurveTag::CurveTo => {
                pseudo_bezier(out, layer, prev[2], c[0], c[1], c[2]);
            }
        }
    }
    ship(out, 0, "SEQEND");
}

fn pseudo_bezier(out: &mut String, layer: &str, a: DPoint, b: DPoint, c: DPoint, d: DPoint) {
    let e = interval(0.75, a, b);
    let g = interval(0.75, d, c);
    let f = interval(0.5, e, g);
    pseudo_quad(out, layer, a, e, f);
    pseudo_quad(out, layer, f, g, d);
}

fn pseudo_quad(out: &mut String, layer: &str, a: DPoint, c: DPoint, b: DPoint) {
    let v = sub(a, c);
    let w = sub(b, c);
    let v2 = iprod(v, v);
    let w2 = iprod(w, w);
    let vw = iprod(v, w);
    let vxw = xprod(v, w);
    let nvw = (v2 * w2).sqrt();
    let aa = v2 + 2.0 * vw + w2;
    let bb = v2 + 2.0 * nvw + w2;
    let cc = 4.0 * nvw;
    if vxw == 0.0 || aa == 0.0 {
        ship_vertex(out, layer, a, 0.0);
        return;
    }
    let y = (bb - (bb * bb - aa * cc).sqrt()) / aa;
    let g = interval(y, c, interval(0.5, a, b));
    let bulge1 = bulge(sub(a, g), v);
    let bulge2 = bulge(w, sub(b, g));
    ship_vertex(out, layer, a, -bulge1);
    ship_vertex(out, layer, g, -bulge2);
}

fn ship_vertex(out: &mut String, layer: &str, point: DPoint, bulge: f64) {
    ship(out, 0, "VERTEX");
    ship(out, 8, layer);
    ship_f64(out, 10, point.x);
    ship_f64(out, 20, point.y);
    ship_f64(out, 42, bulge);
}

fn ship_comment(out: &mut String, comment: &str) {
    ship(out, 999, comment);
}

fn ship_section(out: &mut String, name: &str) {
    ship(out, 0, "SECTION");
    ship(out, 2, name);
}

fn ship_endsec(out: &mut String) {
    ship(out, 0, "ENDSEC");
}

fn ship(out: &mut String, group_code: i32, value: &str) {
    let _ = writeln!(out, "{group_code:3}");
    let _ = writeln!(out, "{value}");
}

fn ship_i32(out: &mut String, group_code: i32, value: i32) {
    let _ = writeln!(out, "{group_code:3}");
    let _ = writeln!(out, "{value}");
}

fn ship_f64(out: &mut String, group_code: i32, value: f64) {
    let _ = writeln!(out, "{group_code:3}");
    let _ = writeln!(out, "{value:.6}");
}

fn dorth_infty(p0: DPoint, p2: DPoint) -> IPoint {
    IPoint {
        x: -sign_f64(p2.y - p0.y),
        y: sign_f64(p2.x - p0.x),
    }
}

fn dpara(p0: DPoint, p1: DPoint, p2: DPoint) -> f64 {
    let x1 = p1.x - p0.x;
    let y1 = p1.y - p0.y;
    let x2 = p2.x - p0.x;
    let y2 = p2.y - p0.y;
    x1 * y2 - x2 * y1
}

fn ddenom(p0: DPoint, p2: DPoint) -> f64 {
    let r = dorth_infty(p0, p2);
    f64::from(r.y) * (p2.x - p0.x) - f64::from(r.x) * (p2.y - p0.y)
}

fn quadform(q: QuadForm, w: DPoint) -> f64 {
    let v = [w.x, w.y, 1.0];
    let mut sum = 0.0;
    for i in 0..3 {
        for j in 0..3 {
            sum += v[i] * q[i][j] * v[j];
        }
    }
    sum
}

fn cprod(p0: DPoint, p1: DPoint, p2: DPoint, p3: DPoint) -> f64 {
    let x1 = p1.x - p0.x;
    let y1 = p1.y - p0.y;
    let x2 = p3.x - p2.x;
    let y2 = p3.y - p2.y;
    x1 * y2 - x2 * y1
}

fn iprod3(p0: DPoint, p1: DPoint, p2: DPoint) -> f64 {
    let x1 = p1.x - p0.x;
    let y1 = p1.y - p0.y;
    let x2 = p2.x - p0.x;
    let y2 = p2.y - p0.y;
    x1 * x2 + y1 * y2
}

fn iprod4(p0: DPoint, p1: DPoint, p2: DPoint, p3: DPoint) -> f64 {
    let x1 = p1.x - p0.x;
    let y1 = p1.y - p0.y;
    let x2 = p3.x - p2.x;
    let y2 = p3.y - p2.y;
    x1 * x2 + y1 * y2
}

fn ddist(p: DPoint, q: DPoint) -> f64 {
    (sq(p.x - q.x) + sq(p.y - q.y)).sqrt()
}

fn bezier(t: f64, p0: DPoint, p1: DPoint, p2: DPoint, p3: DPoint) -> DPoint {
    let s = 1.0 - t;
    DPoint {
        x: s * s * s * p0.x
            + 3.0 * (s * s * t) * p1.x
            + 3.0 * (t * t * s) * p2.x
            + t * t * t * p3.x,
        y: s * s * s * p0.y
            + 3.0 * (s * s * t) * p1.y
            + 3.0 * (t * t * s) * p2.y
            + t * t * t * p3.y,
    }
}

fn tangent(p0: DPoint, p1: DPoint, p2: DPoint, p3: DPoint, q0: DPoint, q1: DPoint) -> f64 {
    let a_cap = cprod(p0, p1, q0, q1);
    let b_cap = cprod(p1, p2, q0, q1);
    let c_cap = cprod(p2, p3, q0, q1);

    let a = a_cap - 2.0 * b_cap + c_cap;
    let b = -2.0 * a_cap + 2.0 * b_cap;
    let c = a_cap;

    let d = b * b - 4.0 * a * c;
    if a == 0.0 || d < 0.0 {
        return -1.0;
    }

    let s = d.sqrt();
    let r1 = (-b + s) / (2.0 * a);
    let r2 = (-b - s) / (2.0 * a);

    if (0.0..=1.0).contains(&r1) {
        r1
    } else if (0.0..=1.0).contains(&r2) {
        r2
    } else {
        -1.0
    }
}

fn interval(lambda: f64, a: DPoint, b: DPoint) -> DPoint {
    DPoint {
        x: a.x + lambda * (b.x - a.x),
        y: a.y + lambda * (b.y - a.y),
    }
}

fn sub(a: DPoint, b: DPoint) -> DPoint {
    DPoint {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}

fn iprod(a: DPoint, b: DPoint) -> f64 {
    a.x * b.x + a.y * b.y
}

fn xprod(a: DPoint, b: DPoint) -> f64 {
    a.x * b.y - a.y * b.x
}

fn bulge(v: DPoint, w: DPoint) -> f64 {
    let v2 = iprod(v, v);
    let w2 = iprod(w, w);
    let vw = iprod(v, w);
    let vxw = xprod(v, w);
    let nvw = (v2 * w2).sqrt();
    if vxw == 0.0 {
        return 0.0;
    }
    (nvw - vw) / vxw
}

fn word_mask(x: i32) -> u64 {
    1u64 << (WORD_BITS - 1 - (x & (WORD_BITS - 1)))
}

fn mod_index(a: i32, n: usize) -> usize {
    let n = n as i32;
    if a >= n {
        (a % n) as usize
    } else if a >= 0 {
        a as usize
    } else {
        (n - 1 - (-1 - a) % n) as usize
    }
}

fn cyclic(a: usize, b: usize, c: usize, _n: usize) -> bool {
    if a <= c {
        a <= b && b < c
    } else {
        a <= b || b < c
    }
}

fn floor_div(a: i32, n: i32) -> i32 {
    if a >= 0 { a / n } else { -1 - (-1 - a) / n }
}

fn xprod_i(a: IPoint, b: IPoint) -> i32 {
    a.x * b.y - a.y * b.x
}

fn sign(value: i32) -> i32 {
    if value > 0 {
        1
    } else if value < 0 {
        -1
    } else {
        0
    }
}

fn sign_f64(value: f64) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

fn sq(value: f64) -> f64 {
    value * value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    #[test]
    fn traces_simple_square_to_dxf_polyline() {
        let bitmap = Bitmap::from_bits(
            4,
            4,
            [
                false, false, false, false, false, true, true, false, false, true, true, false,
                false, false, false, false,
            ],
        )
        .unwrap();

        let dxf = trace_bitmap_to_dxf(&bitmap, Options::default()).unwrap();

        assert!(dxf.contains("POLYLINE"));
        assert!(dxf.contains("VERTEX"));
        assert!(dxf.ends_with("  0\nEOF\n"));
    }

    #[test]
    fn turn_policy_parser_matches_potrace_names() {
        assert_eq!(TurnPolicy::parse("black"), TurnPolicy::Black);
        assert_eq!(TurnPolicy::parse("white"), TurnPolicy::White);
        assert_eq!(TurnPolicy::parse("left"), TurnPolicy::Left);
        assert_eq!(TurnPolicy::parse("right"), TurnPolicy::Right);
        assert_eq!(TurnPolicy::parse("majority"), TurnPolicy::Majority);
        assert_eq!(TurnPolicy::parse("random"), TurnPolicy::Random);
        assert_eq!(TurnPolicy::parse("minority"), TurnPolicy::Minority);
        assert_eq!(TurnPolicy::parse("unknown"), TurnPolicy::Minority);
    }

    #[test]
    #[ignore = "requires Flower.jpg and installed C potrace for native parity smoke testing"]
    fn flower_smoke_has_same_dxf_shape_as_c_potrace_reference() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let image_path = root.join("Flower.jpg");
        let image = image::open(&image_path)
            .expect("Flower.jpg should decode")
            .to_rgba8();
        let (width, height) = image.dimensions();
        let mut bits = Vec::with_capacity((width * height) as usize);
        for y in (0..height).rev() {
            for x in 0..width {
                bits.push(pixel_is_black(image.get_pixel(x, y).0));
            }
        }
        let bitmap = Bitmap::from_bits(width as i32, height as i32, bits).unwrap();
        let no_opti = std::env::var_os("RENGRAVE_POTRACE_NO_OPTI").is_some();
        let native = trace_bitmap_to_dxf(
            &bitmap,
            Options {
                opti_curve: !no_opti,
                ..Options::default()
            },
        )
        .unwrap();

        let pbm = image_to_pbm(&image);
        let input = std::env::temp_dir().join("rengrave-native-potrace-flower.pbm");
        std::fs::write(&input, pbm).unwrap();
        let mut command = Command::new("potrace");
        command.args([
            "-z", "minority", "-t", "2", "-a", "1", "-O", "0.2", "-b", "dxf",
        ]);
        if no_opti {
            command.arg("-n");
        }
        let output = command
            .arg(&input)
            .args(["-o", "-"])
            .output()
            .expect("potrace should run");
        let _ = std::fs::remove_file(&input);
        assert!(output.status.success());
        let reference = String::from_utf8(output.stdout).unwrap();

        if std::env::var_os("RENGRAVE_WRITE_POTRACE_DXF").is_some() {
            std::fs::write(
                std::env::temp_dir().join("rengrave-native-potrace.dxf"),
                &native,
            )
            .unwrap();
            std::fs::write(
                std::env::temp_dir().join("rengrave-c-potrace.dxf"),
                &reference,
            )
            .unwrap();
        }

        assert_eq!(
            count_dxf_entities(&native, "POLYLINE"),
            count_dxf_entities(&reference, "POLYLINE")
        );
        assert_eq!(
            count_dxf_entities(&native, "VERTEX"),
            count_dxf_entities(&reference, "VERTEX")
        );
    }

    fn pixel_is_black(pixel: [u8; 4]) -> bool {
        let alpha = pixel[3] as u32;
        let red = (pixel[0] as u32 * alpha + 255 * (255 - alpha)) / 255;
        let green = (pixel[1] as u32 * alpha + 255 * (255 - alpha)) / 255;
        let blue = (pixel[2] as u32 * alpha + 255 * (255 - alpha)) / 255;
        let luma = (299 * red + 587 * green + 114 * blue) / 1000;
        luma < 128
    }

    fn image_to_pbm(image: &image::RgbaImage) -> Vec<u8> {
        let (width, height) = image.dimensions();
        let mut output = format!("P4\n{width} {height}\n").into_bytes();
        for y in 0..height {
            let mut byte = 0u8;
            let mut bit = 0;
            for x in 0..width {
                if pixel_is_black(image.get_pixel(x, y).0) {
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

    fn count_dxf_entities(dxf: &str, entity: &str) -> usize {
        dxf.lines().filter(|line| line.trim() == entity).count()
    }
}
