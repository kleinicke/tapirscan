use super::*;
#[test]
fn full_decoded_bridge_preserves_separate_same_value_labels() {
    // Independent Code39 writer fixture already used by the Low controls.
    let bits=b"1001011011010101101011001011011010010101101010010110101101010011011010110010101011001010110101001101011010100110110101011001011010100101101101";
    let width = bits.len() * 2 + 40;
    let height = 64;
    for separated in [false, true] {
        let mut pixels = vec![255; width * height];
        for y in 8..56 {
            if separated && (31..33).contains(&y) {
                continue;
            }
            for (x, &bit) in bits.iter().enumerate() {
                if bit == b'1' {
                    pixels[y * width + 20 + x * 2..y * width + 22 + x * 2].fill(0);
                }
            }
        }
        let image = Image {
            data: &pixels,
            width,
            height,
            channels: 1,
            stride: width,
        };
        let quad = |top: f64, bottom: f64| {
            [
                [20., top],
                [usize_f64(width - 20), top],
                [usize_f64(width - 20), bottom],
                [20., bottom],
            ]
        };
        let mut a = Read::primary([0; 13], quad(12., 18.), 4, 0, vec![]);
        a.format = "Code39".into();
        a.text = "SCALE2409".into();
        let mut coincident = a.clone();
        for p in &mut coincident.polygon {
            p[1] += 0.2;
        }
        let mut proof_sampler = barcode_research_core::fast_profile::Sampler::default();
        assert_eq!(
            join_recovered_bands(vec![a.clone(), coincident], image, &mut proof_sampler, 2.).len(),
            1
        );
        let mut b = a.clone();
        b.polygon = quad(44., 50.);
        let im = ImageView::new(&pixels, width, height, 1, width).unwrap();
        let mut sampler = barcode_research_core::fast_profile::Sampler::default();
        let result = decode_bridge(&a, &b, im, &mut sampler, 2., &mut 131_072, &mut 256);
        assert_eq!(result.is_some(), !separated, "separated={separated}");
        assert!(decode_bridge(&a, &b, im, &mut sampler, 2., &mut 0, &mut 256).is_none());
        let (accepted, deferred) =
            append_recovered(vec![a.clone()], vec![b.clone()], image, &mut sampler, 2.);
        assert_eq!(accepted.len(), if separated { 2 } else { 1 });
        assert!(deferred.is_empty());
        // A conflicting module-axis interpretation cannot establish a
        // second physical instance solely from the same value and overlap.
        let mut uncertain = a.clone();
        uncertain.polygon.rotate_left(1);
        let original = uncertain.polygon;
        let (accepted, deferred) =
            append_recovered(vec![uncertain], vec![a.clone()], image, &mut sampler, 2.);
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].polygon, original);
        assert_eq!(deferred.len(), 1);
        // A bounded proof may join continuous bands, never the white gap.
        assert_eq!(
            join_recovered_bands(vec![a, b], image, &mut sampler, 2.).len(),
            if separated { 2 } else { 1 }
        );
    }
}
