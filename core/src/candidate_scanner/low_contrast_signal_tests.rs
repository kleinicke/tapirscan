use super::*;
#[test]
fn dim_symbols_and_noisy_non_symbols_keep_structure_checks() {
    let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let bits = crate::ean::encode(&digits);
    let (w, h) = (512, 80);
    let q = [[40., 10.], [420., 10.], [420., 70.], [40., 70.]];
    for contrast in [12u8, 24, 48] {
        let mut data = vec![32 + contrast; w * h];
        for y in 10..70 {
            for x in 0..380 {
                if bits[x / 4] > 0.5 {
                    data[y * w + 40 + x] = 32;
                }
            }
        }
        let im = ImageView::new(&data, w, h, 1, w).unwrap();
        let f = CandidateScanner::default()
            .scan_frame(
                im,
                &[q],
                crate::multi_scan::Policy {
                    transition_cleanup: true,
                    source_identity: true,
                    interior_normalization: true,
                    guard_bias: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(f.barcodes.len(), 1);
        assert_eq!(f.barcodes[0].detection.digits, digits);
    }
    let mut rng = 1_234_567_u32;
    for mode in 0..8 {
        let mut data = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                data[y * w + x] = 32
                    + if mode == 0 {
                        0
                    } else if mode == 1 {
                        ((x / 3) % 2 * 24).to_le_bytes()[0]
                    } else {
                        (rng % 32) as u8
                    };
            }
        }
        let im = ImageView::new(&data, w, h, 1, w).unwrap();
        let f = CandidateScanner::default()
            .scan_frame(
                im,
                &[q],
                crate::multi_scan::Policy {
                    transition_cleanup: true,
                    source_identity: true,
                    interior_normalization: true,
                    guard_bias: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(f.barcodes.is_empty());
    }
}
