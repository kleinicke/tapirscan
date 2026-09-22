//! Source-pixel identity proofs and competing-payload conflict resolution.
#[cfg(feature = "mode-very-high")]
use super::Detection;
use super::{AssociationBudget, ImageView, ReconciliationWork, Work};

#[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
pub(super) fn same_text_identity(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    counter: &mut Work,
    work: &mut ReconciliationWork,
) -> bool {
    {
        let mut local = Work::default();
        let connected = crate::identity::connected(im, a, b, budget, &mut local);
        counter.continuity_samples += local.continuity_samples;
        counter.continuity_capped_links += local.continuity_capped_links;
        counter.continuity_rejects += local.continuity_rejects;
        // This helper spends pixels only. Preserve outer mandatory failures and
        // conservatively propagate any future check-budget work added to identity.
        counter.association_checks += local.association_checks;
        if local.association_checks > 0 {
            counter.association_truncated |= local.association_truncated;
        }
        if local.association_truncated > 0 {
            work.optional_identity_deferred += 1;
            return false;
        }
        connected
    }
}

#[cfg(feature = "mode-very-high")]
pub(super) fn same_text_identity(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    counter: &mut Work,
    work: &mut ReconciliationWork,
    cache: &mut crate::identity::IdentityCache,
) -> bool {
    {
        let mut local = Work::default();
        let connected = crate::identity::connected_cached(im, a, b, budget, &mut local, cache);
        counter.continuity_samples += local.continuity_samples;
        counter.continuity_capped_links += local.continuity_capped_links;
        counter.continuity_rejects += local.continuity_rejects;
        // This helper spends pixels only. Preserve outer mandatory failures and
        // conservatively propagate any future check-budget work added to identity.
        counter.association_checks += local.association_checks;
        if local.association_checks > 0 {
            counter.association_truncated |= local.association_truncated;
        }
        if local.association_truncated > 0 {
            work.optional_identity_deferred += 1;
            return false;
        }
        connected
    }
}

#[cfg(feature = "mode-very-high")]
pub(super) fn pattern_residual(row: &[f64], digits: &[u8; 13]) -> f64 {
    if digits.iter().any(|d| *d > 9) {
        return f64::INFINITY;
    }
    let bits = crate::ean::encode(digits);
    let mean = row.iter().sum::<f64>() / crate::numeric::usize_f64(row.len());
    let variance = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>();
    if variance < crate::numeric::usize_f64(row.len()) * 16. {
        return f64::INFINITY;
    }
    let mut best = f64::INFINITY;
    for reverse in [false, true] {
        let mut prefix = [0f64; 96];
        for i in 0..95 {
            prefix[i + 1] = prefix[i] + f64::from(bits[if reverse { 94 - i } else { i }]);
        }
        let integral = |x: f64| {
            let x = x.clamp(0., 95.);
            let i = crate::numeric::f64_usize(x.floor()).min(94);
            prefix[i] + (x - crate::numeric::usize_f64(i)) * (prefix[i + 1] - prefix[i])
        };
        for radius in [0.05, 0.15, 0.35, 0.6, 1.0] {
            for shift in [-0.6, -0.3, -0.15, 0., 0.15, 0.3, 0.6] {
                for stretch in [-0.6, 0., 0.6] {
                    let (mut sx, mut sxx, mut covariance) = (0., 0., 0.);
                    for (i, y) in row.iter().enumerate() {
                        let u = shift
                            + (crate::numeric::usize_f64(i) + 0.5)
                                / crate::numeric::usize_f64(row.len())
                                * (95. + stretch);
                        let x = (integral(u + radius) - integral(u - radius)) / (2. * radius);
                        sx += x;
                        sxx += x * x;
                        covariance += x * (y - mean);
                    }
                    let vx = sxx - sx * sx / crate::numeric::usize_f64(row.len());
                    if vx <= 1e-9 || covariance <= 0. {
                        continue;
                    }
                    best = best.min((1. - covariance * covariance / (variance * vx)).max(0.));
                }
            }
        }
    }
    best
}

