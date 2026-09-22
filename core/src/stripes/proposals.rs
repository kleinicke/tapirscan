//! Project grouped edges into ordered parent and band proposals.
use super::{bounds, edge_weight, tensor_magnitude, Edge, GroupDiagnostic, ImageView, Proposal};
use super::{groups::Groups, raster::Raster, refinement::refine_band_angle};
#[derive(Default)]
pub(super) struct Fitted {
    pub proposals: Vec<Proposal>,
    pub extra_proposals: Vec<Proposal>,
    pub extra_bands: Vec<Proposal>,
    pub angle_bands: Vec<Proposal>,
    pub rescued_bands: Vec<Proposal>,
    pub band_refined: usize,
    pub band_unexamined: usize,
}
pub(super) fn fit(
    im: ImageView<'_>,
    raster: &Raster,
    groups: &Groups,
    mut observe: impl FnMut(GroupDiagnostic),
) -> Fitted {
    let mut fitted = Fitted::default();
    let mut bands = Vec::new();
    for (index, queue) in groups.items.iter().take(128).enumerate() {
        fit_group(
            im,
            raster,
            (index, groups.base_count),
            queue,
            &mut observe,
            &mut fitted,
            &mut bands,
        );
    }
    fitted.proposals.extend(bands);
    fitted
}
fn fit_group(
    im: ImageView<'_>,
    raster: &Raster,
    (group_index, base_count): (usize, usize),
    queue: &[usize],
    observe: &mut impl FnMut(GroupDiagnostic),
    fitted: &mut Fitted,
    bands: &mut Vec<Proposal>,
) {
    let (working_width, working_height) = (raster.width, raster.height);
    let tw = working_width.div_ceil(8);
    let tiles = &raster.tiles;
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    for &k in queue {
        xx += tiles[k].xx;
        xy += tiles[k].xy;
        yy += tiles[k].yy;
    }
    let mut angle = 0.5 * (2. * xy).atan2(xx - yy);
    let edges = collect_edges(queue, raster, angle);
    let weight = if cfg!(feature = "mode-low") && raster.sparse {
        4.
    } else {
        1.
    };
    if crate::numeric::usize_f64(edges.len()) * weight < 80. {
        let b = bounds(queue, angle, tw);
        observe(GroupDiagnostic {
            index: group_index,
            tiles: queue.len(),
            edges: edges.len(),
            angle,
            initial_angle: angle,
            bounds: [b.u0, b.u1, b.v0, b.v1],
            reason: "few_edges",
        });
        return;
    }
    let initial = angle;
    angle = projection_angle(&edges, angle, working_width, working_height);
    let (cos_angle, sin_angle) = (angle.cos(), angle.sin());
    let [mut u0, mut u1, mut v0, mut v1] = projected_bounds(&edges, cos_angle, sin_angle);
    let (across, along) = (u1 - u0, v1 - v0);
    observe(GroupDiagnostic {
        index: group_index,
        tiles: queue.len(),
        edges: edges.len(),
        angle,
        initial_angle: initial,
        bounds: [u0, u1, v0, v1],
        reason: parent_reason(across, along),
    });
    let valid_parent = across >= 45. && along >= 7. && (0.65..=30.).contains(&(across / along));
    let rescue_tall =
        group_index < base_count && across >= 45. && along >= 24. && across / along < 0.65;
    if !valid_parent && !rescue_tall {
        return;
    }

    if along >= 24. {
        append_bands(
            &Projection {
                im,
                raster,
                edges: &edges,
                angle,
                cos_angle,
                sin_angle,
                along,
                v0,
                score: 0.99f64.min(tensor_magnitude(xx, xy, yy) / (xx + yy + 1.)),
                weight,
                original: group_index < base_count,
                rescue_tall,
            },
            fitted,
            bands,
        );
    }

    // A rejected parent is never admitted merely because one band is useful.
    if !valid_parent {
        return;
    }
    u0 -= 1.;
    u1 += 1.;
    v0 += 1f64.min(along * 0.05);
    v1 -= 1f64.min(along * 0.05);
    let point = |u: f64, v: f64| {
        [
            (u * cos_angle - v * sin_angle) * crate::numeric::usize_f64(im.width)
                / crate::numeric::usize_f64(working_width),
            (u * sin_angle + v * cos_angle) * crate::numeric::usize_f64(im.height)
                / crate::numeric::usize_f64(working_height),
        ]
    };

    (if group_index < base_count {
        &mut fitted.proposals
    } else {
        &mut fitted.extra_proposals
    })
    .push(Proposal {
        polygon: [point(u0, v0), point(u1, v0), point(u1, v1), point(u0, v1)],
        score: 0.99f64.min(tensor_magnitude(xx, xy, yy) / (xx + yy + 1.)),
    });
}
struct Projection<'a> {
    im: ImageView<'a>,
    raster: &'a Raster,
    edges: &'a [Edge],
    angle: f64,
    cos_angle: f64,
    sin_angle: f64,
    along: f64,
    v0: f64,
    score: f64,
    weight: f64,
    original: bool,
    rescue_tall: bool,
}
fn append_bands(projection: &Projection<'_>, fitted: &mut Fitted, bands: &mut Vec<Proposal>) {
    let Projection {
        im,
        raster,
        edges,
        angle,
        cos_angle,
        sin_angle,
        along,
        v0,
        score,
        weight,
        original,
        rescue_tall,
    } = *projection;
    let (working_width, working_height) = (raster.width, raster.height);
    let (gx, gy) = (&raster.gx, &raster.gy);

    let smooth = band_density(edges, sin_angle, cos_angle, v0, along);
    let bin_count = smooth.len();
    let peak = smooth.iter().copied().fold(0f64, f64::max);

    let threshold = (peak * 0.55).max(12. / weight);

    let mut start = 0;
    let mut admitted = 0;
    while start < bin_count && admitted < 2 {
        if smooth[start] < threshold {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < bin_count && smooth[end] >= threshold {
            end += 1;
        }
        if end - start >= 7 && crate::numeric::usize_f64(end - start) <= along * 0.55 {
            let (bv0, bv1) = (
                v0 + crate::numeric::usize_f64(start),
                v0 + crate::numeric::usize_f64(end),
            );
            let (mut bu0, mut bu1) = (f64::INFINITY, f64::NEG_INFINITY);
            for e in edges {
                let v = -e.x * sin_angle + e.y * cos_angle;
                if v >= bv0 && v <= bv1 {
                    let u = e.x * cos_angle + e.y * sin_angle;
                    bu0 = bu0.min(u);
                    bu1 = bu1.max(u);
                }
            }
            let band_ratio = (bu1 - bu0) / (bv1 - bv0);
            if bu1 - bu0 >= 45. && (!rescue_tall || (0.65..=30.).contains(&band_ratio)) {
                let point = |u: f64, v: f64| {
                    [
                        (u * cos_angle - v * sin_angle) * crate::numeric::usize_f64(im.width)
                            / crate::numeric::usize_f64(working_width),
                        (u * sin_angle + v * cos_angle) * crate::numeric::usize_f64(im.height)
                            / crate::numeric::usize_f64(working_height),
                    ]
                };
                (if rescue_tall {
                    &mut fitted.rescued_bands
                } else if original {
                    &mut *bands
                } else {
                    &mut fitted.extra_bands
                })
                .push(Proposal {
                    polygon: [
                        point(bu0 - 1., bv0),
                        point(bu1 + 1., bv0),
                        point(bu1 + 1., bv1),
                        point(bu0 - 1., bv1),
                    ],
                    score,
                });
                if original {
                    if fitted.band_refined < 8 {
                        fitted.band_refined += 1;
                        if let Some(p) = refine_band_angle(
                            edges,
                            gx,
                            gy,
                            working_width,
                            angle,
                            [bu0, bu1, bv0, bv1],
                            crate::numeric::usize_f64(im.width)
                                / crate::numeric::usize_f64(working_width),
                            crate::numeric::usize_f64(im.height)
                                / crate::numeric::usize_f64(working_height),
                        ) {
                            fitted.angle_bands.push(p);
                        }
                    } else {
                        fitted.band_unexamined += 1;
                    }
                }
                admitted += 1;
            }
        }
        start = end;
    }
}

fn collect_edges(queue: &[usize], raster: &Raster, angle: f64) -> Vec<Edge> {
    let (width, height, tw) = (raster.width, raster.height, raster.width.div_ceil(8));
    let (ax, ay) = (angle.cos(), angle.sin());
    let mut edges = Vec::new();
    for &k in queue {
        let (tx, ty) = (k % tw, k / tw);
        for y in (ty * 8).max(1)..((ty + 1) * 8).min(height - 1) {
            let mut visit = |x: usize| {
                let i = y * width + x;
                if let Some(weight) =
                    edge_weight(f64::from(raster.gx[i]), f64::from(raster.gy[i]), ax, ay)
                {
                    edges.push(Edge {
                        x: crate::numeric::usize_f64(x) + 0.5,
                        y: crate::numeric::usize_f64(y) + 0.5,
                        weight,
                    });
                }
            };
            if cfg!(feature = "mode-low") && raster.sparse {
                let [a, b] = [[0usize, 3], [1, 6], [4, 7], [2, 5]][(4 - (y + y / 2) % 4) % 4];
                for x in [tx * 8 + a, tx * 8 + b] {
                    if x >= 1 && x < width - 1 {
                        visit(x);
                    }
                }
            } else {
                for x in (tx * 8).max(1)..((tx + 1) * 8).min(width - 1) {
                    visit(x);
                }
            }
        }
    }
    edges
}
fn projection_angle(
    edges: &[Edge],
    mut angle: f64,
    working_width: usize,
    working_height: usize,
) -> f64 {
    let mut best = f64::NEG_INFINITY;
    let initial = angle;
    let offset =
        crate::numeric::usize_f64(working_width).hypot(crate::numeric::usize_f64(working_height));
    let mut bins = vec![0f32; crate::numeric::f64_usize((offset * 4.).ceil()) + 8];
    let (mut clear_start, mut clear_end) = (0, 0);

    // Coarse-to-fine search over the same +/-5 degree envelope.
    // Coarse samples are 1 degree apart; refine the winning cell at 0.5 degrees.
    #[cfg(feature = "mode-low")]
    let mut best_step = 0;
    #[cfg(feature = "mode-low")]
    for pass in 0..3 {
        let steps: Vec<i32> = if pass == 0 {
            vec![-4, -2, 0, 2, 4]
        } else if pass == 1 {
            if best_step == -4 {
                vec![-10, -8, -6]
            } else if best_step == 4 {
                vec![6, 8, 10]
            } else {
                vec![]
            }
        } else {
            [best_step - 1, best_step + 1]
                .into_iter()
                .filter(|sin_angle| (-10..=10).contains(sin_angle))
                .collect()
        };
        for step in steps {
            let a = initial + f64::from(step) * std::f64::consts::PI / 360.;
            let (cos_angle, sin_angle) = (a.cos(), a.sin());
            bins[clear_start..clear_end].fill(0.);
            let (mut lo, mut hi) = (bins.len(), 0);
            for e in edges.iter().step_by(edges.len().div_ceil(2048).max(1)) {
                let p = (e.x * cos_angle + e.y * sin_angle + offset) * 2.;
                let bin_index = crate::numeric::f64_isize(p.floor());
                let f = p - crate::numeric::isize_f64(bin_index);
                if bin_index >= 0 && ((bin_index).cast_unsigned() + 1) < bins.len() {
                    let bin_index = (bin_index).cast_unsigned();
                    lo = lo.min(bin_index);
                    hi = hi.max(bin_index + 2);
                    bins[bin_index] =
                        crate::numeric::f64_f32(f64::from(bins[bin_index]) + e.weight * (1. - f));
                    bins[bin_index + 1] =
                        crate::numeric::f64_f32(f64::from(bins[bin_index + 1]) + e.weight * f);
                }
            }
            clear_start = lo;
            clear_end = hi;
            let mut score = 0.;
            for &bin_index in &bins[lo..hi] {
                score += f64::from(bin_index) * f64::from(bin_index);
            }
            if score > best {
                best = score;
                angle = a;
                best_step = step;
            }
        }
    }
    #[cfg(any(
        feature = "mode-medium",
        feature = "mode-high",
        feature = "mode-very-high"
    ))]
    for step in -10..=10 {
        let a = initial + f64::from(step) * std::f64::consts::PI / 360.;
        let (cos_angle, sin_angle) = (a.cos(), a.sin());
        bins[clear_start..clear_end].fill(0.);
        let (mut lo, mut hi) = (bins.len(), 0);
        for e in edges {
            let p = (e.x * cos_angle + e.y * sin_angle + offset) * 2.;
            let neighbor_bounds = crate::numeric::f64_isize(p.floor());
            let f = p - crate::numeric::isize_f64(neighbor_bounds);
            if neighbor_bounds >= 0 && ((neighbor_bounds).cast_unsigned() + 1) < bins.len() {
                let neighbor_bounds = (neighbor_bounds).cast_unsigned();
                lo = lo.min(neighbor_bounds);
                hi = hi.max(neighbor_bounds + 2);
                bins[neighbor_bounds] =
                    crate::numeric::f64_f32(f64::from(bins[neighbor_bounds]) + e.weight * (1. - f));
                bins[neighbor_bounds + 1] =
                    crate::numeric::f64_f32(f64::from(bins[neighbor_bounds + 1]) + e.weight * f);
            }
        }
        clear_start = lo;
        clear_end = hi;
        let mut score = 0.;
        for &neighbor_bounds in &bins[lo..hi] {
            score += f64::from(neighbor_bounds) * f64::from(neighbor_bounds);
        }
        if score > best {
            best = score;
            angle = a;
        }
    }
    angle
}

