use super::*;
const BITS:&str="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
fn fixture(gap: usize) -> (Vec<u8>, Vec<Quad>) {
    let mut p = vec![255u8; 1000 * 240];
    for y in 10..230 {
        if gap > 0 && (110..110 + gap).contains(&y) {
            continue;
        }
        for x in 0..380 {
            if BITS.as_bytes()[x / 4] == b'1' {
                p[y * 1000 + 30 + x] = 0;
                p[y * 1000 + 530 + x] = 0;
            }
        }
    }
    (
        p,
        vec![
            [[30., 10.], [410., 10.], [410., 230.], [30., 230.]],
            [[530., 10.], [910., 10.], [910., 230.], [530., 230.]],
        ],
    )
}

#[test]
fn short_quiet_requires_four_distinct_source_supported_rows_and_keeps_vetoes() {
    let (width, h) = (512, 160);
    let mut pixels = vec![0u8; width * h];
    for y in 0..h {
        for x in 40..460 {
            pixels[y * width + x] = 255;
        }
        for x in 0..380 {
            if BITS.as_bytes()[x / 4] == b'1' {
                pixels[y * width + 60 + x] = 0;
            }
        }
    }
    let im = ImageView::new(&pixels, width, h, 1, width).unwrap();
    let quad = [[60., 20.], [440., 20.], [440., 140.], [60., 140.]];
    let m = scan::transform(quad).unwrap();
    let c = CandidateScanner::default().scan(im, &[quad], MULTI_FIXED);
    let mut obs: Vec<_> = c[0]
        .observations
        .iter()
        .filter(|o| o.short_quiet && !o.ambiguous && o.axis == 0)
        .copied()
        .collect();
    obs.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
    assert!(obs.len() >= 4);
    assert!(assemble_many(im, m.0, &obs[..3], &mut Work::default(), true).is_empty());
    assert_eq!(
        assemble_many(im, m.0, &obs[..4], &mut Work::default(), true).len(),
        1
    );
    let repeated = vec![obs[0]; 4];
    assert!(assemble_many(im, m.0, &repeated, &mut Work::default(), true).is_empty());
    let mut conflict = obs[..4].to_vec();
    {
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            conflict.push(Observation {
                ambiguous: true,
                ..obs[2]
            });
        }
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        {
            conflict.push(Observation {
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                invalid_checksum: false,
                ambiguous: true,
                ..obs[2]
            });
        }
    }

    assert!(assemble_many(im, m.0, &conflict, &mut Work::default(), true).is_empty());
}

