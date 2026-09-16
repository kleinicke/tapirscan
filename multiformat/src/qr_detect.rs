//! Project-owned QR finder grouping, projective grid estimation and sampling.
use crate::{qr, Detection};
mod partial;
#[derive(Clone, Debug)]
struct Finder {
    x: f32,
    y: f32,
    module: f32,
    support: usize,
    quad: Option<[[f32; 2]; 4]>,
}
fn ratio(r: &[usize]) -> Option<f32> {
    let module = r.iter().sum::<usize>() as f32 / 7.;
    if module < 0.75 {
        return None;
    }
    for (i, &n) in r.iter().enumerate() {
        if (n as f32 - module * if i == 2 { 3. } else { 1. }).abs()
            > module * if i == 2 { 1.8 } else { 0.85 }
        {
            return None;
        }
    }
    Some(module)
}
fn cross(
    image: &[bool],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    vertical: bool,
    expected: f32,
) -> Option<(f32, f32)> {
    let limit = if vertical { h } else { w };
    let at = if vertical { y } else { x };
    let pixel = |i: usize| {
        if vertical {
            image[i * w + x]
        } else {
            image[y * w + i]
        }
    };
    if !pixel(at) {
        return None;
    }
    let max = (expected * 12.).ceil() as usize;
    let mut runs = [0usize; 5];
    let mut start = at;
    let mut end = at;
    while start > 0 && pixel(start - 1) && at - start < max {
        start -= 1;
    }
    while end < limit && pixel(end) && end - at < max {
        end += 1;
    }
    runs[2] = end - start;
    let center = (start + end) as f32 * 0.5;
    let mut cursor = start;
    while cursor > 0 && !pixel(cursor - 1) && runs[1] < max {
        cursor -= 1;
        runs[1] += 1;
    }
    while cursor > 0 && pixel(cursor - 1) && runs[0] < max {
        cursor -= 1;
        runs[0] += 1;
    }
    cursor = end;
    while cursor < limit && !pixel(cursor) && runs[3] < max {
        cursor += 1;
        runs[3] += 1;
    }
    while cursor < limit && pixel(cursor) && runs[4] < max {
        cursor += 1;
        runs[4] += 1;
    }
    let module = ratio(&runs)?;
    if module / expected < 0.4 || module / expected > 2.5 {
        return None;
    }
    Some((center, module))
}
fn finders(image: &[bool], w: usize, h: usize) -> (Vec<Finder>, bool) {
    let mut found: Vec<Finder> = Vec::new();
    let step = (h / 600).max(1);
    for y in (0..h).step_by(step) {
        let row = &image[y * w..(y + 1) * w];
        let mut starts = vec![0];
        let mut widths = Vec::new();
        let mut at = 0;
        for x in 1..w {
            if row[x] != row[at] {
                widths.push(x - at);
                starts.push(x);
                at = x;
            }
        }
        widths.push(w - at);
        for i in 0..widths.len().saturating_sub(4) {
            if !row[starts[i]] {
                continue;
            }
            let Some(m) = ratio(&widths[i..i + 5]) else {
                continue;
            };
            let x = starts[i] + widths[i] + widths[i + 1] + widths[i + 2] / 2;
            let Some((cy, my)) = cross(image, w, h, x, y, true, m) else {
                continue;
            };
            let Some((cx, mx)) = cross(
                image,
                w,
                h,
                x,
                cy.floor().min((h - 1) as f32) as usize,
                false,
                my,
            ) else {
                continue;
            };
            let module = (mx + my) * 0.5;
            if let Some(f) = found.iter_mut().find(|f| {
                (f.x - cx).abs() < module * 2.
                    && (f.y - cy).abs() < module * 2.
                    && f.module / module > 0.4
                    && f.module / module < 2.5
            }) {
                let support_count = f.support as f32;
                f.x = (f.x * support_count + cx) / (support_count + 1.);
                f.y = (f.y * support_count + cy) / (support_count + 1.);
                f.module = (f.module * support_count + module) / (support_count + 1.);
                f.support += 1;
            } else {
                found.push(Finder {
                    x: cx,
                    y: cy,
                    module,
                    support: 1,
                    quad: None,
                });
            }
        }
    }
    found.retain(|f| f.support >= 2);
    found.retain(|f| crate::binarization::has_two_directions(image, w, h, f.x, f.y, f.module * 4.));
    found.sort_by_key(|f| std::cmp::Reverse(f.support));
    let limited = found.len() > 512;
    found.truncate(512);
    for f in &mut found {
        f.quad = central_quad(image, w, h, f);
    }
    found.retain(|f| isolated_finder_valid(image, w, h, f));
    (found, limited)
}
fn isolated_finder_valid(image: &[bool], w: usize, h: usize, finder: &Finder) -> bool {
    let Some(quad) = finder.quad else {
        return true;
    };
    let Some(t) = homography([[-1.5, -1.5], [1.5, -1.5], [1.5, 1.5], [-1.5, 1.5]], quad) else {
        return true;
    };
    // The square pattern is invariant to quarter-turns, so its isolated
    // component supplies orientation without borrowing another finder.
    [0.85, 1., 1.15].into_iter().any(|scale| {
        let mut errors = 0;
        let mut moat_errors = 0;
        let mut center_errors = 0;
        for y in -3i32..=3 {
            for x in -3i32..=3 {
                let [px, py] = map(&t, x as f32 * scale, y as f32 * scale);
                let expected = x.abs().max(y.abs()) != 2;
                if px < 0.
                    || py < 0.
                    || px >= w as f32
                    || py >= h as f32
                    || image[py as usize * w + px as usize] != expected
                {
                    errors += 1;
                    moat_errors += usize::from(x.abs().max(y.abs()) == 2);
                    center_errors += usize::from(x.abs().max(y.abs()) <= 1);
                }
            }
        }
        // Stylized symbols can replace the outer frame with corner brackets.
        // Their solid center and separating white ring still provide evidence.
        errors <= 14 || (moat_errors <= 3 && center_errors <= 2)
    })
}
// Nearby triples bound dense-scene work without the cubic combinations of all
// finders. Prefer compact, approximately orthogonal, similarly sized groups.
fn local_triples(finders: &[Finder]) -> impl Iterator<Item = [usize; 3]> {
    let mut triples = Vec::new();
    for (a, finder) in finders.iter().enumerate() {
        let mut neighbors: Vec<_> = finders
            .iter()
            .enumerate()
            .filter(|(b, other)| *b != a && (0.35..=2.85).contains(&(other.module / finder.module)))
            .map(|(b, other)| {
                (
                    ((other.x - finder.x).powi(2) + (other.y - finder.y).powi(2)),
                    b,
                )
            })
            .collect();
        if neighbors.len() > 12 {
            neighbors.select_nth_unstable_by(12, |a, b| a.0.total_cmp(&b.0));
            neighbors.truncate(12);
        }
        for i in 0..neighbors.len() {
            for j in i + 1..neighbors.len() {
                let mut triple = [a, neighbors[i].1, neighbors[j].1];
                triple.sort_unstable();
                triples.push(triple);
            }
        }
    }
    triples.sort_unstable();
    triples.dedup();
    let local_keys = triples.clone();
    let mut ranked: Vec<_> = triples
        .into_iter()
        .map(|triple| {
            let f = triple.map(|i| &finders[i]);
            let mut d = [
                distance(f[0], f[1]),
                distance(f[0], f[2]),
                distance(f[1], f[2]),
            ];
            d.sort_by(f32::total_cmp);
            let module = f.iter().map(|p| p.module).sum::<f32>() / 3.;
            let spread = f.iter().map(|p| p.module).fold(f32::NEG_INFINITY, f32::max)
                / f.iter().map(|p| p.module).fold(f32::INFINITY, f32::min)
                - 1.;
            let angle_error =
                (d[2].powi(2) - d[0].powi(2) - d[1].powi(2)).abs() / (2. * d[0] * d[1]).max(1.);
            let score = (d[0] + d[1]) * 0.5 / module + angle_error * 30. + spread * 10.;
            (score, triple)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    // Large single symbols can have many accidental finder-like data patches
    // nearer than their true corners. Preserve the strongest global triples.
    let strong = finders.len().min(12);
    let mut out: Vec<_> = (0..strong)
        .flat_map(|a| (a + 1..strong).flat_map(move |b| (b + 1..strong).map(move |c| [a, b, c])))
        .collect();
    out.extend(
        ranked
            .into_iter()
            .map(|(_, t)| t)
            .filter(|t| t[2] >= strong),
    );
    // Preserve the exhaustive fallback order, but do not allocate/generate its
    // remaining triples after the caller reaches its decode-attempt budget.
    let end = if finders.len() <= 80 {
        finders.len()
    } else {
        0
    };
    let fallback = (0..end)
        .flat_map(move |a| (a + 1..end).flat_map(move |b| (b + 1..end).map(move |c| [a, b, c])))
        .filter(move |t| t[2] >= strong && local_keys.binary_search(t).is_err());
    out.into_iter().chain(fallback)
}
// Keep scanline and component centers as separate hypotheses: damaged inner
// squares and distorted scanline crossings fail on different images.
fn component_center(finder: &Finder) -> Option<[f32; 2]> {
    let quad = finder.quad?;
    let first_diagonal = [quad[2][0] - quad[0][0], quad[2][1] - quad[0][1]];
    let second_diagonal = [quad[3][0] - quad[1][0], quad[3][1] - quad[1][1]];
    let determinant =
        first_diagonal[0] * second_diagonal[1] - first_diagonal[1] * second_diagonal[0];
    if determinant.abs() < 1e-6 {
        return None;
    }
    let delta = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let t = (delta[0] * second_diagonal[1] - delta[1] * second_diagonal[0]) / determinant;
    let center = [
        quad[0][0] + t * first_diagonal[0],
        quad[0][1] + t * first_diagonal[1],
    ];
    ((0. ..=1.).contains(&t)
        && (center[0] - finder.x).hypot(center[1] - finder.y) <= finder.module * 1.2)
        .then_some(center)
}
fn component_affine(tl: &Finder, tr: &Finder, bl: &Finder, n: f32) -> Option<[f32; 8]> {
    let tl = component_center(tl)?;
    let tr = component_center(tr)?;
    let bl = component_center(bl)?;
    let ex = [(tr[0] - tl[0]) / (n - 7.), (tr[1] - tl[1]) / (n - 7.)];
    let ey = [(bl[0] - tl[0]) / (n - 7.), (bl[1] - tl[1]) / (n - 7.)];
    Some([
        ex[0],
        ey[0],
        tl[0] - 3.5 * (ex[0] + ey[0]),
        ex[1],
        ey[1],
        tl[1] - 3.5 * (ex[1] + ey[1]),
        0.,
        0.,
    ])
}
/// Native diagnostic only; exposes the actual production finder proposals.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_finders(image: &[bool], w: usize, h: usize) -> serde_json::Value {
    serde_json::Value::Array(
        finders(image, w, h).0
            .iter()
            .map(|f| serde_json::json!({"x":f.x,"y":f.y,"module":f.module,"support":f.support,"quad":f.quad}))
            .collect(),
    )
}
fn central_quad(bits: &[bool], w: usize, h: usize, finder: &Finder) -> Option<[[f32; 2]; 4]> {
    let x = finder.x.floor() as isize;
    let y = finder.y.floor() as isize;
    if x < 0 || y < 0 || x >= w as isize || y >= h as isize || !bits[y as usize * w + x as usize] {
        return None;
    }
    let radius = (finder.module * 3.5).ceil() as isize + 2;
    let left = (x - radius).max(0);
    let top = (y - radius).max(0);
    let right = (x + radius + 1).min(w as isize);
    let bottom = (y + radius + 1).min(h as isize);
    crate::component_geometry::quad(
        bits,
        w,
        [x as usize, y as usize],
        [left as usize, top as usize, right as usize, bottom as usize],
        [4., finder.module * finder.module * 20.],
    )
}
fn least_squares_homography(pairs: &[([f32; 2], [f32; 2])]) -> Option<[f32; 8]> {
    let mut normal = [[0f64; 9]; 8];
    for &(src, dst) in pairs {
        let [x, y] = src.map(f64::from);
        let [u, v] = dst.map(f64::from);
        for row in [
            [x, y, 1., 0., 0., 0., -u * x, -u * y, u],
            [0., 0., 0., x, y, 1., -v * x, -v * y, v],
        ] {
            for i in 0..8 {
                for j in 0..9 {
                    normal[i][j] += row[i] * row[j];
                }
            }
        }
    }
    for col in 0..8 {
        let pivot =
            (col..8).max_by(|&i, &j| normal[i][col].abs().total_cmp(&normal[j][col].abs()))?;
        if normal[pivot][col].abs() < 1e-9 {
            return None;
        }
        normal.swap(pivot, col);
        let d = normal[col][col];
        for value in &mut normal[col][col..] {
            *value /= d;
        }
        let pivot_row = normal[col];
        for (i, row) in normal.iter_mut().enumerate() {
            if i == col {
                continue;
            }
            let d = row[col];
            for (value, &coefficient) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *value -= d * coefficient;
            }
        }
    }
    Some(std::array::from_fn(|i| normal[i][8] as f32))
}
fn finder_homography(tl: &Finder, tr: &Finder, bl: &Finder, n: f32) -> Option<[f32; 8]> {
    let ex = [(tr.x - tl.x) / (n - 7.), (tr.y - tl.y) / (n - 7.)];
    let ey = [(bl.x - tl.x) / (n - 7.), (bl.y - tl.y) / (n - 7.)];
    let corners = [[-1.5, -1.5], [1.5, -1.5], [1.5, 1.5], [-1.5, 1.5]];
    let mut pairs = Vec::new();
    for (f, center) in [(tl, [3.5, 3.5]), (tr, [n - 3.5, 3.5]), (bl, [3.5, n - 3.5])] {
        let q = f.quad?;
        let rotation = (0..4).min_by(|&a, &b| {
            let score = |rotation: usize| {
                corners
                    .iter()
                    .enumerate()
                    .map(|(i, &[x, y])| {
                        let p = q[(rotation + i) % 4];
                        (p[0] - f.x - x * ex[0] - y * ey[0]).powi(2)
                            + (p[1] - f.y - x * ex[1] - y * ey[1]).powi(2)
                    })
                    .sum::<f32>()
            };
            score(a).total_cmp(&score(b))
        })?;
        for (i, [x, y]) in corners.into_iter().enumerate() {
            pairs.push(([center[0] + x, center[1] + y], q[(rotation + i) % 4]));
        }
    }
    least_squares_homography(&pairs)
}
#[must_use]
///
/// # Panics
///
/// During local thresholding, panics if `gray` is shorter than `w * h` or if
/// the dimensions overflow intermediate allocation/index calculations.
pub fn binarize(gray: &[u8], w: usize, h: usize, local: bool) -> Vec<bool> {
    if !local {
        return super::threshold(gray, 0);
    }
    let mut sum = vec![0u64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0;
        for x in 0..w {
            row += u64::from(gray[y * w + x]);
            sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
        }
    }
    let radius = 16;
    let mut out = vec![false; w * h];
    for y in 0..h {
        let top = y.saturating_sub(radius);
        let bottom = (y + radius + 1).min(h);
        for x in 0..w {
            let l = x.saturating_sub(radius);
            let r = (x + radius + 1).min(w);
            let area = (bottom - top) * (r - l);
            let total = sum[bottom * (w + 1) + r] + sum[top * (w + 1) + l]
                - sum[top * (w + 1) + r]
                - sum[bottom * (w + 1) + l];
            out[y * w + x] = (u64::from(gray[y * w + x]) + 5) * (area as u64) < total;
        }
    }
    out
}
fn distance(a: &Finder, b: &Finder) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}
#[must_use]
pub fn homography(src: [[f32; 2]; 4], dst: [[f32; 2]; 4]) -> Option<[f32; 8]> {
    let mut equations = [[0.; 9]; 8];
    for i in 0..4 {
        let [x, y] = src[i];
        let [u, v] = dst[i];
        equations[2 * i] = [x, y, 1., 0., 0., 0., -u * x, -u * y, u];
        equations[2 * i + 1] = [0., 0., 0., x, y, 1., -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&i, &j| equations[i][col].abs().total_cmp(&equations[j][col].abs()))?;
        if equations[pivot][col].abs() < 1e-7 {
            return None;
        }
        equations.swap(pivot, col);
        let d = equations[col][col];
        for value in &mut equations[col][col..] {
            *value /= d;
        }
        let pivot_row = equations[col];
        for (i, row) in equations.iter_mut().enumerate() {
            if i == col {
                continue;
            }
            let d = row[col];
            for (value, &coefficient) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *value -= d * coefficient;
            }
        }
    }
    Some(std::array::from_fn(|i| equations[i][8]))
}
#[must_use]
pub fn map(t: &[f32; 8], x: f32, y: f32) -> [f32; 2] {
    let z = t[6] * x + t[7] * y + 1.;
    [
        (t[0] * x + t[1] * y + t[2]) / z,
        (t[3] * x + t[4] * y + t[5]) / z,
    ]
}
fn alignment(
    image: &[bool],
    w: usize,
    h: usize,
    guess: [f32; 2],
    ex: [f32; 2],
    ey: [f32; 2],
) -> Vec<[f32; 2]> {
    let module = (ex[0].hypot(ex[1]) + ey[0].hypot(ey[1])) * 0.5;
    let range = (module * 8.).ceil() as isize;
    let step = (module * 0.4).round().max(1.) as usize;
    let mut candidates = Vec::new();
    for dy in (-range..=range).step_by(step) {
        'locations: for dx in (-range..=range).step_by(step) {
            let center = [guess[0] + dx as f32, guess[1] + dy as f32];
            let mut e = 0;
            for y in -2i32..=2 {
                for x in -2i32..=2 {
                    let xx = (center[0] + x as f32 * ex[0] + y as f32 * ey[0]).floor() as isize;
                    let yy = (center[1] + x as f32 * ex[1] + y as f32 * ey[1]).floor() as isize;
                    let expect = x.abs().max(y.abs()) != 1;
                    if xx < 0
                        || yy < 0
                        || xx >= w as isize
                        || yy >= h as isize
                        || image[yy as usize * w + xx as usize] != expect
                    {
                        e += 1;
                        if e > 3 {
                            continue 'locations;
                        }
                    }
                }
            }
            if e <= 3 {
                let d = (dx * dx + dy * dy) as f32 / module.powi(2);
                candidates.push((e as f32 * 3. + d * 0.04, center));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<[f32; 2]> = Vec::new();
    for (_, p) in candidates {
        if out
            .iter()
            .all(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > module)
        {
            out.push(p);
        }
        if out.len() == 4 {
            break;
        }
    }
    out
}
#[must_use]
pub fn sample(
    image: &[bool],
    w: usize,
    h: usize,
    n: usize,
    transform: &[f32; 8],
    offset: f32,
) -> Option<Vec<bool>> {
    let mut out = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let p = map(transform, x as f32 + 0.5 + offset, y as f32 + 0.5 + offset);
            let xx = p[0].floor() as isize;
            let yy = p[1].floor() as isize;
            if xx < 0 || yy < 0 || xx >= w as isize || yy >= h as isize {
                return None;
            }
            out.push(image[yy as usize * w + xx as usize]);
        }
    }
    Some(out)
}
fn finder_grid_valid(
    image: &[bool],
    w: usize,
    h: usize,
    center: [f32; 2],
    ex: [f32; 2],
    ey: [f32; 2],
) -> bool {
    let mut errors = 0;
    for y in -3i32..=3 {
        for x in -3i32..=3 {
            let xx = (center[0] + x as f32 * ex[0] + y as f32 * ey[0]).floor() as isize;
            let yy = (center[1] + x as f32 * ex[1] + y as f32 * ey[1]).floor() as isize;
            let expected = x.abs().max(y.abs()) != 2;
            if xx < 0
                || yy < 0
                || xx >= w as isize
                || yy >= h as isize
                || image[yy as usize * w + xx as usize] != expected
            {
                errors += 1;
            }
            if errors > 20 {
                return false;
            }
        }
    }
    true
}
///
/// # Panics
///
/// Panics if a supplied binary image or region dimensions are inconsistent.
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    let mut used: Vec<Finder> = Vec::new();
    let mut prior_count = 0;
    let mut partial_modes: Vec<(usize, Vec<Finder>)> = Vec::new();
    for mode in 0..4 {
        if binary_images.is_duplicate(mode) && results.len() == prior_count {
            continue;
        }
        prior_count = results.len();
        let image = binary_images.get(mode);
        let (mut finders, capped) = finders(image, w, h);
        regions.limited |= capped;
        // Already decoded patterns cannot form another physical QR symbol.
        // Removing them also exposes distant corners of a large remaining code
        // after surrounding small symbols were decoded on an earlier pass.
        finders.retain(|p| {
            !used
                .iter()
                .any(|q| distance(p, q) < p.module.min(q.module) * 2.)
        });
        // Dense scenes skip the exhaustive fallback beyond the local triples.
        regions.limited |= finders.len() > 80;
        for [a, b, c] in local_triples(&finders) {
            let finder_triple = [&finders[a], &finders[b], &finders[c]];
            // A decoded symbol owns its three finder patterns. Reusing those
            // physical patterns creates duplicate or cross-symbol triples.
            if finder_triple.iter().any(|p| {
                used.iter()
                    .any(|q| distance(p, q) < p.module.min(q.module) * 2.)
            }) {
                continue;
            }
            let distances = [
                distance(finder_triple[1], finder_triple[2]),
                distance(finder_triple[0], finder_triple[2]),
                distance(finder_triple[0], finder_triple[1]),
            ];
            let corner = (0..3)
                .max_by(|&i, &j| distances[i].total_cmp(&distances[j]))
                .unwrap();
            let tl = finder_triple[corner];
            let mut tr = finder_triple[(corner + 1) % 3];
            let mut bl = finder_triple[(corner + 2) % 3];
            let cross = (tr.x - tl.x) * (bl.y - tl.y) - (tr.y - tl.y) * (bl.x - tl.x);
            if cross < 0. {
                std::mem::swap(&mut tr, &mut bl);
            }
            let dx = distance(tl, tr);
            let dy = distance(tl, bl);
            if dx / dy < 0.4 || dx / dy > 2.5 {
                continue;
            }
            let dot = ((tr.x - tl.x) * (bl.x - tl.x) + (tr.y - tl.y) * (bl.y - tl.y)) / (dx * dy);
            if dot.abs() > 0.55 {
                continue;
            }
            // Axis-aligned cross sections through a rotated square
            // overestimate module width (sqrt(2) at 45 degrees).
            let rotation_scale = ((tr.x - tl.x).abs().max((tr.y - tl.y).abs()) / dx
                + (bl.x - tl.x).abs().max((bl.y - tl.y).abs()) / dy)
                * 0.5;
            let module = (tl.module + tr.module + bl.module) / 3. * rotation_scale;
            // Finder separation is measured along the symbol axes;
            // use the rotation-corrected module width here too.
            if dx < module * 10. || dy < module * 10. {
                continue;
            }
            let estimate = ((dx + dy) * 0.5 / module + 7. - 17.) / 4.;
            if !(0. ..=42.).contains(&estimate) {
                continue;
            }
            let center = [(tr.x + bl.x) * 0.5, (tr.y + bl.y) * 0.5];
            if results.iter().any(|r| {
                r.polygon
                    .iter()
                    .fold([0., 0.], |a, p| [a[0] + p[0] / 4., a[1] + p[1] / 4.])
                    .iter()
                    .zip(center)
                    .map(|(&x, y)| (x - y).powi(2))
                    .sum::<f32>()
                    < module.powi(2) * 36.
            }) {
                continue;
            }
            let nearest = estimate.round() as isize;
            'versions: for delta in [0, -1, 1, -2, 2] {
                let version = nearest + delta;
                if !(1..=40).contains(&version) {
                    continue;
                }
                let n = 17 + version as usize * 4;
                let nf = n as f32;
                let ex = [(tr.x - tl.x) / (nf - 7.), (tr.y - tl.y) / (nf - 7.)];
                let ey = [(bl.x - tl.x) / (nf - 7.), (bl.y - tl.y) / (nf - 7.)];
                let fitted = finder_homography(tl, tr, bl, nf);
                let affine_valid = finder_triple
                    .iter()
                    .all(|finder| finder_grid_valid(image, w, h, [finder.x, finder.y], ex, ey));
                let fitted_valid = fitted.is_some_and(|t| {
                    [
                        (tl, [3.5, 3.5]),
                        (tr, [nf - 3.5, 3.5]),
                        (bl, [3.5, nf - 3.5]),
                    ]
                    .iter()
                    .all(|(_, [x, y])| {
                        let p = map(&t, *x, *y);
                        let px = map(&t, *x + 1., *y);
                        let py = map(&t, *x, *y + 1.);
                        finder_grid_valid(
                            image,
                            w,
                            h,
                            p,
                            [px[0] - p[0], px[1] - p[1]],
                            [py[0] - p[0], py[1] - p[1]],
                        )
                    })
                });
                let centered = component_affine(tl, tr, bl, nf).filter(|t| {
                    [[3.5, 3.5], [nf - 3.5, 3.5], [3.5, nf - 3.5]]
                        .iter()
                        .all(|&[x, y]| {
                            finder_grid_valid(image, w, h, map(t, x, y), [t[0], t[3]], [t[1], t[4]])
                        })
                });
                if !affine_valid && !fitted_valid && centered.is_none() {
                    continue;
                }
                let br = [tr.x + bl.x - tl.x, tr.y + bl.y - tl.y];
                let Some(affine) = homography(
                    [
                        [3.5, 3.5],
                        [nf - 3.5, 3.5],
                        [nf - 3.5, nf - 3.5],
                        [3.5, nf - 3.5],
                    ],
                    [[tl.x, tl.y], [tr.x, tr.y], br, [bl.x, bl.y]],
                ) else {
                    continue;
                };
                // Most front-facing symbols need only the affine grid.
                // Defer the expensive alignment search until that fails.
                for stage in 0..2 {
                    let mut transforms = Vec::new();
                    if stage == 0 {
                        if affine_valid {
                            transforms.push(affine);
                        }
                        if fitted_valid {
                            if let Some(t) = fitted {
                                transforms.push(t);
                            }
                        }
                    }
                    if stage == 0 {
                        if let Some(t) = centered {
                            transforms.push(t);
                        }
                    }
                    if stage == 1 && version > 1 {
                        let guess = [br[0] - 3. * (ex[0] + ey[0]), br[1] - 3. * (ex[1] + ey[1])];
                        for align in alignment(image, w, h, guess, ex, ey) {
                            if let Some(t) = homography(
                                [
                                    [3.5, 3.5],
                                    [nf - 3.5, 3.5],
                                    [nf - 6.5, nf - 6.5],
                                    [3.5, nf - 3.5],
                                ],
                                [[tl.x, tl.y], [tr.x, tr.y], align, [bl.x, bl.y]],
                            ) {
                                transforms.push(t);
                            }
                        }
                    }
                    for t in transforms {
                        for offset in [0., -0.2, 0.2] {
                            attempts += 1;
                            if attempts > 1200 {
                                partial::recover(
                                    image,
                                    w,
                                    h,
                                    &finders,
                                    &mut results,
                                    &mut used,
                                    regions,
                                );
                                for (previous_mode, previous_finders) in partial_modes {
                                    partial::recover(
                                        binary_images.get(previous_mode),
                                        w,
                                        h,
                                        &previous_finders,
                                        &mut results,
                                        &mut used,
                                        regions,
                                    );
                                }
                                return (results, true);
                            }
                            if !qr::plausible_image_header(n, |x, y| {
                                let p = map(&t, x as f32 + 0.5 + offset, y as f32 + 0.5 + offset);
                                let xx = p[0].floor() as isize;
                                let yy = p[1].floor() as isize;
                                (xx >= 0 && yy >= 0 && xx < w as isize && yy < h as isize)
                                    .then(|| image[yy as usize * w + xx as usize])
                            }) {
                                continue;
                            }
                            let Some(matrix) = sample(image, w, h, n, &t, offset) else {
                                continue;
                            };
                            for mirror in [false, true] {
                                let matrix = if mirror {
                                    (0..n * n).map(|i| matrix[(i % n) * n + i / n]).collect()
                                } else {
                                    matrix.clone()
                                };
                                if let Some(read) = qr::decode_matrix(&matrix, n) {
                                    used.extend(finder_triple.iter().map(|p| (*p).clone()));
                                    results.push(Detection {
                                        bytes: Some(read.bytes),
                                        structured_append: read.structured_append,
                                        reader_initialization: false,
                                        addon: None,
                                        format: "QRCode".into(),
                                        text: read.text,
                                        polygon: [[0., 0.], [nf, 0.], [nf, nf], [0., nf]]
                                            .map(|[x, y]| map(&t, x, y)),
                                        support: tl.support.min(tr.support).min(bl.support),
                                        error: read.corrected as f32,
                                        gs1: read.gs1,
                                    });
                                    break 'versions;
                                }
                                if let Some(score) = qr::localization_score(&matrix, n) {
                                    regions.add(
                                        "QRCode",
                                        [[0., 0.], [nf, 0.], [nf, nf], [0., nf]]
                                            .map(|[x, y]| map(&t, x, y)),
                                        score,
                                        tl.support.min(tr.support).min(bl.support),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        partial_modes.push((mode, finders));
    }
    let mut limited = false;
    for (mode, finders) in partial_modes {
        limited |= partial::recover(
            binary_images.get(mode),
            w,
            h,
            &finders,
            &mut results,
            &mut used,
            regions,
        );
    }
    (results, limited)
}

#[cfg(test)]
mod limit_tests {
    #[test]
    fn finder_limit_is_recorded_before_candidate_validation() {
        for count in [512, 513] {
            let width = 23 * 25;
            let height = 23 * 21;
            let mut bits = vec![false; width * height];
            for i in 0..count {
                let left = (i % 25) * 23 + 4;
                let top = (i / 25) * 23 + 4;
                for y in 0..14 {
                    for x in 0..14 {
                        let a = x / 2;
                        let b = y / 2;
                        bits[(top + y) * width + left + x] = a == 0
                            || a == 6
                            || b == 0
                            || b == 6
                            || ((2..=4).contains(&a) && (2..=4).contains(&b));
                    }
                }
            }
            let (finders, limited) = super::finders(&bits, width, height);
            assert_eq!(finders.len(), 512);
            assert_eq!(limited, count > 512);
        }
    }
}
