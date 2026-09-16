//! Project-owned QR finder grouping, projective grid estimation and sampling.
mod finder_index;
use crate::{qr, Detection};
mod curved;
mod partial;
mod ring;
mod single;
#[derive(Clone, Debug)]
struct Finder {
    x: f32,
    y: f32,
    module: f32,
    support: usize,
    quad: Option<[[f32; 2]; 4]>,
}
fn ratio(r: &[usize]) -> Option<f32> {
    let module = crate::numeric::usize_f32(r.iter().sum::<usize>()) / 7.;
    if module < 0.75 {
        return None;
    }
    for (i, &n) in r.iter().enumerate() {
        if (crate::numeric::usize_f32(n) - module * if i == 2 { 3. } else { 1. }).abs()
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
    let max = crate::numeric::f32_usize((expected * 12.).ceil());
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
    let center = crate::numeric::usize_f32(start + end) * 0.5;
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
#[cfg(not(target_arch = "wasm32"))]
fn finders(image: &[bool], w: usize, h: usize) -> (Vec<Finder>, bool) {
    finders_with_rows(image, w, h, None)
}
fn finders_with_rows(
    image: &[bool],
    w: usize,
    h: usize,
    row_cache: Option<&crate::binarization::RowOffsets>,
) -> (Vec<Finder>, bool) {
    let mut found = finder_index::Index::new(w, h);
    let step = (h / 600).max(1);
    let mut offsets = Vec::new();
    for y in (0..h).step_by(step) {
        let row = &image[y * w..(y + 1) * w];
        if let Some(cache) = row_cache {
            cache.copy_or_extract(y, row, &mut offsets);
        } else {
            crate::transition_offsets_into(row, &mut offsets);
        }
        for i in 0..offsets.len().saturating_sub(5) {
            if !row[offsets[i]] {
                continue;
            }
            let widths: [usize; 5] =
                std::array::from_fn(|run| offsets[i + run + 1] - offsets[i + run]);
            let Some(m) = ratio(&widths) else {
                continue;
            };
            let x = offsets[i] + widths[0] + widths[1] + widths[2] / 2;
            let Some((cy, my)) = cross(image, w, h, x, y, true, m) else {
                continue;
            };
            let Some((cx, mx)) = cross(
                image,
                w,
                h,
                x,
                crate::numeric::f32_usize(cy.floor().min(crate::numeric::usize_f32(h - 1))),
                false,
                my,
            ) else {
                continue;
            };
            let module = (mx + my) * 0.5;
            found.insert(cx, cy, module);
        }
    }
    let mut found = found.into_centers();
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
                let [px, py] = map(
                    &t,
                    crate::numeric::f64_f32(f64::from(x)) * scale,
                    crate::numeric::f64_f32(f64::from(y)) * scale,
                );
                let expected = x.abs().max(y.abs()) != 2;
                if px < 0.
                    || py < 0.
                    || px >= crate::numeric::usize_f32(w)
                    || py >= crate::numeric::usize_f32(h)
                    || image[crate::numeric::f32_usize(py) * w + crate::numeric::f32_usize(px)]
                        != expected
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
    let x = crate::numeric::f32_isize(finder.x.floor());
    let y = crate::numeric::f32_isize(finder.y.floor());
    if x < 0
        || y < 0
        || x >= (w).cast_signed()
        || y >= (h).cast_signed()
        || !bits[(y).cast_unsigned() * w + (x).cast_unsigned()]
    {
        return None;
    }
    let radius = crate::numeric::f32_isize((finder.module * 3.5).ceil()) + 2;
    let left = (x - radius).max(0);
    let top = (y - radius).max(0);
    let right = (x + radius + 1).min((w).cast_signed());
    let bottom = (y + radius + 1).min((h).cast_signed());
    crate::component_geometry::quad(
        bits,
        w,
        [(x).cast_unsigned(), (y).cast_unsigned()],
        [
            (left).cast_unsigned(),
            (top).cast_unsigned(),
            (right).cast_unsigned(),
            (bottom).cast_unsigned(),
        ],
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
    Some(std::array::from_fn(|i| {
        crate::numeric::f64_f32(normal[i][8])
    }))
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
    let radius = 16;
    let mut out = vec![false; w * h];
    // Only the current 33-row window is needed. A full-frame u64
    // integral image costs eight bytes per pixel; column sums cost four per
    // image column, with integer-exact totals bounded by 33 * 33 * 255.
    let mut columns = vec![0_u32; w];
    let mut prefix = vec![0_u32; w + 1];
    let mut top = 0;
    let mut bottom = 0;
    for y in 0..h {
        let next_top = y.saturating_sub(radius);
        let next_bottom = y.saturating_add(radius + 1).min(h);
        while bottom < next_bottom {
            for (column, &pixel) in columns.iter_mut().zip(&gray[bottom * w..(bottom + 1) * w]) {
                *column += u32::from(pixel);
            }
            bottom += 1;
        }
        while top < next_top {
            for (column, &pixel) in columns.iter_mut().zip(&gray[top * w..(top + 1) * w]) {
                *column -= u32::from(pixel);
            }
            top += 1;
        }
        if w >= 33 {
            let rows = u32::try_from(bottom - top).expect("window height is at most 33");
            let input = &gray[y * w..(y + 1) * w];
            let output = &mut out[y * w..(y + 1) * w];
            let mut total = 0_u32;
            for (sum, &column) in prefix[1..].iter_mut().zip(&columns) {
                total = total.wrapping_add(column);
                *sum = total;
            }
            // Prefix subtraction is exact modulo u32 even for very wide images:
            // each local window remains bounded by 33*33*255.
            for x in 0..16 {
                let area = rows * u32::try_from(x + 17).expect("edge window is below 33");
                output[x] = (u32::from(input[x]) + 5) * area < prefix[x + 17];
            }
            let area = rows * 33;
            for x in 16..w - 16 {
                let total = prefix[x + 17].wrapping_sub(prefix[x - 16]);
                output[x] = (u32::from(input[x]) + 5) * area < total;
            }
            for x in w - 16..w {
                let area = rows * u32::try_from(w - x + 16).expect("edge window is below 33");
                let total = prefix[w].wrapping_sub(prefix[x - 16]);
                output[x] = (u32::from(input[x]) + 5) * area < total;
            }
            continue;
        }
        let mut left = 0;
        let mut right = 0;
        let mut total = 0_u32;
        while right < w.min(radius + 1) {
            total += columns[right];
            right += 1;
        }
        for x in 0..w {
            let l = x.saturating_sub(radius);
            let r = x.saturating_add(radius + 1).min(w);
            while left < l {
                total -= columns[left];
                left += 1;
            }
            while right < r {
                total += columns[right];
                right += 1;
            }
            let area = (bottom - top) * (r - l);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "Both clamped window sides are at most 33 pixels; area is at most 1089."
            )]
            let area = area as u32;
            out[y * w + x] = (u32::from(gray[y * w + x]) + 5) * area < total;
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
    let range = crate::numeric::f32_isize((module * 8.).ceil());
    let step = crate::numeric::f32_usize((module * 0.4).round().max(1.));
    let mut candidates = Vec::new();
    for dy in (-range..=range).step_by(step) {
        'locations: for dx in (-range..=range).step_by(step) {
            let center = [
                guess[0] + crate::numeric::isize_f32(dx),
                guess[1] + crate::numeric::isize_f32(dy),
            ];
            let mut e = 0;
            for y in -2i32..=2 {
                for x in -2i32..=2 {
                    let xx = crate::numeric::f32_isize(
                        (center[0]
                            + crate::numeric::f64_f32(f64::from(x)) * ex[0]
                            + crate::numeric::f64_f32(f64::from(y)) * ey[0])
                            .floor(),
                    );
                    let yy = crate::numeric::f32_isize(
                        (center[1]
                            + crate::numeric::f64_f32(f64::from(x)) * ex[1]
                            + crate::numeric::f64_f32(f64::from(y)) * ey[1])
                            .floor(),
                    );
                    let expect = x.abs().max(y.abs()) != 1;
                    if xx < 0
                        || yy < 0
                        || xx >= (w).cast_signed()
                        || yy >= (h).cast_signed()
                        || image[(yy).cast_unsigned() * w + (xx).cast_unsigned()] != expect
                    {
                        e += 1;
                        if e > 3 {
                            continue 'locations;
                        }
                    }
                }
            }
            if e <= 3 {
                let d = crate::numeric::isize_f32(dx * dx + dy * dy) / module.powi(2);
                candidates.push((
                    crate::numeric::f64_f32(f64::from(e)) * 3. + d * 0.04,
                    center,
                ));
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
            let p = map(
                transform,
                crate::numeric::usize_f32(x) + 0.5 + offset,
                crate::numeric::usize_f32(y) + 0.5 + offset,
            );
            let xx = crate::numeric::f32_isize(p[0].floor());
            let yy = crate::numeric::f32_isize(p[1].floor());
            if xx < 0 || yy < 0 || xx >= (w).cast_signed() || yy >= (h).cast_signed() {
                return None;
            }
            out.push(image[(yy).cast_unsigned() * w + (xx).cast_unsigned()]);
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
            let xx = crate::numeric::f32_isize(
                (center[0]
                    + crate::numeric::f64_f32(f64::from(x)) * ex[0]
                    + crate::numeric::f64_f32(f64::from(y)) * ey[0])
                    .floor(),
            );
            let yy = crate::numeric::f32_isize(
                (center[1]
                    + crate::numeric::f64_f32(f64::from(x)) * ex[1]
                    + crate::numeric::f64_f32(f64::from(y)) * ey[1])
                    .floor(),
            );
            let expected = x.abs().max(y.abs()) != 2;
            if xx < 0
                || yy < 0
                || xx >= (w).cast_signed()
                || yy >= (h).cast_signed()
                || image[(yy).cast_unsigned() * w + (xx).cast_unsigned()] != expected
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
    detect_with_recovery(w, h, regions, binary_images, false, false)
}
/// Higher-effort QR search retains the standard passes and adds a foreground threshold.
pub fn detect_extended(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    detect_with_recovery(w, h, regions, binary_images, true, false)
}
/// Very-high-effort QR search also tests bounded timing-guided curved grids.
pub fn detect_curved(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    detect_with_recovery(w, h, regions, binary_images, true, true)
}
#[expect(
    clippy::too_many_lines,
    reason = "QR detection preserves its ordered finder hypotheses and shared work budgets in one transaction."
)]
fn detect_with_recovery(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
    extended: bool,
    curved_recovery: bool,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    let mut curved_attempts = 0;
    let mut curved_limited = false;
    let mut single_attempts = 0;
    let mut single_alignments = 0;
    let mut single_limited = false;
    let mut used: Vec<Finder> = Vec::new();
    let mut prior_count = 0;
    let mut partial_modes: Vec<(usize, Vec<Finder>)> = Vec::new();
    for mode in 0..if extended { 6 } else { 4 } {
        if mode >= 4 && !binary_images.has_qr_contrast() {
            continue;
        }
        if binary_images.is_duplicate(mode) && results.len() == prior_count {
            continue;
        }
        prior_count = results.len();
        let step = (h / 600).max(1);
        let (image, row_cache) = binary_images.get_with_runs(mode, step);
        let (mut finders, capped) = finders_with_rows(image, w, h, Some(row_cache));
        regions.limited |= capped;
        single_limited |= single::recover(
            image,
            w,
            h,
            &finders,
            &mut results,
            &mut used,
            &mut single_attempts,
            &mut single_alignments,
        );
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
            let nearest = crate::numeric::f32_isize(estimate.round());
            'versions: for delta in [0, -1, 1, -2, 2] {
                let version = nearest + delta;
                if !(1..=40).contains(&version) {
                    continue;
                }
                let n = 17 + (version).cast_unsigned() * 4;
                let nf = crate::numeric::usize_f32(n);
                let ex = [(tr.x - tl.x) / (nf - 7.), (tr.y - tl.y) / (nf - 7.)];
                let ey = [(bl.x - tl.x) / (nf - 7.), (bl.y - tl.y) / (nf - 7.)];
                let affine_valid = finder_triple
                    .iter()
                    .all(|finder| finder_grid_valid(image, w, h, [finder.x, finder.y], ex, ey));
                let br = [tr.x + bl.x - tl.x, tr.y + bl.y - tl.y];
                let mut any_valid = affine_valid;
                // Preserve transform and offset order while avoiding unused geometric fits.
                for stage in 0..4 {
                    let mut transforms = Vec::new();
                    if stage == 0 && affine_valid {
                        let Some(affine) = homography(
                            [
                                [3.5, 3.5],
                                [nf - 3.5, 3.5],
                                [nf - 3.5, nf - 3.5],
                                [3.5, nf - 3.5],
                            ],
                            [[tl.x, tl.y], [tr.x, tr.y], br, [bl.x, bl.y]],
                        ) else {
                            continue 'versions;
                        };
                        transforms.push(affine);
                    }
                    if stage == 1 {
                        let fitted = finder_homography(tl, tr, bl, nf);
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
                        any_valid |= fitted_valid;
                        if fitted_valid {
                            if let Some(t) = fitted {
                                transforms.push(t);
                            }
                        }
                    }
                    if stage == 2 {
                        let centered = component_affine(tl, tr, bl, nf).filter(|t| {
                            [[3.5, 3.5], [nf - 3.5, 3.5], [3.5, nf - 3.5]]
                                .iter()
                                .all(|&[x, y]| {
                                    finder_grid_valid(
                                        image,
                                        w,
                                        h,
                                        map(t, x, y),
                                        [t[0], t[3]],
                                        [t[1], t[4]],
                                    )
                                })
                        });
                        any_valid |= centered.is_some();
                        if let Some(t) = centered {
                            transforms.push(t);
                        }
                    }
                    if stage == 3 && !any_valid {
                        break;
                    }
                    if stage == 3 && version > 1 {
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
                        for (offset_index, offset) in [0., -0.2, 0.2].into_iter().enumerate() {
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
                            if curved_recovery
                                && stage == 0
                                && offset_index == 0
                                && n >= 25
                                && curved_attempts >= 24
                            {
                                curved_limited = true;
                            }
                            if curved_recovery
                                && stage == 0
                                && offset_index == 0
                                && n >= 25
                                && curved_attempts < 24
                            {
                                curved_attempts += 1;
                                if let Some(read) = curved::recover(image, w, h, n, &t) {
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
                                        error: crate::numeric::usize_f32(read.corrected),
                                        gs1: read.gs1,
                                    });
                                    break 'versions;
                                }
                            }
                            if !qr::plausible_image_header(n, |x, y| {
                                let p = map(
                                    &t,
                                    crate::numeric::usize_f32(x) + 0.5 + offset,
                                    crate::numeric::usize_f32(y) + 0.5 + offset,
                                );
                                let xx = crate::numeric::f32_isize(p[0].floor());
                                let yy = crate::numeric::f32_isize(p[1].floor());
                                (xx >= 0
                                    && yy >= 0
                                    && xx < (w).cast_signed()
                                    && yy < (h).cast_signed())
                                .then(|| image[(yy).cast_unsigned() * w + (xx).cast_unsigned()])
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
                                        error: crate::numeric::usize_f32(read.corrected),
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
    let mut limited = single_limited || curved_limited;
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
mod rolling_binarize_tests {
    use super::binarize;

    fn integral_reference(gray: &[u8], w: usize, h: usize) -> Vec<bool> {
        let mut sum = vec![0_u64; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = 0_u64;
            for x in 0..w {
                row += u64::from(gray[y * w + x]);
                sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
            }
        }
        let mut out = vec![false; w * h];
        for y in 0..h {
            let top = y.saturating_sub(16);
            let bottom = y.saturating_add(17).min(h);
            for x in 0..w {
                let left = x.saturating_sub(16);
                let right = x.saturating_add(17).min(w);
                let area = (bottom - top) * (right - left);
                let total = sum[bottom * (w + 1) + right] + sum[top * (w + 1) + left]
                    - sum[top * (w + 1) + right]
                    - sum[bottom * (w + 1) + left];
                out[y * w + x] = (u64::from(gray[y * w + x]) + 5) * (area as u64) < total;
            }
        }
        out
    }

    #[test]
    fn rolling_matches_integral_reference_at_edges_and_small_sizes() {
        let dimensions = [
            (1, 1),
            (1, 40),
            (40, 1),
            (2, 31),
            (31, 2),
            (17, 33),
            (33, 17),
            (64, 47),
            (257, 193),
            (1024, 513),
        ];
        let mut seed = 0x9e37_79b9_u32;
        for &(w, h) in &dimensions {
            let mut gray = vec![0_u8; w * h];
            for value in &mut gray {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *value = (seed >> 24) as u8;
            }
            assert_eq!(binarize(&gray, w, h, true), integral_reference(&gray, w, h));
        }
    }

    #[test]
    fn rolling_matches_uniform_extremes() {
        for &(w, h, value) in &[(1, 80, 0_u8), (80, 1, 255), (64, 64, 127), (64, 64, 255)] {
            let gray = vec![value; w * h];
            assert_eq!(binarize(&gray, w, h, true), integral_reference(&gray, w, h));
        }
    }
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
