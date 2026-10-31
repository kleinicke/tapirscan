//! Bounded rMQR finder search with format-first rejection and timing refinement.
use super::{finders_with_rows, map, single};
use crate::{Detection, numeric::usize_f32 as f, rmqr, rmqr_tables as tables};

type Transform = [f32; 8];
fn module(image: &[bool], size: [usize; 2], t: &Transform, x: usize, y: usize) -> Option<bool> {
    let p = map(t, f(x) + 0.5, f(y) + 0.5);
    if p[0] < 0. || p[1] < 0. || p[0] >= f(size[0]) || p[1] >= f(size[1]) {
        return None;
    }
    Some(image[crate::numeric::f32_usize(p[1]) * size[0] + crate::numeric::f32_usize(p[0])])
}
fn transform(frame: [f32; 6], scale: f32, angle: f32) -> Transform {
    let (sin, cos) = angle.sin_cos();
    let [row_x, row_y, cx, column_x, column_y, cy] =
        [frame[0], frame[1], frame[4], frame[2], frame[3], frame[5]];
    let (u, v) = (
        [
            scale * (row_x * cos - row_y * sin),
            scale * (row_x * sin + row_y * cos),
        ],
        [
            scale * (column_x * cos - column_y * sin),
            scale * (column_x * sin + column_y * cos),
        ],
    );
    [
        u[0],
        v[0],
        cx - 3.5 * (u[0] + v[0]),
        u[1],
        v[1],
        cy - 3.5 * (u[1] + v[1]),
        0.,
        0.,
    ]
}
fn header(image: &[bool], size: [usize; 2], t: &Transform) -> Option<usize> {
    let mut word = 0u32;
    for bit in 0..18 {
        let (x, y) = if bit < 15 {
            (8 + bit / 5, 1 + bit % 5)
        } else {
            (11, 1 + bit - 15)
        };
        word |= u32::from(module(image, size, t, x, y)?) << bit;
    }
    let mut best = (usize::MAX, 19);
    for (i, &value) in tables::FORMAT_INFO_LEFT.iter().enumerate() {
        let distance = (word ^ value).count_ones();
        if distance < best.1 {
            best = (i, distance);
        }
    }
    (best.1 <= 1).then_some(best.0)
}
fn score(image: &[bool], size: [usize; 2], t: &Transform, version: usize) -> usize {
    let (w, h) = (tables::WIDTH[version], tables::HEIGHT[version]);
    let mut errors = 0;
    // Alignment columns are always dark where they meet a horizontal timing track.
    let group = tables::WIDTH[..5].iter().position(|&v| v == w);
    for x in 12..w - 8 {
        let alignment = group.is_some_and(|g| tables::TABLE_D1[g * 4..g * 4 + 4].contains(&x));
        let expected = x % 2 == 0 || alignment;
        for y in [0, h - 1] {
            errors += usize::from(module(image, size, t, x, y) != Some(expected));
        }
    }
    errors
}
fn alignment_score(image: &[bool], size: [usize; 2], t: &Transform, version: usize) -> usize {
    let mut errors = score(image, size, t, version);
    let (w, h) = (tables::WIDTH[version], tables::HEIGHT[version]);
    for y in 0..5 {
        for x in 0..5 {
            let expected = x == 0 || x == 4 || y == 0 || y == 4 || (x == 2 && y == 2);
            errors +=
                2 * usize::from(module(image, size, t, w - 5 + x, h - 5 + y) != Some(expected));
        }
    }
    errors
}
fn refine(image: &[bool], size: [usize; 2], initial: Transform, version: usize) -> Transform {
    let mut best = initial;
    for (pitch, turn, steps) in [(0.02, 0.015, 3i16), (0.004, 0.003, 2)] {
        let center = map(&best, 3.5, 3.5);
        let mut winner = (alignment_score(image, size, &best, version), best);
        for a in -steps..=steps {
            for b in -steps..=steps {
                let (sin, cos) = (f32::from(b) * turn).sin_cos();
                let scale = 1. + f32::from(a) * pitch;
                let u = [
                    scale * (best[0] * cos - best[3] * sin),
                    scale * (best[0] * sin + best[3] * cos),
                ];
                let mut trial = best;
                trial[0] = u[0];
                trial[3] = u[1];
                trial[2] = center[0] - 3.5 * (trial[0] + trial[1]);
                trial[5] = center[1] - 3.5 * (trial[3] + trial[4]);
                let error = alignment_score(image, size, &trial, version);
                if error < winner.0 {
                    winner = (error, trial);
                }
            }
        }
        best = winner.1;
    }
    for step in [-3i16, -2, -1, 0, 1, 2, 3] {
        let center = map(&best, 3.5, 3.5);
        let mut trial = best;
        let scale = 1. + f32::from(step) * 0.015;
        trial[1] *= scale;
        trial[4] *= scale;
        trial[2] = center[0] - 3.5 * (trial[0] + trial[1]);
        trial[5] = center[1] - 3.5 * (trial[3] + trial[4]);
        if alignment_score(image, size, &trial, version)
            < alignment_score(image, size, &best, version)
        {
            best = trial;
        }
    }
    best
}

