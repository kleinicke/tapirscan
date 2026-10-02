use super::*;
fn q(x: f64, y: f64, w: f64, h: f64) -> Quad {
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}
fn c(index: usize, polygon: Quad, digits: [u8; 13]) -> Candidate {
    Candidate {
        index,
        coverage: polygon,
        observations: vec![],
        detections: vec![Detection {
            digits,
            polygon,
            support: 2,
            axis: 0,
        }],
        work: Work::default(),
        ms: 0.,
        error: false,
    }
}

#[test]
fn rotated_product389_duplicate_keeps_raw_gap_contradiction() {
    // Frozen008 diagnosis:048 supported-scale policy explores additional rows.
    // Actual competing evidence must veto merging even for equal accepted text.
    let mut candidate = c(
        3,
        [
            [158.314_777_938_640_42, 440.082_728_862_731_4],
            [330.719_876_596_870_73, 290.376_169_662_891_8],
            [376.346_119_958_412_94, 342.920_273_476_344_05],
            [203.941_021_300_182_63, 492.626_832_676_183_65],
        ],
        [8, 8, 5, 8, 1, 2, 1, 0, 0, 0, 4, 8, 3],
    );
    candidate.detections[0].polygon = [
        [191.536_982_678_976_22, 430.049_602_013_270_4],
        [336.426_924_177_610_2, 304.235_604_786_738_5],
        [347.534_340_789_899_66, 316.534_208_287_619_1],
        [202.055_710_992_441_93, 442.859_388_189_976_7],
    ];
    let mut second = candidate.detections[0].clone();
    second.polygon = [
        [205.988_942_253_052_2, 448.221_454_147_727_1],
        [352.048_801_572_055_9, 321.391_568_337_869_05],
        [358.811_109_312_205_9, 329.563_501_960_328_6],
        [213.572_476_108_807_42, 455.680_282_780_548_17],
    ];
    candidate.detections.push(second);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let observation = crate::candidate_scanner::Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [7, 8, 5, 8, 1, 7, 7, 0, 0, 0, 4, 8, 3],
        axis: 0,
        fraction: 0.5,
        left: 0.136_914_062_499_999_96,
        right: 0.979_882_812_499_999_8,
        cost: 0.065_870_23,
        gap: 0.028_946_504,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let observation = crate::candidate_scanner::Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [7, 8, 5, 8, 1, 7, 7, 0, 0, 0, 4, 8, 3],
        axis: 0,
        fraction: 0.5,
        left: 0.136_914_062_499_999_96,
        right: 0.979_882_812_499_999_8,
        cost: 0.065_870_23,
        gap: 0.028_946_504,
    };

    let (a, b) = crate::identity::gap_edges(
        candidate.detections[0].polygon,
        candidate.detections[1].polygon,
    )
    .expect("eligible geometry");
    let m = crate::scan::transform(candidate.coverage).unwrap();
    let pt = crate::candidate_scanner::point(
        m.0,
        observation.axis,
        (observation.left + observation.right) * 0.5,
        observation.fraction,
    )
    .unwrap();
    assert!(crate::identity::barrier_between(a, b, pt));
    candidate.observations.push(observation);
    let pixels = vec![255; 1000 * 1000];
    let f = reconcile_image(
        Some(ImageView::new(&pixels, 1000, 1000, 1, 1000).unwrap()),
        vec![candidate],
        Policy {
            source_identity: true,
            ..Policy::default()
        },
    );
    assert_eq!(f.barcodes.len(), 2);
    assert_eq!(f.reconciliation.source_pairs, 0);
    assert_eq!(f.reconciliation.source_pixels, 0);
    assert_eq!(f.candidates[0].observations[0].digits, observation.digits);
}
#[test]
fn rotated_real_duplicate_retains_conflicting_gap_observation() {
    // Frozen whole-scene +40degree development case5021047102439_2.
    // Two accepted bands share a candidate, but a different raw read falls
    // between them. The geometry is eligible; contradiction must veto merging.
    let mut candidate = c(
        0,
        [
            [499.341_561_335_391_13, 1_631.622_487_722_129_3],
            [1_365.190_883_379_574_6, 880.648_752_325_499],
            [1_782.499_267_912_159_7, 1_361.551_777_691_788_9],
            [916.649_945_867_976, 2_112.525_513_088_419_3],
        ],
        [5, 0, 2, 1, 0, 4, 7, 1, 0, 2, 4, 3, 9],
    );
    candidate.detections[0].polygon = [
        [524.758_141_214_695_1, 1_624.252_610_767_705],
        [1_294.550_488_135_453_4, 956.591_599_170_446_1],
        [1_560.887_328_873_230_7, 1_287.487_692_090_278_2],
        [814.950_649_192_182_7, 1_934.458_060_314_289_5],
    ];
    let mut second = candidate.detections[0].clone();
    second.polygon = [
        [821.680_978_167_715_7, 1_941.107_270_009_087_9],
        [1_566.263_134_316_768_9, 1_295.311_715_428_289_7],
        [1_573.980_958_430_841_2, 1_307.347_744_707_973_6],
        [831.384_260_323_076_9, 1_951.421_259_663_886_5],
    ];
    candidate.detections.push(second);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let observation = crate::candidate_scanner::Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [7, 0, 2, 5, 6, 4, 7, 1, 0, 2, 4, 3, 9],
        axis: 0,
        fraction: 0.6875,
        left: 0.034_294_564_269_340_74,
        right: 0.896_728_315_959_380_7,
        cost: 0.061_383_46,
        gap: 0.031_900_98,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let observation = crate::candidate_scanner::Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: [7, 0, 2, 5, 6, 4, 7, 1, 0, 2, 4, 3, 9],
        axis: 0,
        fraction: 0.6875,
        left: 0.034_294_564_269_340_74,
        right: 0.896_728_315_959_380_7,
        cost: 0.061_383_46,
        gap: 0.031_900_98,
    };

    let (a, b) = crate::identity::gap_edges(
        candidate.detections[0].polygon,
        candidate.detections[1].polygon,
    )
    .expect("geometry eligible");
    let m = crate::scan::transform(candidate.coverage).unwrap();
    let pt = crate::candidate_scanner::point(
        m.0,
        observation.axis,
        (observation.left + observation.right) * 0.5,
        observation.fraction,
    )
    .unwrap();
    assert!(crate::identity::barrier_between(a, b, pt));
    candidate.observations.push(observation);
    let pixels = vec![255; 2200 * 2200];
    let f = reconcile_image(
        Some(ImageView::new(&pixels, 2200, 2200, 1, 2200).unwrap()),
        vec![candidate],
        Policy {
            source_identity: true,
            ..Policy::default()
        },
    );
    assert_eq!(f.barcodes.len(), 2);
    assert_eq!(f.reconciliation.source_pairs, 0);
    assert_eq!(f.reconciliation.source_pixels, 0);
    assert_eq!(f.candidates[0].observations[0].digits, observation.digits);
}

