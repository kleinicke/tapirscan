//! `MaxiCode` circular finder localization and affine hexagonal-grid sampling.
use crate::{Detection, maxicode, maxicode_tables::GRID};
#[derive(Clone)]
struct Center {
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    support: usize,
}
const MARKERS: [(usize, usize); 13] = [
    (0, 28),
    (0, 29),
    (9, 10),
    (9, 11),
    (10, 11),
    (15, 7),
    (16, 8),
    (16, 20),
    (17, 20),
    (22, 10),
    (23, 10),
    (22, 17),
    (23, 17),
];
fn pixel(bits: &[bool], w: usize, h: usize, x: f32, y: f32) -> Option<bool> {
    if x < 0. || y < 0. || x >= crate::numeric::usize_f32(w) || y >= crate::numeric::usize_f32(h) {
        None
    } else {
        Some(bits[crate::numeric::f32_usize(y) * w + crate::numeric::f32_usize(x)])
    }
}
fn cross(
    bits: &[bool],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    vertical: bool,
    maximum: usize,
) -> Option<(f32, f32, f32)> {
    let at = if vertical { y } else { x };
    let limit = if vertical { h } else { w };
    let get = |i: usize| {
        crate::count_finder_work(1);
        if vertical {
            bits[i * w + x]
        } else {
            bits[y * w + i]
        }
    };
    if get(at) {
        return None;
    }
    // `maximum` exceeds every hole or ring width an accepted cross can have (see
    // the callers), so longer runs are rejected before following them through a
    // striped or flat image.
    let mut left = at;
    let mut right = at + 1;
    while left > 0 && !get(left - 1) {
        left -= 1;
        if right - left > maximum {
            return None;
        }
    }
    while right < limit && !get(right) {
        right += 1;
        if right - left > maximum {
            return None;
        }
    }
    let center = crate::numeric::usize_f32(left + right) * 0.5;
    let hole = crate::numeric::usize_f32(right - left);
    let mut widths = [0f32; 10];
    for ring in 0..5 {
        let black = ring % 2 == 0;
        // An inner ring longer than `maximum` fails the width check. The outer ring
        // may run into data on one side; there a run cut at `maximum` is still at
        // least 1.5 times any acceptable opposite width, so it is replaced the same.
        let outer = ring == 4;
        let previous = left;
        while left > 0 && get(left - 1) == black {
            left -= 1;
            if previous - left >= maximum {
                if outer {
                    break;
                }
                return None;
            }
        }
        widths[ring * 2] = crate::numeric::usize_f32(previous - left);
        let previous = right;
        while right < limit && get(right) == black {
            right += 1;
            if right - previous >= maximum {
                if outer {
                    break;
                }
                return None;
            }
        }
        widths[ring * 2 + 1] = crate::numeric::usize_f32(right - previous);
    }
    // The outer ring can touch a data hexagon on one side. Its opposite
    // cross-section still measures the ring width without that attachment.
    let outer = widths[8].min(widths[9]);
    if widths[8].max(widths[9]) > outer * 1.5 {
        widths[8] = outer;
        widths[9] = outer;
    }
    let pitch = (hole + widths.iter().sum::<f32>()) / 9.;
    if pitch < 1.
        || !(0.6..=3.).contains(&(hole / pitch))
        || widths.iter().any(|v| (*v / pitch - 0.78453).abs() > 0.58)
    {
        return None;
    }
    Some((center, pitch, hole / pitch))
}
/// Walk bound for a cross whose pitch is at most `pitch`: a longer hole or ring
/// is more than three pitches wide and cannot be accepted.
fn walk_limit(pitch: f32) -> usize {
    crate::numeric::f32_usize((pitch * 3.).ceil()) + 1
}
/// How far the rings on both sides of the hole of the row window starting at run
/// `i` differ, in quarter pitches up to 16: concentric rings are mirror-symmetric
/// about the hole, a window of noise runs is not.
fn asymmetry(runs: &[f32], i: usize, pitch: f32) -> usize {
    let difference: f32 = (1..5)
        .map(|k| (runs[i + 5 - k] - runs[i + 5 + k]).abs())
        .sum();
    crate::numeric::f32_usize((difference / pitch * 4.).round()).min(16)
}
/// Row step of the bullseye scans.
fn row_step(w: usize, h: usize) -> usize {
    crate::finder_budget::row_step(w, h, 700)
}
/// Bullseye checks a frame may spend per image column. Real images, including
/// labels next to text and photos of textured products, need at most about seven;
/// periodic textures (checkerboards, stripes, noise) pass the row test at nearly
/// every run.
const CHECKS_PER_COLUMN: usize = 16;
fn centers(bits: &[bool], w: usize, h: usize) -> Vec<Center> {
    centers_with_rows(bits, w, h, None).0
}
/// Returns the centers by support and whether the check budget cut any row short.
fn centers_with_rows(
    bits: &[bool],
    w: usize,
    h: usize,
    row_cache: Option<&crate::binarization::RowOffsets>,
) -> (Vec<Center>, bool) {
    let mut index = CenterIndex::new(w, h);
    let step = row_step(w, h);
    let mut offsets = Vec::new();
    let mut runs = Vec::new();
    let mut windows = Vec::new();
    let mut keys = Vec::new();
    let mut starts = Vec::new();
    let mut plans = crate::finder_budget::Rows::default();
    // Half of the budget is available from the start, for dense rows such as a
    // linear barcode near the top. Each scanned row adds an equal share of the
    // other half and unused checks carry over, so a textured band cannot exhaust
    // the checks of the rows below it. A check costs `charge` units, which keeps
    // the accounting exact in integers. That is one unit per scanned row, except
    // where the frame's area sets the row step (frames more than four times wider
    // than tall): there the checks per column shrink with the share of rows
    // scanned, so wide frames get the checks of a square frame of their area.
    let rows = h.div_ceil(step);
    let charge = h.min(700).max(rows);
    let share = CHECKS_PER_COLUMN / 2 * w;
    let mut available = share * rows;
    let mut limited = false;
    for y in (0..h).step_by(step) {
        available += share;
        let row = &bits[y * w..(y + 1) * w];
        if let Some(cache) = row_cache {
            cache.copy_or_extract(y, row, &mut offsets);
        } else {
            crate::transition_offsets_into(row, &mut offsets);
        }
        runs.clear();
        #[expect(
            clippy::cast_precision_loss,
            reason = "This preserves the existing usize-to-f32 run-width conversion after exact transition extraction."
        )]
        let widths = offsets.windows(2).map(|pair| (pair[1] - pair[0]) as f32);
        runs.extend(widths);
        // A short row skips windows inside periodic textures and checks the
        // windows whose rings end soonest and are most symmetric about the hole.
        let allowed = available / charge;
        let (first_dark, end) = (usize::from(!row[0]), runs.len().saturating_sub(10));
        let plan = plans.starts(&mut starts, &offsets, first_dark, end, (0, 10), allowed, y);
        let short = plan.is_some();
        limited |= plan == Some(true);
        windows.clear();
        crate::finder_budget::for_each_start(plan, &starts, first_dark, end, |i| {
            let mut widths = [0f32; 11];
            widths.copy_from_slice(&runs[i..i + 11]);
            let outer = widths[0].min(widths[10]);
            if widths[0].max(widths[10]) > outer * 1.5 {
                widths[0] = outer;
                widths[10] = outer;
            }
            let pitch = widths.iter().sum::<f32>() / 9.;
            if pitch >= 1.
                && (0.6..=3.).contains(&(runs[i + 5] / pitch))
                && (0..11)
                    .filter(|&k| k != 5)
                    .all(|k| (widths[k] / pitch - 0.78453).abs() <= 0.58)
            {
                windows.push((i, pitch));
            }
        });
        if windows.len() > allowed {
            limited = true;
            let phase = crate::finder_budget::phase(y);
            crate::finder_budget::thin(&mut windows, allowed, phase, &mut keys, |&(i, pitch)| {
                crate::finder_budget::around(&offsets, i, i + 10, 8) + asymmetry(&runs, i, pitch)
            });
        }
        // A short row spends its whole allowance, also what skipped texture windows
        // leave over: banked, it would pay for checking the texture of later rows.
        available -= if short { allowed } else { windows.len() } * charge;
        for &(i, pitch) in &windows {
            let x = offsets[i + 5] + crate::numeric::f32_usize(runs[i + 5]) / 2;
            // A row through the hole spans at least 0.9 of the bullseye diameter, so
            // the vertical pitch of an accepted (at most 2:1 elliptic) bullseye stays
            // below 3 row pitches, and its hole below 3 of its own pitches.
            let Some((cy, sy, hy)) = cross(bits, w, h, x, y, true, walk_limit(pitch * 3.)) else {
                continue;
            };
            // Accepted horizontal pitches are at most twice the vertical one.
            let Some((cx, sx, hx)) = cross(
                bits,
                w,
                h,
                x,
                crate::numeric::f32_usize(cy.floor()),
                false,
                walk_limit(sy * 2.),
            ) else {
                continue;
            };
            if !(0.5..=2.).contains(&(sx / sy)) {
                continue;
            }
            // Require the three rings also along oblique rays: linear stripes fail here.
            if oblique_errors(bits, w, h, [cx, cy], [sx, sy], hx + hy) > 16 {
                continue;
            }
            index.insert(cx, cy, sx, sy);
        }
    }
    let mut out = index.centers;
    out.sort_by_key(|c| std::cmp::Reverse(c.support));
    (out, limited)
}
/// Count ring mismatches along 16 oblique rays, stopping once more than 16 are
/// found: callers reject such centers, and periodic textures pass the row and
/// cross tests at almost every position.
fn oblique_errors(
    bits: &[bool],
    w: usize,
    h: usize,
    [cx, cy]: [f32; 2],
    [sx, sy]: [f32; 2],
    holes: f32,
) -> usize {
    let mut errors = 0;
    for angle in 0..16 {
        let a = crate::numeric::f64_f32(f64::from(angle)) * std::f32::consts::TAU / 16.;
        for ring in 0..6 {
            let radius = if ring == 0 {
                0.2
            } else {
                holes * 0.25
                    + (crate::numeric::f64_f32(f64::from(ring)) - 0.5) * (4.5 - holes * 0.25) / 5.
            };
            crate::count_finder_work(1);
            if pixel(
                bits,
                w,
                h,
                cx + sx * a.cos() * radius,
                cy + sy * a.sin() * radius,
            ) != Some(ring % 2 == 1)
            {
                errors += 1;
                if errors > 16 {
                    return errors;
                }
            }
        }
    }
    errors
}
/// Clusters centers exactly like a first-match linear search over insertion order,
/// but only compares centers in the 32-pixel cells within the match radius.
/// Tiled ring textures produce thousands of distinct centers.
struct CenterIndex {
    centers: Vec<Center>,
    cells: Vec<Vec<usize>>,
    columns: usize,
    rows: usize,
}
impl CenterIndex {
    fn new(w: usize, h: usize) -> Self {
        let columns = w.div_ceil(32).max(1);
        let rows = h.div_ceil(32).max(1);
        Self {
            centers: Vec::new(),
            cells: vec![Vec::new(); columns * rows],
            columns,
            rows,
        }
    }
    fn cell(&self, x: f32, y: f32) -> usize {
        let column = (crate::numeric::f32_usize(x.max(0.)) / 32).min(self.columns - 1);
        let row = (crate::numeric::f32_usize(y.max(0.)) / 32).min(self.rows - 1);
        row * self.columns + column
    }
    fn insert(&mut self, cx: f32, cy: f32, sx: f32, sy: f32) {
        let radius = sx.min(sy) * 1.2;
        let first = self.cell(cx - radius, cy - radius);
        let last = self.cell(cx + radius, cy + radius);
        let mut matched: Option<usize> = None;
        for row in first / self.columns..=last / self.columns {
            for column in first % self.columns..=last % self.columns {
                for &index in &self.cells[row * self.columns + column] {
                    if matched.is_some_and(|old| old <= index) {
                        continue;
                    }
                    let (dx, dy) = (self.centers[index].x - cx, self.centers[index].y - cy);
                    // hypot is at least max(|dx|, |dy|): the bound rejects only what
                    // hypot rejects.
                    if dx.abs() < radius && dy.abs() < radius && dx.hypot(dy) < radius {
                        matched = Some(index);
                    }
                }
            }
        }
        let Some(index) = matched else {
            let cell = self.cell(cx, cy);
            self.cells[cell].push(self.centers.len());
            self.centers.push(Center {
                x: cx,
                y: cy,
                sx,
                sy,
                support: 1,
            });
            return;
        };
        let before = self.cell(self.centers[index].x, self.centers[index].y);
        let old = &mut self.centers[index];
        let count = crate::numeric::usize_f32(old.support);
        old.x = (old.x * count + cx) / (count + 1.);
        old.y = (old.y * count + cy) / (count + 1.);
        old.sx = (old.sx * count + sx) / (count + 1.);
        old.sy = (old.sy * count + sy) / (count + 1.);
        old.support += 1;
        let after = self.cell(self.centers[index].x, self.centers[index].y);
        if before != after {
            self.cells[before].retain(|&other| other != index);
            self.cells[after].push(index);
        }
    }
}
fn point(t: &[f32; 6], x: f32, y: f32) -> [f32; 2] {
    [t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]]
}
#[must_use]
pub fn diagnostic_centers(bits: &[bool], w: usize, h: usize) -> serde_json::Value {
    serde_json::json!(
        centers(bits, w, h)
            .iter()
            .map(|c| serde_json::json!({"x":c.x,"y":c.y,"sx":c.sx,"sy":c.sy,"support":c.support}))
            .collect::<Vec<_>>()
    )
}
fn cell(row: usize, col: usize) -> [f32; 2] {
    [
        crate::numeric::usize_f32(col) - 14. + crate::numeric::usize_f32(row % 2) * 0.5,
        (crate::numeric::usize_f32(row) - 16.) * 0.866_025_4,
    ]
}
fn template() -> Vec<(f32, f32, bool)> {
    GRID.iter()
        .enumerate()
        .filter_map(|(i, &v)| {
            let row = i / 30;
            let col = i % 30;
            let [x, y] = cell(row, col);
            (v == 0 && !(row % 2 == 1 && col == 29) && x.hypot(y) > 4.7).then_some((
                x,
                y,
                MARKERS.contains(&(row, col)),
            ))
        })
        .collect()
}
fn score(bits: &[bool], w: usize, h: usize, t: &[f32; 6], template: &[(f32, f32, bool)]) -> usize {
    let mut errors = 0;
    for &(x, y, expected) in template {
        let [px, py] = point(t, x, y);
        errors += usize::from(pixel(bits, w, h, px, py) != Some(expected));
        if errors > 5 {
            return errors;
        }
    }
    errors
}
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut limited = false;
    let template = template();
    for mode in 0..4 {
        if images.is_duplicate(mode) {
            continue;
        }
        let step = row_step(w, h);
        let (bits, row_cache) = images.get_with_runs(mode, step);
        let (centers, cut) = centers_with_rows(bits, w, h, Some(row_cache));
        limited |= cut;
        limited |= centers.len() > 32;
        for c in centers.into_iter().take(32) {
            if results.iter().any(|d| {
                let center = d
                    .polygon
                    .iter()
                    .fold([0., 0.], |a, p| [a[0] + p[0] * 0.25, a[1] + p[1] * 0.25]);
                (center[0] - c.x).hypot(center[1] - c.y) < c.sx * 4.
            }) {
                continue;
            }
            let mut candidates = Vec::new();
            for angle in 0..360 {
                let a = crate::numeric::f64_f32(f64::from(angle)).to_radians();
                let (sin, cos) = a.sin_cos();
                for scale in [0.94, 0.97, 1., 1.03, 1.06] {
                    let t = [
                        c.sx * cos * scale,
                        -c.sx * sin * scale,
                        c.x,
                        c.sy * sin * scale,
                        c.sy * cos * scale,
                        c.y,
                    ];
                    let error = score(bits, w, h, &t, &template);
                    if error <= 5 {
                        candidates.push((error, t));
                    }
                }
            }
            candidates.sort_by_key(|c| c.0);
            limited |= candidates.len() > 32;
            for (error, t) in candidates.into_iter().take(32) {
                let polygon = [
                    [-14.5, -14.433_757],
                    [15.5, -14.433_757],
                    [15.5, 14.433_757],
                    [-14.5, 14.433_757],
                ]
                .map(|[x, y]| point(&t, x, y));
                if error <= 2 {
                    regions.add(
                        "MaxiCode",
                        polygon,
                        1. - crate::numeric::usize_f32(error)
                            / crate::numeric::usize_f32(template.len()),
                        c.support,
                    );
                }
                let mut matrix = Vec::with_capacity(990);
                for i in 0..990 {
                    let [x, y] = cell(i / 30, i % 30);
                    let [px, py] = point(&t, x, y);
                    matrix.push(pixel(bits, w, h, px, py).unwrap_or(false));
                }
                if let Some(read) = maxicode::decode_matrix(&matrix) {
                    results.push(Detection {
                        assembled_rows: false,
                        bytes: Some(read.bytes),
                        structured_append: read.structured_append,
                        reader_initialization: read.reader_initialization,
                        addon: None,
                        format: "MaxiCode".into(),
                        text: read.text,
                        polygon,
                        support: c.support,
                        error: crate::numeric::usize_f32(read.corrected),
                        gs1: false,
                    });
                    break;
                }
            }
        }
    }
    (results, limited)
}