#[cfg(feature = "mode-very-high")]
pub(super) fn source_pair_winner(
    im: ImageView<'_>,
    winner: &Detection,
    other: &Detection,
    budget: &mut AssociationBudget,
    counter: &mut Work,
) -> bool {
    const PIXELS: usize = 6 * 384;
    const FITS: usize = 6 * 2 * 2 * 5 * 7 * 3;

    if winner.digits == other.digits || winner.support < 3 || other.support < 2 {
        return false;
    }
    if crate::identity::gap_edges(winner.polygon, other.polygon).is_none() {
        return false;
    }
    if budget.pixels_left < PIXELS || budget.checks_left < FITS {
        return false;
    }
    budget.pixels_left -= PIXELS;
    budget.checks_left -= FITS;
    counter.continuity_samples += PIXELS;
    counter.association_checks += FITS;
    for (q, max_residual) in [(winner.polygon, 0.45), (other.polygon, 0.8)] {
        let Ok(m) = crate::scan::transform(q) else {
            return false;
        };
        for fraction in [0.2, 0.5, 0.8] {
            let mut row = [0f64; 384];
            for (i, value) in row.iter_mut().enumerate() {
                let Ok(p) = crate::experiment::point(
                    m.0,
                    0,
                    (crate::numeric::usize_f64(i) + 0.5) / 384.,
                    fraction,
                ) else {
                    return false;
                };
                *value = 255. - f64::from(im.bilinear(p[0], p[1]));
            }
            let a = pattern_residual(&row, &winner.digits);
            let b = pattern_residual(&row, &other.digits);
            if !a.is_finite() || !b.is_finite() || a > max_residual || b - a < 0.04 || a >= 0.95 * b
            {
                return false;
            }
        }
    }
    true
}

#[cfg(feature = "mode-very-high")]
pub(super) fn source_conflict_winner(
    im: ImageView<'_>,
    broad: &Detection,
    thin: &Detection,
    budget: &mut AssociationBudget,
    counter: &mut Work,
) -> bool {
    const PIXELS: usize = 3 * 512;

    if broad.digits == thin.digits
        || broad.support < 8
        || broad.support < thin.support.saturating_mul(3)
    {
        return false;
    }
    let quad = broad.polygon;
    let thin_quad = thin.polygon;
    let width = crate::experiment::distance(quad[0], quad[1]);
    let tw = crate::experiment::distance(thin_quad[0], thin_quad[1]);
    let height = crate::experiment::distance(quad[0], quad[3]);
    let th = crate::experiment::distance(thin_quad[0], thin_quad[3]);
    if width < 95.
        || width.min(tw) / width.max(tw) < 0.85
        || height < 4. * th.max(1.)
        || th > width / 95.
    {
        return false;
    }
    let top = [
        (quad[0][0] + quad[1][0]) * 0.5,
        (quad[0][1] + quad[1][1]) * 0.5,
    ];
    let bottom = [
        (quad[2][0] + quad[3][0]) * 0.5,
        (quad[2][1] + quad[3][1]) * 0.5,
    ];
    let direction = [bottom[0] - top[0], bottom[1] - top[1]];
    let v2 = direction[0] * direction[0] + direction[1] * direction[1];
    if v2 < 16. {
        return false;
    }
    let center = [
        thin_quad.iter().map(|profile| profile[0]).sum::<f64>() * 0.25,
        thin_quad.iter().map(|profile| profile[1]).sum::<f64>() * 0.25,
    ];
    let fraction = ((center[0] - top[0]) * direction[0] + (center[1] - top[1]) * direction[1]) / v2;
    let dy = 1.5 / v2.sqrt();
    if fraction - dy < 0. || fraction + dy > 1. {
        return false;
    }
    let Ok(matrix) = crate::scan::transform(quad) else {
        return false;
    };
    if budget.pixels_left < PIXELS || budget.checks_left < 3 {
        return false;
    }
    budget.pixels_left -= PIXELS;
    budget.checks_left -= 3;
    counter.continuity_samples += PIXELS;
    counter.association_checks += 3;
    let mut agreeing_rows = 0;
    for fraction in [fraction - dy, fraction, fraction + dy] {
        let mut profile = [0f32; 512];
        for (i, value) in profile.iter_mut().enumerate() {
            let Ok(a) = crate::experiment::point(
                matrix.0,
                0,
                -0.15 + 1.3 * (crate::numeric::usize_f64(i) + 0.5) / 512.,
                fraction,
            ) else {
                return false;
            };
            *value = im.bilinear(a[0], a[1]);
        }
        let mut sorted = profile;
        let (lo, hi) = crate::sampling::contrast_bounds(&mut sorted);
        if hi - lo < 24. {
            continue;
        }
        for value in &mut profile {
            *value = (1. - (*value - lo) / (hi - lo)).clamp(0., 1.);
        }
        let Ok(reads) = crate::multi_profile::decode_many(&profile, 4) else {
            return false;
        };
        if reads.ambiguous_intervals > 0 {
            return false;
        }
        if reads.symbols.iter().any(|read| read.digits != broad.digits) {
            return false;
        }
        agreeing_rows += usize::from(reads.symbols.iter().any(|read| read.digits == broad.digits));
    }
    agreeing_rows >= 2
}
