//! `MaxiCode` circular finder localization and affine hexagonal-grid sampling.
use crate::{maxicode, maxicode_tables::GRID, Detection};
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
) -> Option<(f32, f32, f32)> {
    let at = if vertical { y } else { x };
    let limit = if vertical { h } else { w };
    let get = |i: usize| {
        if vertical {
            bits[i * w + x]
        } else {
            bits[y * w + i]
        }
    };
    if get(at) {
        return None;
    }
    let mut left = at;
    let mut right = at + 1;
    while left > 0 && !get(left - 1) {
        left -= 1;
    }
    while right < limit && !get(right) {
        right += 1;
    }
    let center = crate::numeric::usize_f32(left + right) * 0.5;
    let hole = crate::numeric::usize_f32(right - left);
    let mut widths = [0f32; 10];
    for ring in 0..5 {
        let black = ring % 2 == 0;
        let previous = left;
        while left > 0 && get(left - 1) == black {
            left -= 1;
        }
        widths[ring * 2] = crate::numeric::usize_f32(previous - left);
        let previous = right;
        while right < limit && get(right) == black {
            right += 1;
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
fn centers(bits: &[bool], w: usize, h: usize) -> Vec<Center> {
    centers_with_rows(bits, w, h, None)
}
fn centers_with_rows(
    bits: &[bool],
    w: usize,
    h: usize,
    row_cache: Option<&crate::binarization::RowOffsets>,
) -> Vec<Center> {
    let mut out: Vec<Center> = Vec::new();
    let step = (h / 700).max(1);
    let mut offsets = Vec::new();
    let mut runs = Vec::new();
    for y in (0..h).step_by(step) {
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
        for i in 0..runs.len().saturating_sub(10) {
            if !row[offsets[i]] {
                continue;
            }
            let mut widths = [0f32; 11];
            widths.copy_from_slice(&runs[i..i + 11]);
            let outer = widths[0].min(widths[10]);
            if widths[0].max(widths[10]) > outer * 1.5 {
                widths[0] = outer;
                widths[10] = outer;
            }
            let pitch = widths.iter().sum::<f32>() / 9.;
            if pitch < 1.
                || !(0.6..=3.).contains(&(runs[i + 5] / pitch))
                || (0..11)
                    .filter(|&k| k != 5)
                    .any(|k| (widths[k] / pitch - 0.78453).abs() > 0.58)
            {
                continue;
            }
            let x = offsets[i + 5] + crate::numeric::f32_usize(runs[i + 5]) / 2;
            let Some((cy, sy, hy)) = cross(bits, w, h, x, y, true) else {
                continue;
            };
            let Some((cx, sx, hx)) =
                cross(bits, w, h, x, crate::numeric::f32_usize(cy.floor()), false)
            else {
                continue;
            };
            if !(0.5..=2.).contains(&(sx / sy)) {
                continue;
            }
            // Require the three rings also along oblique rays: linear stripes fail here.
            let mut errors = 0;
            for angle in 0..16 {
                let a = crate::numeric::f64_f32(f64::from(angle)) * std::f32::consts::TAU / 16.;
                for ring in 0..6 {
                    let radius = if ring == 0 {
                        0.2
                    } else {
                        (hx + hy) * 0.25
                            + (crate::numeric::f64_f32(f64::from(ring)) - 0.5)
                                * (4.5 - (hx + hy) * 0.25)
                                / 5.
                    };
                    if pixel(
                        bits,
                        w,
                        h,
                        cx + sx * a.cos() * radius,
                        cy + sy * a.sin() * radius,
                    ) != Some(ring % 2 == 1)
                    {
                        errors += 1;
                    }
                }
            }
            if errors > 16 {
                continue;
            }
            if let Some(old) = out
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
                out.push(Center {
                    x: cx,
                    y: cy,
                    sx,
                    sy,
                    support: 1,
                });
            }
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.support));
    out
}
fn point(t: &[f32; 6], x: f32, y: f32) -> [f32; 2] {
    [t[0] * x + t[1] * y + t[2], t[3] * x + t[4] * y + t[5]]
}
#[must_use]
pub fn diagnostic_centers(bits: &[bool], w: usize, h: usize) -> serde_json::Value {
    serde_json::json!(centers(bits, w, h)
        .iter()
        .map(|c| serde_json::json!({"x":c.x,"y":c.y,"sx":c.sx,"sy":c.sy,"support":c.support}))
        .collect::<Vec<_>>())
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
        let step = (h / 700).max(1);
        let (bits, row_cache) = images.get_with_runs(mode, step);
        let centers = centers_with_rows(bits, w, h, Some(row_cache));
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
