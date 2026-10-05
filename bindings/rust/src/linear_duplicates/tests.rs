use super::*;

#[cfg(feature = "low")]
#[test]
fn contrast_shortcut_preserves_boundary_color_and_budget_semantics() {
    for channels in [1, 3, 4] {
        for contrast in [0_u8, 23, 24, 200] {
            let mut pixels = vec![0; 80 * 64 * channels];
            for y in 0..64 {
                for x in 0..80 {
                    for c in 0..channels {
                        pixels[(y * 80 + x) * channels + c] = if (x / 3 + y / 5 + c) % 2 == 0 {
                            20
                        } else {
                            20 + contrast
                        };
                    }
                }
            }
            let image = Image {
                data: &pixels,
                width: 80,
                height: 64,
                channels,
                stride: 80 * channels,
            };
            for left in [
                [0., 0.],
                [2., 2.],
                [40., 31.],
                [-0.49, 10.],
                [-1., 20.],
                [79.4, 63.4],
            ] {
                for right in [
                    [79., 63.],
                    [70., 10.],
                    [2., 2.],
                    [-1., 0.],
                    [80., 64.],
                    [40., 31.],
                ] {
                    for budget in [0, 63, 64, 128, 4096] {
                        let mut reference = Evidence {
                            image,
                            remaining: budget,
                        };
                        let mut candidate = Evidence {
                            image,
                            remaining: budget,
                        };
                        assert_eq!(
                            candidate.has_contrast(left, right),
                            reference.profile(left, right).is_some()
                        );
                        assert_eq!(candidate.remaining, reference.remaining);
                    }
                }
            }
        }
    }
}

