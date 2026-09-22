//! Construct bounded retry plans independently of executing them.
use super::{experiment, Candidate, Error, Policy, Quad, Segment, Work};

/// Breadth-first interval centers spread every short prefix across the extent.
pub(super) fn spread_order(n: usize) -> Vec<usize> {
    let mut queue = std::collections::VecDeque::from([(0, n)]);
    let mut out = Vec::with_capacity(n);
    while let Some((lo, hi)) = queue.pop_front() {
        if lo >= hi {
            continue;
        }
        let mid = usize::midpoint(lo, hi);
        out.push(mid);
        queue.push_back((lo, mid));
        queue.push_back((mid + 1, hi));
    }
    out
}

#[cfg(test)]
pub(super) fn plan(m: [f64; 9], policy: Policy, work: &mut Work) -> Result<Vec<Segment>, Error> {
    scaled_plan(m, policy, work, None, false)
}

pub(super) fn scaled_plan(
    m: [f64; 9],
    policy: Policy,
    work: &mut Work,
    symbol_width: Option<f64>,
    scaled: bool,
) -> Result<Vec<Segment>, Error> {
    scaled_plan_density(m, policy, work, symbol_width, scaled, false)
}

#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
pub(super) fn scaled_plan_density(
    m: [f64; 9],
    policy: Policy,
    work: &mut Work,
    symbol_width: Option<f64>,
    scaled: bool,
    dense: bool,
) -> Result<Vec<Segment>, Error> {
    let original = scaled_plan_allowance(m, policy, work, symbol_width, scaled, dense, 128)?;

    if scaled && symbol_width.is_none() && policy.max_retry_paths_per_candidate > 128 {
        // Preserve the complete original exploratory prefix, including its
        // original row/tile order. Append only previously unscheduled work.
        // Both plans count the same full set as pending; never count it twice.
        let supplemental = scaled_plan_allowance(
            m,
            policy,
            &mut Work::default(),
            symbol_width,
            scaled,
            dense,
            256,
        )?;
        let mut paths = original;
        for s in supplemental {
            if paths.len() >= policy.max_retry_paths_per_candidate.min(256) {
                break;
            }
            if !paths.iter().any(|a| {
                a.axis == s.axis && a.fraction == s.fraction && a.lo == s.lo && a.hi == s.hi
            }) {
                paths.push(s);
            }
        }
        return Ok(paths);
    }
    Ok(original)
}

