//! Bounded find-all research policy. Work limits describe sampling effort, never
//! expected symbol counts. All original candidates receive the fixed pass before
//! round-robin retries, including candidates that already produced a read.
#![forbid(unsafe_code)]
use crate::scanner_clock::Timer;
use crate::{
    experiment::{self, Candidate, Experiment, Work},
    sampling::{Error, ImageView},
    scan::{self, Quad},
};

#[expect(
    clippy::struct_excessive_bools,
    reason = "These independent research switches form a combinatorial experiment policy, not mutually exclusive states."
)]
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub max_retry_paths_per_candidate: usize,
    pub max_retry_paths_per_frame: usize,
    pub max_association_checks: usize,
    pub max_association_pixels: usize,
    pub max_results: usize,
    pub transition_cleanup: bool,
    pub source_identity: bool,
    pub interior_normalization: bool,
    pub guard_bias: bool,
    pub allow_single_row: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            max_retry_paths_per_candidate: 512,
            max_retry_paths_per_frame: 8192,
            max_association_checks: 200_000,
            max_association_pixels: 2_000_000,
            max_results: 1024,
            transition_cleanup: false,
            source_identity: false,
            interior_normalization: false,
            guard_bias: false,
            allow_single_row: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Segment {
    axis: usize,
    fraction: f64,
    lo: f64,
    hi: f64,
    samples: usize,
    sample_cap: bool,
    unresolved: bool,
}

#[cfg(feature = "experimental-verified-coverage-reuse")]
#[path = "verified_coverage.rs"]
mod verified_coverage;
#[cfg(feature = "experimental-verified-coverage-reuse")]
use verified_coverage::{reuse_plan, verified_claims, ReuseBudget};