#[test]
fn frozen_derived_duplicate_retains_competing_gap_read() {
    // Iteration039 derived_2_sameTrue_turn0_proposals candidate1. Actual
    // geometry/raw competing observation; blank pixels cannot affect the veto.
    let mut candidate = c(
        1,
        [[34.0, 572.0], [735.0, 531.0], [755.0, 871.0], [54.0, 912.0]],
        [9, 7, 8, 5, 0, 9, 0, 3, 5, 9, 3, 0, 6],
    );
    candidate.detections[0].polygon = [
        [67.765_297_553_845_09, 649.102_172_325_552_9],
        [652.674_169_404_885_6, 614.892_095_655_235_4],
        [657.672_307_411_457_4, 682.833_716_684_808_2],
        [68.770_883_534_129_87, 717.277_309_236_834_5],
    ];
    let mut second = candidate.detections[0].clone();
    second.polygon = [
        [68.443_136_181_094_64, 762.785_779_481_676],
        [660.338_974_078_137_3, 728.167_050_018_368_2],
        [664.003_778_751_369, 773.442_004_381_161],
        [69.113_526_834_611_15, 808.235_870_755_750_3],
    ];
    candidate.detections.push(second);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let observation = crate::candidate_scanner::Observation {
        short_quiet: false,
        digits: [3, 6, 4, 1, 2, 0, 0, 3, 5, 9, 3, 0, 6],
        axis: 0,
        fraction: 0.5,
        left: 0.053_730_147_864_183_98,
        right: 0.877_139_495_481_927_5,
        cost: 0.099_777_33,
        gap: 0.020_716_548,
        ambiguous: false,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let observation = crate::candidate_scanner::Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        digits: [3, 6, 4, 1, 2, 0, 0, 3, 5, 9, 3, 0, 6],
        axis: 0,
        fraction: 0.5,
        left: 0.053_730_147_864_183_98,
        right: 0.877_139_495_481_927_5,
        cost: 0.099_777_33,
        gap: 0.020_716_548,
        ambiguous: false,
    };

    let (a, b) = crate::identity::gap_edges(
        candidate.detections[0].polygon,
        candidate.detections[1].polygon,
    )
    .expect("geometry is eligible");
    let m = crate::scan::transform(candidate.coverage).unwrap();
    let p = crate::candidate_scanner::point(
        m.0,
        observation.axis,
        (observation.left + observation.right) * 0.5,
        observation.fraction,
    )
    .unwrap();
    assert!(crate::identity::barrier_between(a, b, p));
    candidate.observations.push(observation);
    let pixels = vec![255; 800 * 1000];
    let frame = reconcile_image(
        Some(ImageView::new(&pixels, 800, 1000, 1, 800).unwrap()),
        vec![candidate],
        Policy {
            source_identity: true,
            ..Policy::default()
        },
    );
    assert_eq!(frame.barcodes.len(), 2);
    assert_eq!(frame.reconciliation.source_pairs, 0);
    assert_eq!(frame.reconciliation.source_pixels, 0);
    assert!(!frame.reconciliation.truncated);
    assert_eq!(
        frame.candidates[0].observations[0].digits,
        observation.digits
    );
}
#[test]
fn identical_shifted_nested_and_distinct_equal_values() {
    let d = [0; 13];
    let frame = reconcile(
        vec![
            c(0, q(0., 0., 100., 50.), d),
            c(1, q(1., 1., 100., 50.), d),
            c(2, q(10., 10., 80., 30.), d),
            c(3, q(200., 0., 100., 50.), d),
        ],
        Policy::default(),
    );
    assert_eq!(frame.barcodes.len(), 2);
    assert_eq!(frame.candidates.len(), 4);
    assert_eq!(frame.barcodes[0].candidate_indices, vec![0, 1, 2]);
    assert_eq!(frame.barcodes[1].candidate_indices, vec![3]);
    assert!(!frame.unfinished);
}
#[test]
fn a_broad_equal_read_cannot_bridge_two_instances() {
    let d = [0; 13];
    let frame = reconcile(
        vec![
            c(0, q(0., 0., 100., 50.), d),
            c(1, q(200., 0., 100., 50.), d),
            c(2, q(0., 0., 300., 50.), d),
        ],
        Policy::default(),
    );
    assert_eq!(frame.barcodes.len(), 2);
    assert_eq!(frame.reconciliation.ambiguous, 1);
    assert!(frame.unfinished);
}
#[test]
fn comparison_and_result_caps_keep_original_evidence() {
    for policy in [
        Policy {
            max_association_checks: 0,
            ..Policy::default()
        },
        Policy {
            max_results: 1,
            ..Policy::default()
        },
    ] {
        let f = reconcile(
            vec![
                c(0, q(0., 0., 10., 10.), [0; 13]),
                c(1, q(30., 0., 10., 10.), [0; 13]),
            ],
            policy,
        );
        assert_eq!(
            f.barcodes.len(),
            usize::from(policy.max_association_checks != 0)
        );
        assert_eq!(f.candidates.len(), 2);
        assert!(f.unfinished);
        assert!(f.reconciliation.truncated);
    }
}
#[test]
fn exhausted_conflict_checks_publish_only_pending_evidence() {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        for limit in 0..3 {
            let cs = order
                .iter()
                .enumerate()
                .map(|(i, &v)| c(i, q(0., 0., 100., 50.), [if v == 2 { 2 } else { 1 }; 13]))
                .collect();
            let f = reconcile(
                cs,
                Policy {
                    max_association_checks: limit,
                    ..Policy::default()
                },
            );
            assert!(f.barcodes.is_empty());
            assert_eq!(f.candidates.len(), 3);
            if f.reconciliation.truncated {
                assert_eq!(f.reconciliation.pending_observations, 3);
            }
        }
    }
    for order in [[1, 2], [2, 1]] {
        let f = reconcile(
            order
                .iter()
                .enumerate()
                .map(|(i, &v)| c(i, q(0., 0., 100., 50.), [v; 13]))
                .collect(),
            Policy {
                max_association_checks: 0,
                ..Policy::default()
            },
        );
        assert!(f.barcodes.is_empty());
        assert_eq!(f.reconciliation.pending_observations, 2);
    }
}
#[test]
fn exhausted_candidate_partial_and_later_conflict_are_pending() {
    for reverse in [false, true] {
        let mut candidate = c(0, q(0., 0., 100., 50.), [1; 13]);
        let mut later = candidate.detections[0].clone();
        later.digits = [2; 13];
        candidate.detections.push(later);
        if reverse {
            candidate.detections.reverse();
        }
        candidate.work.association_truncated = 1;
        let f = reconcile(vec![candidate], Policy::default());
        assert!(f.barcodes.is_empty());
        assert_eq!(f.reconciliation.pending_observations, 2);
        assert_eq!(f.candidates[0].detections.len(), 2);
    }
}
#[test]
fn overlapping_tile_bands_require_source_agreement_and_no_contradiction() {
    for reverse in [false, true] {
        for white_gap in [false, true] {
            for contradictory in [false, true] {
                let mut pixels = vec![255; 500 * 200];
                for y in 10..150 {
                    for x in 50..450 {
                        if (x / 4) % 3 == 0 && !(white_gap && (65..70).contains(&y)) {
                            pixels[y * 500 + x] = 0;
                        }
                    }
                }
                let mut a = c(0, q(0., 0., 500., 200.), [1; 13]);
                a.detections[0].polygon = q(50., 20., 400., 60.);
                let mut b = c(1, q(0., 0., 500., 200.), [1; 13]);
                b.detections[0].polygon = q(50., 55., 400., 60.);
                if contradictory {
                    {
                        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                        {
                            b.observations.push(crate::candidate_scanner::Observation {
                                short_quiet: false,
                                ambiguous: false,
                                digits: [2; 13],
                                axis: 0,
                                fraction: 0.35,
                                left: 0.1,
                                right: 0.9,
                                cost: 0.,
                                gap: 1.,
                            });
                        }
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        {
                            b.observations.push(crate::candidate_scanner::Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
                                short_quiet: false,
                                ambiguous: false,
                                digits: [2; 13],
                                axis: 0,
                                fraction: 0.35,
                                left: 0.1,
                                right: 0.9,
                                cost: 0.,
                                gap: 1.,
                            });
                        }
                    }
                }
                let mut cs = vec![a, b];
                if reverse {
                    cs.reverse();
                }
                let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
                let f = reconcile_image(
                    Some(im),
                    cs,
                    Policy {
                        source_identity: true,
                        ..Policy::default()
                    },
                );
                assert_eq!(
                    f.barcodes.len(),
                    if white_gap || contradictory { 2 } else { 1 }
                );
                assert_eq!(f.candidates.len(), 2);
            }
        }
    }
}