fn projected_bounds(edges: &[Edge], cos_angle: f64, sin_angle: f64) -> [f64; 4] {
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for e in edges {
        let (u, v) = (
            e.x * cos_angle + e.y * sin_angle,
            -e.x * sin_angle + e.y * cos_angle,
        );
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    [u0, u1, v0, v1]
}

fn band_density(edges: &[Edge], sin_angle: f64, cos_angle: f64, v0: f64, along: f64) -> Vec<f64> {
    let bin_count = crate::numeric::f64_usize(along.ceil()) + 1;

    let mut density = vec![0usize; bin_count];

    for e in edges {
        let v = crate::numeric::f64_usize((-e.x * sin_angle + e.y * cos_angle - v0).floor());
        if v < bin_count {
            density[v] += 1;
        }
    }

    let smooth: Vec<f64> = (0..bin_count)
        .map(|i| {
            let lo = i.saturating_sub(2);
            let hi = (i + 3).min(bin_count);
            crate::numeric::usize_f64(density[lo..hi].iter().sum::<usize>())
                / crate::numeric::usize_f64(hi - lo)
        })
        .collect();

    smooth
}

fn parent_reason(across: f64, along: f64) -> &'static str {
    if across < 45. {
        "narrow"
    } else if along < 7. {
        "short"
    } else if across / along < 0.65 {
        "tall"
    } else if across / along > 30. {
        "flat"
    } else {
        "accepted_parent"
    }
}
