//! Image-evidence comparison for fragmented bands. No EAN tables or digits.
#![forbid(unsafe_code)]
use crate::{
    experiment::{self, AssociationBudget, Work},
    sampling::ImageView,
    scan::Quad,
};
const N: usize = 96;
fn midpoint(e: [[f64; 2]; 2]) -> [f64; 2] {
    [(e[0][0] + e[1][0]) * 0.5, (e[0][1] + e[1][1]) * 0.5]
}
fn edges(q: Quad) -> [[[f64; 2]; 2]; 2] {
    [[q[0], q[1]], [q[3], q[2]]]
}
/// Nearest reading-direction edges, with conservative alignment/scale gates.
pub(crate) fn gap_edges(a: Quad, b: Quad) -> Option<([[f64; 2]; 2], [[f64; 2]; 2])> {
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
            let degrees: f64 = if cfg!(feature = "experimental-flexible-identity") {
                15.
            } else {
                5.
            };
            if (u[0] * v[0] + u[1] * v[1]) / (wa * wb) < degrees.to_radians().cos() {
                continue;
            }
            let (ma, mb) = (midpoint(x), midpoint(y));
            let delta = [mb[0] - ma[0], mb[1] - ma[1]];
            let along = (delta[0] * u[0] + delta[1] * u[1]).abs() / wa;
            let cross = (delta[0] * u[1] - delta[1] * u[0]).abs() / wa;
            if along > wa.min(wb) * 0.05
                || cross
                    > wa.min(wb)
                        * if cfg!(feature = "experimental-flexible-identity") {
                            0.75
                        } else {
                            0.25
                        }
                || cross < 0.5
            {
                continue;
            }
            let d = experiment::distance(ma, mb);
            if d < distance {
                distance = d;
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
    let d = [mb[0] - ma[0], mb[1] - ma[1]];
    let n = d[0] * d[0] + d[1] * d[1];
    if n <= 0. {
        return false;
    }
    let t = ((p[0] - ma[0]) * d[0] + (p[1] - ma[1]) * d[1]) / n;
    let along = ((p[0] - ma[0]) * d[1] - (p[1] - ma[1]) * d[0]).abs() / n.sqrt();
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
        let t = (i as f64 + 0.5) / N as f64;
        let p = [
            e[0][0] + t * (e[1][0] - e[0][0]),
            e[0][1] + t * (e[1][1] - e[0][1]),
        ];
        if p[0] < 0. || p[1] < 0. || p[0] >= im.width as f64 || p[1] >= im.height as f64 {
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
    let mean = values.iter().sum::<f64>() / N as f64;
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
/// Every <=0.5pixel cross-gap step must retain contrast and correlate with both
/// observed endpoint patterns. No expected-code reconstruction is used.
pub(crate) fn connected(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> bool {
    let steps = (experiment::distance(a[0], b[0]).max(experiment::distance(a[1], b[1])) * 2.).ceil()
        as usize;
    if steps > 512
        && (!cfg!(feature = "experimental-identity-budgeted-link")
            || steps.saturating_add(1).saturating_mul(N) > budget.pixels_left)
    {
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
        #[cfg(feature = "experimental-identity-phase")]
        {
            return connected_phase(im, a, b, budget, work);
        }
        #[cfg(not(feature = "experimental-identity-phase"))]
        {
            return false;
        }
    }
    for i in 1..steps {
        let t = i as f64 / steps as f64;
        let e = std::array::from_fn(|j| {
            [
                a[j][0] + t * (b[j][0] - a[j][0]),
                a[j][1] + t * (b[j][1] - a[j][1]),
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
// Only a failed endpoint comparison can invoke this bounded submodule
// alignment check. Every intermediate source row must still prove continuity.
#[cfg(feature = "experimental-identity-phase")]
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
        let t = (i as f64 + 0.5) / 384.;
        let p = [
            e[0][0] + t * (e[1][0] - e[0][0]),
            e[0][1] + t * (e[1][1] - e[0][1]),
        ];
        if p[0] < 0. || p[1] < 0. || p[0] >= im.width as f64 || p[1] >= im.height as f64 {
            return None;
        }
        let n = im.gray(p[0].round(), p[1].round());
        lo = lo.min(n);
        hi = hi.max(n);
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
#[cfg(feature = "experimental-identity-phase")]
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
        let t = f64::from(lag) / 384.;
        let delta = [t * (b[1][0] - b[0][0]), t * (b[1][1] - b[0][1])];
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
    let steps = (experiment::distance(a[0], b[0]).max(experiment::distance(a[1], b[1])) * 2.).ceil()
        as usize;
    if steps > 512
        && (!cfg!(feature = "experimental-identity-budgeted-link")
            || steps.saturating_sub(1).saturating_mul(384 * 5) > budget.pixels_left)
    {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    for i in 1..steps {
        let t = i as f64 / steps as f64;
        let e = std::array::from_fn(|j| {
            [
                a[j][0] + t * (b[j][0] - a[j][0]),
                a[j][1] + t * (b[j][1] - a[j][1]),
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
#[cfg(test)]
mod tests {
    #[cfg(feature = "experimental-identity-phase")]
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
        assert!(connected_phase(
            im,
            a,
            b,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        let mut budget = AssociationBudget {
            checks_left: 100,
            pixels_left: 1919,
        };
        let mut work = Work::default();
        assert!(!connected_phase(im, a, b, &mut budget, &mut work));
        assert_eq!(work.association_truncated, 1);
        assert_eq!(work.continuity_samples, 0);
        pixels[60 * 420..61 * 420].fill(255);
        assert!(!connected_phase(
            ImageView::new(&pixels, 420, 140, 1, 420).unwrap(),
            a,
            b,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
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
        #[cfg(not(feature = "experimental-identity-budgeted-link"))]
        {
            let mut w = Work::default();
            assert!(!connected(
                ImageView::new(&pixels, 400, 120, 1, 400).unwrap(),
                a,
                [[20., 300.], [380., 300.]],
                &mut AssociationBudget::default(),
                &mut w
            ));
            assert_eq!(w.continuity_capped_links, 1);
            assert_eq!(w.continuity_samples, 0);
            assert_eq!(w.association_truncated, 1);
        }
    }
}

#[cfg(all(test, feature = "experimental-identity-budgeted-link"))]
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