/// Breadth-first interval centers spread every short prefix across the extent.
fn spread_order(n: usize) -> Vec<usize> {
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
fn plan(m: [f64; 9], policy: Policy, work: &mut Work) -> Result<Vec<Segment>, Error> {
    scaled_plan(m, policy, work, None, false)
}

fn scaled_plan(
    m: [f64; 9],
    policy: Policy,
    work: &mut Work,
    symbol_width: Option<f64>,
    scaled: bool,
) -> Result<Vec<Segment>, Error> {
    scaled_plan_density(m, policy, work, symbol_width, scaled, false)
}
#[cfg_attr(
    feature = "experimental-unresolved-256",
    expect(
        clippy::float_cmp,
        reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
    )
)]
fn scaled_plan_density(
    m: [f64; 9],
    policy: Policy,
    work: &mut Work,
    symbol_width: Option<f64>,
    scaled: bool,
    dense: bool,
) -> Result<Vec<Segment>, Error> {
    let original = scaled_plan_allowance(m, policy, work, symbol_width, scaled, dense, 128)?;
    #[cfg(feature = "experimental-unresolved-256")]
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
#[cfg_attr(
    feature = "experimental-complete-tile-prefix",
    expect(
        clippy::float_cmp,
        reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
    )
)]
#[expect(
    clippy::too_many_lines,
    reason = "The retry scheduler keeps deterministic stage order, shared budgets and unfinished-work reporting in one transaction."
)]
fn scaled_plan_allowance(
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
            (
                lo,
                lo + width,
                if cfg!(feature = "experimental-native-wide-tiles") {
                    crate::numeric::f64_usize((length * width).ceil().clamp(512., 4096.))
                } else {
                    512
                },
            )
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
                cfg!(feature = "experimental-native-wide-tiles") && length * width > 4096.
            },
            unresolved: false,
        });
    }
    // Preserve the exploratory prefix, then visit its unselected row/tile pairs.
    // These paths were already counted as pending; the same allowance still applies.
    #[cfg(feature = "experimental-complete-tile-prefix")]
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
                        samples: if cfg!(feature = "experimental-native-wide-tiles") {
                            crate::numeric::f64_usize((length * width).ceil().clamp(512., 4096.))
                        } else {
                            512
                        },
                        sample_cap: cfg!(feature = "experimental-native-wide-tiles")
                            && length * width > 4096.,
                        unresolved: false,
                    });
                }
            }
        }
    }
    Ok(paths)
}
// A decoded polygon claims only its actual supported band. Invert source
// coordinates to find its intersection with a prospective fine probe row.
#[expect(
    clippy::many_single_char_names,
    reason = "The 2x2 projective inverse uses conventional a,b,c,d,e,g coefficients and paired x,y and u,v coordinates."
)]
fn claimed_interval(
    matrix: [f64; 9],
    axis: usize,
    fraction: f64,
    quad: Quad,
) -> Option<(f64, f64)> {
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
        points[i] = if axis == 0 { [u, v] } else { [v, u] };
    }
    let mut xs = Vec::with_capacity(4);
    for i in 0..4 {
        let (a, b) = (points[i], points[(i + 1) % 4]);
        if (a[1] <= fraction && fraction < b[1]) || (b[1] <= fraction && fraction < a[1]) {
            xs.push(a[0] + (b[0] - a[0]) * (fraction - a[1]) / (b[1] - a[1]));
        }
    }
    if xs.len() != 2 {
        return None;
    }
    xs.sort_by(f64::total_cmp);
    Some((xs[0], xs[1]))
}
fn unresolved_plan(
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
// A low-resolution hint must belong to an already supported spatial symbol.
// Two unrelated single-row reads are not corroboration of either width.
fn supported_scale_width(m: [f64; 9], c: &Candidate) -> Option<f64> {
    supported_scale_width_checked(m, c, false)
}
fn supported_scale_width_checked(matrix: [f64; 9], c: &Candidate, all_widths: bool) -> Option<f64> {
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
    #[cfg(feature = "experimental-single-row-search")]
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
    #[cfg(not(feature = "experimental-single-row-search"))]
    let _ = (o, a, b, all_widths);
    Some(width)
}
impl Experiment {
    /// Diagnostic only: explicit bounded source rows through real retry/assembly rules.
    /// The caller's rows are not a deployable scheduler or a coverage claim.
    /// # Errors
    /// Returns `Parameters` for invalid scan limits or schedule inputs; propagates invalid quadrilateral and sampling errors.
    pub fn diagnostic_retry_rows(
        &mut self,
        im: ImageView<'_>,
        q: Quad,
        fractions: &[f64],
    ) -> Result<Candidate, Error> {
        if fractions.len() > 64
            || fractions
                .iter()
                .any(|f| !f.is_finite() || !(0.0..=1.0).contains(f))
        {
            return Err(Error::Parameters);
        }
        let m = scan::transform(q)?;
        let mut c = Candidate {
            index: 0,
            coverage: q,
            observations: vec![],
            detections: vec![],
            work: Work::default(),
            ms: 0.,
            error: false,
        };
        for &fraction in fractions {
            let length = experiment::distance(
                experiment::point(m.0, 0, -0.15, fraction)?,
                experiment::point(m.0, 0, 1.15, fraction)?,
            );
            let s = Segment {
                axis: 0,
                fraction,
                lo: -0.15,
                hi: 1.15,
                samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
                sample_cap: length > 4096.,
                unresolved: false,
            };
            self.retry_segment(im, m.0, s, &mut c, true, true, true);
        }
        c.detections = experiment::assemble_many_budget(
            im,
            m.0,
            &c.observations,
            &mut c.work,
            true,
            &mut experiment::AssociationBudget::default(),
        );
        Ok(c)
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "A retry couples its path geometry, candidate accumulator and independent decoder policies."
    )]
    fn retry_segment(
        &mut self,
        im: ImageView<'_>,
        m: [f64; 9],
        s: Segment,
        c: &mut Candidate,
        cleanup: bool,
        interior: bool,
        guard_bias: bool,
    ) {
        #[cfg(all(feature = "diagnostic-tile-events", not(target_arch = "wasm32")))]
        let observation_start = c.observations.len();
        let start = Timer::now();
        c.work.paths += 1;
        c.work.retry_paths += 1;
        c.work.capped_paths += usize::from(s.sample_cap);
        c.work.unresolved_probe_paths += usize::from(s.unresolved);
        let mut normalized = false;
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        eprintln!("{{\"retry\":true,\"axis\":{},\"fraction\":{},\"lo\":{},\"hi\":{},\"samples\":{},\"normalization\":\"global\"}}",s.axis,s.fraction,s.lo,s.hi,s.samples);
        match self.sample_segment(
            im,
            m,
            s.axis,
            s.fraction,
            s.lo,
            s.hi,
            s.samples,
            interior,
            &mut c.work,
        ) {
            Ok(true) => {
                normalized = true;
                self.collect_policy(
                    s.axis,
                    s.fraction,
                    s.lo,
                    s.hi,
                    &mut c.work,
                    &mut c.observations,
                    cleanup,
                    guard_bias,
                );
            }
            Ok(false) => {
                if s.unresolved && self.normalize_sparse_signal() {
                    normalized = true;
                    c.work.sparse_normalizations += 1;
                    self.collect_policy(
                        s.axis,
                        s.fraction,
                        s.lo,
                        s.hi,
                        &mut c.work,
                        &mut c.observations,
                        cleanup,
                        guard_bias,
                    );
                } else {
                    c.work.low_contrast += 1;
                }
            }
            Err(_) => c.error = true,
        }
        if interior && !c.error && self.normalize_interior(s.lo, s.hi, &mut c.work) {
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            eprintln!("{{\"normalization\":\"interior\"}}");
            normalized = true;
            self.collect_policy(
                s.axis,
                s.fraction,
                s.lo,
                s.hi,
                &mut c.work,
                &mut c.observations,
                cleanup,
                guard_bias,
            );
        }
        #[cfg(feature = "experimental-native-sharpen")]
        if interior
            && !c.error
            && s.lo == -0.15
            && s.hi == 1.15
            && self.sharpen_native_profile(normalized)
        {
            self.collect_policy(
                s.axis,
                s.fraction,
                s.lo,
                s.hi,
                &mut c.work,
                &mut c.observations,
                cleanup,
                guard_bias,
            );
        }
        #[cfg(not(feature = "experimental-native-sharpen"))]
        let _ = normalized;
        #[cfg(all(feature = "diagnostic-tile-events", not(target_arch = "wasm32")))]
        eprintln!("{{\"candidate\":{},\"retry\":{},\"axis\":{},\"fraction\":{},\"lo\":{},\"hi\":{},\"samples\":{},\"observations\":{:?},\"ambiguous\":{}}}",c.index,c.work.retry_paths,s.axis,s.fraction,s.lo,s.hi,s.samples,c.observations[observation_start..].iter().filter(|o|!o.ambiguous).map(|o|o.digits).collect::<Vec<_>>(),c.observations[observation_start..].iter().filter(|o|o.ambiguous).count());
        c.ms += start.ms();
    }
    /// # Errors
    /// Returns `Parameters` for invalid scan limits or schedule inputs; propagates invalid quadrilateral and sampling errors.
    pub fn scan_all(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
    ) -> Result<Vec<Candidate>, Error> {
        self.scan_policy(im, candidates, policy, false)
    }
    /// # Errors
    /// Returns `Parameters` for invalid scan limits or schedule inputs; propagates invalid quadrilateral and sampling errors.
    pub fn scan_scaled(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
    ) -> Result<Vec<Candidate>, Error> {
        self.scan_policy(im, candidates, policy, true)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The retry scheduler keeps deterministic stage order, shared budgets and unfinished-work reporting in one transaction."
    )]
    fn scan_policy(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
        scaled: bool,
    ) -> Result<Vec<Candidate>, Error> {
        if candidates.len() > 64
            || policy.max_retry_paths_per_candidate > 4096
            || policy.max_retry_paths_per_frame > 65536
            || policy.max_association_checks > 2_000_000
            || policy.max_association_pixels > 16_000_000
            || !(1..=4096).contains(&policy.max_results)
        {
            return Err(Error::Parameters);
        }
        // The first pass is completed across every initial candidate before any
        // retry. Coverage is retained independently of observations/detections.
        let mut budget = experiment::AssociationBudget {
            checks_left: policy.max_association_checks,
            pixels_left: policy.max_association_pixels,
        };
        let mut outputs =
            self.scan_with_budget(im, candidates, experiment::MULTI_FIXED, &mut budget);
        let mut used = 0;
        if scaled {
            // Independent broad discovery after every candidate's fixed pass.
            // Ten source paths continue even after success and may reveal smaller
            // or repeated symbols before selecting subdivision scale.
            for i in 0..10 {
                for c in &mut outputs {
                    if used >= policy.max_retry_paths_per_frame
                        || c.work.retry_paths >= policy.max_retry_paths_per_candidate
                    {
                        c.work.retry_paths_pending += 1;
                        continue;
                    }
                    let Ok(m) = scan::transform(c.coverage) else {
                        continue;
                    };
                    let axis = i % 2;
                    let fraction = 0.1 + 0.2 * crate::numeric::usize_f64(i / 2);
                    let (Ok(a), Ok(b)) = (
                        experiment::point(m.0, axis, -0.15, fraction),
                        experiment::point(m.0, axis, 1.15, fraction),
                    ) else {
                        c.error = true;
                        continue;
                    };
                    let length = experiment::distance(a, b);
                    let segment = Segment {
                        axis,
                        fraction,
                        lo: -0.15,
                        hi: 1.15,
                        samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
                        sample_cap: length > 4096.,
                        unresolved: false,
                    };
                    self.retry_segment(
                        im,
                        m.0,
                        segment,
                        c,
                        policy.transition_cleanup,
                        policy.interior_normalization,
                        policy.guard_bias,
                    );
                    used += 1;
                    c.work.discovery_paths += 1;
                }
            }
        }
        #[cfg(feature = "experimental-structural-retry")]
        for c in &mut outputs {
            c.work.structural_discovery_complete = true;
        }
        #[cfg(feature = "experimental-verified-coverage-reuse")]
        let mut reuse_budget = ReuseBudget {
            remaining: policy.max_association_checks,
            pixels_remaining: 262_144,
            ..Default::default()
        };
        #[cfg(feature = "experimental-verified-coverage-reuse")]
        let claims = {
            let mut w = Work::default();
            let q = verified_claims(&outputs, &mut reuse_budget, &mut w, im);
            if let Some(c) = outputs.first_mut() {
                c.work.extension_cache_hits += w.extension_cache_hits;
                c.work.extension_samples += w.extension_samples;
                c.work.extension_claims += w.extension_claims;
                c.work.extension_capped += w.extension_capped;
                c.work.reuse_claims += w.reuse_claims;
                c.work.reuse_claims_rejected += w.reuse_claims_rejected;
                c.work.reuse_checks += w.reuse_checks;
                c.work.reuse_checks_capped += w.reuse_checks_capped;
            }
            q
        };
        let mut plans = Vec::with_capacity(candidates.len());
        for c in &mut outputs {
            let start = Timer::now();
            let p = match scan::transform(c.coverage) {
                Ok(m) => {
                    if let Ok(p) = (|| -> Result<Vec<Segment>, Error> {
                        let width = if scaled {
                            supported_scale_width(m.0, c)
                        } else {
                            None
                        };
                        c.work.scale_hint_used = usize::from(width.is_some());
                        let remaining = Policy {
                            max_retry_paths_per_candidate: policy
                                .max_retry_paths_per_candidate
                                .saturating_sub(c.work.retry_paths),
                            ..policy
                        };
                        // A visual width may guide tile size and retain its existing work allowance,
                        // but one physical row must not reduce independent-row coverage.
                        let dense = cfg!(feature = "experimental-dense-unsupported-scale")
                            && scaled
                            && width.is_some()
                            && supported_scale_width_checked(m.0, c, true).is_none();
                        let mut p =
                            scaled_plan_density(m.0, remaining, &mut c.work, width, scaled, dense)?;
                        if scaled && width.is_some_and(|w| w > 288.) {
                            let mut fine = unresolved_plan(
                                m.0,
                                &c.detections,
                                remaining.max_retry_paths_per_candidate,
                                &mut c.work,
                            )?;
                            fine.extend(
                                p.into_iter().take(
                                    remaining
                                        .max_retry_paths_per_candidate
                                        .saturating_sub(fine.len()),
                                ),
                            );
                            p = fine;
                        }
                        #[cfg(feature = "experimental-verified-coverage-reuse")]
                        let p = reuse_plan(m.0, p, &claims, &mut reuse_budget, &mut c.work);
                        #[cfg(feature = "experimental-structural-retry")]
                        let p = defer_structurally_weak_retries(c, scaled, p);
                        Ok(p)
                    })() {
                        Some((m.0, p))
                    } else {
                        c.error = true;
                        None
                    }
                }
                Err(_) => None,
            };
            plans.push(p);
            c.ms += start.ms();
        }
        for step in 0..policy.max_retry_paths_per_candidate {
            if used >= policy.max_retry_paths_per_frame {
                break;
            }
            for (c, p) in outputs.iter_mut().zip(&plans) {
                if used >= policy.max_retry_paths_per_frame {
                    break;
                }
                let Some((m, paths)) = p else { continue };
                let Some(s) = paths.get(step) else { continue };
                #[cfg(feature = "experimental-verified-coverage-reuse")]
                if c.work.retry_paths >= policy.max_retry_paths_per_candidate {
                    continue;
                }
                used += 1;
                c.work.retry_paths_pending -= 1;
                self.retry_segment(
                    im,
                    *m,
                    *s,
                    c,
                    policy.transition_cleanup,
                    policy.interior_normalization,
                    policy.guard_bias,
                );
            }
        }
        // Low-resolution interpolation is complementary to native evidence.
        // Finish all original planned work first; never replace a native row.
        #[cfg(feature = "experimental-lowres-profile-refine")]
        if scaled {
            #[cfg(feature = "experimental-verified-coverage-reuse")]
            let claims = {
                let mut w = Work::default();
                let q = verified_claims(&outputs, &mut reuse_budget, &mut w, im);
                if let Some(c) = outputs.first_mut() {
                    c.work.extension_cache_hits += w.extension_cache_hits;
                    c.work.extension_samples += w.extension_samples;
                    c.work.extension_claims += w.extension_claims;
                    c.work.extension_capped += w.extension_capped;
                    c.work.reuse_claims += w.reuse_claims;
                    c.work.reuse_claims_rejected += w.reuse_claims_rejected;
                    c.work.reuse_checks += w.reuse_checks;
                    c.work.reuse_checks_capped += w.reuse_checks_capped;
                }
                q
            };
            let mut refinements = Vec::with_capacity(outputs.len());
            for (c, p) in outputs.iter_mut().zip(&plans) {
                let mut extra = Vec::new();
                if c.work.scale_hint_used == 0 {
                    if let Some((m, paths)) = p {
                        for s in paths {
                            if s.lo != -0.15 || s.hi != 1.15 || s.samples == 512 {
                                continue;
                            }
                            let (Ok(a), Ok(b)) = (
                                experiment::point(*m, s.axis, 0., s.fraction),
                                experiment::point(*m, s.axis, 1., s.fraction),
                            ) else {
                                continue;
                            };
                            if (76.0..=285.0).contains(&experiment::distance(a, b)) {
                                extra.push(Segment { samples: 512, ..*s });
                            }
                        }
                    }
                }
                c.work.retry_paths_pending += extra.len();
                #[cfg(feature = "experimental-verified-coverage-reuse")]
                let extra = if let Some((m, _)) = p {
                    reuse_plan(*m, extra, &claims, &mut reuse_budget, &mut c.work)
                } else {
                    extra
                };
                refinements.push(extra);
            }
            for step in 0..if cfg!(feature = "experimental-verified-coverage-reuse") {
                policy.max_retry_paths_per_candidate
            } else {
                42
            } {
                if used >= policy.max_retry_paths_per_frame {
                    break;
                }
                for ((c, p), extra) in outputs.iter_mut().zip(&plans).zip(&refinements) {
                    if used >= policy.max_retry_paths_per_frame {
                        break;
                    }
                    if c.work.retry_paths >= policy.max_retry_paths_per_candidate {
                        continue;
                    }
                    let (Some((m, _)), Some(s)) = (p, extra.get(step)) else {
                        continue;
                    };
                    used += 1;
                    c.work.retry_paths_pending -= 1;
                    self.retry_segment(
                        im,
                        *m,
                        *s,
                        c,
                        policy.transition_cleanup,
                        policy.interior_normalization,
                        policy.guard_bias,
                    );
                }
            }
        }
        // Follow actual unambiguous reads with adjacent source rows. This adds
        // evidence rather than inventing checksum-selected alternate values.
        // Existing discovery runs first; finite extra probes share its budgets.
        #[cfg(feature = "experimental-verified-coverage-reuse")]
        let claims = {
            let mut w = Work::default();
            let q = verified_claims(&outputs, &mut reuse_budget, &mut w, im);
            if let Some(c) = outputs.first_mut() {
                c.work.extension_cache_hits += w.extension_cache_hits;
                c.work.extension_samples += w.extension_samples;
                c.work.extension_claims += w.extension_claims;
                c.work.extension_capped += w.extension_capped;
                c.work.reuse_claims += w.reuse_claims;
                c.work.reuse_claims_rejected += w.reuse_claims_rejected;
                c.work.reuse_checks += w.reuse_checks;
                c.work.reuse_checks_capped += w.reuse_checks_capped;
            }
            q
        };
        let mut confirmation = Vec::with_capacity(outputs.len());
        let mut confirmation_allowance = budget.checks_left;
        for (c, p) in outputs.iter_mut().zip(&plans) {
            let mut extra: Vec<Segment> = Vec::new();
            // At tiny association budgets extra evidence would exhaust the
            // existing assembly rather than improve it. Use a conservative planning
            // allowance for the extra probes; actual assembly retains hard caps.
            if let Some((m, _)) = p
                .as_ref()
                .filter(|_| confirmation_allowance >= (c.observations.len() + 64).saturating_pow(2))
            {
                confirmation_allowance = confirmation_allowance
                    .saturating_sub((c.observations.len() + 64).saturating_pow(2));
                let mut anchors: Vec<_> = c
                    .observations
                    .iter()
                    .copied()
                    .filter(|o| !o.ambiguous)
                    .collect();
                anchors.sort_by(|a, b| a.cost.total_cmp(&b.cost));
                for o in anchors {
                    let (Ok(a), Ok(b), Ok(top), Ok(bottom)) = (
                        experiment::point(*m, o.axis, o.left, o.fraction),
                        experiment::point(*m, o.axis, o.right, o.fraction),
                        experiment::point(*m, o.axis, 0.5, 0.),
                        experiment::point(*m, o.axis, 0.5, 1.),
                    ) else {
                        continue;
                    };
                    let cross = experiment::distance(top, bottom);
                    if cross < 1. {
                        continue;
                    }
                    let width = o.right - o.left;
                    if width <= 0. {
                        continue;
                    }
                    let lo = o.left - 0.15 * width;
                    let hi = o.right + 0.15 * width;
                    let length = experiment::distance(a, b) * 1.3;
                    for delta in [-1., 1., -2., 2.] {
                        let fraction = o.fraction + delta / cross;
                        if !(0.0..=1.0).contains(&fraction) {
                            continue;
                        }
                        if extra.iter().any(|s| {
                            s.axis == o.axis
                                && (s.fraction - fraction).abs() * cross < 0.5
                                && (s.lo - lo).abs() < 0.1 * width
                                && (s.hi - hi).abs() < 0.1 * width
                        }) {
                            continue;
                        }
                        if extra.len() >= 64 {
                            c.work.sampling_plan_capped += 1;
                            break;
                        }
                        extra.push(Segment {
                            axis: o.axis,
                            fraction,
                            lo,
                            hi,
                            samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
                            sample_cap: length > 4096.,
                            unresolved: false,
                        });
                    }
                    if extra.len() >= 64 {
                        break;
                    }
                }
            }
            c.work.retry_paths_pending += extra.len();
            #[cfg(feature = "experimental-verified-coverage-reuse")]
            let extra = if let Some((m, _)) = p {
                reuse_plan(*m, extra, &claims, &mut reuse_budget, &mut c.work)
            } else {
                extra
            };
            confirmation.push(extra);
        }
        for step in 0..if cfg!(feature = "experimental-verified-coverage-reuse") {
            policy.max_retry_paths_per_candidate
        } else {
            64
        } {
            if used >= policy.max_retry_paths_per_frame {
                break;
            }
            for ((c, p), extra) in outputs.iter_mut().zip(&plans).zip(&confirmation) {
                if used >= policy.max_retry_paths_per_frame {
                    break;
                }
                if c.work.retry_paths >= policy.max_retry_paths_per_candidate {
                    continue;
                }
                let (Some((m, _)), Some(s)) = (p, extra.get(step)) else {
                    continue;
                };
                used += 1;
                c.work.retry_paths_pending -= 1;
                self.retry_segment(
                    im,
                    *m,
                    *s,
                    c,
                    policy.transition_cleanup,
                    policy.interior_normalization,
                    policy.guard_bias,
                );
            }
        }
        for (c, p) in outputs.iter_mut().zip(&plans) {
            if let Some((m, _)) = p {
                if c.work.retry_paths > 0 {
                    let start = Timer::now();
                    let refined = experiment::assemble_many_budget_options(
                        im,
                        *m,
                        &c.observations,
                        &mut c.work,
                        true,
                        &mut budget,
                        policy.allow_single_row,
                    );
                    if c.work.association_truncated > 0 && !c.detections.is_empty() {
                        c.work.retained_initial_detections = c.detections.len();
                    } else {
                        c.detections = refined;
                    }
                    // Frame reconciliation withholds every exhausted candidate,
                    // including these raw partial detections when initial was empty.
                    c.ms += start.ms();
                }
            }
        }
        Ok(outputs)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initially_empty_midbudget_and_zero_retry_assembly_stay_pending() {
        let bits=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
        for rotated in [false, true] {
            for retries in [0, 8192] {
                for checks in [0, 20, 40, 80] {
                    let (w, h) = if rotated { (1000, 500) } else { (500, 1000) };
                    let mut pixels = vec![255; w * h];
                    for y in if retries == 0 { 95..340 } else { 175..260 } {
                        for x in 0..380 {
                            if bits[x / 4] == b'1' {
                                let (px, py) = if rotated { (y, 50 + x) } else { (50 + x, y) };
                                pixels[py * w + px] = 0;
                            }
                        }
                    }
                    let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
                    let q = [
                        [0., 0.],
                        [crate::numeric::usize_f64(w), 0.],
                        [crate::numeric::usize_f64(w), crate::numeric::usize_f64(h)],
                        [0., crate::numeric::usize_f64(h)],
                    ];
                    let f = Experiment::default()
                        .scan_frame(
                            im,
                            &[q],
                            Policy {
                                transition_cleanup: true,
                                source_identity: true,
                                interior_normalization: true,
                                max_association_checks: checks,
                                max_retry_paths_per_frame: retries,
                                ..Policy::default()
                            },
                        )
                        .unwrap();
                    if f.candidates[0].work.association_truncated > 0 {
                        assert!(f.barcodes.is_empty());
                        assert!(f.unfinished);
                        assert!(
                            f.reconciliation.pending_observations
                                >= f.candidates[0].detections.len()
                        );
                    }
                    if checks == 80 && retries > 0 {
                        assert_eq!(f.barcodes.len(), 1);
                    }
                }
            }
        }
    }
    #[test]
    fn cheap_pass_precedes_any_budget_and_invalid_sibling_survives() {
        let pixels = vec![255; 600 * 200];
        let im = ImageView::new(&pixels, 600, 200, 1, 600).unwrap();
        let q = [[0., 0.], [600., 0.], [600., 200.], [0., 200.]];
        let mut ex = Experiment::default();
        for scaled in [false, true] {
            for budget in [0, 1, 3] {
                let out = ex
                    .scan_policy(
                        im,
                        &[q, [[0.; 2]; 4], q],
                        Policy {
                            max_retry_paths_per_candidate: 10,
                            max_retry_paths_per_frame: budget,
                            ..Policy::default()
                        },
                        scaled,
                    )
                    .unwrap();
                assert_eq!(out.len(), 3);
                assert!(out[1].error);
                assert_eq!(out[0].work.paths, 10 + out[0].work.retry_paths);
                assert_eq!(out[2].work.paths, 10 + out[2].work.retry_paths);
                assert!(out[0].work.retry_paths_pending > 0);
                assert!(out[2].work.retry_paths_pending > 0);
                assert_eq!(
                    out.iter().map(|c| c.work.retry_paths).sum::<usize>(),
                    budget
                );
                if budget == 3 {
                    assert_eq!(out[0].work.retry_paths, 2);
                    assert_eq!(out[2].work.retry_paths, 1);
                }
            }
        }
        assert!(ex.scan_all(im, &vec![q; 65], Policy::default()).is_err());
        assert!(ex
            .scan_all(
                im,
                &[q],
                Policy {
                    max_retry_paths_per_candidate: 4097,
                    ..Policy::default()
                }
            )
            .is_err());
    }
    #[test]
    fn observed_scale_reduces_work_without_exhausting_a_read_region() {
        let quad = [[0., 0.], [2400., 0.], [2400., 800.], [0., 800.]];
        let m = scan::transform(quad).unwrap();
        let mut dense = Work::default();
        let a = plan(m.0, Policy::default(), &mut dense).unwrap();
        let mut scaled = Work::default();
        let b = scaled_plan(m.0, Policy::default(), &mut scaled, Some(2000.), true).unwrap();
        assert!(b.len() > 10);
        assert!(b.len() < a.len() / 4);
        assert!(b.iter().any(|p| p.axis == 0));
        assert!(b.iter().any(|p| p.axis == 1));
        assert!(b.iter().any(|p| p.fraction < 0.15));
        assert!(b.iter().any(|p| p.fraction > 0.85));
        let mut unknown = Work::default();
        let p = scaled_plan(m.0, Policy::default(), &mut unknown, None, true).unwrap();
        assert_eq!(
            p.len(),
            if cfg!(feature = "experimental-unresolved-256") {
                256
            } else {
                128
            }
        );
        assert!(unknown.retry_paths_pending > p.len());
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn decoded_band_does_not_claim_whole_candidate() {
        let q = [[0., 0.], [1000., 0.], [1000., 750.], [0., 750.]];
        let m = scan::transform(q).unwrap();
        let polygon = [
            experiment::point(m.0, 0, 0.1, 0.2).unwrap(),
            experiment::point(m.0, 0, 0.8, 0.2).unwrap(),
            experiment::point(m.0, 0, 0.8, 0.6).unwrap(),
            experiment::point(m.0, 0, 0.1, 0.6).unwrap(),
        ];
        let d = experiment::Detection {
            digits: [0; 13],
            polygon,
            support: 3,
            axis: 0,
        };
        assert!(claimed_interval(m.0, 0, 0.8, polygon).is_none());
        let interval = claimed_interval(m.0, 0, 0.4, polygon).unwrap();
        assert!((interval.0 - 0.1).abs() < 1e-9);
        assert!((interval.1 - 0.8).abs() < 1e-9);
        let mut work = Work::default();
        let p = unresolved_plan(m.0, &[d], 512, &mut work).unwrap();
        assert!(p
            .iter()
            .any(|s| s.axis == 0 && s.fraction > 0.75 && s.lo == -0.15 && s.hi == 1.15));
        assert!(p
            .iter()
            .any(|s| s.axis == 0 && s.fraction > 0.3 && s.fraction < 0.5 && s.lo >= 0.79));
        assert!(p
            .iter()
            .filter(|s| s.axis == 0 && s.fraction > 0.3 && s.fraction < 0.5)
            .all(|s| s.hi <= 0.11 || s.lo >= 0.79));
    }
    #[test]
    fn capped_schedule_covers_the_whole_extent_and_both_axes() {
        let m = scan::transform([[0., 0.], [4000., 0.], [4000., 4000.], [0., 4000.]]).unwrap();
        let p = scaled_plan(m.0, Policy::default(), &mut Work::default(), None, true).unwrap();
        assert_eq!(
            p.len(),
            if cfg!(feature = "experimental-unresolved-256") {
                256
            } else {
                128
            }
        );
        for axis in 0..2 {
            assert!(p.iter().any(|s| s.axis == axis && s.fraction < 0.1));
            assert!(p.iter().any(|s| s.axis == axis && s.fraction > 0.9));
        }
        let p = unresolved_plan(m.0, &[], 8, &mut Work::default()).unwrap();
        assert_eq!(p.len(), 8);
        assert_eq!(p.iter().filter(|s| s.axis == 0).count(), 4);
        assert_eq!(p.iter().filter(|s| s.axis == 1).count(), 4);
        assert!(p.iter().any(|s| s.fraction > 0.7));
        assert!(p.iter().any(|s| s.fraction < 0.3));
    }
    #[test]
    fn unknown_large_region_executes_tiles_on_both_axes() {
        let m = scan::transform([[0., 0.], [8440., 0.], [8440., 3200.], [0., 3200.]]).unwrap();
        let p = scaled_plan(m.0, Policy::default(), &mut Work::default(), None, true).unwrap();
        assert_eq!(
            p.len(),
            if cfg!(feature = "experimental-unresolved-256") {
                256
            } else {
                128
            }
        );
        for axis in 0..2 {
            let tiles: Vec<_> = p
                .iter()
                .filter(|s| s.axis == axis && s.hi - s.lo < 1.29)
                .collect();
            assert!(tiles.len() >= 8);
            assert!(tiles.iter().any(|s| s.lo < 0.2));
            assert!(tiles.iter().any(|s| s.hi > 0.8));
        }
    }
    #[test]
    fn oversize_geometry_reports_sampling_plan_limit() {
        for size in [1e6, 1e100] {
            let q = [[0., 0.], [size, 0.], [size, size], [0., size]];
            let m = scan::transform(q).unwrap();
            let mut work = Work::default();
            let p = plan(m.0, Policy::default(), &mut work).unwrap();
            assert_eq!(p.len(), 512);
            assert!(work.retry_paths_pending > p.len());
            assert!(work.sampling_plan_capped > 0);
            assert!(p.iter().all(|p| (64..=4096).contains(&p.samples)));
        }
    }
    #[test]
    fn exhausted_reassembly_retains_initial_evidence_as_pending() {
        let bits=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
        let mut pixels = vec![255; 500 * 200];
        for y in 0..200 {
            for x in 0..380 {
                if bits[x / 4] == b'1' {
                    pixels[y * 500 + 50 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
        let q = [[0., 0.], [500., 0.], [500., 200.], [0., 200.]];
        let mut ex = Experiment::default();
        let first = ex.scan(im, &[q], experiment::MULTI_FIXED);
        assert_eq!(first[0].detections.len(), 1);
        let p = Policy {
            max_association_checks: first[0].work.association_checks,
            ..Policy::default()
        };
        let f = ex.scan_frame(im, &[q], p).unwrap();
        assert_eq!(f.candidates[0].detections.len(), 1);
        assert_eq!(f.candidates[0].work.retained_initial_detections, 1);
        assert!(f.candidates[0].work.association_truncated > 0);
        assert!(f.unfinished);
        assert!(f.barcodes.is_empty());
        assert_eq!(f.reconciliation.pending_observations, 1);
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn unread_small_region_refines_rows_with_explicit_pending_budget() {
        let m = scan::transform([[0., 0.], [350., 0.], [350., 117.], [0., 117.]]).unwrap();
        let mut work = Work::default();
        let p = scaled_plan(m.0, Policy::default(), &mut work, None, true).unwrap();
        assert_eq!(
            p.iter()
                .filter(|p| p.axis == 0 && p.lo == -0.15 && p.hi == 1.15)
                .count(),
            21
        );
        let known = scaled_plan(
            m.0,
            Policy::default(),
            &mut Work::default(),
            Some(350.),
            true,
        )
        .unwrap();
        assert_eq!(
            known
                .iter()
                .filter(|p| p.axis == 0 && p.lo == -0.15 && p.hi == 1.15)
                .count(),
            5
        );
        let mut work = Work::default();
        let p = scaled_plan(
            m.0,
            Policy {
                max_retry_paths_per_candidate: 7,
                ..Policy::default()
            },
            &mut work,
            None,
            true,
        )
        .unwrap();
        assert_eq!(p.len(), 7);
        assert!(work.retry_paths_pending > p.len());
    }

    #[cfg(feature = "experimental-lowres-profile-refine")]
    #[test]
    fn lowres_refinement_obeys_frame_budget_and_retains_all_candidates() {
        let pixels = vec![255; 600 * 240];
        let im = ImageView::new(&pixels, 600, 240, 1, 600).unwrap();
        let quads = [
            [[10., 20.], [210., 20.], [210., 120.], [10., 120.]],
            [[300., 40.], [500., 40.], [500., 140.], [300., 140.]],
        ];
        for frame_limit in [0, 3, 120, 512] {
            let mut ex = Experiment::default();
            let p = Policy {
                max_retry_paths_per_frame: frame_limit,
                max_retry_paths_per_candidate: 100,
                transition_cleanup: true,
                interior_normalization: true,
                guard_bias: true,
                ..Policy::default()
            };
            let f = ex.scan_frame(im, &quads, p).unwrap();
            assert!(f.barcodes.is_empty());
            assert_eq!(f.candidates.len(), 2);
            assert!(f
                .candidates
                .iter()
                .all(|c| c.work.paths >= 10 && c.work.retry_paths <= 100));
            assert!(
                f.candidates
                    .iter()
                    .map(|c| c.work.retry_paths)
                    .sum::<usize>()
                    <= frame_limit
            );
            if frame_limit < 188 {
                assert!(f.unfinished);
                assert!(f.candidates.iter().any(|c| c.work.retry_paths_pending > 0));
            } else {
                assert!(f
                    .candidates
                    .iter()
                    .all(|c| c.work.retry_paths == 94 && c.work.retry_paths_pending == 0));
            }
        }
    }

    #[cfg(feature = "experimental-single-row-search")]
    #[cfg(feature = "experimental-native-wide-tiles")]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn wide_tiles_preserve_native_sampling_and_small_tiles_floor() {
        for width in [200., 1400., 4000.] {
            let q = [[0., 0.], [7000., 0.], [7000., 200.], [0., 200.]];
            let m = scan::transform(q).unwrap().0;
            let p = scaled_plan(
                m,
                Policy::default(),
                &mut Work::default(),
                Some(width),
                true,
            )
            .unwrap();
            let tiles: Vec<_> = p
                .iter()
                .filter(|s| s.axis == 0 && (s.lo != -0.15 || s.hi != 1.15))
                .collect();
            assert!(!tiles.is_empty());
            for s in tiles {
                let n = (7000. * (s.hi - s.lo)).ceil();
                assert!((crate::numeric::usize_f64(s.samples) - n.clamp(512., 4096.)).abs() <= 1.);
                assert_eq!(s.sample_cap, n > 4096.);
            }
        }
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn unsupported_wide_scale_keeps_density_and_known_budget() {
        let q = [[0., 0.], [3000., 0.], [3000., 1000.], [0., 1000.]];
        let m = scan::transform(q).unwrap().0;
        let p = scaled_plan_density(
            m,
            Policy::default(),
            &mut Work::default(),
            Some(420.),
            true,
            true,
        )
        .unwrap();
        assert_eq!(
            p.len(),
            512,
            "known-scale work allowance must not fall to 128"
        );
        assert_eq!(
            p.iter()
                .filter(|s| s.axis == 0 && s.lo == -0.15 && s.hi == 1.15)
                .count(),
            29
        );
        let q = [[0., 0.], [600., 0.], [600., 100.], [0., 100.]];
        let m = scan::transform(q).unwrap().0;
        let p = scaled_plan_density(
            m,
            Policy::default(),
            &mut Work::default(),
            Some(420.),
            true,
            true,
        )
        .unwrap();
        assert_eq!(
            p.iter()
                .filter(|s| s.axis == 0 && s.lo == -0.15 && s.hi == 1.15)
                .count(),
            21
        );
    }
    #[cfg(feature = "experimental-single-row-search")]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn unrelated_single_row_symbols_cannot_corroborate_scale() {
        let quad = [[0., 0.], [600., 0.], [600., 200.], [0., 200.]];
        let matrix = scan::transform(quad).unwrap().0;
        let obs = |digits, left, right, fraction| experiment::Observation {
            short_quiet: false,
            ambiguous: false,
            digits,
            axis: 0,
            fraction,
            left,
            right,
            cost: 0.01,
            gap: 0.3,
        };
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        let mut candidate = Candidate {
            index: 0,
            coverage: quad,
            observations: vec![obs(a, 0.05, 0.30, 0.3), obs(b, 0.65, 0.95, 0.7)],
            detections: vec![],
            work: Work::default(),
            ms: 0.,
            error: false,
        };
        assert_eq!(supported_scale_width(matrix, &candidate), None);
        candidate.observations[1].digits = a;
        assert_eq!(supported_scale_width(matrix, &candidate), None);
        let p = scaled_plan(
            matrix,
            Policy::default(),
            &mut Work::default(),
            supported_scale_width(matrix, &candidate),
            true,
        )
        .unwrap();
        assert_eq!(
            p.iter()
                .filter(|s| s.axis == 0 && s.lo == -0.15 && s.hi == 1.15)
                .count(),
            21
        );
        candidate.detections.push(experiment::Detection {
            digits: a,
            polygon: [
                [389.5, 129.5],
                [569.5, 129.5],
                [569.5, 149.5],
                [389.5, 149.5],
            ],
            support: 3,
            axis: 0,
        });
        assert_eq!(
            supported_scale_width(matrix, &candidate),
            None,
            "separate equal-value track cannot support smallest width"
        );
        candidate.detections.push(experiment::Detection {
            digits: a,
            polygon: [[29.5, 49.5], [179.5, 49.5], [179.5, 69.5], [29.5, 69.5]],
            support: 3,
            axis: 0,
        });
        assert!((supported_scale_width(matrix, &candidate).unwrap() - 150.).abs() < 1e-6);
    }
}

/// Diagnostic unresolved retry plan before execution; excludes fixed/discovery passes.
/// This is not a claim that a budget-limited frame executed every returned path.
/// One scheduled path: axis, row fraction, start, end, and sample count.
pub type ScheduledPath = (usize, f64, f64, f64, usize);

/// # Errors
/// Returns `Parameters` for invalid scan limits or schedule inputs; propagates invalid quadrilateral and sampling errors.
pub fn diagnostic_unresolved_schedule(
    q: Quad,
    remaining: usize,
) -> Result<Vec<ScheduledPath>, Error> {
    if remaining > 4096 {
        return Err(Error::Parameters);
    }
    let m = scan::transform(q)?;
    let policy = Policy {
        max_retry_paths_per_candidate: remaining,
        ..Policy::default()
    };
    Ok(scaled_plan(m.0, policy, &mut Work::default(), None, true)?
        .into_iter()
        .map(|s| (s.axis, s.fraction, s.lo, s.hi, s.samples))
        .collect())
}
#[cfg(test)]
mod schedule_diagnostic_tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn unresolved_schedule_uses_actual_plan() {
        let q = [[160., 576.], [480., 576.], [480., 744.], [160., 744.]];
        let p = diagnostic_unresolved_schedule(q, 502).unwrap();
        assert_eq!(p.len(), 42);
        let a: Vec<_> = p.iter().filter(|p| p.0 == 0).collect();
        assert_eq!(a.len(), 21);
        assert!(a.iter().all(|p| p.4 == 416 && p.2 == -0.15 && p.3 == 1.15));
        assert!(!a.iter().any(|p| (p.1 - 0.6).abs() < 1e-12));
        assert!(a.iter().any(|p| (p.1 - 12.5 / 21.).abs() < 1e-12));
        assert!(diagnostic_unresolved_schedule(q, 4097).is_err());
    }
}

#[cfg(test)]
mod policy_diagnostic_tests {
    use super::*;
    #[test]
    fn diagnostic_rejects_invalid_rows_before_sampling() {
        let data = vec![255; 100 * 100];
        let im = ImageView::new(&data, 100, 100, 1, 100).unwrap();
        let q = [[10., 10.], [90., 10.], [90., 90.], [10., 90.]];
        let mut e = Experiment::default();
        for f in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(matches!(
                e.diagnostic_retry_rows(im, q, &[f]),
                Err(Error::Parameters)
            ));
        }
        assert!(e.diagnostic_retry_rows(im, q, &[0.5; 65]).is_err());
        assert!(e.diagnostic_retry_rows(im, [[0., 0.]; 4], &[0.5]).is_err());
    }
    #[test]
    fn blank_and_duplicate_rows_supply_no_evidence() {
        let data = vec![255; 100 * 100];
        let im = ImageView::new(&data, 100, 100, 1, 100).unwrap();
        let q = [[10., 10.], [90., 10.], [90., 90.], [10., 90.]];
        let mut e = Experiment::default();
        for f in [vec![], vec![0.5], vec![0.5; 64], vec![0., 1.]] {
            let c = e.diagnostic_retry_rows(im, q, &f).unwrap();
            assert!(c.observations.is_empty() && c.detections.is_empty());
            assert_eq!(c.work.paths, f.len());
            assert!(!c.error);
            assert_eq!(c.work.association_truncated, 0);
        }
    }
}

#[cfg(all(test, feature = "experimental-complete-tile-prefix"))]
mod tile_completion_tests {
    use super::*;
    #[test]
    fn unequal_axis_lengths_do_not_omit_pending_tiles() {
        for [width, height] in [[400., 1000.], [1000., 400.], [600., 1600.], [1600., 600.]] {
            let matrix =
                scan::transform([[0., 0.], [width, 0.], [width, height], [0., height]]).unwrap();
            let mut work = Work::default();
            let policy = Policy {
                max_retry_paths_per_candidate: 65536,
                ..Policy::default()
            };
            let paths =
                scaled_plan_allowance(matrix.0, policy, &mut work, None, true, false, 65536)
                    .unwrap();
            assert_eq!(
                paths.len(),
                work.retry_paths_pending,
                "incomplete schedule for {width} by {height}"
            );
            assert!(paths.iter().any(|path| path.axis == 0));
            assert!(paths.iter().any(|path| path.axis == 1));
        }
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn visits_every_counted_pair_when_it_fits_and_keeps_caps() {
        let m = scan::transform([[0., 0.], [600., 0.], [600., 100.], [0., 100.]]).unwrap();
        let mut work = Work::default();
        let p = scaled_plan(m.0, Policy::default(), &mut work, None, true).unwrap();
        assert_eq!(p.len(), work.retry_paths_pending);
        assert!(p.len() <= 128);
        for (i, a) in p.iter().enumerate() {
            assert!(!p[..i].iter().any(|b| a.axis == b.axis
                && a.fraction == b.fraction
                && a.lo == b.lo
                && a.hi == b.hi));
        }
        for limit in [0, 1, 7, 42, 64] {
            let mut w = Work::default();
            let q = scaled_plan(
                m.0,
                Policy {
                    max_retry_paths_per_candidate: limit,
                    ..Policy::default()
                },
                &mut w,
                None,
                true,
            )
            .unwrap();
            assert_eq!(q.len(), limit);
            assert_eq!(w.retry_paths_pending, work.retry_paths_pending);
            for (a, b) in q.iter().zip(&p) {
                assert_eq!(
                    (a.axis, a.fraction, a.lo, a.hi),
                    (b.axis, b.fraction, b.lo, b.hi)
                );
            }
        }
    }
}

// Deliberately lossy research ablation, not a proven absence-of-barcode test.
// Every candidate's fixed pass and mandatory native discovery precede this.
// The planner has already added these rows to retry_paths_pending. Removing
// materialized retry paths must NOT decrement that unresolved-work counter.
#[cfg(feature = "experimental-structural-retry")]
fn defer_structurally_weak_retries(
    c: &mut Candidate,
    scaled: bool,
    paths: Vec<Segment>,
) -> Vec<Segment> {
    if scaled
        && c.work.discovery_paths == 10
        && c.work.accepted_paths == 0
        && c.work.max_run_count < 30
    {
        c.work.structural_retry_skipped += paths.len();
        Vec::new()
    } else {
        paths
    }
}

#[cfg(all(test, feature = "experimental-structural-retry"))]
mod structural_retry_tests {
    use super::*;
    fn quad(x: f64, y: f64, w: f64, h: f64) -> Quad {
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    }
    fn candidate() -> Candidate {
        Candidate {
            index: 0,
            coverage: quad(0., 0., 1000., 240.),
            observations: vec![],
            detections: vec![],
            work: Work::default(),
            ms: 0.,
            error: false,
        }
    }
    fn segment() -> Segment {
        Segment {
            axis: 0,
            fraction: 0.2,
            lo: 0.,
            hi: 1.,
            samples: 1000,
            sample_cap: false,
            unresolved: false,
        }
    }
    #[test]
    fn structural_skips_are_pending_after_every_candidates_discovery() {
        let pixels = vec![255; 1000 * 240];
        let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
        let qs = [
            quad(0., 0., 1000., 240.),
            quad(20., 20., 960., 200.),
            quad(40., 40., 920., 160.),
        ];
        let result = Experiment::default()
            .scan_scaled(im, &qs, Policy::default())
            .unwrap();
        assert_eq!(result.len(), qs.len());
        for (c, q) in result.iter().zip(qs) {
            assert_eq!(c.coverage, q);
            assert_eq!(c.work.discovery_paths, 10);
            assert_eq!(c.work.paths, 20);
            assert_eq!(c.work.retry_paths, 10);
            assert_eq!(c.work.accepted_paths, 0);
            assert_eq!(c.work.max_run_count, 0);
            assert!(c.work.structural_retry_skipped > 0);
            assert!(c.work.retry_paths_pending >= c.work.structural_retry_skipped);
            assert!(c.work.structural_discovery_complete);
        }
        let json = crate::region_json::candidate_json(&result);
        assert_eq!(json.matches("\"unfinished\":true").count(), qs.len());
        assert!(json.contains("\"max_run_count\":0"));
        assert!(json.contains("\"structural_retry_skipped\":"));
    }
    #[test]
    fn structural_boundary_supported_and_incomplete_discovery_are_not_suppressed() {
        for (scaled, discovery, accepted, runs, skip) in [
            (true, 10, 0, 29, true),
            (true, 10, 0, 30, false),
            (true, 10, 1, 0, false),
            (true, 9, 0, 0, false),
            (false, 10, 0, 0, false),
        ] {
            let mut c = candidate();
            c.work.discovery_paths = discovery;
            c.work.accepted_paths = accepted;
            c.work.max_run_count = runs;
            c.work.retry_paths_pending = 7;
            let p = defer_structurally_weak_retries(&mut c, scaled, vec![segment(), segment()]);
            assert_eq!(p.len(), if skip { 0 } else { 2 });
            assert_eq!(c.work.structural_retry_skipped, if skip { 2 } else { 0 });
            assert_eq!(c.work.retry_paths_pending, 7);
        }
    }
    #[test]
    fn structural_retains_equal_and_different_supported_symbols() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        for second in [a, b] {
            let mut pixels = vec![255; 1000 * 240];
            for (left, digits) in [(50, a), (550, second)] {
                let bits = crate::ean::encode(&digits);
                for y in 40..200 {
                    for x in 0..380 {
                        if bits[x / 4] > 0.5 {
                            pixels[y * 1000 + left + x] = 0;
                        }
                    }
                }
            }
            let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
            let qs = [quad(0., 0., 1000., 240.), quad(0., 20., 1000., 200.)];
            let result = Experiment::default()
                .scan_scaled(
                    im,
                    &qs,
                    Policy {
                        transition_cleanup: true,
                        interior_normalization: true,
                        guard_bias: true,
                        ..Policy::default()
                    },
                )
                .unwrap();
            for c in result {
                assert_eq!(c.work.discovery_paths, 10);
                assert!(c.work.accepted_paths > 0);
                assert_eq!(c.work.structural_retry_skipped, 0);
                assert!(c
                    .detections
                    .iter()
                    .any(|d| d.digits == a && d.polygon.iter().all(|p| p[0] < 500.)));
                assert!(c
                    .detections
                    .iter()
                    .any(|d| d.digits == second && d.polygon.iter().all(|p| p[0] > 500.)));
            }
        }
    }
}

#[cfg(all(test, feature = "experimental-unresolved-256"))]
mod unresolved_256_tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn extra_work_preserves_original_prefix_and_pending_denominator() {
        for (w, h) in [(1600., 1200.), (4000., 4000.), (2400., 800.)] {
            let m = scan::transform([[0., 0.], [w, 0.], [w, h], [0., h]]).unwrap();
            let mut before = Work::default();
            let mut after = Work::default();
            let a =
                scaled_plan_allowance(m.0, Policy::default(), &mut before, None, true, false, 128)
                    .unwrap();
            let b = scaled_plan(m.0, Policy::default(), &mut after, None, true).unwrap();
            assert_eq!(a.len(), 128);
            assert_eq!(b.len(), 256);
            assert_eq!(before.retry_paths_pending, after.retry_paths_pending);
            assert!(after.retry_paths_pending >= b.len());
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(
                    (x.axis, x.fraction, x.lo, x.hi, x.samples),
                    (y.axis, y.fraction, y.lo, y.hi, y.samples)
                );
            }
            for (i, x) in b.iter().enumerate() {
                assert!(!b[..i].iter().any(|y| x.axis == y.axis
                    && x.fraction == y.fraction
                    && x.lo == y.lo
                    && x.hi == y.hi));
            }
            for cap in [0, 1, 64, 127, 128] {
                let mut work = Work::default();
                let p = scaled_plan(
                    m.0,
                    Policy {
                        max_retry_paths_per_candidate: cap,
                        ..Policy::default()
                    },
                    &mut work,
                    None,
                    true,
                )
                .unwrap();
                assert_eq!(p.len(), cap);
            }
        }
    }
}