#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
#[expect(
    clippy::too_many_lines,
    reason = "The retry scheduler keeps deterministic stage order, shared budgets and unfinished-work reporting in one transaction."
)]
pub(super) fn scaled_plan_allowance(
    m: [f64; 9],
    policy: Policy,
    work: &mut Work,
    symbol_width: Option<f64>,
    scaled: bool,
    dense: bool,
    allowance: usize,
) -> Result<Vec<Segment>, Error> {
    // Scale affects effort allocation, never digit acceptance. A successful
    // symbol supplies a visual width, not evidence that its region is exhausted.
    let cross_step = symbol_width.map_or(24., |w| (w / 12.).max(24.));
    let window_pixels = symbol_width.map_or(512., |w| (w * 1.3).max(512.));
    // Unresolved regions have a declared bounded exploratory allowance; expose
    // the remainder rather than silently treating them as complete coverage.
    let limit = if scaled && symbol_width.is_none() {
        policy.max_retry_paths_per_candidate.min(allowance)
    } else {
        policy.max_retry_paths_per_candidate
    };
    let mut dimensions = [(0., 0., 0usize, 0usize); 2];
    for (axis, dimensions_entry) in dimensions.iter_mut().enumerate() {
        let length = experiment::distance(
            experiment::point(m, axis, 0., 0.5)?,
            experiment::point(m, axis, 1., 0.5)?,
        );
        let cross = experiment::distance(
            experiment::point(m, axis, 0.5, 0.)?,
            experiment::point(m, axis, 0.5, 1.)?,
        );
        // Unread proposals need sub-row coverage even when their source height is small.
        // Preserve all existing candidate-first work and global/per-candidate caps.
        let minimum_rows = if dense || (scaled && symbol_width.is_none()) {
            21.
        } else {
            5.
        };
        let requested_rows =
            crate::numeric::f64_usize((cross / cross_step).ceil().max(minimum_rows));
        let rows = requested_rows.min(128);
        // Overlapping 512-source-pixel windows complement the full source path.
        // Native path length is bounded; fixed512 remains the initial control.
        let width = (window_pixels / length.max(1.)).min(1.3);
        let requested_tiles = if width >= 1.3 {
            0
        } else {
            crate::numeric::f64_usize(((1.3 - width) / (width * 0.5)).ceil()).saturating_add(1)
        };
        let tiles = requested_tiles.min(64);
        work.sampling_plan_capped +=
            usize::from(requested_rows > rows) + usize::from(requested_tiles > tiles);
        work.retry_paths_pending = work
            .retry_paths_pending
            .saturating_add(rows.saturating_mul(1 + tiles));
        (*dimensions_entry) = (length, width, rows, tiles);
    }
    let orders: [Vec<usize>; 2] = std::array::from_fn(|axis| {
        if scaled {
            spread_order(dimensions[axis].2)
        } else {
            (0..dimensions[axis].2).collect()
        }
    });
    let mut paths = Vec::new();
    // Unknown-scale prefixes alternate whole rows and spatially spread tiles.
    // Known-scale control keeps the previous complete-row ordering.
    let tile_orders: [Vec<usize>; 2] = std::array::from_fn(|axis| spread_order(dimensions[axis].3));
    let unknown = scaled && symbol_width.is_none();
    let schedule: Box<dyn Iterator<Item = (usize, usize, usize)>> =
        if unknown {
            Box::new((0..128).flat_map(|row| {
                (0..2).flat_map(move |kind| (0..2).map(move |axis| (kind, row, axis)))
            }))
        } else {
            Box::new((0..=64).flat_map(|tile| {
                (0..128).flat_map(move |row| (0..2).map(move |axis| (tile, row, axis)))
            }))
        };
    let mut scheduled = [0usize; 2];
    for (mut tile, row, axis) in schedule {
        if unknown && tile == 1 {
            let order = &tile_orders[axis];
            if order.is_empty() {
                continue;
            }
            tile = 1 + order[row % order.len()];
        }
        let (length, width, rows, tiles) = dimensions[axis];
        if row >= rows || tile > tiles {
            continue;
        }
        if paths.len() >= limit {
            return Ok(paths);
        }
        if unknown {
            let kind = usize::from(tile > 0);
            if scheduled[kind] >= allowance / 2 {
                continue;
            }
            scheduled[kind] += 1;
        }
        let fraction =
            (crate::numeric::usize_f64(orders[axis][row]) + 0.5) / crate::numeric::usize_f64(rows);
        let (lo, hi, samples) = if tile == 0 {
            (
                -0.15,
                1.15,
                crate::numeric::f64_usize((length * 1.3).ceil().clamp(64., 4096.)),
            )
        } else {
            let lo = -0.15
                + (1.3 - width) * crate::numeric::usize_f64(tile - 1)
                    / crate::numeric::usize_f64((tiles - 1).max(1));
            (lo, lo + width, {
                crate::numeric::f64_usize((length * width).ceil().clamp(512., 4096.))
            })
        };
        paths.push(Segment {
            axis,
            fraction,
            lo,
            hi,
            samples,
            sample_cap: if tile == 0 {
                length * 1.3 > 4096.
            } else {
                length * width > 4096.
            },
            unresolved: false,
        });
    }
    // Preserve the exploratory prefix, then visit its unselected row/tile pairs.
    // These paths were already counted as pending; the same allowance still applies.

    if unknown {
        #[expect(
            clippy::needless_range_loop,
            reason = "Rows and tiles are ranks across BOTH axes with unequal list lengths; iterating either axis alone omits pending work on the other."
        )]
        for tile_rank in 0..64 {
            for row in 0..128 {
                for axis in 0..2 {
                    let (length, width, rows, tiles) = dimensions[axis];
                    if row >= rows || tile_rank >= tiles {
                        continue;
                    }
                    if paths.len() >= limit {
                        return Ok(paths);
                    }
                    let tile = tile_orders[axis][tile_rank];
                    let fraction = (crate::numeric::usize_f64(orders[axis][row]) + 0.5)
                        / crate::numeric::usize_f64(rows);
                    let lo = -0.15
                        + (1.3 - width) * crate::numeric::usize_f64(tile)
                            / crate::numeric::usize_f64((tiles - 1).max(1));
                    let hi = lo + width;
                    if paths.iter().any(|s| {
                        s.axis == axis && s.fraction == fraction && s.lo == lo && s.hi == hi
                    }) {
                        continue;
                    }
                    paths.push(Segment {
                        axis,
                        fraction,
                        lo,
                        hi,
                        samples: {
                            crate::numeric::f64_usize((length * width).ceil().clamp(512., 4096.))
                        },
                        sample_cap: length * width > 4096.,
                        unresolved: false,
                    });
                }
            }
        }
    }
    Ok(paths)
}

#[expect(
    clippy::many_single_char_names,
    reason = "The 2x2 projective inverse uses conventional a,b,c,d,e,g coefficients and paired x,y and u,v coordinates."
)]
pub(super) fn project_claim(matrix: [f64; 9], quad: Quad) -> Option<Quad> {
    let mut points = [[0.; 2]; 4];
    for (i, p) in quad.iter().enumerate() {
        let (x, y) = (p[0] + 0.5, p[1] + 0.5);
        let (a, b, c, d, e, g) = (
            matrix[0] - x * matrix[6],
            matrix[1] - x * matrix[7],
            x * matrix[8] - matrix[2],
            matrix[3] - y * matrix[6],
            matrix[4] - y * matrix[7],
            y * matrix[8] - matrix[5],
        );
        let det = a * e - b * d;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let (u, v) = ((c * e - b * g) / det, (a * g - c * d) / det);
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        points[i] = [u, v];
    }
    Some(points)
}