#[test]
fn short_quiet_keeps_separate_equal_and_different_symbols() {
    let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    for second in [a, b] {
        let (w, height) = (512, 320);
        let mut pixels = vec![255u8; w * height];
        for (top, d) in [(10, a), (180, second)] {
            let bits = crate::ean::encode(&d);
            for y in top..top + 130 {
                for x in 0..w {
                    pixels[y * w + x] = if (40..460).contains(&x) { 255 } else { 0 };
                }
                for x in 0..380 {
                    if bits[x / 4] > 0.5 {
                        pixels[y * w + 60 + x] = 0;
                    }
                }
            }
        }
        let im = ImageView::new(&pixels, w, height, 1, w).unwrap();
        let quad = [[60., 10.], [440., 10.], [440., 310.], [60., 310.]];
        let c = CandidateScanner::default().scan(
            im,
            &[quad],
            Config::new("short_dense", false, true, DecoderMode::Many).unwrap(),
        );
        assert!(
            c[0].observations
                .iter()
                .filter(|o| o.short_quiet && !o.ambiguous)
                .count()
                >= 8
        );
        assert_eq!(c[0].detections.len(), 2);
        assert_eq!(c[0].detections[0].digits, a);
        assert_eq!(c[0].detections[1].digits, second);
        assert!(c[0].detections[0].polygon[2][1] < c[0].detections[1].polygon[0][1]);
    }
}
#[test]
fn broad_region_associates_parallel_equal_tracks() {
    let (p, _) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let q = [[[0., 10.], [1000., 10.], [1000., 230.], [0., 230.]]];
    let out = CandidateScanner::default().scan(im, &q, MULTI_FIXED);
    assert_eq!(out[0].observations.len(), 10);
    assert_eq!(out[0].detections.len(), 2);
    assert!(out[0].detections.iter().all(|d| d.support == 5));
    assert!(out[0].detections[0].polygon[1][0] < out[0].detections[1].polygon[0][0]);
}
#[test]
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
)]
fn canonical_rows_deduplicate_and_veto_conflicts_without_merging_nearby_rows() {
    let (pixels, qs) = fixture(0);
    let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap();
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let a = Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.3,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let a = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.3,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let b = Observation {
        fraction: 0.1 + 0.2,
        ..a
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let b = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        fraction: 0.1 + 0.2,
        ..a
    };

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let c = Observation { fraction: 0.7, ..a };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let c = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        fraction: 0.7,
        ..a
    };

    let ds = assemble_many(im, m.0, &[a, b, c], &mut Work::default(), false);
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].support, 2);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let conflict = Observation {
        digits: [1; 13],
        ..b
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let conflict = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        digits: [1; 13],
        ..b
    };

    assert!(assemble_many(im, m.0, &[a, conflict, c], &mut Work::default(), false).is_empty());
    assert_ne!(canonical_row(0.3), canonical_row(0.3 + 1e-8));
}
#[test]
fn invalid_boundary_diagnostics_return_none() {
    for p in [vec![], vec![0.], vec![f32::NAN; 512], vec![1.1; 512]] {
        assert!(profile_at_run_boundary(&p, 0., 95.).is_none());
    }
    for (a, b) in [
        (f64::NAN, 95.),
        (0., f64::INFINITY),
        (5., 5.),
        (5., 4.),
        (-1., 95.),
        (0., 512.),
    ] {
        assert!(profile_at_run_boundary(&[0.; 512], a, b).is_none());
    }
}
#[test]
fn one_fraction_cannot_supply_its_own_consensus() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap();
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let a = Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.5,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let a = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.5,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };

    assert!(assemble_many(im, m.0, &[a, a], &mut Work::default(), false).is_empty());
}
#[test]
fn incompatible_intervening_read_ends_a_track() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap();
    let mut obs = vec![];
    for (fraction, value) in [(0.1, 0), (0.2, 0), (0.3, 1), (0.4, 1), (0.5, 0), (0.6, 0)] {
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                obs.push(Observation {
                    short_quiet: false,
                    ambiguous: false,
                    digits: [value; 13],
                    axis: 0,
                    fraction,
                    left: 0.,
                    right: 1.,
                    cost: 0.,
                    gap: 1.,
                });
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                obs.push(Observation {
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    invalid_checksum: false,
                    short_quiet: false,
                    ambiguous: false,
                    digits: [value; 13],
                    axis: 0,
                    fraction,
                    left: 0.,
                    right: 1.,
                    cost: 0.,
                    gap: 1.,
                });
            }
        }
    }
    let out = assemble_many(im, m.0, &obs, &mut Work::default(), true);
    assert_eq!(out.len(), 3);
    assert!(out.iter().all(|d| d.support == 2));
}
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Mode-specific acceptance controls share the same adversarial observation fixture."
)]
fn permissive_single_row_is_opt_in_and_keeps_ambiguity_veto() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap().0;
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let o = Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7],
        axis: 0,
        fraction: 0.5,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let o = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7],
        axis: 0,
        fraction: 0.5,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };

    let run = |obs: &[Observation], allow| {
        assemble_many_budget_options(
            im,
            m,
            obs,
            &mut Work::default(),
            true,
            &mut AssociationBudget::default(),
            allow,
        )
    };
    assert!(run(&[o], false).is_empty());
    let ds = run(&[o], true);
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].support, 1);
    assert!((distance(ds[0].polygon[0], ds[0].polygon[3]) - 1.).abs() < 1e-8);
    {
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            assert!(run(
                &[Observation {
                    ambiguous: true,
                    ..o
                }],
                true
            )
            .is_empty());
        }
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        {
            assert!(run(
                &[Observation {
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    invalid_checksum: false,
                    ambiguous: true,
                    ..o
                }],
                true
            )
            .is_empty());
        }
    }

    {
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            assert!(run(
                &[Observation {
                    short_quiet: true,
                    ..o
                }],
                true
            )
            .is_empty());
        }
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        {
            assert!(run(
                &[Observation {
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    invalid_checksum: false,
                    short_quiet: true,
                    ..o
                }],
                true
            )
            .is_empty());
        }
    }

    for fraction in [0.5005, 0.505] {
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                assert!(run(
                    &[
                        o,
                        Observation {
                            fraction,
                            ambiguous: true,
                            ..o
                        }
                    ],
                    true
                )
                .is_empty());
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                assert!(run(
                    &[
                        o,
                        Observation {
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            invalid_checksum: false,
                            fraction,
                            ambiguous: true,
                            ..o
                        }
                    ],
                    true
                )
                .is_empty());
            }
        }

        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                assert!(run(
                    &[
                        o,
                        Observation {
                            fraction,
                            digits: [4; 13],
                            ..o
                        }
                    ],
                    true
                )
                .is_empty());
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                assert!(run(
                    &[
                        o,
                        Observation {
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            invalid_checksum: false,
                            fraction,
                            digits: [4; 13],
                            ..o
                        }
                    ],
                    true
                )
                .is_empty());
            }
        }
    }
    {
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            assert!(run(
                &[
                    o,
                    Observation {
                        digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
                        ..o
                    }
                ],
                true
            )
            .is_empty());
        }
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        {
            assert!(run(
                &[
                    o,
                    Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
                        ..o
                    }
                ],
                true
            )
            .is_empty());
        }
    }

    assert_eq!(run(&[o, o, o], true)[0].support, 1);
}
#[test]
fn ambiguous_row_is_a_spatial_barrier() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap();
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let o = |fraction, digits| Observation {
        short_quiet: false,
        ambiguous: false,
        digits,
        axis: 0,
        fraction,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let o = |fraction, digits| Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits,
        axis: 0,
        fraction,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };

    let obs = [
        o(0.1, [0; 13]),
        o(0.2, [0; 13]),
        o(0.3, [0; 13]),
        o(0.3, [1; 13]),
        o(0.5, [0; 13]),
        o(0.6, [0; 13]),
    ];
    let out = assemble_many(im, m.0, &obs, &mut Work::default(), true);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|d| d.support == 2));
    let mut direct = vec![obs[0], obs[1], obs[2], obs[4], obs[5]];
    direct[2].ambiguous = true;
    assert_eq!(
        assemble_many(im, m.0, &direct, &mut Work::default(), true).len(),
        2
    );
}
#[test]
fn aggregate_link_and_pixel_limits_are_reported() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap();
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let a = Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.2,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let a = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.2,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let b = Observation { fraction: 0.8, ..a };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let b = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        fraction: 0.8,
        ..a
    };

    for (checks, pixels) in [(0, 100_000), (100, 31)] {
        let mut work = Work::default();
        let mut budget = AssociationBudget {
            checks_left: checks,
            pixels_left: pixels,
        };
        assert!(assemble_many_budget(im, m.0, &[a, b], &mut work, true, &mut budget).is_empty());
        assert_eq!(work.association_truncated, 1);
        assert!(work.association_checks <= checks);
        assert!(work.continuity_samples <= pixels);
    }
    let long = scan::transform([[0., 0.], [1000., 0.], [1000., 10000.], [0., 10000.]]).unwrap();
    let mut work = Work::default();
    assert!(!connected_budget(
        im,
        long.0,
        a,
        b,
        &mut work,
        Some(&mut AssociationBudget::default()),
        &mut std::collections::HashMap::new()
    ));
    assert_eq!(work.continuity_capped_links, 1);
    assert_eq!(work.association_truncated, 1);
    assert_eq!(work.continuity_samples, 0);
}
#[test]
fn integer_run_decoder_matches_original_at512() {
    let mut ex = CandidateScanner::default();
    for reverse in [false, true] {
        for shift in 40..44 {
            let mut p = vec![0f32; 512];
            for i in 0..380 {
                p[shift + i] = f32::from(BITS.as_bytes()[i / 4] - b'0');
            }
            if reverse {
                p.reverse();
            }
            ex.signal = p.clone();
            let old = crate::run_profile::decode(&p).unwrap();
            let new = ex.run_decode(&mut Work::default()).map(|r| r.0);
            assert_eq!(
                old.map(|r| (r.digits, r.left, r.right)),
                new.map(|r| (r.digits, r.left, r.right))
            );
        }
    }
}
#[test]
fn public_configuration_rejects_incompatible_native_profiles() {
    for mode in [DecoderMode::Profile, DecoderMode::Combined] {
        assert!(matches!(
            Config::new("invalid", true, false, mode),
            Err(Error::Parameters)
        ));
    }
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    for mode in [DecoderMode::Runs, DecoderMode::Many] {
        let c = Config::new("valid", true, false, mode).unwrap();
        let out = CandidateScanner::default().scan(im, &qs, c);
        assert!(!out.is_empty());
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
)]
fn interior_normalization_ignores_padding_and_preserves_quiet_values() {
    let mut ex = CandidateScanner::default();
    let n = 512;
    ex.raw_signal = (0..n)
        .map(|i| {
            let u = -0.15 + 1.3 * (f64::from(i) + 0.5) / f64::from(n);
            if !(0.0..=1.0).contains(&u) {
                255.
            } else if i % 8 < 4 {
                60.
            } else {
                150.
            }
        })
        .collect();
    let mut w = Work::default();
    assert!(ex.normalize_interior(-0.15, 1.15, &mut w));
    let a = ex.signal.clone();
    for (i, v) in ex.raw_signal.iter_mut().enumerate() {
        let u = -0.15 + 1.3 * (crate::numeric::usize_f64(i) + 0.5) / f64::from(n);
        if !(0.0..=1.0).contains(&u) {
            *v = 180.;
        }
    }
    assert!(ex.normalize_interior(-0.15, 1.15, &mut w));
    assert_eq!(a, ex.signal);
    assert_eq!(a[0], 0.);
    assert_eq!(w.interior_paths, 2);
    assert!(!ex.normalize_interior(0., 1., &mut w));
    ex.raw_signal.fill(150.);
    assert!(!ex.normalize_interior(-0.15, 1.15, &mut w));
    ex.raw_signal = vec![0.; 64];
    assert!(!ex.normalize_interior(-0.15, 1.15, &mut w));
}
#[test]
fn copied_fixed_sampler_is_identical_and_native_changes_only_length() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let m = scan::transform(qs[0]).unwrap();
    let mut a = Sampler::default();
    let old = a.sample(im, m, Path::default()).unwrap().unwrap().to_vec();
    let mut ex = CandidateScanner::default();
    let mut work = Work::default();
    assert!(ex.native_sample(im, m.0, 0, 0.5, &mut work).unwrap());
    assert_eq!(ex.signal.len(), 494);
    let reference = a.sample(im, m, Path::default()).unwrap().unwrap();
    assert_eq!(old, reference);
}
#[test]
fn blank_noise_invalid_and_checksum_are_not_reads() {
    let mut ex = CandidateScanner::default();
    let q = [[[20., 10.], [400., 10.], [400., 230.], [20., 230.]]];
    let mut state = 1u32;
    for mode in 0..8 {
        let mut p = vec![255u8; 420 * 240];
        for v in &mut p {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *v = match mode {
                0 => 0,
                1 => 128,
                2 => 255,
                _ => (state >> 24) as u8,
            };
        }
        let im = ImageView::new(&p, 420, 240, 1, 420).unwrap();
        for config in CONFIGS {
            let out = ex.scan(im, &q, config);
            assert_eq!(out.len(), 1);
            assert!(out[0].detections.is_empty());
            assert_eq!(out[0].work.paths, if config.dense { 42 } else { 10 });
        }
        let r = ex.scan(im, &[[[0.; 2]; 4]], CONFIGS[0]);
        assert!(r[0].error);
    }
    let mut bad = BITS.as_bytes().to_vec();
    bad[85..92].copy_from_slice(b"1001000");
    ex.signal = vec![0.; 512];
    for i in 0..380 {
        ex.signal[50 + i] = f32::from(bad[i / 4] - b'0');
    }
    assert!(ex.run_decode(&mut Work::default()).is_none());
    assert!(ex.profile_decode(&mut Work::default()).is_none());
}
#[test]
fn equal_text_instances_and_known_white_gaps_survive() {
    let mut ex = CandidateScanner::default();
    for gap in [0, 1, 2, 3, 10] {
        let (p, qs) = fixture(gap);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        for config in CONFIGS {
            let results = ex.scan(im, &qs, config);
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].coverage, qs[0]);
            assert_eq!(results[1].coverage, qs[1]);
            for r in results {
                if gap == 0 {
                    assert_eq!(r.detections.len(), 1, "{}", config.name);
                } else {
                    assert!(
                        r.detections.len() >= 2,
                        "gap {gap} {} {:?}",
                        config.name,
                        r.detections
                    );
                }
                for d in r.detections {
                    assert_eq!(d.digits, [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
                    let min = d.polygon.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                    let max = d
                        .polygon
                        .iter()
                        .map(|p| p[1])
                        .fold(f64::NEG_INFINITY, f64::max);
                    assert!(
                        gap == 0 || max < 110. || min >= 110. + crate::numeric::usize_f64(gap) - 1.,
                        "merged gap {gap} {min} {max}"
                    );
                }
            }
        }
    }
}
#[test]
fn both_predicted_axes_and_directions() {
    let (p, qs) = fixture(0);
    let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
    let mut ex = CandidateScanner::default();
    for turn in 0..4 {
        let mut q = qs[0];
        q.rotate_left(turn);
        for c in CONFIGS {
            let out = ex.scan(im, &[q], c);
            assert_eq!(out[0].detections.len(), 1, "{} turn {turn}", c.name);
            assert_eq!(
                out[0].detections[0].digits,
                [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]
            );
        }
    }
}
#[test]
fn unused_pixel_budget_does_not_silently_stop_disjoint_tracks() {
    let pixels = vec![255; 600 * 200];
    let im = ImageView::new(&pixels, 600, 200, 1, 600).unwrap();
    let m = scan::transform([[0., 0.], [600., 0.], [600., 200.], [0., 200.]]).unwrap();
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let a = Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [1; 13],
        axis: 0,
        fraction: 0.2,
        left: 0.1,
        right: 0.4,
        cost: 0.,
        gap: 1.,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let a = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [1; 13],
        axis: 0,
        fraction: 0.2,
        left: 0.1,
        right: 0.4,
        cost: 0.,
        gap: 1.,
    };

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let b = Observation {
        digits: [2; 13],
        fraction: 0.5,
        left: 0.6,
        right: 0.9,
        ..a
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let b = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        digits: [2; 13],
        fraction: 0.5,
        left: 0.6,
        right: 0.9,
        ..a
    };

    for pixels_left in [0, 31] {
        let mut width = Work::default();
        let mut budget = AssociationBudget {
            checks_left: 100,
            pixels_left,
        };
        let r = assemble_many_budget(im, m.0, &[a, b], &mut width, true, &mut budget);
        assert!(r.is_empty());
        assert_eq!(width.association_truncated, 0);
        assert_eq!(width.continuity_samples, 0);
        assert!(width.association_checks > 0);
    }
}
