//! Associate observations using source continuity, contradiction checks and a shared budget.
use super::{
    distance, point, scan, AssociationBudget, Detection, ImageView, Observation, Timer, Work,
};

pub(super) fn connected(
    im: ImageView<'_>,
    m: [f64; 9],
    a: Observation,
    b: Observation,
    work: &mut Work,
) -> bool {
    connected_budget(
        im,
        m,
        a,
        b,
        work,
        None,
        &mut std::collections::HashMap::new(),
    )
}

pub(super) fn connected_budget(
    im: ImageView<'_>,
    matrix: [f64; 9],
    a: Observation,
    b: Observation,
    work: &mut Work,
    budget: Option<&mut AssociationBudget>,
    cache: &mut std::collections::HashMap<[u64; 5], Option<(usize, usize)>>,
) -> bool {
    {
        connected_density(im, matrix, a, b, work, budget, cache)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "This continuity proof shares cached row evidence, pixel budgets and rejection decisions across its path."
)]
pub(super) fn connected_density(
    im: ImageView<'_>,
    matrix: [f64; 9],
    a: Observation,
    b: Observation,
    work: &mut Work,
    mut budget: Option<&mut AssociationBudget>,
    cache: &mut std::collections::HashMap<[u64; 5], Option<(usize, usize)>>,
) -> bool {
    let u = (a.left + a.right + b.left + b.right) / 4.;
    let pa = point(matrix, a.axis, u, a.fraction).unwrap();
    let pb = point(matrix, a.axis, u, b.fraction).unwrap();
    let steps = crate::numeric::f64_usize((distance(pa, pb) * 2.).ceil());
    if steps > 4096 {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    let mut sample = |t: f64, n: usize| -> Option<(usize, usize)> {
        let f = a.fraction + (b.fraction - a.fraction) * t;
        let left = a.left + (b.left - a.left) * t;
        let right = a.right + (b.right - a.right) * t;
        let key = [
            a.axis as u64,
            f.to_bits(),
            left.to_bits(),
            right.to_bits(),
            n as u64,
        ];
        if let Some(value) = cache.get(&key) {
            work.continuity_cache_hits += 1;
            return *value;
        }
        if let Some(budget) = budget.as_deref_mut() {
            if !budget.pixels(n, work) {
                return None;
            }
        }
        let mut values = [0.; 192];
        let (mut lo, mut hi) = (255f64, 0f64);
        for (j, v) in values[..n].iter_mut().enumerate() {
            let point_sample = point(
                matrix,
                a.axis,
                left + (right - left) * (crate::numeric::usize_f64(j) + 0.5)
                    / crate::numeric::usize_f64(n),
                f,
            )
            .unwrap();
            *v = im.gray(point_sample[0].round(), point_sample[1].round());
            work.continuity_samples += 1;
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        if hi - lo < 8. {
            work.continuity_rejects += 1;
            if cache.len() < 4096 {
                cache.insert(key, None);
            }
            return None;
        }
        let result = Some((
            values[..n]
                .iter()
                .filter(|&&v| v < lo + 0.35 * (hi - lo))
                .count(),
            values[..n]
                .iter()
                .filter(|&&v| v > lo + 0.65 * (hi - lo))
                .count(),
        ));
        if cache.len() < 4096 {
            cache.insert(key, result);
        }
        result
    };
    let Some(first) = sample(0., 32) else {
        return false;
    };
    if steps == 0 {
        return true;
    }
    let Some(last) = sample(1., 32) else {
        return false;
    };
    let minimum = (first.0.min(last.0), first.1.min(last.1));
    for i in 1..steps {
        let Some((dark, _light)) = sample(
            crate::numeric::usize_f64(i) / crate::numeric::usize_f64(steps),
            32,
        ) else {
            return false;
        };
        // For ordinary dark bars on a light substrate, require lost DARK occupancy.
        // A bright specular maximum can reduce normalized light occupancy while
        // preserving every black bar; that alone is not a separating gap.
        // Only a large dark-density collapse relative to BOTH endpoints proves
        // a separator. Weak endpoints cannot establish a stronger density promise.
        if minimum.0 >= 6 && minimum.1 >= 6 && dark * 2 < minimum.0 {
            // Sparse samples can alias an ordinary EAN row. Confirm a suspected
            // separator against densely sampled endpoint and intervening profiles.
            let (Some(da), Some(db), Some(dc)) = (
                sample(0., 192),
                sample(1., 192),
                sample(
                    crate::numeric::usize_f64(i) / crate::numeric::usize_f64(steps),
                    192,
                ),
            ) else {
                return false;
            };
            let dm = (da.0.min(db.0), da.1.min(db.1));
            if dm.0 >= 36 && dm.1 >= 36 && dc.0 * 2 < dm.0 {
                work.continuity_rejects += 1;
                return false;
            }
        }
    }
    true
}

pub(super) fn assemble(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
) -> Vec<Detection> {
    let mut results = vec![];
    for axis in 0..2 {
        let mut group: Vec<Observation> = vec![];
        let emit = |g: &[Observation], out: &mut Vec<Detection>| {
            if g.len() < 2 {
                return;
            }
            let a = g[0];
            let b = *g.last().unwrap();
            if b.fraction - a.fraction < 0.149_999 {
                return;
            }
            let polygon = [
                point(m, axis, a.left, a.fraction).unwrap(),
                point(m, axis, a.right, a.fraction).unwrap(),
                point(m, axis, b.right, b.fraction).unwrap(),
                point(m, axis, b.left, b.fraction).unwrap(),
            ];
            if scan::transform(polygon).is_ok() {
                out.push(Detection {
                    digits: a.digits,
                    polygon,
                    support: g.len(),
                    axis,
                });
            }
        };
        for &b in observations.iter().filter(|r| r.axis == axis) {
            if let Some(&a) = group.last() {
                let overlap = a.right.min(b.right) - a.left.max(b.left);
                if a.digits != b.digits
                    || overlap <= 0.8 * (a.right - a.left).max(b.right - b.left)
                    || !connected(im, m, a, b, work)
                {
                    emit(&group, &mut results);
                    group.clear();
                }
            }
            group.push(b);
        }
        emit(&group, &mut results);
    }
    results
}

/// Associate multiple intervals on each scanline with distinct spatial tracks.
/// A track receives at most one observation per fraction. Non-unique links are
/// left unassociated, rather than joining neighboring equal-value instances.
#[cfg(test)]
pub(crate) fn assemble_many(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
) -> Vec<Detection> {
    assemble_many_budget(
        im,
        m,
        observations,
        work,
        source_support,
        &mut AssociationBudget::default(),
    )
}

pub(super) fn canonical_row(f: f64) -> f64 {
    (f * 1e12).round() / 1e12
}

pub(crate) fn assemble_many_budget(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
    budget: &mut AssociationBudget,
) -> Vec<Detection> {
    assemble_many_budget_options(im, m, observations, work, source_support, budget, false)
}

pub(crate) fn assemble_many_budget_options(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
    budget: &mut AssociationBudget,
    allow_single_row: bool,
) -> Vec<Detection> {
    let timer = Timer::now();
    let result = assemble_many_budget_inner(
        im,
        m,
        observations,
        work,
        source_support,
        budget,
        allow_single_row,
    );
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    {
        work.support_ms += timer.ms();
    }
    let _ = timer;
    result
}

#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
pub(super) fn invalid_contains(detection: &Detection, p: [f64; 2]) -> bool {
    let (mut positive, mut negative) = (false, false);
    for i in 0..4 {
        let a = detection.polygon[i];
        let b = detection.polygon[(i + 1) % 4];
        let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        positive |= c > 1e-6;
        negative |= c < -1e-6;
    }
    !(positive && negative)
}

#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
pub(super) fn invalid_interval(
    matrix: [f64; 9],
    t: &Detection,
    o: &Observation,
) -> Option<([f64; 2], [f64; 2])> {
    if o.axis != t.axis || o.digits != t.digits {
        return None;
    }
    let (a, b) = (
        point(matrix, o.axis, o.left, o.fraction).ok()?,
        point(matrix, o.axis, o.right, o.fraction).ok()?,
    );
    (invalid_contains(t, a) && invalid_contains(t, b)).then_some((a, b))
}

#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
pub(super) fn invalid_track_spaced(
    m: [f64; 9],
    t: &Detection,
    observations: &[Observation],
    budget: &mut AssociationBudget,
    work: &mut Work,
) -> bool {
    if t.support < 3 {
        return false;
    }
    let mut rows = Vec::new();
    for o in observations {
        if !budget.check(work) {
            return false;
        }
        if let Some((a, b)) = invalid_interval(m, t, o) {
            rows.push((o.fraction, a, b));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut selected: Vec<([f64; 2], [f64; 2])> = Vec::new();
    'rows: for (_, a, b) in rows {
        for (x, y) in &selected {
            if !budget.check(work) {
                return false;
            }
            let v = [y[0] - x[0], y[1] - x[1]];
            let n = v[0].hypot(v[1]);
            if n <= 0.
                || ((a[0] - x[0]) * v[1] - (a[1] - x[1]) * v[0]).abs() / n < 1. - 1e-6
                || ((b[0] - y[0]) * v[1] - (b[1] - y[1]) * v[0]).abs() / n < 1. - 1e-6
            {
                continue 'rows;
            }
        }
        selected.push((a, b));
        if selected.len() == 3 {
            return true;
        }
    }
    false
}

#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
#[expect(
    clippy::too_many_lines,
    reason = "This evidence pass keeps ordered hypotheses, contradiction vetoes and work accounting together within one scan transaction."
)]
pub(super) fn assemble_many_budget_inner(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
    budget: &mut AssociationBudget,
    allow_single_row: bool,
) -> Vec<Detection> {
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        if observations.iter().any(|o| o.invalid_checksum) {
            let invalid: Vec<_> = observations
                .iter()
                .filter(|o| o.invalid_checksum)
                .map(|o| {
                    let mut o = *o;
                    o.invalid_checksum = false;
                    o.ambiguous = false;
                    o
                })
                .collect();
            // Same grouping/continuity machinery, but invalid values are never emitted.
            let tracks: Vec<_> =
                assemble_many_budget_inner(im, m, &invalid, work, source_support, budget, false)
                    .into_iter()
                    .filter(|t| invalid_track_spaced(m, t, &invalid, budget, work))
                    .collect();
            let mut valid: Vec<_> = observations
                .iter()
                .filter(|o| !o.invalid_checksum)
                .copied()
                .collect();
            for mut o in invalid {
                let mut supported = false;
                for t in &tracks {
                    if !budget.check(work) {
                        break;
                    }
                    if invalid_interval(m, t, &o).is_some() {
                        supported = true;
                        break;
                    }
                }
                if supported {
                    o.ambiguous = true;
                    valid.push(o);
                    work.invalid_consensus_blocks += 1;
                }
                if work.association_truncated > 0 {
                    break;
                }
            }
            return assemble_many_budget_inner(
                im,
                m,
                &valid,
                work,
                source_support,
                budget,
                allow_single_row,
            );
        }
    }

    let mut density_cache = std::collections::HashMap::new();
    let mut results = vec![];
    for axis in 0..2 {
        let mut obs: Vec<_> = observations
            .iter()
            .copied()
            .filter(|o| o.axis == axis)
            .map(|mut o| {
                o.fraction = canonical_row(o.fraction);
                o
            })
            .collect();
        obs.sort_by(|a, b| {
            a.fraction
                .total_cmp(&b.fraction)
                .then(a.left.total_cmp(&b.left))
        });
        // Different sampling windows can observe the same physical interval on one
        // row. Consolidate only heavily overlapping equal evidence; veto conflicts.
        let mut keep: Vec<_> = obs.iter().map(|o| !o.ambiguous).collect();
        for i in 0..obs.len() {
            for j in i + 1..obs.len() {
                if obs[j].fraction != obs[i].fraction {
                    break;
                }
                if !budget.check(work) {
                    return results;
                }
                let overlap = obs[i].right.min(obs[j].right) - obs[i].left.max(obs[j].left);
                if overlap > 0.
                    && (obs[i].ambiguous || obs[j].ambiguous || obs[i].digits != obs[j].digits)
                {
                    keep[i] = false;
                    keep[j] = false;
                    work.conflicts += 1;
                }
            }
        }
        let barriers: Vec<_> = obs
            .iter()
            .zip(&keep)
            .filter_map(|(o, &keep)| (!keep).then_some(*o))
            .collect();
        let mut unique: Vec<Observation> = vec![];
        for (i, o) in obs.into_iter().enumerate() {
            if !keep[i] {
                continue;
            }
            let mut found = false;
            for a in unique
                .iter_mut()
                .rev()
                .take_while(|a| a.fraction == o.fraction)
            {
                if !budget.check(work) {
                    return results;
                }
                if a.digits == o.digits
                    && a.right.min(o.right) - a.left.max(o.left)
                        > 0.8 * (a.right - a.left).max(o.right - o.left)
                {
                    if (!o.short_quiet && a.short_quiet)
                        || (o.short_quiet == a.short_quiet && o.cost < a.cost)
                    {
                        *a = o;
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                unique.push(o);
            }
        }
        let obs = unique;
        let mut tracks: Vec<Vec<Observation>> = vec![];
        let mut active: Vec<bool> = vec![];
        let mut offset = 0;
        'rows: while offset < obs.len() {
            let end = offset
                + obs[offset..]
                    .iter()
                    .take_while(|o| o.fraction == obs[offset].fraction)
                    .count();
            let row = &obs[offset..end];
            // Decoded contradictory evidence between matching rows ends the earlier
            // track. Without this, alternating values in touching stacked symbols can
            // reconnect across a different symbol because contrast alone stays high.
            for (i, t) in tracks.iter().enumerate() {
                if !budget.check(work) {
                    break 'rows;
                }
                if !active[i] {
                    continue;
                }
                let a = *t.last().unwrap();
                for b in &barriers {
                    if !budget.check(work) {
                        break 'rows;
                    }
                    if b.fraction <= a.fraction || b.fraction > row[0].fraction {
                        continue;
                    }
                    if a.right.min(b.right) > a.left.max(b.left) {
                        active[i] = false;
                        break;
                    }
                }
                for b in row {
                    if !budget.check(work) {
                        break 'rows;
                    }
                    if a.digits != b.digits
                        && a.right.min(b.right) - a.left.max(b.left)
                            > 0.8 * (a.right - a.left).max(b.right - b.left)
                    {
                        active[i] = false;
                        break;
                    }
                }
            }
            let mut links = vec![vec![]; row.len()];
            let mut degree = vec![0; tracks.len()];
            for (j, &b) in row.iter().enumerate() {
                for (i, t) in tracks.iter().enumerate() {
                    if !budget.check(work) {
                        break 'rows;
                    }
                    let a = *t.last().unwrap();
                    let overlap = a.right.min(b.right) - a.left.max(b.left);
                    if active[i]
                        && a.digits == b.digits
                        && overlap > 0.8 * (a.right - a.left).max(b.right - b.left)
                        && connected_budget(im, m, a, b, work, Some(budget), &mut density_cache)
                    {
                        links[j].push(i);
                        degree[i] += 1;
                    }
                    if work.association_truncated > 0 {
                        break 'rows;
                    }
                }
            }
            for (j, &b) in row.iter().enumerate() {
                if links[j].len() == 1 && degree[links[j][0]] == 1 {
                    tracks[links[j][0]].push(b);
                } else {
                    tracks.push(vec![b]);
                    active.push(true);
                }
            }
            offset = end;
        }
        'tracks: for t in tracks {
            let a = t[0];
            let b = *t.last().unwrap();
            let supported =
                t.len() >= 2 && (t.iter().filter(|o| !o.short_quiet).count() >= 2 || t.len() >= 4);
            let separation = if source_support {
                let cross = distance(
                    point(m, axis, 0.5, 0.).unwrap(),
                    point(m, axis, 0.5, 1.).unwrap(),
                );
                // Weak visual evidence requires a wider physical baseline. Adjacent
                // interpolated rows can repeat the same edge defect or parity alias.
                let strong = t.iter().any(|o| o.cost <= 0.08 && o.gap >= 0.08);
                let left = point(m, axis, a.left, a.fraction).unwrap();
                let right = point(m, axis, a.right, a.fraction).unwrap();
                let pixels = if strong {
                    1.
                } else {
                    {
                        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                        {
                            (distance(left, right)
                                / match a.digits[0] {
                                    14 => 67.,
                                    18 => 51.,
                                    _ => 95.,
                                })
                            .clamp(2., 12.)
                        }
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        {
                            (distance(left, right) / 95.).clamp(2., 12.)
                        }
                    }
                };
                pixels / cross.max(1.)
            } else {
                0.15
            };
            if !supported || b.fraction - a.fraction < separation - 1e-6 {
                if allow_single_row {
                    // Retain the row-level ambiguity veto and full quiet-zone requirement.
                    // A permissive read claims a one-source-pixel band, not the entire region.
                    if let Some(o) = t
                        .iter()
                        .filter(|o| !o.short_quiet && (o.cost <= 0.06 && o.gap >= 0.1))
                        .min_by(|a, b| a.cost.total_cmp(&b.cost))
                    {
                        let cross = distance(
                            point(m, axis, 0.5, 0.).unwrap(),
                            point(m, axis, 0.5, 1.).unwrap(),
                        );
                        if observations.iter().any(|other| {
                            other.axis == axis
                                && (other.ambiguous || other.digits != o.digits)
                                && (other.fraction - o.fraction).abs() * cross <= 2.
                                && other.right.min(o.right) - other.left.max(o.left)
                                    > 0.8 * (other.right - other.left).max(o.right - o.left)
                        }) {
                            work.conflicts += 1;
                            continue 'tracks;
                        }
                        let half = 0.5 / cross.max(1.);
                        let ps = [
                            point(m, axis, o.left, o.fraction - half),
                            point(m, axis, o.right, o.fraction - half),
                            point(m, axis, o.right, o.fraction + half),
                            point(m, axis, o.left, o.fraction + half),
                        ];
                        if let [Ok(p0), Ok(p1), Ok(p2), Ok(p3)] = ps {
                            let polygon = [p0, p1, p2, p3];
                            if scan::transform(polygon).is_ok() {
                                results.push(Detection {
                                    digits: o.digits,
                                    polygon,
                                    support: 1,
                                    axis,
                                });
                            }
                        }
                    }
                }
                continue;
            }
            let points = [
                point(m, axis, a.left, a.fraction),
                point(m, axis, a.right, a.fraction),
                point(m, axis, b.right, b.fraction),
                point(m, axis, b.left, b.fraction),
            ];
            if let [Ok(p0), Ok(p1), Ok(p2), Ok(p3)] = points {
                let polygon = [p0, p1, p2, p3];
                if scan::transform(polygon).is_ok() {
                    results.push(Detection {
                        digits: a.digits,
                        polygon,
                        support: t.len(),
                        axis,
                    });
                }
            }
        }
    }
    results
}
