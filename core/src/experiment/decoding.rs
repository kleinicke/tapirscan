//! Interpret sampled profiles and collect accepted observations.
use super::{profile, run_ean, CandidateScanner, Observation, Timer, Work};

#[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
fn trace_reads(stage: &str, r: &crate::multi_profile::Reads) {
    eprintln!(
        "{{\"stage\":\"{}\",\"ambiguous\":{},\"truncated\":{}}}",
        stage, r.ambiguous_intervals, r.truncated
    );
    for s in &r.symbols {
        eprintln!(
            "{{\"stage\":\"{}\",\"digits\":{:?},\"left\":{},\"right\":{},\"cost\":{},\"gap\":{}}}",
            stage, s.digits, s.left, s.right, s.cost, s.gap
        );
    }
    for (left, right) in &r.rejected_intervals {
        eprintln!(
            "{{\"stage\":\"{}\",\"rejectedLeft\":{},\"rejectedRight\":{}}}",
            stage, left, right
        );
    }
}
impl CandidateScanner {
    pub(super) fn run_decode(
        &mut self,
        work: &mut Work,
    ) -> Option<(crate::run_profile::Read, bool)> {
        let p = &self.signal;
        let n = p.len();
        self.runs.clear();
        let (mut start, mut black) = (0, p[0] >= 0.5);
        #[expect(
            clippy::needless_range_loop,
            reason = "The inclusive final index is a synthetic run terminator beyond the samples; a slice iterator would omit the final run."
        )]
        for i in 1..=n {
            let next = i < n && p[i] >= 0.5;
            if i == n || next != black {
                self.runs.push((start, i, black));
                start = i;
                black = next;
            }
        }
        let mut accepted: Option<(crate::run_profile::Read, bool)> = None;
        for i in 0..self.runs.len().saturating_sub(60) {
            let r = &self.runs[i..i + 61];
            if !r[1].2 {
                continue;
            }
            work.windows += 1;
            let (left, right) = (r[1].0, r[59].1);
            let module = crate::numeric::usize_f64(right - left) / 95.;
            if module < 0.8
                || crate::numeric::usize_f64(r[0].1 - r[0].0) < 7. * module
                || crate::numeric::usize_f64(r[60].1 - r[60].0) < 7. * module
            {
                continue;
            }
            work.quiet_pass += 1;
            let mut widths = [0.; 59];
            for j in 0..59 {
                widths[j] = crate::numeric::usize_f32(r[j + 1].1 - r[j + 1].0);
            }
            if [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58]
                .iter()
                .all(|&j| (widths[j] / crate::numeric::f64_f32(module) - 1.).abs() <= 0.65)
            {
                work.guard_pass += 1;
            }
            for reversed in [false, true] {
                if reversed {
                    widths.reverse();
                }
                work.decoder_calls += 1;
                let Some(e) = run_ean::decode_evidence(&widths) else {
                    continue;
                };
                let r = crate::run_profile::Read {
                    digits: e.digits,
                    left: crate::numeric::usize_f64(left) - 0.5,
                    right: crate::numeric::usize_f64(right) - 0.5,
                    cost: e.cost,
                    gap: e.gap,
                };
                if accepted.is_some_and(|(a, _)| a.digits != r.digits) {
                    work.conflicts += 1;
                    return None;
                }
                if accepted.is_none_or(|(a, _)| r.cost < a.cost) {
                    accepted = Some((r, reversed));
                }
            }
        }
        accepted
    }

    pub(super) fn profile_decode(&mut self, work: &mut Work) -> Option<crate::run_profile::Read> {
        self.blur_rejected_intervals.clear();
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
            {
                if self.signal.len() != 512
                    && !(cfg!(any(feature = "mode-high", feature = "mode-very-high"))
                        && (76..=384).contains(&self.signal.len()))
                {
                    return None;
                }
            }
            #[cfg(feature = "mode-very-high")]
            {
                if self.signal.len() != 512
                    && !(cfg!(any(feature = "mode-high", feature = "mode-very-high"))
                        && (76..=1536).contains(&self.signal.len()))
                {
                    return None;
                }
            }
        }

        // Count actual boundary pairs and digit hypotheses used by unchanged profile.
        // Forward-blur records the pairs in profile::BlurTrace, including the native
        // variable-length path; do not retain the previous fixed-512 duplicate scan.

        let result = {
            let (result, trace) = {
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                if self.signal.len() == 512 {
                    profile::decode_with_blur_trace(&self.signal)
                } else {
                    profile::decode_native_with_blur_trace(&self.signal)
                }
                #[cfg(not(any(feature = "mode-high", feature = "mode-very-high")))]
                profile::decode_with_blur_trace(&self.signal)
            };
            work.profile_boundary_pairs += trace.boundary_pairs;
            work.profile_digit_hypotheses += trace.digit_hypotheses;
            work.forward_blur_calls += 1;
            work.forward_blur_windows += trace.gated_windows;
            work.forward_blur_model_attempts += trace.model_attempts;
            work.forward_blur_accepted_windows += trace.accepted_windows;
            work.forward_blur_conflicts += trace.conflicts;
            work.conflicts += trace.conflicts;
            self.blur_rejected_intervals.extend(
                trace
                    .rejected_intervals
                    .iter()
                    .map(|&(a, b)| (f64::from(a), f64::from(b))),
            );
            result
        };
        result.ok().flatten().map(|r| {
            let (left, right) = if r.reversed {
                (
                    crate::numeric::usize_f64(self.signal.len() - 1) - f64::from(r.right),
                    crate::numeric::usize_f64(self.signal.len() - 1) - f64::from(r.left),
                )
            } else {
                (f64::from(r.left), f64::from(r.right))
            };
            crate::run_profile::Read {
                digits: r.digits,
                left,
                right,
                cost: r.cost,
                gap: r.gap,
            }
        })
    }

    pub(crate) fn collect_many(
        &mut self,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        work: &mut Work,
        observations: &mut Vec<Observation>,
    ) {
        self.collect_policy(axis, fraction, lo, hi, work, observations, false, false);
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Coordinates, reversal and decoder switches are independent dimensions forwarded to the evidence collector."
    )]
    pub(crate) fn collect_policy(
        &mut self,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        work: &mut Work,
        observations: &mut Vec<Observation>,
        cleanup: bool,
        guard_bias: bool,
    ) {
        let timer = Timer::now();
        self.collect_policy_inner(
            axis,
            fraction,
            lo,
            hi,
            work,
            observations,
            cleanup,
            guard_bias,
        );
        #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
        {
            work.interpretation_ms += timer.ms();
        }
        let _ = timer;
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The collector receives independent path geometry and decoder switches with shared work state."
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "This evidence pass keeps ordered hypotheses, contradiction vetoes and work accounting together within one scan transaction."
    )]
    pub(super) fn collect_policy_inner(
        &mut self,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        work: &mut Work,
        observations: &mut Vec<Observation>,
        cleanup: bool,
        guard_bias: bool,
    ) {
        let (start, mut run_visual, mut visual_capped, mut soft_reads) =
            (observations.len(), Vec::new(), false, Vec::new());
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        eprintln!("{{\"collect\":true,\"axis\":{},\"fraction\":{},\"lo\":{},\"hi\":{},\"samples\":{},\"signal\":{:?}}}",axis,fraction,lo,hi,self.signal.len(),self.signal);
        crate::multi_profile::sample_runs(&self.signal, 64, &mut self.runs)
            .expect("validated sampled profile");
        #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
        if !work.structural_discovery_complete {
            work.max_run_count = work.max_run_count.max(self.runs.len());
        }

        let mut short_accepted;

        {
            // Raw observed runs only: no cleanup or synthesized transitions. Keep
            // weaker quiet-zone evidence marked until independent row assembly.
            let short = crate::multi_profile::decode_short_quiet(&self.runs, 64, guard_bias);

            crate::invalid_visual::collect(&mut run_visual, &mut visual_capped, &short);
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("short", &short);
            short_accepted = !short.symbols.is_empty();
            work.bias_model_pass += short.bias_model_pass;
            work.bias_guard_pass += short.bias_guard_pass;
            work.windows += short.windows_examined;
            work.quiet_pass += short.quiet_pass;
            work.guard_pass += short.guard_pass;
            work.decoder_calls += short.decoder_calls;
            work.conflicts += short.ambiguous_intervals;
            work.truncated_paths += usize::from(short.truncated);
            let n = crate::numeric::usize_f64(self.signal.len());
            for (left, right) in short.rejected_intervals {
                {
                    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                    {
                        observations.push(Observation {
                            short_quiet: true,
                            ambiguous: true,
                            digits: [0; 13],
                            axis,
                            fraction,
                            left: lo + (hi - lo) * (left + 0.5) / n,
                            right: lo + (hi - lo) * (right + 0.5) / n,
                            cost: 0.,
                            gap: 0.,
                        });
                    }
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    {
                        observations.push(Observation {
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            invalid_checksum: false,
                            short_quiet: true,
                            ambiguous: true,
                            digits: [0; 13],
                            axis,
                            fraction,
                            left: lo + (hi - lo) * (left + 0.5) / n,
                            right: lo + (hi - lo) * (right + 0.5) / n,
                            cost: 0.,
                            gap: 0.,
                        });
                    }
                }
            }
            for r in short.symbols {
                {
                    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                    {
                        observations.push(Observation {
                            short_quiet: true,
                            ambiguous: false,
                            digits: r.digits,
                            axis,
                            fraction,
                            left: lo + (hi - lo) * (r.left + 0.5) / n,
                            right: lo + (hi - lo) * (r.right + 0.5) / n,
                            cost: r.cost,
                            gap: r.gap,
                        });
                    }
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    {
                        observations.push(Observation {
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            invalid_checksum: false,
                            short_quiet: true,
                            ambiguous: false,
                            digits: r.digits,
                            axis,
                            fraction,
                            left: lo + (hi - lo) * (r.left + 0.5) / n,
                            right: lo + (hi - lo) * (r.right + 0.5) / n,
                            cost: r.cost,
                            gap: r.gap,
                        });
                    }
                }
            }
        }
        let raw = crate::multi_profile::decode_runs(&self.runs, 64);
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            self.retail
                .raw(&self.runs, &raw, axis, fraction, lo, hi, self.signal.len());
        }
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            if !cleanup || self.retail.peak_retry {
                self.retail
                    .peaks(&self.signal, &raw, axis, fraction, lo, hi);
            }
        }
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        trace_reads("raw", &raw);
        let mut reads = if cleanup {
            let (clean, cw) =
                crate::transition::decode_clustered_reusing(&self.signal, 64, &raw, &self.runs);
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("cleanup", &clean);
            work.cleanup_paths += 1;
            work.cleanup_examined += cw.examined;
            work.cleanup_removed_runs += cw.removed_runs;
            work.cleanup_pixels += cw.removed_pixels;
            crate::transition::merge_reads(raw, clean, 64)
        } else {
            raw
        };
        if guard_bias {
            work.bias_paths += 1;
            let bias = crate::multi_profile::decode_guard_runs(&self.runs, 64);
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("guard", &bias);
            reads = crate::transition::merge_reads(reads, bias, 64);
        }

        if cleanup {
            let local = crate::local_signal::decode_reusing_validated(
                &self.signal,
                64,
                guard_bias,
                &mut self.local_scratch,
            )
            .expect("validated sampled profile");
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("local", &local);

            {
                let short =
                    crate::local_signal::prepared_short(&self.local_scratch, 64, guard_bias);

                crate::invalid_visual::collect(&mut run_visual, &mut visual_capped, &short);
                short_accepted |= !short.symbols.is_empty();
                work.bias_model_pass += short.bias_model_pass;
                work.bias_guard_pass += short.bias_guard_pass;
                work.windows += short.windows_examined;
                work.quiet_pass += short.quiet_pass;
                work.guard_pass += short.guard_pass;
                work.decoder_calls += short.decoder_calls;
                work.conflicts += short.ambiguous_intervals;
                work.truncated_paths += usize::from(short.truncated);
                let n = crate::numeric::usize_f64(self.signal.len());
                for (left, right) in short.rejected_intervals {
                    {
                        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                        {
                            observations.push(Observation {
                                short_quiet: true,
                                ambiguous: true,
                                digits: [0; 13],
                                axis,
                                fraction,
                                left: lo + (hi - lo) * (left + 0.5) / n,
                                right: lo + (hi - lo) * (right + 0.5) / n,
                                cost: 0.,
                                gap: 0.,
                            });
                        }
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        {
                            observations.push(Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
                                short_quiet: true,
                                ambiguous: true,
                                digits: [0; 13],
                                axis,
                                fraction,
                                left: lo + (hi - lo) * (left + 0.5) / n,
                                right: lo + (hi - lo) * (right + 0.5) / n,
                                cost: 0.,
                                gap: 0.,
                            });
                        }
                    }
                }
                for r in short.symbols {
                    {
                        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                        {
                            observations.push(Observation {
                                short_quiet: true,
                                ambiguous: false,
                                digits: r.digits,
                                axis,
                                fraction,
                                left: lo + (hi - lo) * (r.left + 0.5) / n,
                                right: lo + (hi - lo) * (r.right + 0.5) / n,
                                cost: r.cost,
                                gap: r.gap,
                            });
                        }
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        {
                            observations.push(Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
                                short_quiet: true,
                                ambiguous: false,
                                digits: r.digits,
                                axis,
                                fraction,
                                left: lo + (hi - lo) * (r.left + 0.5) / n,
                                right: lo + (hi - lo) * (r.right + 0.5) / n,
                                cost: r.cost,
                                gap: r.gap,
                            });
                        }
                    }
                }
            }

            {
                work.redundant_decode_calls_avoided +=
                    crate::local_signal::reused_calls(&self.local_scratch);
            }

            {
                let e = &self.local_scratch.extrema;
                work.extrema_calls += usize::from(e.attempted);
                work.extrema_examined += e.examined;
                work.extrema_capped += usize::from(e.capped);
                work.extrema_ambiguous += usize::from(e.ambiguous);
                work.extrema_decoder_calls += e.decoder_calls;
            }
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                self.retail.local(
                    &self.local_scratch,
                    &local,
                    axis,
                    fraction,
                    lo,
                    hi,
                    self.signal.len(),
                );
            }
            reads = crate::transition::merge_reads(reads, local, 64);
        }

        // Under-resolved source profiles can lose narrow dark/bright elements
        // at the middle threshold. Two fixed photometric hypotheses use observed
        // pixels only; their competing values still veto overlapping evidence.
        if cleanup && (76..=384).contains(&self.signal.len()) && (45..61).contains(&self.runs.len())
        {
            for threshold in [0.35f32, 0.65] {
                let shifted: Vec<_> = self
                    .signal
                    .iter()
                    .map(|v| (v + 0.5 - threshold).clamp(0., 1.))
                    .collect();
                let raw =
                    crate::multi_profile::decode_many(&shifted, 64).expect("valid shifted profile");
                let extra = if guard_bias {
                    crate::transition::merge_reads(
                        raw,
                        crate::multi_profile::decode_guard_bias(&shifted, 64)
                            .expect("valid shifted profile"),
                        64,
                    )
                } else {
                    raw
                };
                reads = crate::transition::merge_reads(reads, extra, 64);
            }
        }

        crate::invalid_visual::collect(&mut run_visual, &mut visual_capped, &reads);
        work.bias_model_pass += reads.bias_model_pass;
        work.bias_guard_pass += reads.bias_guard_pass;
        work.windows += reads.windows_examined;
        work.quiet_pass += reads.quiet_pass;
        work.guard_pass += reads.guard_pass;
        work.decoder_calls += reads.decoder_calls;
        work.conflicts += reads.ambiguous_intervals;
        work.truncated_paths += usize::from(reads.truncated);

        // Complementary global evidence is valid only on fixed512 signals. A run
        // ambiguity must not be resurrected through the single-result fallback.
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        {
            if (self.signal.len() == 512
                || (cfg!(any(feature = "mode-high", feature = "mode-very-high"))
                    && (76..=384).contains(&self.signal.len())))
                && reads.ambiguous_intervals == 0
            {
                if let Some(p) = self.profile_decode(work) {
                    soft_reads.push(p);
                    let overlaps =
                        |r: &crate::run_profile::Read| r.left < p.right && p.left < r.right;
                    if reads
                        .symbols
                        .iter()
                        .any(|r| overlaps(r) && r.digits != p.digits)
                    {
                        work.conflicts += 1;
                        reads.rejected_intervals.push((p.left, p.right));
                        reads.symbols.retain(|r| !overlaps(r));
                    } else if !reads
                        .symbols
                        .iter()
                        .any(|r| overlaps(r) && r.digits == p.digits)
                    {
                        reads.symbols.push(p);
                    }
                }
            }
        }
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        {
            if (self.signal.len() == 512
                || (cfg!(any(feature = "mode-high", feature = "mode-very-high"))
                    && (76..=384).contains(&self.signal.len())))
                && reads.ambiguous_intervals == 0
            {
                // A same-window legacy/blur contradiction must not be resurrected by
                // Many's retained run reads. Preserve disjoint source intervals.
                for &(left, right) in &self.blur_rejected_intervals {
                    reads.rejected_intervals.push((left, right));
                    reads.symbols.retain(|r| r.right <= left || r.left >= right);
                }
            }
        }

        // Complementary global evidence is valid only on fixed512 signals. A run
        // ambiguity must not be resurrected through the single-result fallback.
        #[cfg(feature = "mode-very-high")]
        {
            if (self.signal.len() == 512
                || (cfg!(any(feature = "mode-high", feature = "mode-very-high"))
                    && (76..=1536).contains(&self.signal.len())))
                && reads.ambiguous_intervals == 0
            {
                if let Some(p) = self.profile_decode(work) {
                    soft_reads.push(p);
                    let overlaps =
                        |r: &crate::run_profile::Read| r.left < p.right && p.left < r.right;
                    if reads
                        .symbols
                        .iter()
                        .any(|r| overlaps(r) && r.digits != p.digits)
                    {
                        work.conflicts += 1;
                        reads.rejected_intervals.push((p.left, p.right));
                        reads.symbols.retain(|r| !overlaps(r));
                    } else if !reads
                        .symbols
                        .iter()
                        .any(|r| overlaps(r) && r.digits == p.digits)
                    {
                        reads.symbols.push(p);
                    }
                }
            }
        }
        #[cfg(feature = "mode-very-high")]
        {
            if (self.signal.len() == 512
                || (cfg!(any(feature = "mode-high", feature = "mode-very-high"))
                    && (76..=1536).contains(&self.signal.len())))
                && reads.ambiguous_intervals == 0
            {
                // A same-window legacy/blur contradiction must not be resurrected by
                // Many's retained run reads. Preserve disjoint source intervals.
                for &(left, right) in &self.blur_rejected_intervals {
                    reads.rejected_intervals.push((left, right));
                    reads.symbols.retain(|r| r.right <= left || r.left >= right);
                }
            }
        }
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        trace_reads("merged", &reads);

        for (left, right) in reads.rejected_intervals {
            let n = crate::numeric::usize_f64(self.signal.len());
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    observations.push(Observation {
                        short_quiet: false,
                        ambiguous: true,
                        digits: [0; 13],
                        axis,
                        fraction,
                        left: lo + (hi - lo) * (left + 0.5) / n,
                        right: lo + (hi - lo) * (right + 0.5) / n,
                        cost: 0.,
                        gap: 0.,
                    });
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    observations.push(Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous: true,
                        digits: [0; 13],
                        axis,
                        fraction,
                        left: lo + (hi - lo) * (left + 0.5) / n,
                        right: lo + (hi - lo) * (right + 0.5) / n,
                        cost: 0.,
                        gap: 0.,
                    });
                }
            }
        }
        for r in reads.symbols {
            let n = crate::numeric::usize_f64(self.signal.len());
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    observations.push(Observation {
                        short_quiet: false,
                        ambiguous: false,
                        digits: r.digits,
                        axis,
                        fraction,
                        left: lo + (hi - lo) * (r.left + 0.5) / n,
                        right: lo + (hi - lo) * (r.right + 0.5) / n,
                        cost: r.cost,
                        gap: r.gap,
                    });
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    observations.push(Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous: false,
                        digits: r.digits,
                        axis,
                        fraction,
                        left: lo + (hi - lo) * (r.left + 0.5) / n,
                        right: lo + (hi - lo) * (r.right + 0.5) / n,
                        cost: r.cost,
                        gap: r.gap,
                    });
                }
            }
        }

        {
            let _ = short_accepted;
            crate::invalid_visual::apply(
                &run_visual,
                visual_capped,
                &soft_reads,
                observations,
                start,
                axis,
                fraction,
                lo,
                hi,
                self.signal.len(),
                work,
            );
            work.accepted_paths += usize::from(observations[start..].iter().any(|o| !o.ambiguous));
        }
    }
}
