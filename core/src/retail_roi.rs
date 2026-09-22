//! Bounded axis-aligned source resampling of unresolved guard-bearing proposals.
// Preserve validated arithmetic and observation layout. Coordinates are bounded
// by image/profile limits; digit and pixel casts follow explicit clamps.
// Low intentionally compiles the zero-budget ROI path out.
#![allow(
    clippy::absurd_extreme_comparisons,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
use super::*;
impl Collector {
    pub(super) fn recover_boxes(
        &mut self,
        im: ImageView<'_>,
        done: &[bool],
        work: &mut Work,
        budget: &mut AssociationBudget,
    ) -> Vec<crate::experiment::Detection> {
        let mut order: Vec<usize> = (0..done.len())
            .filter(|&i| !done[i] && self.group_calls[i] > 0)
            .collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.group_calls[i]));
        let mut boxes: Vec<(Quad, usize)> = Vec::new();
        for i in order {
            let q = self.groups[i].0;
            let x0 = q
                .iter()
                .map(|p| p[0])
                .fold(f64::INFINITY, f64::min)
                .clamp(0., im.width.saturating_sub(1) as f64);
            let x1 = q
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max)
                .clamp(0., im.width.saturating_sub(1) as f64);
            let y0 = q
                .iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min)
                .clamp(0., im.height.saturating_sub(1) as f64);
            let y1 = q
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max)
                .clamp(0., im.height.saturating_sub(1) as f64);
            if x1 - x0 < 8. || y1 - y0 < 8. {
                continue;
            }
            let rect = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
            if boxes
                .iter()
                .any(|(a, _)| crate::frame::same_space(*a, rect))
            {
                continue;
            }
            boxes.push((rect, i));
            if boxes.len() >= ROI_LIMIT {
                break;
            }
        }
        let mut sampler = crate::experiment::Experiment::default();
        let mut found = Vec::new();
        let mut raw = Vec::new();
        for (q, source) in boxes {
            if self.recovery_samples >= 65_536 {
                self.capped = true;
                break;
            }
            self.coverage(q);
            let group = self.active;
            let old_cap = self.peak_counts[group];
            self.peak_counts[group] = 0;
            self.recovery_boxes += 1;
            let source_q = self.groups[source].0;
            let source_axis = self.groups[source]
                .1
                .iter()
                .find(|o| !o.ambiguous)
                .map_or(0, |o| o.axis);
            let end = if source_axis == 0 {
                source_q[1]
            } else {
                source_q[3]
            };
            let preferred =
                usize::from((end[1] - source_q[0][1]).abs() > (end[0] - source_q[0][0]).abs());
            for axis in if ROI_ORIENT {
                [preferred, 1 - preferred]
            } else {
                [0, 1]
            } {
                for j in 0..ROI_ROWS {
                    if self.recovery_samples >= 65_536 {
                        self.capped = true;
                        break;
                    }
                    let extent = if axis == 0 {
                        q[1][0] - q[0][0]
                    } else {
                        q[3][1] - q[0][1]
                    };
                    let expected = ((extent * 1.3).ceil() as usize + 2).clamp(64, 4096);
                    if expected > 65_536 - self.recovery_samples {
                        self.capped = true;
                        continue;
                    }
                    let f = (j as f64 + 0.5) / ROI_ROWS as f64;
                    let (p, f, lo, hi) = if ROI_MODE == 1 {
                        let Ok(Some(p)) = sampler.diagnostic_profile(im, q, axis, f, true) else {
                            continue;
                        };
                        (p, f, -0.15, 1.15)
                    } else {
                        let (origin, span, cross_origin, cross_span, max) = if axis == 0 {
                            (
                                q[0][0],
                                q[1][0] - q[0][0],
                                q[0][1],
                                q[3][1] - q[0][1],
                                im.width,
                            )
                        } else {
                            (
                                q[0][1],
                                q[3][1] - q[0][1],
                                q[0][0],
                                q[1][0] - q[0][0],
                                im.height,
                            )
                        };
                        let start = (origin - 0.15 * span).floor().max(0.) as usize;
                        let end = (origin + 1.15 * span)
                            .ceil()
                            .min(max.saturating_sub(1) as f64)
                            as usize;
                        if end <= start
                            || !(64..=4096).contains(&(end - start + 1))
                            || end - start + 1 > 65_536 - self.recovery_samples
                        {
                            continue;
                        }
                        let fixed = (cross_origin + f * cross_span).round();
                        let p: Vec<f32> = (start..=end)
                            .map(|at| {
                                let (x, y) = if axis == 0 {
                                    (at as f64, fixed)
                                } else {
                                    (fixed, at as f64)
                                };
                                1. - im.bilinear(x, y) / 255.
                            })
                            .collect();
                        (
                            p,
                            (fixed - cross_origin) / cross_span,
                            (start as f64 - origin) / span,
                            ((end + 1) as f64 - origin) / span,
                        )
                    };
                    self.recovery_paths += 1;
                    self.recovery_samples += p.len();
                    if multi_profile::sample_runs(&p, 64, &mut raw).is_err() {
                        continue;
                    }
                    let primary = multi_profile::decode_runs(&raw, 64);
                    let before = self.groups[group].1.len();
                    self.raw(&raw, &primary, axis, f, lo, hi, p.len());
                    self.peaks(&p, &primary, axis, f, lo, hi);
                    if self.groups[group].1.len() == before {
                        self.recovery_profile(&p, &primary, axis, f, lo, hi);
                    }
                }
            }
            self.peak_counts[group] = old_cap;
            let Ok(m) = crate::scan::transform(q) else {
                continue;
            };
            found.extend(crate::experiment::assemble_many_budget_options(
                im,
                m.0,
                &self.groups[group].1,
                work,
                true,
                budget,
                false,
            ));
        }
        found
    }
}
