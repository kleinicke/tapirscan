use super::*;
#[test]
fn weak_repeated_edges_need_wider_source_support() {
    let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let (width, h) = (360, 100);
    let mut pixels = vec![255; width * h];
    let bits = crate::ean::encode(&d);
    for y in 0..h {
        for x in 0..285 {
            if bits[x / 3] > 0.5 {
                pixels[y * width + 30 + x] = 0;
            }
        }
    }
    let im = ImageView::new(&pixels, width, h, 1, width).unwrap();
    let quad = [[0., 0.], [360., 0.], [360., 100.], [0., 100.]];
    let m = scan::transform(quad).unwrap().0;
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let obs = |f, cost, gap| Observation {
        short_quiet: false,
        ambiguous: false,
        digits: d,
        axis: 0,
        fraction: f,
        left: 30. / 360.,
        right: 315. / 360.,
        cost,
        gap,
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let obs = |f, cost, gap| Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        short_quiet: false,
        ambiguous: false,
        digits: d,
        axis: 0,
        fraction: f,
        left: 30. / 360.,
        right: 315. / 360.,
        cost,
        gap,
    };

    for strong in [false, true] {
        let cost = if strong { 0.04 } else { 0.11 };
        let pair = [obs(0.5, cost, 0.11), obs(0.52, cost, 0.11)];
        let ds = assemble_many(im, m, &pair, &mut Work::default(), true);
        assert_eq!(ds.len(), usize::from(strong));
    }
    let pair = [obs(0.5, 0.11, 0.11), obs(0.54, 0.11, 0.11)];
    assert_eq!(
        assemble_many(im, m, &pair, &mut Work::default(), true).len(),
        1
    );
}
