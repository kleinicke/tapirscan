//! Bullseye localization and independent Aztec grid sampling.
use crate::{aztec, qr_detect, Detection};
mod center_index;
#[derive(Clone)]
struct Center {
    x: f32,
    y: f32,
    module: f32,
    support: usize,
}
fn cross(
    bits: &[bool],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    vertical: bool,
    expected: f32,
) -> Option<(f32, f32)> {
    if !bits[y * w + x] {
        return None;
    }
    let limit = if vertical { h } else { w };
    let pixel = |at: usize| {
        if vertical {
            bits[at * w + x]
        } else {
            bits[y * w + at]
        }
    };
    let mut start = if vertical { y } else { x };
    let mut end = start + 1;
    // Accepted cross scales are at most 1.7 times the row scale, and each
    // accepted run is at most 1.7 times that cross scale. Larger runs cannot
    // pass the existing checks; stop before following them across the image.
    let maximum = (expected * 2.9).ceil() as usize;
    while start > 0 && pixel(start - 1) {
        start -= 1;
        if end - start > maximum {
            return None;
        }
    }
    while end < limit && pixel(end) {
        end += 1;
        if end - start > maximum {
            return None;
        }
    }
    let center = (start + end) as f32 / 2.;
    let mut widths = [0.; 7];
    widths[0] = (end - start) as f32;
    for ring in 1..=3 {
        let expect = ring % 2 == 0;
        let old = start;
        while start > 0 && pixel(start - 1) == expect {
            start -= 1;
            if old - start > maximum {
                return None;
            }
        }
        widths[2 * ring - 1] = (old - start) as f32;
        let old = end;
        while end < limit && pixel(end) == expect {
            end += 1;
            if end - old > maximum {
                return None;
            }
        }
        widths[2 * ring] = (end - old) as f32;
    }
    let module = widths[..7].iter().sum::<f32>() / 7.;
    if module < 0.8
        || widths[..7]
            .iter()
            .any(|v| (*v - module).abs() > module * 0.7)
    {
        return None;
    }
    Some((center, module))
}
fn centers(bits: &[bool], w: usize, h: usize) -> Vec<Center> {
    let mut found = center_index::Index::new(w, h);
    let mut refined = center_index::Index::new(w, h);
    for y in (0..h).step_by((h / 600).max(1)) {
        let row = &bits[y * w..(y + 1) * w];
        let (runs, offsets) = crate::runs(row);
        for i in 0..runs.len().saturating_sub(8) {
            if !row[offsets[i]] {
                continue;
            }
            let module = runs[i + 1..i + 8].iter().sum::<f32>() / 7.;
            if module < 0.8
                || runs[i + 1..i + 8]
                    .iter()
                    .any(|v| (*v - module).abs() > module * 0.7)
            {
                continue;
            }
            let x = offsets[i + 4] + runs[i + 4] as usize / 2;
            let Some((cy, my)) = cross(bits, w, h, x, y, true, module) else {
                continue;
            };
            if my / module < 0.6 || my / module > 1.7 {
                continue;
            }
            found.insert(
                offsets[i + 4] as f32 + runs[i + 4] * 0.5,
                cy,
                (my + module) * 0.5,
                module * 1.5,
            );
            let Some((cx, mx)) = cross(
                bits,
                w,
                h,
                x,
                cy.floor().min((h - 1) as f32) as usize,
                false,
                my,
            ) else {
                continue;
            };
            if !(0.6..=1.7).contains(&(mx / my)) {
                continue;
            }
            let module = (mx + my) * 0.5;
            refined.insert(cx, cy, module, module * 1.5);
        }
    }
    let mut found = found.centers;
    for c in refined.centers {
        if !found.iter().any(|old| {
            (old.x - c.x).hypot(old.y - c.y) < c.module * 0.2
                && (old.module / c.module - 1.).abs() < 0.05
        }) {
            found.push(c);
        }
    }
    found.retain(|c| {
        crate::binarization::has_two_directions(bits, w, h, c.x, c.y, c.module * 5.)
            && has_oblique_rings(bits, w, h, c, 3)
    });
    found.sort_by_key(|c| std::cmp::Reverse(c.support));
    found
}
// Parallel stripes can satisfy both axis scans at 45 degrees. A bullseye also
// crosses several rings in both diagonal directions, independent of its pose.
fn has_oblique_rings(bits: &[bool], w: usize, h: usize, center: &Center, required: usize) -> bool {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    for (dx, dy) in [(diagonal, diagonal), (-diagonal, diagonal)] {
        for sign in [-1., 1.] {
            let mut previous = None;
            let mut transitions = 0;
            for step in 0..=28 {
                let distance = step as f32 * center.module * 0.25 * sign;
                let x = (center.x + dx * distance).floor() as isize;
                let y = (center.y + dy * distance).floor() as isize;
                if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
                    break;
                }
                let bit = bits[y as usize * w + x as usize];
                if previous.is_some_and(|p| p != bit) {
                    transitions += 1;
                }
                previous = Some(bit);
                if transitions >= required {
                    break;
                }
            }
            if transitions < required {
                return false;
            }
        }
    }
    true
}
/// Native diagnostic only; exposes the actual production bullseye proposals.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_centers(bits: &[bool], w: usize, h: usize) -> serde_json::Value {
    serde_json::Value::Array(
        centers(bits, w, h)
            .iter()
            .map(|c| serde_json::json!({"x":c.x,"y":c.y,"module":c.module,"support":c.support}))
            .collect(),
    )
}
fn transform(c: &Center, theta: f32, module: f32, n: usize) -> [f32; 8] {
    let a = theta.cos() * module;
    let b = theta.sin() * module;
    let half = n as f32 / 2.;
    [
        a,
        -b,
        c.x - half * (a - b),
        b,
        a,
        c.y - half * (a + b),
        0.,
        0.,
    ]
}
fn rotate(matrix: &[bool], n: usize, turn: usize, mirror: bool) -> Vec<bool> {
    (0..n * n)
        .map(|i| {
            let mut x = i % n;
            let mut y = i / n;
            if mirror {
                std::mem::swap(&mut x, &mut y);
            }
            for _ in 0..turn {
                (x, y) = (n - 1 - y, x);
            }
            matrix[y * n + x]
        })
        .collect()
}
fn template_error(bits: &[bool], w: usize, h: usize, t: &[f32; 8], limit: usize) -> usize {
    let mut errors = 0;
    for y in 0_i32..9 {
        for x in 0_i32..9 {
            let [px, py] = qr_detect::map(t, x as f32 + 0.5, y as f32 + 0.5);
            if px < 0. || py < 0. || px >= w as f32 || py >= h as f32 {
                return 81;
            }
            let ring = (x - 4).abs().max((y - 4).abs());
            errors += usize::from(bits[py as usize * w + px as usize] != (ring % 2 == 0));
            if errors > limit {
                return errors;
            }
        }
    }
    errors
}
fn ring_error(bits: &[bool], w: usize, h: usize, c: &Center, theta: f32, module: f32) -> usize {
    template_error(bits, w, h, &transform(c, theta, module, 9), 81)
}
fn centered(t: &[f32; 8], n: usize) -> [f32; 8] {
    let half = n as f32 * 0.5;
    let d = 1. - half * (t[6] + t[7]);
    [
        t[0] / d,
        t[1] / d,
        (t[2] - half * (t[0] + t[1])) / d,
        t[3] / d,
        t[4] / d,
        (t[5] - half * (t[3] + t[4])) / d,
        t[6] / d,
        t[7] / d,
    ]
}
fn ring_quads(
    bits: &[bool],
    w: usize,
    h: usize,
    center: &Center,
    theta: f32,
    module: f32,
    ring: usize,
    limited: &mut bool,
) -> Option<Vec<[f32; 8]>> {
    let x = (center.x + theta.cos() * ring as f32 * module).floor() as isize;
    let y = (center.y + theta.sin() * ring as f32 * module).floor() as isize;
    let color = ring.is_multiple_of(2);
    if x < 0
        || y < 0
        || x >= w as isize
        || y >= h as isize
        || bits[y as usize * w + x as usize] != color
    {
        return None;
    }
    let radius = (module * (ring + 2) as f32).ceil() as isize + 2;
    let left = (center.x.floor() as isize - radius).max(0);
    let top = (center.y.floor() as isize - radius).max(0);
    let right = (center.x.ceil() as isize + radius).min(w as isize);
    let bottom = (center.y.ceil() as isize + radius).min(h as isize);
    let expected_area = module * module * 8. * ring as f32;
    let quads = crate::component_geometry::quads(
        bits,
        w,
        [x as usize, y as usize],
        [left as usize, top as usize, right as usize, bottom as usize],
        [expected_area * 0.5, expected_area * 1.75],
        limited,
    )?;
    let half = ring as f32 + 0.5;
    Some(
        quads
            .into_iter()
            .filter_map(|quad| {
                qr_detect::homography(
                    [[-half, -half], [half, -half], [half, half], [-half, half]],
                    quad,
                )
            })
            .collect(),
    )
}
// Full Aztec symbols have alternating reference tracks every sixteen modules.
// These tracks constrain sampling away from the small central finder, where a
// slight projective error otherwise grows across a large symbol.
fn reference_points(n: usize) -> &'static [(f32, f32, bool)] {
    type Points = Vec<(f32, f32, bool)>;
    static GRIDS: [std::sync::OnceLock<Points>; 76] = [const { std::sync::OnceLock::new() }; 76];
    GRIDS[n / 2].get_or_init(|| {
        let center = (n / 2) as isize;
        let mut points = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let xx = x as isize - center;
                let yy = y as isize - center;
                let expected = if xx.abs().max(yy.abs()) <= 4 {
                    xx.abs().max(yy.abs()) % 2 == 0
                } else if xx.abs().max(yy.abs()) <= 7 {
                    continue;
                } else if xx % 16 == 0 {
                    yy % 2 == 0
                } else if yy % 16 == 0 {
                    xx % 2 == 0
                } else {
                    continue;
                };
                points.push((x as f32 + 0.5, y as f32 + 0.5, expected));
            }
        }
        points
    })
}
fn reference_error(bits: &[bool], w: usize, h: usize, t: &[f32; 8], n: usize) -> f32 {
    let points = reference_points(n);
    let mut error = 0.;
    for &(x, y, expected) in points {
        let [px, py] = qr_detect::map(t, x, y);
        let px = px - 0.5;
        let py = py - 0.5;
        let ix = px.floor() as isize;
        let iy = py.floor() as isize;
        if ix < 0 || iy < 0 || ix + 1 >= w as isize || iy + 1 >= h as isize {
            error += 1.;
            continue;
        }
        let dx = px - ix as f32;
        let dy = py - iy as f32;
        let at = iy as usize * w + ix as usize;
        let value = (f32::from(u8::from(bits[at])) * (1. - dx)
            + f32::from(u8::from(bits[at + 1])) * dx)
            * (1. - dy)
            + (f32::from(u8::from(bits[at + w])) * (1. - dx)
                + f32::from(u8::from(bits[at + w + 1])) * dx)
                * dy;
        error += if expected { 1. - value } else { value };
    }
    error / points.len() as f32
}
fn refine_reference(
    bits: &[bool],
    w: usize,
    h: usize,
    t: [f32; 8],
    n: usize,
    module: f32,
) -> [f32; 8] {
    let source = [
        [0., 0.],
        [n as f32, 0.],
        [n as f32, n as f32],
        [0., n as f32],
    ];
    let mut quad = source.map(|[x, y]| qr_detect::map(&t, x, y));
    let mut best = t;
    let mut error = reference_error(bits, w, h, &best, n);
    for scale in [0.8, 0.4, 0.2, 0.1] {
        for _ in 0..3 {
            let mut changed = false;
            for corner in 0..4 {
                for axis in 0..2 {
                    for sign in [-1., 1.] {
                        let mut trial = quad;
                        trial[corner][axis] += module * scale * sign;
                        let Some(t) = qr_detect::homography(source, trial) else {
                            continue;
                        };
                        let score = reference_error(bits, w, h, &t, n);
                        if score + 0.0001 < error {
                            error = score;
                            quad = trial;
                            best = t;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
    best
}
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut limited = false;
    for mode in 0..4 {
        if binary_images.is_duplicate(mode) {
            continue;
        }
        let bits = binary_images.get(mode);
        let mut candidates = centers(bits, w, h);
        candidates.sort_by_cached_key(|c| {
            let error = (0..90)
                .step_by(15)
                .map(|a| {
                    let theta = (a as f32).to_radians();
                    ring_error(
                        bits,
                        w,
                        h,
                        c,
                        theta,
                        c.module * theta.cos().abs().max(theta.sin().abs()),
                    )
                })
                .min()
                .unwrap_or(81);
            (error, std::cmp::Reverse(c.support))
        });
        if candidates.len() > 64 {
            limited = true;
            candidates.truncate(64);
        }
        'candidate: for c in candidates {
            if results.iter().any(|d| {
                let p = d
                    .polygon
                    .iter()
                    .fold([0., 0.], |a, p| [a[0] + p[0] * 0.25, a[1] + p[1] * 0.25]);
                (p[0] - c.x).hypot(p[1] - c.y) < c.module * 3.
            }) {
                continue;
            }
            // A connected inner ring supplies projective geometry even when a
            // square, uniform-scale template cannot fit a perspective view.
            let mut transforms = Vec::new();
            for degrees in [0_f32, 30., 60.] {
                let theta = degrees.to_radians();
                for scale in [1., 0.8, 1.2] {
                    let module = c.module * theta.cos().abs().max(theta.sin().abs()) * scale;
                    for ring in [3, 5, 2] {
                        for t in ring_quads(bits, w, h, &c, theta, module, ring, &mut limited)
                            .into_iter()
                            .flatten()
                        {
                            let error = template_error(bits, w, h, &centered(&t, 9), 8);
                            if error <= 8
                                && !transforms.iter().any(|previous: &[f32; 8]| {
                                    [[-4., -4.], [4., -4.], [4., 4.], [-4., 4.]].iter().all(
                                        |&[x, y]| {
                                            let a = qr_detect::map(previous, x, y);
                                            let b = qr_detect::map(&t, x, y);
                                            (a[0] - b[0]).hypot(a[1] - b[1]) < module * 0.3
                                        },
                                    )
                                })
                            {
                                transforms.push(t);
                            }
                        }
                    }
                }
            }
            let mut grids = Vec::new();
            for degrees in (0..90).step_by(3) {
                let theta = (degrees as f32).to_radians();
                for scale in [1., 0.9, 1.1] {
                    let module = c.module * theta.cos().abs().max(theta.sin().abs()) * scale;
                    let error = template_error(bits, w, h, &transform(&c, theta, module, 9), 12);
                    if error <= 12 {
                        grids.push((error, theta, module));
                    }
                }
            }
            grids.sort_by_key(|g| g.0);
            limited |= grids.len() > 12;
            for (_, theta, module) in grids.into_iter().take(12) {
                transforms.push(transform(&c, theta, module, 0));
                for t in ring_quads(bits, w, h, &c, theta, module, 2, &mut limited)
                    .into_iter()
                    .flatten()
                {
                    transforms.push(t);
                }
            }
            let original_transforms = transforms.len();
            for scale in [0.975, 1.025, 0.95, 1.05, 0.92, 1.08] {
                for index in 0..original_transforms {
                    let mut t = transforms[index];
                    for coefficient in [0, 1, 3, 4, 6, 7] {
                        t[coefficient] *= scale;
                    }
                    if template_error(bits, w, h, &centered(&t, 9), 8) <= 8 {
                        transforms.push(t);
                    }
                }
            }
            let mut reference_retries = Vec::new();
            for base_transform in transforms {
                let rune_transform = centered(&base_transform, 11);
                if let Some(grid) = qr_detect::sample(bits, w, h, 11, &rune_transform, 0.) {
                    for mirror in [false, true] {
                        for turn in 0..4 {
                            if !aztec::orientation_valid(&grid, 11, true, turn, mirror) {
                                continue;
                            }
                            if let Some(read) =
                                aztec::decode_rune(&rotate(&grid, 11, turn, mirror), 11)
                            {
                                results.push(Detection {
                                    bytes: Some(read.bytes),
                                    structured_append: read.structured_append,
                                    reader_initialization: read.reader_initialization,
                                    addon: None,
                                    format: "Aztec".into(),
                                    text: read.text,
                                    polygon: [[0., 0.], [11., 0.], [11., 11.], [0., 11.]]
                                        .map(|[x, y]| qr_detect::map(&rune_transform, x, y)),
                                    support: c.support,
                                    error: read.corrected as f32,
                                    gs1: false,
                                });
                                continue 'candidate;
                            }
                        }
                    }
                }
                let t = centered(&base_transform, 15);
                let Some(mode_grid) = qr_detect::sample(bits, w, h, 15, &t, 0.) else {
                    continue;
                };
                for mirror in [false, true] {
                    for turn in 0..4 {
                        if ![true, false].into_iter().any(|compact| {
                            aztec::orientation_valid(&mode_grid, 15, compact, turn, mirror)
                        }) {
                            continue;
                        }
                        let mode_grid = rotate(&mode_grid, 15, turn, mirror);
                        for compact in [true, false] {
                            let Some(mode) = aztec::read_mode(&mode_grid, 15, compact) else {
                                continue;
                            };
                            let layers = mode.layers;
                            let base = layers * 4 + if compact { 11 } else { 14 };
                            let n = if compact {
                                base
                            } else {
                                base + 1 + 2 * ((base / 2 - 1) / 15)
                            };
                            let t = centered(&base_transform, n);
                            let Some(grid) = qr_detect::sample(bits, w, h, n, &t, 0.) else {
                                continue;
                            };
                            regions.add(
                                "Aztec",
                                [
                                    [0., 0.],
                                    [n as f32, 0.],
                                    [n as f32, n as f32],
                                    [0., n as f32],
                                ]
                                .map(|[x, y]| qr_detect::map(&t, x, y)),
                                1.,
                                c.support,
                            );
                            if let Some(read) =
                                aztec::decode_matrix(&rotate(&grid, n, turn, mirror), n)
                            {
                                results.push(Detection {
                                    bytes: Some(read.bytes),
                                    structured_append: read.structured_append,
                                    reader_initialization: read.reader_initialization,
                                    addon: None,
                                    format: "Aztec".into(),
                                    text: read.text,
                                    polygon: [
                                        [0., 0.],
                                        [n as f32, 0.],
                                        [n as f32, n as f32],
                                        [0., n as f32],
                                    ]
                                    .map(|[x, y]| qr_detect::map(&t, x, y)),
                                    support: c.support,
                                    error: read.corrected as f32,
                                    gs1: read.gs1,
                                });
                                continue 'candidate;
                            }
                            if !compact && n >= 35 {
                                let score = reference_error(bits, w, h, &t, n);
                                if score < 0.35 {
                                    reference_retries.push((score, t, n, turn, mirror));
                                }
                            }
                        }
                    }
                }
            }
            reference_retries.sort_by(|a, b| a.0.total_cmp(&b.0));
            limited |= reference_retries.len() > 6;
            for (_, t, n, turn, mirror) in reference_retries.into_iter().take(6) {
                let t = refine_reference(bits, w, h, t, n, c.module);
                let Some(grid) = qr_detect::sample(bits, w, h, n, &t, 0.) else {
                    continue;
                };
                if let Some(read) = aztec::decode_matrix(&rotate(&grid, n, turn, mirror), n) {
                    results.push(Detection {
                        bytes: Some(read.bytes),
                        structured_append: read.structured_append,
                        reader_initialization: read.reader_initialization,
                        addon: None,
                        format: "Aztec".into(),
                        text: read.text,
                        polygon: [
                            [0., 0.],
                            [n as f32, 0.],
                            [n as f32, n as f32],
                            [0., n as f32],
                        ]
                        .map(|[x, y]| qr_detect::map(&t, x, y)),
                        support: c.support,
                        error: read.corrected as f32,
                        gs1: read.gs1,
                    });
                    continue 'candidate;
                }
            }
        }
    }
    (results, limited)
}