#[test]
fn disjoint_tile_bands_require_source_agreement_and_no_contradiction() {
    for reverse in [false, true] {
        for white_gap in [false, true] {
            for contradictory in [false, true] {
                let mut pixels = vec![255; 500 * 200];
                for y in 10..150 {
                    for x in 50..450 {
                        if (x / 4) % 3 == 0 && !(white_gap && (65..70).contains(&y)) {
                            pixels[y * 500 + x] = 0;
                        }
                    }
                }
                let mut a = c(0, q(0., 0., 500., 200.), [1; 13]);
                a.detections[0].polygon = q(50., 20., 400., 40.);
                let mut b = c(1, q(0., 0., 500., 200.), [1; 13]);
                b.detections[0].polygon = q(50., 80., 400., 40.);
                if contradictory {
                    {
                        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                        {
                            b.observations.push(crate::candidate_scanner::Observation {
                                short_quiet: false,
                                ambiguous: false,
                                digits: [2; 13],
                                axis: 0,
                                fraction: 0.35,
                                left: 0.1,
                                right: 0.9,
                                cost: 0.,
                                gap: 1.,
                            });
                        }
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        {
                            b.observations.push(crate::candidate_scanner::Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
                                short_quiet: false,
                                ambiguous: false,
                                digits: [2; 13],
                                axis: 0,
                                fraction: 0.35,
                                left: 0.1,
                                right: 0.9,
                                cost: 0.,
                                gap: 1.,
                            });
                        }
                    }
                }
                let mut cs = vec![a, b];
                if reverse {
                    cs.reverse();
                }
                let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
                let f = reconcile_image(
                    Some(im),
                    cs,
                    Policy {
                        source_identity: true,
                        ..Policy::default()
                    },
                );
                assert_eq!(
                    f.barcodes.len(),
                    if white_gap || contradictory { 2 } else { 1 }
                );
                assert_eq!(f.candidates.len(), 2);
            }
        }
    }
}
#[test]
fn pending_candidates_quarantine_overlap_but_not_independent_reads() {
    for swapped in [false, true] {
        for different in [false, true] {
            let done = c(0, q(0., 0., 100., 50.), [1; 13]);
            let mut pending = c(1, q(0., 0., 100., 50.), [if different { 2 } else { 1 }; 13]);
            pending.work.association_truncated = 1;
            let independent = c(2, q(200., 0., 100., 50.), [3; 13]);
            let mut cs = vec![done, pending, independent];
            if swapped {
                cs.reverse();
            }
            let f = reconcile(cs, Policy::default());
            assert_eq!(f.barcodes.len(), 1);
            assert_eq!(f.barcodes[0].detection.digits, [3; 13]);
            assert_eq!(f.reconciliation.pending_quarantined, 1);
            assert_eq!(f.reconciliation.pending_observations, 2);
            assert!(f.unfinished);
        }
    }
}
#[test]
fn image_direction_identity_survives_cyclic_proposal_axes() {
    let mut pixels = vec![255; 500 * 200];
    for y in 10..150 {
        for x in 50..450 {
            if (x / 4) % 3 == 0 {
                pixels[y * 500 + x] = 0;
            }
        }
    }
    for shift in 0..4 {
        for reverse in [false, true] {
            let mut a = c(0, q(0., 0., 500., 200.), [1; 13]);
            a.detections[0].polygon = q(50., 20., 400., 60.);
            let mut b = c(1, q(0., 0., 500., 200.), [1; 13]);
            b.coverage.rotate_left(shift);
            b.detections[0].axis = shift % 2;
            b.detections[0].polygon = q(50., 55., 400., 60.);
            let mut cs = vec![a, b];
            if reverse {
                cs.reverse();
            }
            let f = reconcile_image(
                Some(ImageView::new(&pixels, 500, 200, 1, 500).unwrap()),
                cs,
                Policy {
                    source_identity: true,
                    ..Policy::default()
                },
            );
            assert_eq!(f.barcodes.len(), 1);
        }
    }
}
#[test]
fn result_cap_keeps_highest_support_and_raw_candidates() {
    let a = c(0, q(0., 0., 10., 10.), [0; 13]);
    let mut b = c(1, q(30., 0., 10., 10.), [1; 13]);
    b.detections[0].support = 99;
    let f = reconcile(
        vec![a, b],
        Policy {
            max_results: 1,
            ..Policy::default()
        },
    );
    assert_eq!(f.barcodes[0].detection.support, 99);
    assert_eq!(f.candidates.len(), 2);
    assert!(f.unfinished);
}
#[test]
fn source_association_budget_is_shared_across_candidates() {
    let bits=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    let mut pixels = vec![255; 1000 * 240];
    for y in 10..230 {
        for x in 0..380 {
            if bits[x / 4] == b'1' {
                pixels[y * 1000 + 30 + x] = 0;
                pixels[y * 1000 + 530 + x] = 0;
            }
        }
    }
    let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
    let policy = Policy {
        max_retry_paths_per_candidate: 0,
        max_retry_paths_per_frame: 0,
        max_association_checks: 1000,
        max_association_pixels: 4096,
        ..Policy::default()
    };
    let frame = CandidateScanner::default()
        .scan_frame(
            im,
            &[q(30., 10., 380., 220.), q(530., 10., 380., 220.)],
            policy,
        )
        .unwrap();
    assert!(frame.unfinished);
    {
        #[cfg(feature = "mode-low")]
        {
            assert!(frame
                .candidates
                .iter()
                .all(|c| matches!(c.work.paths, 3 | 6)));
        }
        #[cfg(feature = "mode-medium")]
        {
            assert!(frame.candidates.iter().all(|c| c.work.paths == 6));
        }
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        {
            assert!(frame.candidates.iter().all(|c| c.work.paths == 10));
        }
    }

    assert!(
        frame
            .candidates
            .iter()
            .map(|c| c.work.continuity_samples)
            .sum::<usize>()
            <= 4096
    );
    assert!(
        frame
            .candidates
            .iter()
            .map(|c| c.work.association_checks)
            .sum::<usize>()
            + frame.reconciliation.comparisons
            <= 1000
    );
    assert!(frame
        .candidates
        .iter()
        .any(|c| c.work.association_truncated > 0));
}
#[test]
fn geometry_and_conflicting_text() {
    assert!(!same_space(q(0., 0., 10., 10.), q(10., 0., 10., 10.)));
    let mut b = q(0., 0., 10., 10.);
    b.reverse();
    assert!(same_space(q(0., 0., 10., 10.), b));
    let f = reconcile(vec![c(0, b, [0; 13]), c(1, b, [1; 13])], Policy::default());
    assert!(f.barcodes.is_empty());
    assert_eq!(f.reconciliation.conflicting, 1);
    assert_eq!(f.candidates.len(), 2);
}
#[test]
fn conflicts_with_nonrepresentative_members_survive_permutations() {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut candidates = vec![];
        for i in order {
            let mut d = match i {
                0 => c(i, q(0., 0., 100., 50.), [1; 13]),
                1 => c(i, q(15., 0., 100., 50.), [1; 13]),
                _ => c(i, q(100., 0., 15., 50.), [2; 13]),
            };
            d.detections[0].support = if i == 1 { 9 } else { 2 };
            candidates.push(d);
        }
        let f = reconcile(candidates, Policy::default());
        assert!(f.unfinished);
        assert!(f.reconciliation.conflicting > 0);
        assert!(f.barcodes.is_empty());
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the paired identity and separating-gap controls in one regression."
)]
fn source_identity_preserves_ambiguity_and_neighbor_gaps() {
    let mut pixels = vec![255; 500 * 200];
    for y in 0..200 {
        for x in 50..450 {
            if (x / 4) % 3 == 0 {
                pixels[y * 500 + x] = 0;
            }
        }
    }
    let make = || {
        let mut c = c(0, q(0., 0., 500., 200.), [1; 13]);
        c.detections = vec![
            Detection {
                digits: [1; 13],
                polygon: q(50., 20., 400., 60.),
                support: 3,
                axis: 0,
            },
            Detection {
                digits: [1; 13],
                polygon: q(50., 120., 400., 60.),
                support: 4,
                axis: 0,
            },
        ];
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                c.observations.push(crate::candidate_scanner::Observation {
                    short_quiet: false,
                    ambiguous: true,
                    digits: [0; 13],
                    axis: 0,
                    fraction: 0.5,
                    left: 0.1,
                    right: 0.9,
                    cost: 0.,
                    gap: 0.,
                });
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                c.observations.push(crate::candidate_scanner::Observation {
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    invalid_checksum: false,
                    short_quiet: false,
                    ambiguous: true,
                    digits: [0; 13],
                    axis: 0,
                    fraction: 0.5,
                    left: 0.1,
                    right: 0.9,
                    cost: 0.,
                    gap: 0.,
                });
            }
        }

        c
    };
    let policy = Policy {
        source_identity: true,
        ..Policy::default()
    };
    let f = reconcile_image(
        Some(ImageView::new(&pixels, 500, 200, 1, 500).unwrap()),
        vec![make()],
        policy,
    );
    assert_eq!(f.barcodes.len(), 1);
    assert_eq!(f.candidates[0].detections.len(), 2);
    assert!(f.candidates[0].observations[0].ambiguous);
    assert!(f.unfinished);
    assert_eq!(f.barcodes[0].detection.polygon, q(50., 120., 400., 60.));
    assert_eq!(f.barcodes[0].detection.support, 4);
    let mut contradictory = make();
    {
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        {
            contradictory
                .observations
                .push(crate::candidate_scanner::Observation {
                    short_quiet: false,
                    ambiguous: false,
                    digits: [2; 13],
                    ..contradictory.observations[0]
                });
        }
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        {
            contradictory
                .observations
                .push(crate::candidate_scanner::Observation {
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    invalid_checksum: false,
                    short_quiet: false,
                    ambiguous: false,
                    digits: [2; 13],
                    ..contradictory.observations[0]
                });
        }
    }

    let f = reconcile_image(
        Some(ImageView::new(&pixels, 500, 200, 1, 500).unwrap()),
        vec![contradictory],
        policy,
    );
    assert_eq!(f.barcodes.len(), 2);
    assert_eq!(f.reconciliation.source_pairs, 0);
    // A single original-pixel blank gap must preserve two identical instances.
    pixels[100 * 500..101 * 500].fill(255);
    let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
    let f = reconcile_image(Some(im), vec![make()], policy);
    assert_eq!(f.barcodes.len(), 2);
    assert_eq!(f.reconciliation.source_matches, 0);
    assert!(f.reconciliation.source_pixels > 0);
    let mut a = make();
    a.observations.clear();
    let f = reconcile_image(Some(im), vec![a], policy);
    assert_eq!(f.barcodes.len(), 2);
    assert_eq!(f.reconciliation.source_pairs, 0);
}