#[cfg(test)]
mod bounded_work_tests {
    use super::*;
    /// Whether a texture pixel is dark.
    type Texture = fn(usize, usize) -> bool;
    fn pattern(w: usize, h: usize, dark: impl Fn(usize, usize) -> bool) -> Vec<bool> {
        (0..w * h).map(|i| dark(i % w, i / w)).collect()
    }
    /// A bullseye of 5-pixel bands (hole, then three dark and two light rings), in
    /// the pitch-normalized proportions the row and cross tests accept.
    fn draw_bullseye(bits: &mut [bool], w: usize, [cx, cy]: [usize; 2]) {
        for y in cy - 30..=cy + 30 {
            for x in cx - 30..=cx + 30 {
                let dx = crate::numeric::usize_f32(x) - crate::numeric::usize_f32(cx);
                let dy = crate::numeric::usize_f32(y) - crate::numeric::usize_f32(cy);
                let band = crate::numeric::f32_usize((dx.hypot(dy) + 2.5) / 5.);
                bits[y * w + x] = band <= 5 && band % 2 == 1;
            }
        }
    }
    #[test]
    fn periodic_textures_spend_bounded_bullseye_work() {
        let (w, h) = (640, 360);
        let mut rings = vec![false; w * h];
        for cy in (31..h - 31).step_by(62) {
            for cx in (31..w - 31).step_by(62) {
                draw_bullseye(&mut rings, w, [cx, cy]);
            }
        }
        for (name, bits) in [
            ("vertical stripes", pattern(w, h, |x, _| x / 2 % 2 == 0)),
            (
                "checkerboard",
                pattern(w, h, |x, y| (x / 2 + y / 2) % 2 == 0),
            ),
            (
                "diagonal stripes",
                pattern(w, h, |x, y| (x + y) / 3 % 2 == 0),
            ),
            ("tiled bullseyes", rings),
        ] {
            crate::take_finder_work();
            let _ = centers_with_rows(&bits, w, h, None);
            let work = crate::take_finder_work();
            // Hole walks that follow a column read about 20 million pixels on the
            // stripes; checking every run of the checkerboard about 9 million.
            assert!(work < 8 * w * h, "{name}: {work} pixel reads");
        }
    }
    #[test]
    fn a_textured_band_leaves_checks_for_a_bullseye_below_it() {
        let (w, h) = (640, 360);
        let mut bits = pattern(w, h, |x, y| y < 120 && (x / 2 + y / 2) % 2 == 0);
        draw_bullseye(&mut bits, w, [500, 300]);
        let (centers, limited) = centers_with_rows(&bits, w, h, None);
        assert!(limited, "the checkerboard band exhausts the initial checks");
        let found = centers
            .iter()
            .any(|c| (c.x - 500.).abs() < 2. && (c.y - 300.).abs() < 2.);
        assert!(
            found,
            "bullseye below the band among {} centers",
            centers.len()
        );
    }
    #[test]
    fn bullseyes_right_of_and_inside_textures_are_checked() {
        let (w, h) = (640, 360);
        let textures: [(&str, Texture); 3] = [
            ("checkerboard", |x, y| (x / 2 + y / 2) % 2 == 0),
            ("vertical stripes", |x, _| x / 2 % 2 == 0),
            ("diagonal stripes", |x, y| (x + y) / 3 % 2 == 0),
        ];
        for (name, texture) in textures {
            for (place, covered) in [
                ("right of", (|x, _| x < 440) as fn(usize, usize) -> bool),
                ("inside", |_, _| true),
            ] {
                // The bullseye's 61-pixel box sits in a light 81-pixel box.
                let quiet = |x: usize, y: usize| x.abs_diff(500) <= 40 && y.abs_diff(200) <= 40;
                let mut bits = pattern(w, h, |x, y| covered(x, y) && !quiet(x, y) && texture(x, y));
                draw_bullseye(&mut bits, w, [500, 200]);
                let (centers, limited) = centers_with_rows(&bits, w, h, None);
                assert!(limited, "{name}, {place}: the texture exhausts the checks");
                assert!(
                    centers
                        .iter()
                        .any(|c| (c.x - 500.).abs() < 2. && (c.y - 200.).abs() < 2.),
                    "{name}, {place}: bullseye among {} centers",
                    centers.len()
                );
            }
        }
    }
    #[test]
    fn indexed_clusters_match_first_match_linear_search() {
        let mut indexed = CenterIndex::new(320, 180);
        let mut expected: Vec<Center> = Vec::new();
        let mut seed = 52_711_u32;
        for _ in 0..5000 {
            let mut random = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                crate::numeric::f64_f32(f64::from(seed))
                    / crate::numeric::f64_f32(f64::from(u32::MAX))
            };
            let (cx, cy) = (random() * 320., random() * 180.);
            let (sx, sy) = (random() * 8. + 1., random() * 8. + 1.);
            indexed.insert(cx, cy, sx, sy);
            if let Some(old) = expected
                .iter_mut()
                .find(|p| (p.x - cx).hypot(p.y - cy) < sx.min(sy) * 1.2)
            {
                let count = crate::numeric::usize_f32(old.support);
                old.x = (old.x * count + cx) / (count + 1.);
                old.y = (old.y * count + cy) / (count + 1.);
                old.sx = (old.sx * count + sx) / (count + 1.);
                old.sy = (old.sy * count + sy) / (count + 1.);
                old.support += 1;
            } else {
                expected.push(Center {
                    x: cx,
                    y: cy,
                    sx,
                    sy,
                    support: 1,
                });
            }
        }
        assert_eq!(indexed.centers.len(), expected.len());
        for (a, b) in indexed.centers.iter().zip(expected) {
            assert_eq!(
                (a.x, a.y, a.sx, a.sy, a.support),
                (b.x, b.y, b.sx, b.sy, b.support)
            );
        }
    }
}
