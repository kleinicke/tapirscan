//! Project-owned QR finder grouping, projective grid estimation and sampling.
//! Mode-dependent choices arrive as a [`Policy`]: this crate has no mode features.
mod alignment_cached;
mod curved;
mod finder_index;
use crate::{Detection, qr};
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
/// How far a window that passed [`ratio`] is from 1:1:3:1:1, as the largest run
/// deviation in 1/1024 of its tolerance. Five equal runs score about 900.
fn ratio_error(r: &[usize], module: f32) -> usize {
    let worst = r.iter().enumerate().fold(0f32, |worst, (i, &n)| {
        let (expected, tolerance) = if i == 2 { (3., 1.8) } else { (1., 0.85) };
        worst.max((crate::numeric::usize_f32(n) - module * expected).abs() / (module * tolerance))
    });
    crate::numeric::f32_usize((worst * 1024.).round())
}
/// Row step of the finder scans.
fn row_step(w: usize, h: usize) -> usize {
    crate::finder_budget::row_step(w, h, 600)
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
        crate::count_finder_work(1);
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
/// Finder candidates verified per threshold image, by support.
const MAX_FINDERS: usize = 512;
/// Scanned row pixels per finder cross check. Real images need at most one check
/// per 50 pixels; checkerboards and stripes pass the row ratio test at nearly every
/// run.
const PIXELS_PER_CHECK: usize = 16;
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
    let step = row_step(w, h);
    let mut offsets = Vec::new();
    let mut windows = Vec::new();
    let mut keys = Vec::new();
    let mut starts = Vec::new();
    let mut plans = crate::finder_budget::Rows::default();
    // Cross checks are budgeted per scanned pixel. Half of the budget is available
    // from the start and each row adds its share of the other half; unused checks
    // carry over, so a textured band cannot exhaust the rows below it.
    let rows = h.div_ceil(step);
    let mut available = w * rows;
    let mut cut = false;
    for y in (0..h).step_by(step) {
        available += w;
        let row = &image[y * w..(y + 1) * w];
        if let Some(cache) = row_cache {
            cache.copy_or_extract(y, row, &mut offsets);
        } else {
            crate::transition_offsets_into(row, &mut offsets);
        }
        // A short row skips windows inside periodic textures (equal runs pass the
        // ratio test) and checks the windows closest to 1:1:3:1:1.
        let allowed = available / (2 * PIXELS_PER_CHECK);
        let (first_dark, end) = (usize::from(!row[0]), offsets.len().saturating_sub(5));
        let plan = plans.starts(&mut starts, &offsets, first_dark, end, (0, 4), allowed, y);
        let short = plan.is_some();
        cut |= plan == Some(true);
        windows.clear();
        crate::finder_budget::for_each_start(plan, &starts, first_dark, end, |i| {
            let widths: [usize; 5] =
                std::array::from_fn(|run| offsets[i + run + 1] - offsets[i + run]);
            if let Some(m) = ratio(&widths) {
                windows.push((i, m));
            }
        });
        if windows.len() > allowed {
            cut = true;
            let phase = crate::finder_budget::phase(y);
            crate::finder_budget::thin(&mut windows, allowed, phase, &mut keys, |&(i, m)| {
                let widths: [usize; 5] =
                    std::array::from_fn(|run| offsets[i + run + 1] - offsets[i + run]);
                ratio_error(&widths, m)
            });
        }
        // A short row spends its whole allowance, also what skipped texture windows
        // leave over: banked, it would pay for checking the texture of later rows.
        available -= if short { allowed } else { windows.len() } * 2 * PIXELS_PER_CHECK;
        for &(i, m) in &windows {
            let x = offsets[i + 2] + (offsets[i + 3] - offsets[i + 2]) / 2;
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
            let module = f32::midpoint(mx, my);
            found.insert(cx, cy, module);
        }
    }
    let mut found = found.into_centers();
    found.retain(|f| f.support >= 2);
    found.sort_by_key(|f| std::cmp::Reverse(f.support));
    // Check directions in support order and stop after one finder more than are
    // kept: periodic textures produce tens of thousands of candidates, and beyond
    // that count only `limited` depends on them.
    let mut verified = 0;
    found.retain(|f| {
        if verified > MAX_FINDERS {
            return false;
        }
        let kept = crate::binarization::has_two_directions(image, w, h, f.x, f.y, f.module * 4.);
        verified += usize::from(kept);
        kept
    });
    let limited = cut || found.len() > MAX_FINDERS;
    found.truncate(MAX_FINDERS);
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
/// Resample module centers in gray and threshold each module against the mean of
/// its 7x7 module neighborhood, which follows shadows across the symbol; then
/// decode. With `erasures`, also retry with the lowest-margin modules as erasures.
fn gray_decode(
    gray: &[u8],
    [width, height]: [usize; 2],
    n: usize,
    transform: &[f32; 8],
    hard: &[bool],
    erasures: bool,
) -> Option<qr::Payload> {
    let mut values = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let point = map(
                transform,
                crate::numeric::usize_f32(x) + 0.5,
                crate::numeric::usize_f32(y) + 0.5,
            );
            let xx = crate::numeric::f32_isize(point[0].floor());
            let yy = crate::numeric::f32_isize(point[1].floor());
            if xx < 0 || yy < 0 || xx >= width.cast_signed() || yy >= height.cast_signed() {
                return None;
            }
            values.push(i32::from(
                gray[yy.cast_unsigned() * width + xx.cast_unsigned()],
            ));
        }
    }
    // Module-space integral image for the neighborhood means.
    let mut sum = vec![0_i32; (n + 1) * (n + 1)];
    for y in 0..n {
        for x in 0..n {
            sum[(y + 1) * (n + 1) + x + 1] =
                values[y * n + x] + sum[y * (n + 1) + x + 1] + sum[(y + 1) * (n + 1) + x]
                    - sum[y * (n + 1) + x];
        }
    }
    let radius = 3;
    let mut bits = vec![false; n * n];
    let mut margins = vec![0_i32; n * n];
    for y in 0..n {
        for x in 0..n {
            let (x0, y0, x1, y1) = (
                x.saturating_sub(radius),
                y.saturating_sub(radius),
                (x + radius + 1).min(n),
                (y + radius + 1).min(n),
            );
            let area = i32::try_from((x1 - x0) * (y1 - y0)).ok()?;
            let total = sum[y1 * (n + 1) + x1] - sum[y0 * (n + 1) + x1] - sum[y1 * (n + 1) + x0]
                + sum[y0 * (n + 1) + x0];
            let value = values[y * n + x] * area;
            bits[y * n + x] = value < total;
            margins[y * n + x] = (value - total).abs() / area;
        }
    }
    // Match the polarity of the threshold image the grid was found in.
    if bits.iter().zip(hard).filter(|(a, b)| a == b).count() * 2 < n * n {
        for bit in &mut bits {
            *bit = !*bit;
        }
    }
    // Optionally retry with the weakest twelfth of the modules as erasures.
    let uncertain = erasures.then(|| {
        let mut order: Vec<usize> = (0..n * n).collect();
        let weakest = n * n / 12;
        order.select_nth_unstable_by_key(weakest, |&i| (margins[i], i));
        let mut uncertain = vec![false; n * n];
        for &i in &order[..weakest] {
            uncertain[i] = true;
        }
        uncertain
    });
    for mirror in [false, true] {
        let transpose = |v: &[bool]| -> Vec<bool> {
            if mirror {
                (0..n * n).map(|i| v[(i % n) * n + i / n]).collect()
            } else {
                v.to_vec()
            }
        };
        let matrix = transpose(&bits);
        if let Some(read) = qr::decode_matrix(&matrix, n) {
            return Some(read);
        }
        if let Some(uncertain) = &uncertain
            && let Some(read) = qr::decode_matrix_soft(&matrix, n, &transpose(uncertain))
        {
            return Some(read);
        }
    }
    None
}
/// Lower is more plausible: distance in modules, right-angle error and module spread.
fn triple_score(finders: &[Finder], triple: [usize; 3]) -> f32 {
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
    // Shape dominates: a large symbol's true triple spans 100+ modules and must
    // not rank behind compact accidental triples.
    f32::midpoint(d[0], d[1]) / module * 0.1 + angle_error * 30. + spread * 10.
}
fn local_triples(finders: &[Finder]) -> impl Iterator<Item = [usize; 3]> + use<> {
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
    // Large single symbols can have many accidental finder-like data patches
    // nearer than their true corners, so the strongest finders' global triples
    // join the local ones. All are tried in order of geometric plausibility,
    // which puts the triples of further symbols before accidental ones.
    let strong = finders.len().min(12);
    let mut ranked: Vec<_> = (0..strong)
        .flat_map(|a| (a + 1..strong).flat_map(move |b| (b + 1..strong).map(move |c| [a, b, c])))
        .chain(triples.into_iter().filter(|t| t[2] >= strong))
        .map(|triple| (triple_score(finders, triple), triple))
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let out: Vec<_> = ranked.into_iter().map(|(_, t)| t).collect();
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
/// Block thresholds in the style of the `ZXing` hybrid binarizer: 8-pixel blocks,
/// flat blocks inherit their neighbors' black point, and each block uses the mean
/// black point of its 5x5 block neighborhood.
///
/// # Panics
///
/// Panics if `gray` is shorter than `w * h`.
#[must_use]
pub fn binarize_hybrid(gray: &[u8], w: usize, h: usize) -> Vec<bool> {
    const BLOCK: usize = 8;
    let (columns, rows) = (w.div_ceil(BLOCK), h.div_ceil(BLOCK));
    if columns < 5 || rows < 5 {
        return binarize(gray, w, h, true);
    }
    let offset = |block: usize, limit: usize| (block * BLOCK).min(limit - BLOCK);
    let mut black = vec![0_u32; columns * rows];
    for by in 0..rows {
        let y0 = offset(by, h);
        for bx in 0..columns {
            let x0 = offset(bx, w);
            let (mut sum, mut min, mut max) = (0_u32, 255_u8, 0_u8);
            for y in y0..y0 + BLOCK {
                for &pixel in &gray[y * w + x0..y * w + x0 + BLOCK] {
                    sum += u32::from(pixel);
                    min = min.min(pixel);
                    max = max.max(pixel);
                }
            }
            let mut average = sum >> 6;
            if max - min <= 24 {
                average = u32::from(min) / 2;
                if by > 0 && bx > 0 {
                    let neighbors = (black[(by - 1) * columns + bx]
                        + 2 * black[by * columns + bx - 1]
                        + black[(by - 1) * columns + bx - 1])
                        / 4;
                    if u32::from(min) < neighbors {
                        average = neighbors;
                    }
                }
            }
            black[by * columns + bx] = average;
        }
    }
    let mut out = vec![false; w * h];
    for by in 0..rows {
        let y0 = offset(by, h);
        let top = by.clamp(2, rows - 3);
        for bx in 0..columns {
            let x0 = offset(bx, w);
            let left = bx.clamp(2, columns - 3);
            let mut sum = 0;
            for ny in top - 2..=top + 2 {
                for nx in left - 2..=left + 2 {
                    sum += black[ny * columns + nx];
                }
            }
            let threshold = sum / 25;
            for y in y0..y0 + BLOCK {
                for x in x0..x0 + BLOCK {
                    out[y * w + x] = u32::from(gray[y * w + x]) <= threshold;
                }
            }
        }
    }
    out
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
    alignment_cached::search(image, w, h, guess, ex, ey)
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
    // The error count does not depend on the cell order, so the cells run column
    // by column with each column's base position computed once. Every position
    // is `(center + x * ex) + y * ey`.
    for x in -3i32..=3 {
        let fx = crate::numeric::f64_f32(f64::from(x));
        let base = [center[0] + fx * ex[0], center[1] + fx * ex[1]];
        for y in -3i32..=3 {
            let fy = crate::numeric::f64_f32(f64::from(y));
            let xx = crate::numeric::f32_isize((base[0] + fy * ey[0]).floor());
            let yy = crate::numeric::f32_isize((base[1] + fy * ey[1]).floor());
            let expected = x.abs().max(y.abs()) != 2;
            if xx < 0
                || yy < 0
                || xx >= (w).cast_signed()
                || yy >= (h).cast_signed()
                || image[(yy).cast_unsigned() * w + (xx).cast_unsigned()] != expected
            {
                errors += 1;
                if errors > 20 {
                    return false;
                }
            }
        }
    }
    true
}
/// Decode attempts of a complete detection; reaching it starts partial recovery.
pub const FULL_BUDGET: usize = 1200;
/// Mode-dependent work limits and decoding retries of a QR detection. Every QR
/// reader, alone or in a multi-format group, uses [`Policy::for_mode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Once a symbol is read, a threshold mode stops trying triples after this
    /// many decode attempts without a further read. Further symbols' triples rank
    /// early, so this skips the long tail of accidental triples; images without
    /// any read keep the full attempt budget.
    pub patience: usize,
    /// Alignment searches per detection pass. Plausible but false triples on
    /// textured frames reach this costly stage.
    pub alignment_budget: usize,
    /// Search for an alignment pattern whatever the format bits look like.
    /// Otherwise search only where an affine fit from the finders reads intact
    /// format information.
    pub align_anyway: bool,
    /// Retry the majority of the offset samplings with the modules on which they
    /// disagree as erasures.
    pub offset_erasures: bool,
    /// The gray resampling retry after the threshold samplings.
    pub gray: GrayRetry,
}
/// Resampling module centers in gray with module-space thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrayRetry {
    /// No gray resampling.
    Off,
    /// Decode the gray resampling.
    Hard,
    /// Also retry the gray resampling with its weakest modules as erasures.
    Erasures,
}
impl Policy {
    /// The policy of scanner `mode` (0 Low, 1 Medium, 2 High, 3 Very High).
    #[must_use]
    pub const fn for_mode(mode: usize) -> Self {
        Self {
            // Low serves live camera frames, where the next frame is another chance.
            patience: if mode == 0 { 50 } else { 100 },
            // Low and Medium bound the alignment searches per pass; High keeps searching.
            alignment_budget: match mode {
                0 => 150,
                1 => 25,
                _ => usize::MAX,
            },
            // Low checks the format bits first: false triples would otherwise use up
            // its budget. Medium's budget is small enough that the check would save
            // nothing.
            align_anyway: mode != 0,
            offset_erasures: true,
            // Low keeps its live-frame budget without the gray resampling pass. In
            // Medium its erasure retry read nothing extra.
            gray: match mode {
                0 => GrayRetry::Off,
                1 => GrayRetry::Hard,
                _ => GrayRetry::Erasures,
            },
        }
    }
}
/// The threshold modes and budgets of one detection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pass {
    /// Threshold modes of [`crate::binarization::Images`], in search order.
    pub modes: std::ops::Range<usize>,
    /// Also test bounded timing-guided curved grids (Very High effort).
    pub curved: bool,
    /// Decode attempts. A budget below [`FULL_BUDGET`] ends the pass without
    /// partial recovery or a work-limited flag.
    pub budget: usize,
}
impl Pass {
    /// The standard threshold modes; `extended` adds the foreground split and
    /// `curved` the curved grids.
    #[must_use]
    pub fn effort(extended: bool, curved: bool) -> Self {
        Self {
            modes: 0..if extended { 6 } else { 4 },
            curved,
            budget: FULL_BUDGET,
        }
    }
}
/// Detect QR symbols in the threshold modes of `pass` under `policy`.
///
/// # Panics
///
/// Panics if a supplied binary image or region dimensions are inconsistent.
pub fn detect_with(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
    policy: Policy,
    pass: Pass,
) -> (Vec<Detection>, bool) {
    detect_with_recovery(w, h, regions, binary_images, policy, pass)
}
/// The QR reader of the multi-format groups under `policy`: effort 2 adds the
/// foreground split and effort 3 the curved grids. Above
/// effort 1, frames up to one megapixel are searched again after sharpening;
/// its reads join unless the same payload already covers that area.
///
/// # Panics
///
/// Panics if a supplied binary image or region dimensions are inconsistent.
pub fn detect_group(
    image: &[u8],
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
    policy: Policy,
    effort: usize,
) -> (Vec<Detection>, bool) {
    let pass = Pass::effort(effort > 1, effort > 2);
    let (mut reads, mut limited) =
        detect_with_recovery(w, h, regions, binary_images, policy, pass.clone());
    if effort > 1 && w * h <= 1_048_576 {
        let enhanced = crate::qr_enhance::sharpen(image, w, h);
        let mut recovery = crate::binarization::Images::new(&enhanced, w, h);
        let (extra, extra_limited) =
            detect_with_recovery(w, h, regions, &mut recovery, policy, pass);
        limited |= extra_limited;
        for read in extra {
            if !reads.iter().any(|prior| {
                prior.text == read.text
                    && prior.structured_append == read.structured_append
                    && crate::regions::overlap(&prior.polygon, &read.polygon) >= 0.65
            }) {
                reads.push(read);
            }
        }
    }
    (reads, limited)
}
/// Sampling offsets, in modules, of the three grids tried for every transform.
const GRID_OFFSETS: [f32; 3] = [0., -0.2, 0.2];
/// Whether any of the transforms reads intact format information.
fn format_intact_under(
    image: &[bool],
    [w, h]: [usize; 2],
    n: usize,
    transforms: &[[f32; 8]],
) -> bool {
    transforms.iter().any(|t| {
        GRID_OFFSETS.into_iter().any(|offset| {
            let read = |x: usize, y: usize| {
                let p = map(
                    t,
                    crate::numeric::usize_f32(x) + 0.5 + offset,
                    crate::numeric::usize_f32(y) + 0.5 + offset,
                );
                let xx = crate::numeric::f32_isize(p[0].floor());
                let yy = crate::numeric::f32_isize(p[1].floor());
                (xx >= 0 && yy >= 0 && xx < w.cast_signed() && yy < h.cast_signed())
                    .then(|| image[yy.cast_unsigned() * w + xx.cast_unsigned()])
            };
            qr::format_intact(n, read)
        })
    })
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
    policy: Policy,
    Pass {
        modes,
        curved: curved_recovery,
        budget,
    }: Pass,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    let mut align_calls = 0_usize;
    let mut curved_attempts = 0;
    let mut curved_limited = false;
    let mut single_attempts = 0;
    let mut single_alignments = 0;
    let mut single_limited = false;
    let mut used: Vec<Finder> = Vec::new();
    let mut prior_count = 0;
    let mut partial_modes: Vec<(usize, Vec<Finder>)> = Vec::new();
    // The offset samplings are kept only for the retries that use them.
    let keep_samples = policy.offset_erasures || policy.gray != GrayRetry::Off;
    for mode in modes {
        let mut read_count = results.len();
        let mut last_read = attempts;
        if (4..6).contains(&mode) && !binary_images.has_qr_contrast() {
            continue;
        }
        if mode >= 6 && !binary_images.has_qr_upper_contrast() {
            continue;
        }
        if binary_images.is_duplicate(mode) && results.len() == prior_count {
            continue;
        }
        prior_count = results.len();
        let step = row_step(w, h);
        let gray = binary_images.gray();
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
            if results.len() > read_count {
                read_count = results.len();
                last_read = attempts;
            }
            if read_count > 0 && attempts - last_read > policy.patience {
                // The remaining triples were skipped: report limited work.
                regions.limited = true;
                break;
            }
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
            let rotation_scale = f32::midpoint(
                (tr.x - tl.x).abs().max((tr.y - tl.y).abs()) / dx,
                (bl.x - tl.x).abs().max((bl.y - tl.y).abs()) / dy,
            );
            let module = (tl.module + tr.module + bl.module) / 3. * rotation_scale;
            // Finder separation is measured along the symbol axes;
            // use the rotation-corrected module width here too.
            if dx < module * 10. || dy < module * 10. {
                continue;
            }
            let estimate = (f32::midpoint(dx, dy) / module + 7. - 17.) / 4.;
            if !(0. ..=42.).contains(&estimate) {
                continue;
            }
            let center = [f32::midpoint(tr.x, bl.x), f32::midpoint(tr.y, bl.y)];
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
                let mut early: Vec<[f32; 8]> = Vec::new();
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
                        if fitted_valid && let Some(t) = fitted {
                            transforms.push(t);
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
                    if stage < 3 {
                        early.extend_from_slice(&transforms);
                    }
                    if stage == 3
                        && version > 1
                        && align_calls < policy.alignment_budget
                        && (policy.align_anyway || format_intact_under(image, [w, h], n, &early))
                    {
                        align_calls += 1;
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
                        let mut samples: Vec<Vec<bool>> = Vec::new();
                        for (offset_index, offset) in GRID_OFFSETS.into_iter().enumerate() {
                            attempts += 1;
                            if attempts > budget && budget < FULL_BUDGET {
                                // A bounded extra pass stops quietly.
                                return (results, false);
                            }
                            if attempts > budget {
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
                            if keep_samples {
                                samples.push(matrix.clone());
                            }
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
                        // Modules on which the offset samplings disagree are
                        // uncertain; their codewords become erasures.
                        if policy.offset_erasures && samples.len() >= 2 {
                            let majority: Vec<bool> = (0..n * n)
                                .map(|i| {
                                    samples.iter().filter(|m| m[i]).count() * 2 > samples.len()
                                })
                                .collect();
                            let uncertain: Vec<bool> = (0..n * n)
                                .map(|i| samples.iter().any(|m| m[i] != samples[0][i]))
                                .collect();
                            // Without uncertain modules this repeats the hard decode.
                            let mirrors: &[bool] = if uncertain.contains(&true) {
                                &[false, true]
                            } else {
                                &[]
                            };
                            for &mirror in mirrors {
                                let transpose = |v: &[bool]| -> Vec<bool> {
                                    if mirror {
                                        (0..n * n).map(|i| v[(i % n) * n + i / n]).collect()
                                    } else {
                                        v.to_vec()
                                    }
                                };
                                if let Some(read) = qr::decode_matrix_soft(
                                    &transpose(&majority),
                                    n,
                                    &transpose(&uncertain),
                                ) {
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
                        }
                        if policy.gray != GrayRetry::Off
                            && !samples.is_empty()
                            && let Some(read) = gray_decode(
                                gray,
                                [w, h],
                                n,
                                &t,
                                &samples[0],
                                policy.gray == GrayRetry::Erasures,
                            )
                        {
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
mod gray_decode_tests {
    use super::gray_decode;

    #[test]
    fn module_space_thresholds_read_a_symbol_under_an_illumination_gradient() {
        // Version 1-M "TAPIRSCAN" under a strong illumination gradient.
        let rows = [
            "111111101101001111111",
            "100000101100001000001",
            "101110100011001011101",
            "101110101110101011101",
            "101110100001001011101",
            "100000100111101000001",
            "111111101010101111111",
            "000000001111100000000",
            "101101110111101001011",
            "000001001001111100011",
            "011110110111000001001",
            "100011010001001011111",
            "001100100010100100000",
            "000000001101001010111",
            "111111101111100101101",
            "100000101010000110100",
            "101110100010111000111",
            "101110101101001101010",
            "101110101000101100000",
            "100000100110010100100",
            "111111101000010011010",
        ];
        let n = 21;
        let bits: Vec<bool> = rows
            .iter()
            .flat_map(|r| r.bytes().map(|b| b == b'1'))
            .collect();
        let (scale, margin) = (6, 4);
        let w = (n + 2 * margin) * scale;
        let gray: Vec<u8> = (0..w * w)
            .map(|i| {
                let (x, y) = (i % w / scale, i / w / scale);
                let dark = x >= margin
                    && y >= margin
                    && x < n + margin
                    && y < n + margin
                    && bits[(y - margin) * n + x - margin];
                // Illumination falls from full on the right to a fifth on the left.
                let value: usize = if dark { 40 } else { 220 };
                let light = 20 + 80 * (i % w) / w;
                u8::try_from(value * light / 100).unwrap()
            })
            .collect();
        let offset = crate::numeric::usize_f32(margin * scale);
        let step = crate::numeric::usize_f32(scale);
        let t = [step, 0., offset, 0., step, offset, 0., 0.];
        // A single global cut turns the dim light modules dark.
        let global: Vec<bool> = (0..n * n)
            .map(|i| gray[((i / n + margin) * scale + 3) * w + (i % n + margin) * scale + 3] < 128)
            .collect();
        assert!(crate::qr::decode_matrix(&global, n).is_none());
        let read =
            gray_decode(&gray, [w, w], n, &t, &bits, false).expect("gray decode reads the symbol");
        assert_eq!(read.text, "TAPIRSCAN");
        // A light-on-dark symbol: the inverted threshold image marks its light
        // modules, and the polarity check follows that.
        let inverted_gray: Vec<u8> = gray.iter().map(|v| 255 - v).collect();
        assert_eq!(
            gray_decode(&inverted_gray, [w, w], n, &t, &bits, false).map(|r| r.text),
            Some("TAPIRSCAN".into())
        );
    }
}

#[cfg(test)]
mod rolling_binarize_tests {
    use super::{binarize, binarize_hybrid};

    /// Per-pixel restatement: a pixel belongs to the last block covering it.
    fn hybrid_reference(gray: &[u8], w: usize, h: usize) -> Vec<bool> {
        let (columns, rows) = (w.div_ceil(8), h.div_ceil(8));
        let start = |block: usize, limit: usize| (block * 8).min(limit - 8);
        let block_of = |v: usize, count: usize, limit: usize| {
            (0..count).rev().find(|&b| start(b, limit) <= v).unwrap()
        };
        let mut black = vec![vec![0_u32; columns]; rows];
        for by in 0..rows {
            for bx in 0..columns {
                let pixels: Vec<u8> = (0..64)
                    .map(|i| gray[(start(by, h) + i / 8) * w + start(bx, w) + i % 8])
                    .collect();
                let (min, max) = (*pixels.iter().min().unwrap(), *pixels.iter().max().unwrap());
                let mut value = pixels.iter().map(|&p| u32::from(p)).sum::<u32>() / 64;
                if max - min <= 24 {
                    value = u32::from(min) / 2;
                    if by > 0 && bx > 0 {
                        let neighbors =
                            (black[by - 1][bx] + 2 * black[by][bx - 1] + black[by - 1][bx - 1]) / 4;
                        if u32::from(min) < neighbors {
                            value = neighbors;
                        }
                    }
                }
                black[by][bx] = value;
            }
        }
        (0..w * h)
            .map(|i| {
                let (bx, by) = (block_of(i % w, columns, w), block_of(i / w, rows, h));
                let (left, top) = (bx.clamp(2, columns - 3), by.clamp(2, rows - 3));
                let sum: u32 = (top - 2..=top + 2)
                    .flat_map(|y| (left - 2..=left + 2).map(move |x| (x, y)))
                    .map(|(x, y)| black[y][x])
                    .sum();
                u32::from(gray[i]) <= sum / 25
            })
            .collect()
    }

    #[test]
    fn hybrid_thresholds_match_the_per_pixel_reference() {
        let mut state = 0x2545_f491_u32;
        for (w, h) in [(40, 40), (41, 33), (39, 47), (57, 40), (64, 72), (101, 53)] {
            let gray: Vec<u8> = (0..w * h)
                .map(|i| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    // Flat patches and noisy patches exercise both block rules.
                    let flat = (i % w / 9 + i / w / 7) % 3 == 0;
                    if flat {
                        200
                    } else {
                        u8::try_from(state >> 24).unwrap()
                    }
                })
                .collect();
            assert_eq!(
                binarize_hybrid(&gray, w, h),
                hybrid_reference(&gray, w, h),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn hybrid_thresholds_follow_a_shadow_edge() {
        // 5-pixel checker squares, so blocks contain module edges as in real
        // symbols; the left half lies in shadow, where dark is
        // 20 and light is 90, while the right half has 110 and 230. One global
        // cut cannot separate both halves.
        let (w, h) = (192, 96);
        let gray: Vec<u8> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let dark = (x / 5 + y / 5) % 2 == 0;
                match (x < w / 2, dark) {
                    (true, true) => 20,
                    (true, false) => 90,
                    (false, true) => 110,
                    (false, false) => 230,
                }
            })
            .collect();
        let bits = binarize_hybrid(&gray, w, h);
        for (i, &bit) in bits.iter().enumerate() {
            let (x, y) = (i % w, i / w);
            // Block neighborhoods span 40 pixels; next to the edge they mix both sides.
            if x.abs_diff(w / 2) < 24 {
                continue;
            }
            assert_eq!(bit, (x / 5 + y / 5) % 2 == 0, "pixel {x},{y}");
        }
        assert!(
            binarize(&gray, w, h, false)
                .iter()
                .enumerate()
                .any(|(i, &bit)| { bit != ((i % w / 24 + i / w / 24) % 2 == 0) })
        );
        // Too small for 5x5 blocks: the local mean decides.
        let small = vec![128; 30 * 30];
        assert_eq!(
            binarize_hybrid(&small, 30, 30),
            binarize(&small, 30, 30, true)
        );
    }

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

#[cfg(test)]
fn alignment_reference(
    image: &[bool],
    w: usize,
    h: usize,
    guess: [f32; 2],
    ex: [f32; 2],
    ey: [f32; 2],
) -> Vec<[f32; 2]> {
    let module = f32::midpoint(ex[0].hypot(ex[1]), ey[0].hypot(ey[1]));
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
#[cfg(test)]
mod alignment_cache_tests {
    #[test]
    fn sampling_tables_preserve_exact_ranked_centers() {
        let mut state = 0x1357_9bdf_u32;
        for kind in 0..240 {
            let bits: Vec<_> = (0..33 * 29)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    match kind {
                        0 => false,
                        1 => true,
                        _ => state & 1 != 0,
                    }
                })
                .collect();
            for (guess, ex, ey) in [
                ([16.5, 14.5], [1., 0.], [0., 1.]),
                ([2.5, 3.1], [2.1, 0.3], [-0.2, 1.7]),
                ([31.2, 27.8], [3.2, -0.5], [0.4, 2.8]),
            ] {
                assert_eq!(
                    super::alignment(&bits, 33, 29, guess, ex, ey),
                    super::alignment_reference(&bits, 33, 29, guess, ex, ey)
                );
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "Test arithmetic on small pixel coordinates."
)]
mod finder_grid_tests {
    use super::finder_grid_valid;

    /// Cell-by-cell reference for the column-wise check.
    fn reference(
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
                let (fx, fy) = (x as f32, y as f32);
                let xx = (center[0] + fx * ex[0] + fy * ey[0]).floor() as isize;
                let yy = (center[1] + fx * ex[1] + fy * ey[1]).floor() as isize;
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

    #[test]
    fn column_order_keeps_every_decision() {
        let (w, h) = (48, 40);
        let mut state = 0x2545_f491_u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1_u32 << 24) as f32
        };
        let mut valid = 0;
        for round in 0..6000 {
            // Mostly noise; every third image is a finder pattern at a random pose.
            let mut image: Vec<bool> = (0..w * h).map(|_| next() < 0.5).collect();
            let angle = next() * std::f32::consts::TAU;
            let module = 1. + next() * 2.;
            let ex = [angle.cos() * module, angle.sin() * module];
            let ey = [-ex[1] * (0.8 + next() * 0.4), ex[0] * (0.8 + next() * 0.4)];
            let center = [10. + next() * 28., 10. + next() * 20.];
            if round % 3 == 0 {
                for y in -3i32..=3 {
                    for x in -3i32..=3 {
                        let px = (center[0] + x as f32 * ex[0] + y as f32 * ey[0]).floor();
                        let py = (center[1] + x as f32 * ex[1] + y as f32 * ey[1]).floor();
                        if (0. ..w as f32).contains(&px) && (0. ..h as f32).contains(&py) {
                            image[py as usize * w + px as usize] = x.abs().max(y.abs()) != 2;
                        }
                    }
                }
            }
            let expected = reference(&image, w, h, center, ex, ey);
            valid += usize::from(expected);
            assert_eq!(finder_grid_valid(&image, w, h, center, ex, ey), expected);
        }
        assert!(valid > 500, "the test must exercise both outcomes");
    }
}

#[cfg(test)]
mod bounded_work_tests {
    use super::*;
    /// Whether a texture pixel is dark.
    type Texture = fn(usize, usize) -> bool;
    fn pattern(w: usize, h: usize, dark: impl Fn(usize, usize) -> bool) -> Vec<bool> {
        (0..w * h).map(|i| dark(i % w, i / w)).collect()
    }
    #[test]
    fn periodic_textures_spend_bounded_finder_work() {
        let (w, h) = (640, 360);
        // Without the check budget these read 2.2, 7.5 and 4.3 million pixels; the
        // diagonal stripes also check the directions of thousands of candidates.
        for (name, bits, limit) in [
            (
                "checkerboard",
                pattern(w, h, |x, y| (x / 2 + y / 2) % 2 == 0),
                6,
            ),
            (
                "diagonal stripes",
                pattern(w, h, |x, y| (x + y) / 3 % 2 == 0),
                20,
            ),
            ("vertical stripes", pattern(w, h, |x, _| x / 2 % 2 == 0), 8),
        ] {
            crate::take_finder_work();
            let (_, limited) = finders(&bits, w, h);
            let work = crate::take_finder_work();
            assert!(limited, "{name}: the check budget is exhausted");
            assert!(work < limit * w * h, "{name}: {work} pixel reads");
        }
    }
    /// Three finder patterns of 4-pixel modules with their top-left corners at
    /// `corners`, each with a 4-pixel light margin.
    fn draw_finders(bits: &mut [bool], w: usize, corners: &[[usize; 2]]) {
        for &[left, top] in corners {
            for y in 0..36_usize {
                for x in 0..36_usize {
                    let (mx, my) = ((x / 4).abs_diff(4_usize), (y / 4).abs_diff(4_usize));
                    let ring = mx.max(my);
                    bits[(top + y - 4) * w + left + x - 4] = ring != 2 && ring != 4;
                }
            }
        }
    }
    #[test]
    fn finders_right_of_and_inside_textures_are_checked() {
        let (w, h) = (640, 360);
        let corners = [[420_usize, 120_usize], [540, 120], [420, 240]];
        let quiet = |x: usize, y: usize| (400..600).contains(&x) && (100..300).contains(&y);
        let textures: [(&str, Texture); 3] = [
            ("checkerboard", |x, y| (x / 2 + y / 2) % 2 == 0),
            ("vertical stripes", |x, _| x / 2 % 2 == 0),
            ("diagonal stripes", |x, y| (x + y) / 3 % 2 == 0),
        ];
        for (name, texture) in textures {
            for (place, covered) in [
                // The texture fills every row left of the finders.
                ("right of", (|x, _| x < 400) as fn(usize, usize) -> bool),
                // The texture surrounds the finders' light box.
                ("inside", |_, _| true),
            ] {
                let mut bits = pattern(w, h, |x, y| covered(x, y) && !quiet(x, y) && texture(x, y));
                draw_finders(&mut bits, w, &corners);
                let (found, limited) = finders(&bits, w, h);
                assert!(limited, "{name}, {place}: the texture exhausts the checks");
                for [left, top] in corners {
                    let (cx, cy) = (
                        crate::numeric::usize_f32(left + 14),
                        crate::numeric::usize_f32(top + 14),
                    );
                    assert!(
                        found
                            .iter()
                            .any(|f| (f.x - cx).abs() < 2. && (f.y - cy).abs() < 2.),
                        "{name}, {place}: finder at {left},{top}"
                    );
                }
            }
        }
    }
    #[test]
    fn a_textured_band_leaves_checks_for_finders_below_it() {
        let (w, h) = (640, 360);
        let mut bits = pattern(w, h, |x, y| y < 120 && (x / 2 + y / 2) % 2 == 0);
        // Three finder patterns of 4-pixel modules at the bottom.
        for [left, top] in [[400_usize, 220_usize], [520, 220], [400, 320]] {
            for y in 0..28_usize {
                for x in 0..28_usize {
                    let ring = (x / 4).abs_diff(3_usize).max((y / 4).abs_diff(3_usize));
                    bits[(top + y) * w + left + x] = ring != 2;
                }
            }
        }
        let (found, limited) = finders(&bits, w, h);
        assert!(limited, "the checkerboard band exhausts the initial checks");
        for [left, top] in [[400_usize, 220_usize], [520, 220], [400, 320]] {
            let (cx, cy) = (
                crate::numeric::usize_f32(left + 14),
                crate::numeric::usize_f32(top + 14),
            );
            assert!(
                found
                    .iter()
                    .any(|f| (f.x - cx).abs() < 2. && (f.y - cy).abs() < 2.),
                "finder at {left},{top}"
            );
        }
    }
}