pub(super) fn projected_interval(points: Quad, axis: usize, fraction: f64) -> Option<(f64, f64)> {
    let mut xs = [0.; 4];
    let mut count = 0;
    for i in 0..4 {
        let (a, b) = (points[i], points[(i + 1) % 4]);
        let (a, b) = if axis == 0 {
            (a, b)
        } else {
            ([a[1], a[0]], [b[1], b[0]])
        };
        if (a[1] <= fraction && fraction < b[1]) || (b[1] <= fraction && fraction < a[1]) {
            xs[count] = a[0] + (b[0] - a[0]) * (fraction - a[1]) / (b[1] - a[1]);
            count += 1;
        }
    }
    if count != 2 {
        return None;
    }
    if xs[0].total_cmp(&xs[1]).is_gt() {
        xs.swap(0, 1);
    }
    Some((xs[0], xs[1]))
}

pub(super) fn claimed_interval(
    matrix: [f64; 9],
    axis: usize,
    fraction: f64,
    quad: Quad,
) -> Option<(f64, f64)> {
    projected_interval(project_claim(matrix, quad)?, axis, fraction)
}

pub(super) fn unresolved_plan(
    m: [f64; 9],
    detections: &[experiment::Detection],
    limit: usize,
    work: &mut Work,
) -> Result<Vec<Segment>, Error> {
    let mut paths = Vec::new();
    let mut orders: [Vec<usize>; 2] = [vec![], vec![]];
    for (axis, orders_entry) in orders.iter_mut().enumerate() {
        let cross = experiment::distance(
            experiment::point(m, axis, 0.5, 0.)?,
            experiment::point(m, axis, 0.5, 1.)?,
        );
        let requested = crate::numeric::f64_usize((cross / 24.).ceil().max(5.));
        let rows = requested.min(128);
        work.sampling_plan_capped += usize::from(requested > rows);
        (*orders_entry) = spread_order(rows);
    }
    for rank in 0..128 {
        for (axis, orders_entry) in orders.iter().enumerate() {
            let Some(&row) = (*orders_entry).get(rank) else {
                continue;
            };
            let rows = (*orders_entry).len();
            let fraction = (crate::numeric::usize_f64(row) + 0.5) / crate::numeric::usize_f64(rows);
            let mut claimed: Vec<_> = detections
                .iter()
                .take(64)
                .filter_map(|d| claimed_interval(m, axis, fraction, d.polygon))
                .collect();
            claimed.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut intervals = Vec::new();
            let mut lo = -0.15f64;
            for (a, b) in claimed {
                let a = a.clamp(-0.15, 1.15);
                let b = b.clamp(-0.15, 1.15);
                if a > lo {
                    intervals.push((lo, a));
                }
                lo = lo.max(b);
            }
            if lo < 1.15 {
                intervals.push((lo, 1.15));
            }
            for (lo, hi) in intervals {
                let length = experiment::distance(
                    experiment::point(m, axis, lo, fraction)?,
                    experiment::point(m, axis, hi, fraction)?,
                );
                // Below the existing 0.8sample/module gate no EAN can fit at
                // source resolution. This is a sampling limit, not an instance cap.
                if length < 76. {
                    continue;
                }
                work.retry_paths_pending += 1;
                if paths.len() < limit {
                    paths.push(Segment {
                        axis,
                        fraction,
                        lo,
                        hi,
                        samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
                        sample_cap: length > 4096.,
                        unresolved: true,
                    });
                }
            }
        }
    }
    Ok(paths)
}

pub(super) fn supported_scale_width(m: [f64; 9], c: &Candidate) -> Option<f64> {
    supported_scale_width_checked(m, c, false)
}

pub(super) fn supported_scale_width_checked(
    matrix: [f64; 9],
    c: &Candidate,
    all_widths: bool,
) -> Option<f64> {
    let (o, a, b, width) = c
        .observations
        .iter()
        .filter(|o| !o.ambiguous)
        .filter_map(|o| {
            let a = experiment::point(matrix, o.axis, o.left, o.fraction).ok()?;
            let b = experiment::point(matrix, o.axis, o.right, o.fraction).ok()?;
            let width = experiment::distance(a, b);
            (width.is_finite() && width > 0.).then_some((o, a, b, width))
        })
        .min_by(|a, b| a.3.total_cmp(&b.3))?;

    if width <= 285. || all_widths {
        let contains = |quad: &Quad, p: [f64; 2]| {
            let (mut positive, mut negative) = (false, false);
            for i in 0..4 {
                let a = quad[i];
                let b = quad[(i + 1) % 4];
                let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                let tol = 2. * experiment::distance(a, b);
                positive |= cross > tol;
                negative |= cross < -tol;
            }
            !(positive && negative)
        };
        if !c.detections.iter().any(|d| {
            d.axis == o.axis
                && d.digits == o.digits
                && contains(&d.polygon, a)
                && contains(&d.polygon, b)
        }) {
            return None;
        }
    }

    Some(width)
}