/// Find rectangular symbols independently of QR triples; every attempt is bounded.
pub fn detect(
    size: [usize; 2],
    images: &mut crate::binarization::Images<'_>,
    regions: &mut crate::regions::Regions,
) -> (Vec<Detection>, bool) {
    let [w, h] = size;
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    let mut limited = false;
    for mode in 0..4 {
        if images.is_duplicate(mode) {
            continue;
        }
        let image = images.get(mode);
        let (found, cut) = finders_with_rows(image, w, h, None);
        limited |= cut;
        limited |= found.len() > 64;
        for finder in found.iter().take(64) {
            if results
                .iter()
                .any(|d| super::micro::inside(&d.polygon, finder.x, finder.y))
            {
                continue;
            }
            let mut candidates = Vec::new();
            for (quad, span) in [
                (finder.quad, 3.),
                (single::outer_quad(image, w, h, finder), 7.),
            ] {
                let Some(quad) = quad else {
                    continue;
                };
                for rotation in 0..4 {
                    for mirror in [false, true] {
                        let frame = single::frame(quad, rotation, mirror, span);
                        for scale in [1., 0.96, 1.04] {
                            for angle in [0., -0.04, 0.04] {
                                let t = transform(frame, scale, angle);
                                if let Some(info) = header(image, size, &t) {
                                    let version = info % 32;
                                    let error = score(image, size, &t, version);
                                    if error * 3 <= 2 * (tables::WIDTH[version] - 20) {
                                        candidates.push((error, version, t));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            candidates.sort_by_key(|c| c.0);
            'candidate: for (_, version, initial) in candidates.into_iter().take(4) {
                let refined = refine(image, size, initial, version);
                let (cols, rows) = (tables::WIDTH[version], tables::HEIGHT[version]);
                let polygon = [[0., 0.], [f(cols), 0.], [f(cols), f(rows)], [0., f(rows)]]
                    .map(|[x, y]| map(&refined, x, y));
                if score(image, size, &refined, version) * 8 <= 2 * (cols - 20) {
                    regions.add("RMQRCode", polygon, 0.9, finder.support);
                }
                for t in [initial, refined] {
                    if attempts >= 128 {
                        return (results, true);
                    }
                    attempts += 1;
                    let grid: Option<Vec<bool>> = (0..rows)
                        .flat_map(|y| (0..cols).map(move |x| (x, y)))
                        .map(|(x, y)| module(image, size, &t, x, y))
                        .collect();
                    if let Some(payload) =
                        grid.and_then(|grid| rmqr::decode_matrix(&grid, cols, rows))
                    {
                        let polygon = [[0., 0.], [f(cols), 0.], [f(cols), f(rows)], [0., f(rows)]]
                            .map(|[x, y]| map(&t, x, y));
                        results.push(Detection {
                            format: "RMQRCode".into(),
                            text: payload.text,
                            bytes: Some(payload.bytes),
                            polygon,
                            support: finder.support,
                            error: f(payload.corrected),
                            structured_append: payload.structured_append,
                            reader_initialization: false,
                            gs1: payload.gs1,
                            assembled_rows: false,
                            addon: None,
                            codabar_start_stop: None,
                        });
                        break 'candidate;
                    }
                }
            }
        }
    }
    (results, limited)
}
