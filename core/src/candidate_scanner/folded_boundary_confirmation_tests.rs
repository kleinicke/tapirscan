use super::*;
const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
#[test]
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
)]
fn narrow_trailing_quiet_needs_four_rows_and_preserves_instances_after_rotation() {
    for second in [A, B] {
        for rotated in [false, true] {
            let (width, height) = if rotated { (320, 512) } else { (512, 320) };
            let mut pixels = vec![255u8; width * height];
            let put = |pixels: &mut Vec<u8>, x: usize, y: usize, value| {
                let (a, b) = if rotated { (319 - y, x) } else { (x, y) };
                pixels[b * width + a] = value;
            };
            for (top, d) in [(10, A), (180, second)] {
                let bits = crate::ean::encode(&d);
                for y in top..top + 130 {
                    for x in 448..512 {
                        put(&mut pixels, x, y, 0);
                    }
                    for x in 0..380 {
                        if bits[x / 4] > 0.5 {
                            put(&mut pixels, 60 + x, y, 0);
                        }
                    }
                }
            }
            let im = ImageView::new(&pixels, width, height, 1, width).unwrap();
            let mut quad = [[60., 10.], [440., 10.], [440., 310.], [60., 310.]];
            if rotated {
                for p in &mut quad {
                    *p = [319. - p[1], p[0]];
                }
            }
            let c = CandidateScanner::default().scan(
                im,
                &[quad],
                Config::new("folded_boundary", false, true, DecoderMode::Many).unwrap(),
            );
            assert_eq!(
                c[0].detections.len(),
                2,
                "distinct symbols, rotated={rotated}"
            );
            assert_eq!(
                c[0].detections.iter().filter(|d| d.digits == A).count(),
                if second == A { 2 } else { 1 }
            );
            let mut obs: Vec<_> = c[0]
                .observations
                .iter()
                .filter(|o| o.short_quiet && !o.ambiguous && o.axis == 0 && o.fraction < 0.45)
                .copied()
                .collect();
            obs.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
            obs.dedup_by(|a, b| a.fraction == b.fraction);
            assert!(obs.len() >= 4);
            let m = scan::transform(quad).unwrap();
            assert!(assemble_many(im, m.0, &obs[..3], &mut Work::default(), true).is_empty());
            assert_eq!(
                assemble_many(im, m.0, &obs[..4], &mut Work::default(), true).len(),
                1
            );
            let mut conflict = obs[..4].to_vec();
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    conflict.push(Observation {
                        ambiguous: true,
                        ..obs[1]
                    });
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    conflict.push(Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        ambiguous: true,
                        ..obs[1]
                    });
                }
            }

            assert!(assemble_many(im, m.0, &conflict, &mut Work::default(), true).is_empty());
        }
    }
}