#[test]
fn continuous_bands_merge_but_separators_and_supplements_preserve_products() {
    let mut pixels = vec![255; 460 * 460];
    for y in 20..420 {
        for x in 60..252 {
            pixels[y * 460 + x] = if (x - 60) / 3 % 3 == 0 { 20 } else { 220 };
        }
    }
    let read = |lo, hi| {
        crate::read::Read::primary(
            [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
            [[60., lo], [252., lo], [252., hi], [60., hi]],
            7,
            0,
            vec![],
        )
    };
    let reads = vec![read(30., 120.), read(280., 400.)];
    let scan = |pixels: &[u8], reads| {
        merge(
            reads,
            Image {
                data: pixels,
                width: 460,
                height: 460,
                channels: 1,
                stride: 460,
            },
        )
    };
    assert_eq!(scan(&pixels, reads.clone()).len(), 1);
    let mut supplements = reads.clone();
    supplements[0].addon = Some("12".into());
    supplements[1].addon = Some("34".into());
    assert_eq!(scan(&pixels, supplements).len(), 2);
    pixels[200 * 460..202 * 460].fill(255);
    assert_eq!(scan(&pixels, reads).len(), 2);
}
#[test]
fn glare_across_the_bars_does_not_split_one_symbol() {
    // Bars stay faintly visible under a wide glare band, unlike a printed separator.
    let mut pixels = vec![255; 460 * 460];
    for y in 20..420 {
        for x in 60..252 {
            let bar = (x - 60) / 3 % 3 == 0;
            pixels[y * 460 + x] = match (bar, (190..220).contains(&y)) {
                (true, false) => 20,
                (false, false) => 220,
                (true, true) => 200,
                (false, true) => 250,
            };
        }
    }
    let read = |lo, hi| {
        crate::read::Read::primary(
            [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
            [[60., lo], [252., lo], [252., hi], [60., hi]],
            7,
            0,
            vec![],
        )
    };
    let image = Image {
        data: &pixels,
        width: 460,
        height: 460,
        channels: 1,
        stride: 460,
    };
    let reads = merge_output(vec![read(60., 120.), read(300., 360.)], image);
    assert_eq!(reads.len(), 1);
    let ys: Vec<f64> = reads[0].polygon.iter().map(|p| p[1]).collect();
    assert!(
        ys.iter().any(|&y| y < 30.) && ys.iter().any(|&y| y > 410.),
        "{ys:?}"
    );
}
#[test]
fn selected_code93_ownership_preserves_separate_equal_labels() {
    let mut pixels = vec![255; 320 * 160];
    for y in 10..150 {
        for x in 40..280 {
            pixels[y * 320 + x] = if (x - 40) / 3 % 3 == 0 { 20 } else { 220 };
        }
    }
    let read = |top, bottom| {
        let mut r = crate::read::Read::primary(
            [0; 13],
            [[40., top], [280., top], [280., bottom], [40., bottom]],
            7,
            0,
            vec![],
        );
        r.format = "Code93".into();
        r.text = "SAME-CODE".into();
        r
    };
    let reads = vec![read(20., 50.), read(110., 140.)];
    let scan = |pixels: &[u8], enabled| {
        merge_selected(
            reads.clone(),
            Image {
                data: pixels,
                width: 320,
                height: 160,
                channels: 1,
                stride: 320,
            },
            enabled,
        )
    };
    if !crate::LOW_FAST_PATH {
        assert_eq!(scan(&pixels, false).len(), 2);
    }
    assert_eq!(scan(&pixels, true).len(), 1);
    pixels[79 * 320..81 * 320].fill(255);
    assert_eq!(scan(&pixels, true).len(), 2);
}

#[test]
fn crossing_bands_share_ink_but_an_oblique_separator_breaks_the_proof() {
    let mut pixels = vec![255; 320 * 260];
    for y in 20..240 {
        for x in 60..252 {
            pixels[y * 320 + x] = if (x - 60) / 3 % 3 == 0 { 20 } else { 220 };
        }
    }
    let a = [[60., 125.], [252., 125.], [252., 140.], [60., 140.]];
    let b = [[60., 75.], [252., 155.], [252., 180.], [60., 100.]];
    let proof = |pixels: &[u8], a, b| {
        Evidence {
            image: Image {
                data: pixels,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
            remaining: 32768,
        }
        .connected_warped(a, b)
    };
    assert!(proof(&pixels, a, b).is_some());
    assert!(proof(&pixels, b, a).is_some());
    let trace = |pixels: &[u8], a, b| {
        Evidence {
            image: Image {
                data: pixels,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
            remaining: 32768,
        }
        .connected_traces(a, b)
    };
    assert!(trace(&pixels, a, b).is_some());
    let near_upper = [[60., 50.], [252., 50.], [252., 70.], [60., 70.]];
    let near_lower = [[60., 120.], [252., 120.], [252., 140.], [60., 140.]];
    assert!(trace(&pixels, near_upper, near_lower).is_some());
    // Two disjoint equal-value labels separated by an oblique one-pixel gap.
    let upper = [[60., 30.], [252., 55.], [252., 95.], [60., 70.]];
    let lower = [[60., 165.], [252., 185.], [252., 225.], [60., 205.]];
    assert!(proof(&pixels, upper, lower).is_some());
    for x in 60..252 {
        let y = 110 + (x - 60) / 8;
        pixels[y * 320 + x] = 255;
    }
    assert!(proof(&pixels, upper, lower).is_none());
    assert!(trace(&pixels, near_upper, near_lower).is_none());
}
#[test]
fn changing_bar_width_keeps_continuity_but_white_cuts_break_it() {
    let mut pixels = vec![220; 320 * 180];
    for y in 30_usize..160 {
        let width = (5 + y.saturating_sub(70) / 20).min(7);
        for x in 40..280 {
            if (x - 40) % 12 < width {
                pixels[y * 320 + x] = 20;
            }
        }
    }
    let a = [[40., 77.], [280., 77.], [280., 83.], [40., 83.]];
    let b = [[40., 125.], [280., 125.], [280., 131.], [40., 131.]];
    let proof = |pixels: &[u8]| {
        Evidence {
            image: Image {
                data: pixels,
                width: 320,
                height: 180,
                channels: 1,
                stride: 320,
            },
            remaining: 32768,
        }
        .connected_traces(a, b)
    };
    assert!(proof(&pixels).is_some());
    pixels[100 * 320..101 * 320].fill(255);
    assert!(proof(&pixels).is_none());
}
#[cfg(any(feature = "high", feature = "very-high"))]
#[test]
fn long_ownership_traces_require_unbroken_bars() {
    let mut pixels = vec![220; 1200 * 700];
    for y in 40..660 {
        for x in 80..1120 {
            if (x - 80) % 16 < 7 {
                pixels[y * 1200 + x] = 20;
            }
        }
    }
    let a = [[80., 97.], [1120., 97.], [1120., 103.], [80., 103.]];
    let b = [[80., 547.], [1120., 547.], [1120., 553.], [80., 553.]];
    let proof = |pixels: &[u8]| {
        Evidence {
            image: Image {
                data: pixels,
                width: 1200,
                height: 700,
                channels: 1,
                stride: 1200,
            },
            remaining: 262_144,
        }
        .owned_bars(a, b, false)
    };
    assert!(proof(&pixels).is_some());
    pixels[320 * 1200..324 * 1200].fill(255);
    assert!(proof(&pixels).is_none());
}

#[test]
fn weak_itf_requires_shared_bars_not_just_overlapping_retail_boxes() {
    let mut pixels = vec![220; 320 * 260];
    for y in 20..240 {
        for x in 60..252 {
            if (x - 60) / 3 % 3 == 0 {
                pixels[y * 320 + x] = 20;
            }
        }
    }
    let primary = crate::read::Read::primary(
        [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
        [[60., 50.], [252., 50.], [252., 160.], [60., 160.]],
        7,
        0,
        vec![],
    );
    let mut weak = primary.clone();
    weak.format = "ITF".into();
    weak.text = "123456".into();
    weak.support = 2;
    weak.polygon = [[60., 70.], [252., 70.], [252., 180.], [60., 180.]];
    let scan = |pixels: &[u8], weak: crate::read::Read| {
        merge(
            vec![primary.clone(), weak],
            Image {
                data: pixels,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
        )
    };
    assert_eq!(scan(&pixels, weak.clone()).len(), 1);
    let mut supported = weak.clone();
    supported.support = 3;
    // The full measured footprint now proves this stronger alias uses the same bars.
    assert_eq!(scan(&pixels, supported).len(), 1);
    pixels[115 * 320..116 * 320].fill(255);
    assert_eq!(scan(&pixels, weak).len(), 2);
}
#[test]
fn footprints_arbitrate_different_payloads_only_on_connected_source_bars() {
    let mut pixels = vec![240; 320 * 260];
    for y in 20..240 {
        for x in 60..252 {
            if (x - 60) / 3 % 3 == 0 {
                pixels[y * 320 + x] = 20;
            }
        }
    }
    let make = |text: &str, y, support| Read {
        text: text.to_owned(),
        format: "EAN8".to_owned(),
        addon: None,
        gs1: false,
        reader_initialization: false,
        support,
        polygon: [[60., y - 2.], [252., y - 2.], [252., y + 2.], [60., y + 2.]],
        geometry_changed: false,
        allow_code93: false,
        payload: (),
    };
    let scan = |p: &[u8]| {
        complete_footprints(
            vec![make("42267638", 70., 7), make("12345670", 180., 3)],
            Image {
                data: p,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
            true,
        )
    };
    assert_eq!(scan(&pixels).len(), 1);
    pixels[120 * 320..121 * 320].fill(240);
    assert_eq!(scan(&pixels).len(), 2);
}
#[test]
#[cfg(any(feature = "high", feature = "very-high"))]
fn upce_fragment_requires_continuous_source_bars() {
    let mut pixels = vec![220; 320 * 260];
    for y in 20..240 {
        for x in 60..252 {
            if (x - 60) / 3 % 3 == 0 {
                pixels[y * 320 + x] = 20;
            }
        }
    }
    let primary = crate::read::Read::primary(
        [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
        [[60., 50.], [252., 50.], [252., 160.], [60., 160.]],
        7,
        0,
        vec![],
    );
    let mut fragment = primary.clone();
    fragment.format = "UPCE".into();
    fragment.text = "12562213".into();
    fragment.support = 5;
    fragment.polygon = [[60., 70.], [156., 70.], [156., 180.], [60., 180.]];
    let scan = |pixels: &[u8]| {
        merge(
            vec![primary.clone(), fragment.clone()],
            Image {
                data: pixels,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
        )
    };
    assert_eq!(scan(&pixels).len(), 1);
    // The same overlapping outlines cannot prove ownership across a white gap.
    pixels[115 * 320..116 * 320].fill(255);
    assert_eq!(scan(&pixels).len(), 2);
}
