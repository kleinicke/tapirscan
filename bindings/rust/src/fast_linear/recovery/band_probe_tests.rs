use super::*;

#[test]
fn probe_requires_a_value_and_restores_confirmation_state() {
    let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let bits = barcode_research_core::ean::encode(&digits);
    for printed in [false, true] {
        let mut pixels = vec![255; 345 * 60];
        if printed {
            for row in pixels.chunks_exact_mut(345) {
                for (i, bit) in bits.iter().enumerate() {
                    if *bit > 0.5 {
                        row[30 + i * 3..33 + i * 3].fill(0);
                    }
                }
            }
        }
        let image = Image {
            data: &pixels,
            width: 345,
            height: 60,
            channels: 1,
            stride: 345,
        };
        let mut candidate = Candidate {
            image,
            im: ImageView::new(&pixels, 345, 60, 1, 345).unwrap(),
            quad: [[30., 6.], [315., 6.], [315., 54.], [30., 54.]],
            mask: 1,
            dense: false,
            restored: false,
            profile: SourceProfile::BandEan(1.5),
            localized: true,
            remaining: 131_072,
            observations: Vec::new(),
            row_positions: Vec::new(),
        };
        let mut sampler = barcode_research_core::fast_profile::Sampler::default();
        sampler.runs = vec![1., 2., 3.];
        let previous_runs = sampler.runs.clone();
        assert_eq!(band_has_evidence(&mut candidate, &mut sampler), printed);
        assert_eq!(sampler.runs, previous_runs);
        assert_eq!(candidate.remaining, 131_072);
        assert!(!candidate.restored);
        assert!(candidate.observations.is_empty());
        assert!(candidate.row_positions.is_empty());
    }
}
