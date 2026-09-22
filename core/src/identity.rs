//! Image-evidence comparison for fragmented bands. No EAN tables or digits.
#![forbid(unsafe_code)]
use crate::{
    experiment::{self, AssociationBudget, Work},
    sampling::ImageView,
    scan::Quad,
};
const N: usize = 96;

// Keys preserve every coordinate bit. Only complete row computations enter the
// finite cache, including definitive flat/out-of-image rejections. Budget
// exhaustion is never cached. A cache belongs to one immutable source frame.
#[cfg(feature = "mode-very-high")]
#[derive(Default)]
pub(crate) struct IdentityCache {
    rows: std::collections::BTreeMap<[u64; 4], Option<[f64; N]>>,

    dense: std::collections::BTreeMap<[u64; 4], Option<[f64; 384]>>,
    pub hits: usize,
}
#[cfg(feature = "mode-very-high")]
fn row_key(e: [[f64; 2]; 2]) -> [u64; 4] {
    [
        e[0][0].to_bits(),
        e[0][1].to_bits(),
        e[1][0].to_bits(),
        e[1][1].to_bits(),
    ]
}
#[cfg(feature = "mode-very-high")]
fn cached_row(
    im: ImageView<'_>,
    e: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
    cache: &mut IdentityCache,
) -> Option<[f64; N]> {
    let key = row_key(e);
    if let Some(r) = cache.rows.get(&key) {
        cache.hits += 1;
        return *r;
    }
    let before = work.association_truncated;
    let enough = budget.pixels_left >= N;
    let result = row(im, e, budget, work);
    if enough && work.association_truncated == before && cache.rows.len() < 256 {
        cache.rows.insert(key, result);
    }
    result
}
#[cfg(feature = "mode-very-high")]
fn cached_dense_row(
    im: ImageView<'_>,
    e: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
    cache: &mut IdentityCache,
) -> Option<[f64; 384]> {
    let key = row_key(e);
    if let Some(r) = cache.dense.get(&key) {
        cache.hits += 1;
        return *r;
    }
    let before = work.association_truncated;
    let enough = budget.pixels_left >= 384 * 5;
    let result = dense_row(im, e, budget, work);
    if enough && work.association_truncated == before && cache.dense.len() < 256 {
        cache.dense.insert(key, result);
    }
    result
}
fn midpoint(e: [[f64; 2]; 2]) -> [f64; 2] {
    [(e[0][0] + e[1][0]) * 0.5, (e[0][1] + e[1][1]) * 0.5]
}
fn edges(q: Quad) -> [[[f64; 2]; 2]; 2] {
    [[q[0], q[1]], [q[3], q[2]]]
}
/// Nearest reading-direction edges, with conservative alignment/scale gates.
type Edge = [[f64; 2]; 2];
pub(crate) fn gap_edges(a: Quad, b: Quad) -> Option<(Edge, Edge)> {
    let mut best = None;
    let mut distance = f64::INFINITY;
    for x in edges(a) {
        for mut y in edges(b) {
            let u = [x[1][0] - x[0][0], x[1][1] - x[0][1]];
            let mut v = [y[1][0] - y[0][0], y[1][1] - y[0][1]];
            if u[0] * v[0] + u[1] * v[1] < 0. {
                y.reverse();
                v = [-v[0], -v[1]];
            }
            let (wa, wb) = (u[0].hypot(u[1]), v[0].hypot(v[1]));
            if wa < 76. || wb < 76. || wa.min(wb) / wa.max(wb) < 0.9 {
                continue;
            }
            let degrees: f64 = { 15. };
            if (u[0] * v[0] + u[1] * v[1]) / (wa * wb) < degrees.to_radians().cos() {
                continue;
            }
            let (ma, mb) = (midpoint(x), midpoint(y));
            let delta = [mb[0] - ma[0], mb[1] - ma[1]];
            let along = (delta[0] * u[0] + delta[1] * u[1]).abs() / wa;
            let cross = (delta[0] * u[1] - delta[1] * u[0]).abs() / wa;
            if along > wa.min(wb) * 0.05 || cross > wa.min(wb) * { 0.75 } || cross < 0.5 {
                continue;
            }
            let edge_distance = experiment::distance(ma, mb);
            if edge_distance < distance {
                distance = edge_distance;
                best = Some((x, y));
            }
        }
    }
    best
}
/// Require an existing rejected path spatially between the two bands. This
/// does not accept that path: raw ambiguity survives the identity grouping.
pub(crate) fn barrier_between(a: [[f64; 2]; 2], b: [[f64; 2]; 2], p: [f64; 2]) -> bool {
    let (ma, mb) = (midpoint(a), midpoint(b));
    let direction = [mb[0] - ma[0], mb[1] - ma[1]];
    let length_squared = direction[0] * direction[0] + direction[1] * direction[1];
    if length_squared <= 0. {
        return false;
    }
    let t = ((p[0] - ma[0]) * direction[0] + (p[1] - ma[1]) * direction[1]) / length_squared;
    let along = ((p[0] - ma[0]) * direction[1] - (p[1] - ma[1]) * direction[0]).abs()
        / length_squared.sqrt();
    t > 0. && t < 1. && along < experiment::distance(a[0], a[1]) * 0.4
}
fn row(
    im: ImageView<'_>,
    e: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> Option<[f64; N]> {
    if !budget.pixels(N, work) {
        work.association_truncated = 1;
        return None;
    }
    let mut values = [0.; N];
    let (mut lo, mut hi) = (255f64, 0f64);
    for (i, v) in values.iter_mut().enumerate() {
        let t = (crate::numeric::usize_f64(i) + 0.5) / crate::numeric::usize_f64(N);
        let p = [
            e[0][0] + t * (e[1][0] - e[0][0]),
            e[0][1] + t * (e[1][1] - e[0][1]),
        ];
        if p[0] < 0.
            || p[1] < 0.
            || p[0] >= crate::numeric::usize_f64(im.width)
            || p[1] >= crate::numeric::usize_f64(im.height)
        {
            return None;
        }
        *v = im.gray(p[0].round(), p[1].round());
        work.continuity_samples += 1;
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if hi - lo < 8. {
        return None;
    }
    let mean = values.iter().sum::<f64>() / crate::numeric::usize_f64(N);
    let mut norm = 0.;
    for v in &mut values {
        *v -= mean;
        norm += *v * *v;
    }
    if norm < 1. {
        return None;
    }
    let norm = norm.sqrt();
    for v in &mut values {
        *v /= norm;
    }
    Some(values)
}
fn corr(a: &[f64; N], b: &[f64; N]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
#[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
/// Every <=0.5pixel cross-gap step must retain contrast and correlate with both
/// observed endpoint patterns. No expected-code reconstruction is used.
pub(crate) fn connected(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> bool {
    let steps = crate::numeric::f64_usize(
        (experiment::distance(a[0], b[0]).max(experiment::distance(a[1], b[1])) * 2.).ceil(),
    );
    if steps > 512 && (steps.saturating_add(1).saturating_mul(N) > budget.pixels_left) {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    let Some(ra) = row(im, a, budget, work) else {
        return false;
    };
    let Some(rb) = row(im, b, budget, work) else {
        return false;
    };
    if corr(&ra, &rb) < 0.85 {
        {
            return connected_phase(im, a, b, budget, work);
        }
    }
    for i in 1..steps {
        let fraction = crate::numeric::usize_f64(i) / crate::numeric::usize_f64(steps);
        let e = std::array::from_fn(|j| {
            [
                a[j][0] + fraction * (b[j][0] - a[j][0]),
                a[j][1] + fraction * (b[j][1] - a[j][1]),
            ]
        });
        let Some(r) = row(im, e, budget, work) else {
            return false;
        };
        if corr(&r, &ra) < 0.85 || corr(&r, &rb) < 0.85 {
            return false;
        }
    }
    true
}
#[cfg(feature = "mode-very-high")]
/// Every <=0.5pixel cross-gap step must retain contrast and correlate with both
/// observed endpoint patterns. No expected-code reconstruction is used.
#[cfg(test)]
pub(crate) fn connected(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> bool {
    connected_cached(im, a, b, budget, work, &mut IdentityCache::default())
}
#[cfg(feature = "mode-very-high")]
pub(crate) fn connected_cached(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
    cache: &mut IdentityCache,
) -> bool {
    let steps = crate::numeric::f64_usize(
        (experiment::distance(a[0], b[0]).max(experiment::distance(a[1], b[1])) * 2.).ceil(),
    );
    if steps > 512 && (steps.saturating_add(1).saturating_mul(N) > budget.pixels_left) {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    let Some(ra) = cached_row(im, a, budget, work, cache) else {
        return false;
    };
    let Some(rb) = cached_row(im, b, budget, work, cache) else {
        return false;
    };
    if corr(&ra, &rb) < 0.85 {
        {
            return connected_phase(im, a, b, budget, work, cache);
        }
    }
    for i in 1..steps {
        let fraction = crate::numeric::usize_f64(i) / crate::numeric::usize_f64(steps);
        let e = std::array::from_fn(|j| {
            [
                a[j][0] + fraction * (b[j][0] - a[j][0]),
                a[j][1] + fraction * (b[j][1] - a[j][1]),
            ]
        });
        let Some(r) = cached_row(im, e, budget, work, cache) else {
            return false;
        };
        if corr(&r, &ra) < 0.85 || corr(&r, &rb) < 0.85 {
            return false;
        }
    }
    true
}

// Only a failed endpoint comparison can invoke this bounded submodule
// alignment check. Every intermediate source row must still prove continuity.

fn dense_row(
    im: ImageView<'_>,
    e: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> Option<[f64; 384]> {
    // Charge four bilinear neighbors plus the nearest-pixel contrast veto.
    if !budget.pixels(384 * 5, work) {
        work.association_truncated = 1;
        return None;
    }
    let mut v = [0.; 384];
    let (mut lo, mut hi) = (255f64, 0f64);
    for (i, x) in v.iter_mut().enumerate() {
        let t = (crate::numeric::usize_f64(i) + 0.5) / 384.;
        let p = [
            e[0][0] + t * (e[1][0] - e[0][0]),
            e[0][1] + t * (e[1][1] - e[0][1]),
        ];
        if p[0] < 0.
            || p[1] < 0.
            || p[0] >= crate::numeric::usize_f64(im.width)
            || p[1] >= crate::numeric::usize_f64(im.height)
        {
            return None;
        }
        let count = im.gray(p[0].round(), p[1].round());
        lo = lo.min(count);
        hi = hi.max(count);
        *x = f64::from(im.bilinear(p[0], p[1]));
        work.continuity_samples += 5;
    }
    if hi - lo < 8. {
        return None;
    }
    let mean = v.iter().sum::<f64>() / 384.;
    let mut norm = 0.;
    for x in &mut v {
        *x -= mean;
        norm += *x * *x;
    }
    if norm < 1. {
        return None;
    }
    let norm = norm.sqrt();
    for x in &mut v {
        *x /= norm;
    }
    Some(v)
}
#[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
fn connected_phase(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> bool {
    let Some(ra) = dense_row(im, a, budget, work) else {
        return false;
    };
    let dot = |a: &[f64; 384], b: &[f64; 384]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut best = None;
    let mut score = 0.85;
    // At most3/384 of the decoded span; seven fixed phase choices, no digits.
    for lag in -3..=3 {
        let fraction = f64::from(lag) / 384.;
        let delta = [
            fraction * (b[1][0] - b[0][0]),
            fraction * (b[1][1] - b[0][1]),
        ];
        let shifted = b.map(|p| [p[0] + delta[0], p[1] + delta[1]]);
        let Some(rb) = dense_row(im, shifted, budget, work) else {
            return false;
        };
        let c = dot(&ra, &rb);
        if c > score {
            score = c;
            best = Some((shifted, rb));
        }
    }
    let Some((b, rb)) = best else { return false };
    let steps = crate::numeric::f64_usize(
        (experiment::distance(a[0], b[0]).max(experiment::distance(a[1], b[1])) * 2.).ceil(),
    );
    if steps > 512 && (steps.saturating_sub(1).saturating_mul(384 * 5) > budget.pixels_left) {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    for i in 1..steps {
        let fraction = crate::numeric::usize_f64(i) / crate::numeric::usize_f64(steps);
        let e = std::array::from_fn(|j| {
            [
                a[j][0] + fraction * (b[j][0] - a[j][0]),
                a[j][1] + fraction * (b[j][1] - a[j][1]),
            ]
        });
        let Some(r) = dense_row(im, e, budget, work) else {
            return false;
        };
        if dot(&r, &ra) < 0.85 || dot(&r, &rb) < 0.85 {
            return false;
        }
    }
    true
}
#[cfg(feature = "mode-very-high")]
fn connected_phase(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
    cache: &mut IdentityCache,
) -> bool {
    let Some(ra) = cached_dense_row(im, a, budget, work, cache) else {
        return false;
    };
    let dot = |a: &[f64; 384], b: &[f64; 384]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut best = None;
    let mut score = 0.85;
    // At most3/384 of the decoded span; seven fixed phase choices, no digits.
    for lag in -3..=3 {
        let fraction = f64::from(lag) / 384.;
        let delta = [
            fraction * (b[1][0] - b[0][0]),
            fraction * (b[1][1] - b[0][1]),
        ];
        let shifted = b.map(|p| [p[0] + delta[0], p[1] + delta[1]]);
        let Some(rb) = cached_dense_row(im, shifted, budget, work, cache) else {
            return false;
        };
        let c = dot(&ra, &rb);
        if c > score {
            score = c;
            best = Some((shifted, rb));
        }
    }
    let Some((b, rb)) = best else { return false };
    let steps = crate::numeric::f64_usize(
        (experiment::distance(a[0], b[0]).max(experiment::distance(a[1], b[1])) * 2.).ceil(),
    );
    if steps > 512 && (steps.saturating_sub(1).saturating_mul(384 * 5) > budget.pixels_left) {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    for i in 1..steps {
        let fraction = crate::numeric::usize_f64(i) / crate::numeric::usize_f64(steps);
        let e = std::array::from_fn(|j| {
            [
                a[j][0] + fraction * (b[j][0] - a[j][0]),
                a[j][1] + fraction * (b[j][1] - a[j][1]),
            ]
        });
        let Some(r) = cached_dense_row(im, e, budget, work, cache) else {
            return false;
        };
        if dot(&r, &ra) < 0.85 || dot(&r, &rb) < 0.85 {
            return false;
        }
    }
    true
}
#[cfg(test)]
mod tests {

    #[test]
    fn bounded_phase_keeps_one_pixel_gap_and_budget_veto() {
        let mut pixels = vec![255; 420 * 140];
        for y in 0..140 {
            for x in 20..400 {
                pixels[y * 420 + x] = if (x / 2) % 3 == 0 { 0 } else { 255 }
            }
        }
        let a = [[30., 20.], [390., 20.]];
        let b = [[31., 100.], [391., 100.]];
        let im = ImageView::new(&pixels, 420, 140, 1, 420).unwrap();
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
            {
                assert!(connected_phase(
                    im,
                    a,
                    b,
                    &mut AssociationBudget::default(),
                    &mut Work::default()
                ));
            }
            #[cfg(feature = "mode-very-high")]
            {
                assert!(connected_phase(
                    im,
                    a,
                    b,
                    &mut AssociationBudget::default(),
                    &mut Work::default(),
                    &mut IdentityCache::default()
                ));
            }
        }

        let mut budget = AssociationBudget {
            checks_left: 100,
            pixels_left: 1919,
        };
        let mut work = Work::default();
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
            {
                assert!(!connected_phase(im, a, b, &mut budget, &mut work));
            }
            #[cfg(feature = "mode-very-high")]
            {
                assert!(!connected_phase(
                    im,
                    a,
                    b,
                    &mut budget,
                    &mut work,
                    &mut IdentityCache::default()
                ));
            }
        }

        assert_eq!(work.association_truncated, 1);
        assert_eq!(work.continuity_samples, 0);
        pixels[60 * 420..61 * 420].fill(255);
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
            {
                assert!(!connected_phase(
                    ImageView::new(&pixels, 420, 140, 1, 420).unwrap(),
                    a,
                    b,
                    &mut AssociationBudget::default(),
                    &mut Work::default()
                ));
            }
            #[cfg(feature = "mode-very-high")]
            {
                assert!(!connected_phase(
                    ImageView::new(&pixels, 420, 140, 1, 420).unwrap(),
                    a,
                    b,
                    &mut AssociationBudget::default(),
                    &mut Work::default(),
                    &mut IdentityCache::default()
                ));
            }
        }
    }

    use super::*;
    #[test]
    fn pattern_continuity_rejects_blank_gaps_and_changed_stripes() {
        let mut pixels = vec![255; 400 * 120];
        for y in 0..120 {
            for x in 20..380 {
                pixels[y * 400 + x] = if (x / 4) % 3 == 0 { 0 } else { 255 }
            }
        }
        let a = [[20., 20.], [380., 20.]];
        let b = [[20., 100.], [380., 100.]];
        assert!(connected(
            ImageView::new(&pixels, 400, 120, 1, 400).unwrap(),
            a,
            b,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        for y in 60..61 {
            pixels[y * 400..(y + 1) * 400].fill(255);
        }
        assert!(!connected(
            ImageView::new(&pixels, 400, 120, 1, 400).unwrap(),
            a,
            b,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        for y in 60..61 {
            for x in 20..380 {
                pixels[y * 400 + x] = if (x / 4) % 3 == 1 { 0 } else { 255 }
            }
        }
        assert!(!connected(
            ImageView::new(&pixels, 400, 120, 1, 400).unwrap(),
            a,
            b,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
    }
    #[test]
    fn identity_pixel_exhaustion_is_explicit() {
        let pixels = vec![0; 400 * 120];
        let im = ImageView::new(&pixels, 400, 120, 1, 400).unwrap();
        let mut w = Work::default();
        let mut budget = AssociationBudget {
            checks_left: 10,
            pixels_left: 95,
        };
        assert!(!connected(
            im,
            [[20., 20.], [380., 20.]],
            [[20., 100.], [380., 100.]],
            &mut budget,
            &mut w
        ));
        assert_eq!(w.continuity_samples, 0);
        assert_eq!(w.association_truncated, 1);
    }
    #[test]
    fn rotated_fractional_gap_and_oversize_budget_guards() {
        let mut pixels = vec![255; 400 * 120];
        for y in 0..120 {
            for x in 20..380 {
                if y != 60 && ((x / 4) % 3 == 0) {
                    pixels[y * 400 + x] = 0;
                }
            }
        }
        let mut rotated = vec![255; 400 * 120];
        for y in 0..120 {
            for x in 0..400 {
                rotated[(399 - x) * 120 + y] = pixels[y * 400 + x];
            }
        }
        let a = [[20.25, 20.], [379.25, 20.]];
        let b = [[20.25, 100.], [379.25, 100.]];
        let rot = |p: [f64; 2]| [p[1], 399. - p[0]];
        assert!(!connected(
            ImageView::new(&rotated, 120, 400, 1, 120).unwrap(),
            a.map(rot),
            b.map(rot),
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
    }
}
#[cfg(test)]
mod budgeted_link_tests {
    use super::*;
    #[test]
    fn long_links_keep_half_pixel_density_gap_veto_and_shared_budget() {
        let mut pixels = vec![255; 400 * 400];
        for y in 0..400 {
            for x in 20..380 {
                pixels[y * 400 + x] = if (x / 4) % 3 == 0 { 0 } else { 255 }
            }
        }
        let a = [[20., 20.], [380., 20.]];
        let b = [[20., 350.], [380., 350.]];
        let mut w = Work::default();
        let mut budget = AssociationBudget::default();
        let initial = budget.pixels_left;
        assert!(connected(
            ImageView::new(&pixels, 400, 400, 1, 400).unwrap(),
            a,
            b,
            &mut budget,
            &mut w
        ));
        assert_eq!(w.continuity_samples, 661 * 96);
        assert_eq!(initial - budget.pixels_left, w.continuity_samples);
        assert_eq!(w.association_truncated, 0);
        pixels[180 * 400..181 * 400].fill(255);
        let mut w = Work::default();
        assert!(!connected(
            ImageView::new(&pixels, 400, 400, 1, 400).unwrap(),
            a,
            b,
            &mut AssociationBudget::default(),
            &mut w
        ));
        assert_eq!(w.association_truncated, 0);
        let mut budget = AssociationBudget {
            checks_left: 10,
            pixels_left: 661 * 96 - 1,
        };
        let mut w = Work::default();
        assert!(!connected(
            ImageView::new(&pixels, 400, 400, 1, 400).unwrap(),
            a,
            b,
            &mut budget,
            &mut w
        ));
        assert_eq!(w.association_truncated, 1);
        assert_eq!(w.continuity_samples, 0);
        assert_eq!(w.continuity_capped_links, 1);
    }
}
#[cfg(feature = "mode-very-high")]
#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    fn exact_rows_reuse_work_and_exhaustion_is_not_cached() {
        let pixels: Vec<u8> = (0..420 * 50)
            .map(|i| if (i % 420) / 3 % 2 == 0 { 0 } else { 255 })
            .collect();
        let im = ImageView::new(&pixels, 420, 50, 1, 420).unwrap();
        let e = [[20., 20.], [400., 20.]];
        let mut cache = IdentityCache::default();
        let mut work = Work::default();
        let mut budget = AssociationBudget {
            checks_left: 10,
            pixels_left: 0,
        };
        assert!(cached_row(im, e, &mut budget, &mut work, &mut cache).is_none());
        assert!(cache.rows.is_empty());
        let mut work = Work::default();
        budget.pixels_left = N;
        let a = cached_row(im, e, &mut budget, &mut work, &mut cache).unwrap();
        assert_eq!(work.continuity_samples, N);
        assert_eq!(budget.pixels_left, 0);
        let b = cached_row(im, e, &mut budget, &mut work, &mut cache).unwrap();
        assert_eq!(a, b);
        assert_eq!(work.continuity_samples, N);
        assert_eq!(cache.hits, 1);
        assert_eq!(work.association_truncated, 0);
        let mut fresh = IdentityCache::default();
        let inverted: Vec<_> = pixels.iter().map(|v| 255 - v).collect();
        let im2 = ImageView::new(&inverted, 420, 50, 1, 420).unwrap();
        budget.pixels_left = N;
        let c = cached_row(im2, e, &mut budget, &mut Work::default(), &mut fresh).unwrap();
        assert_ne!(a, c);
        assert_eq!(fresh.hits, 0);
    }
    #[test]
    fn row_cache_has_a_fixed_capacity() {
        let pixels: Vec<u8> = (0..420 * 310)
            .map(|i| if (i % 420) / 3 % 2 == 0 { 0 } else { 255 })
            .collect();
        let im = ImageView::new(&pixels, 420, 310, 1, 420).unwrap();
        let mut cache = IdentityCache::default();
        let mut budget = AssociationBudget::default();
        let mut work = Work::default();
        for y in 1..300 {
            let e = [[20., f64::from(y)], [400., f64::from(y)]];
            assert!(cached_row(im, e, &mut budget, &mut work, &mut cache).is_some());
        }
        assert_eq!(cache.rows.len(), 256);
        assert_eq!(cache.hits, 0);
    }
}
