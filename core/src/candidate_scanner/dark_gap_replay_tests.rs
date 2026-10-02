use super::*;
// Fixed grayscale samples from an ordinary continuous barcode under glare.
// Endpoint A, endpoint B, interior row; no payload enters this fixture/runtime.
const DENSE: [[u8; 192]; 3] = [
    [
        31, 95, 129, 146, 16, 70, 127, 134, 135, 143, 141, 140, 57, 28, 23, 38, 136, 125, 143, 32,
        170, 124, 27, 30, 123, 127, 128, 127, 31, 17, 16, 17, 16, 54, 129, 131, 130, 127, 125, 124,
        36, 16, 18, 40, 130, 137, 62, 20, 119, 130, 128, 133, 80, 14, 121, 128, 127, 118, 60, 26,
        17, 15, 110, 124, 127, 124, 34, 13, 61, 134, 21, 12, 14, 13, 16, 11, 123, 119, 70, 11, 109,
        121, 124, 124, 60, 18, 16, 14, 13, 23, 91, 111, 61, 10, 100, 116, 69, 4, 84, 122, 76, 18,
        13, 14, 14, 16, 69, 118, 124, 123, 114, 5, 75, 121, 106, 14, 14, 13, 13, 15, 49, 115, 119,
        118, 102, 11, 28, 124, 51, 26, 64, 112, 121, 118, 129, 4, 38, 112, 121, 120, 120, 121, 123,
        30, 15, 11, 29, 96, 117, 119, 125, 26, 13, 11, 15, 92, 116, 45, 13, 11, 12, 13, 19, 81,
        108, 44, 9, 87, 123, 119, 119, 67, 9, 65, 113, 116, 124, 79, 22, 17, 15, 15, 17, 50, 116,
        132, 6, 27, 131, 128, 3, 7,
    ],
    [
        80, 124, 113, 37, 60, 116, 135, 152, 151, 140, 130, 144, 31, 33, 9, 123, 139, 56, 27, 120,
        141, 47, 22, 123, 129, 131, 131, 23, 19, 20, 18, 20, 21, 100, 127, 154, 136, 130, 124, 59,
        19, 20, 9, 121, 120, 74, 25, 116, 125, 126, 133, 88, 11, 110, 122, 124, 123, 101, 19, 14,
        10, 82, 121, 124, 131, 58, 8, 94, 122, 97, 21, 14, 14, 17, 13, 52, 123, 92, 5, 60, 119,
        123, 120, 88, 18, 16, 14, 12, 13, 37, 135, 120, 6, 10, 120, 120, 18, 35, 120, 128, 31, 12,
        13, 12, 14, 22, 120, 121, 119, 115, 16, 10, 124, 128, 19, 14, 12, 11, 14, 18, 87, 115, 118,
        115, 53, 12, 79, 118, 67, 7, 93, 117, 113, 115, 70, 17, 84, 120, 116, 119, 117, 115, 100,
        13, 11, 11, 26, 114, 115, 118, 99, 27, 11, 11, 5, 122, 133, 14, 14, 10, 10, 11, 15, 107,
        133, 24, 6, 95, 114, 115, 119, 16, 13, 91, 116, 112, 114, 57, 15, 12, 10, 12, 13, 56, 124,
        104, 6, 18, 108, 121, 21, 7,
    ],
    [
        36, 94, 124, 76, 23, 83, 124, 130, 195, 156, 134, 124, 33, 37, 21, 100, 144, 169, 16, 99,
        139, 110, 33, 81, 123, 130, 134, 107, 26, 17, 17, 15, 15, 66, 126, 130, 133, 131, 120, 101,
        29, 16, 13, 88, 125, 121, 11, 38, 127, 127, 130, 124, 29, 36, 127, 125, 126, 131, 37, 17,
        18, 31, 120, 128, 124, 122, 27, 33, 123, 132, 26, 16, 15, 15, 13, 20, 127, 130, 8, 9, 128,
        123, 123, 124, 52, 15, 14, 13, 12, 17, 105, 111, 31, 7, 102, 130, 55, 7, 122, 132, 75, 14,
        12, 11, 13, 23, 83, 122, 120, 121, 82, 10, 96, 116, 74, 12, 10, 11, 11, 16, 71, 120, 119,
        114, 82, 10, 42, 129, 108, 4, 88, 118, 121, 116, 112, 4, 70, 129, 123, 119, 116, 116, 112,
        18, 13, 14, 14, 127, 122, 119, 113, 15, 13, 11, 23, 86, 121, 29, 11, 11, 11, 11, 12, 98,
        125, 47, 3, 98, 114, 115, 116, 76, 3, 69, 113, 115, 111, 83, 16, 12, 15, 16, 14, 33, 124,
        100, 3, 8, 120, 120, 6, 11,
    ],
];
const SPARSE: [[u8; 32]; 3] = [
    [
        132, 141, 24, 124, 122, 22, 124, 125, 129, 124, 127, 124, 20, 126, 15, 19, 128, 14, 51, 14,
        120, 18, 27, 121, 46, 12, 9, 126, 139, 126, 8, 111,
    ],
    [
        126, 155, 106, 95, 77, 74, 111, 116, 121, 128, 122, 127, 27, 121, 13, 7, 111, 15, 5, 12,
        115, 18, 11, 116, 70, 14, 11, 99, 133, 130, 9, 111,
    ],
    [
        117, 199, 24, 130, 130, 26, 127, 128, 136, 122, 125, 120, 17, 116, 13, 10, 112, 15, 16, 11,
        118, 5, 30, 121, 70, 9, 10, 114, 127, 121, 22, 116,
    ],
];
fn counts(v: &[u8]) -> (usize, usize) {
    let lo = f64::from(*v.iter().min().unwrap());
    let hi = f64::from(*v.iter().max().unwrap());
    (
        v.iter()
            .filter(|&&x| f64::from(x) < lo + 0.35 * (hi - lo))
            .count(),
        v.iter()
            .filter(|&&x| f64::from(x) > lo + 0.65 * (hi - lo))
            .count(),
    )
}
#[test]
fn specular_light_density_loss_preserves_dark_bar_continuity() {
    for profiles in [
        SPARSE.iter().map(<[u8; 32]>::as_slice).collect::<Vec<_>>(),
        DENSE.iter().map(<[u8; 192]>::as_slice).collect::<Vec<_>>(),
    ] {
        let a = counts(profiles[0]);
        let b = counts(profiles[1]);
        let mid = counts(profiles[2]);
        assert!(
            mid.1 * 2 < a.1.min(b.1),
            "reproduces previous light-density rejection"
        );
        assert!(mid.0 * 2 >= a.0.min(b.0), "black occupancy remains intact");
    }
    let mut pixels = vec![255u8; 384 * 3];
    // The384px fixture places dense samples on odd x and sparse samples on
    // distinct even x. Preserve exactly both recorded sampling phases.
    for (y, which) in [(0, 0), (1, 2), (2, 1)] {
        for j in 0..192 {
            pixels[y * 384 + 2 * j] = DENSE[which][j];
            pixels[y * 384 + 2 * j + 1] = DENSE[which][j];
        }
        for j in 0..32 {
            pixels[y * 384 + 12 * j + 6] = SPARSE[which][j];
        }
    }
    let quad = [[0., 0.], [384., 0.], [384., 4.], [0., 4.]];
    let m = scan::transform(quad).unwrap().0;
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let a = Observation {
        short_quiet: false,
        ambiguous: false,
        digits: [0; 13],
        axis: 0,
        fraction: 0.125,
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
        fraction: 0.125,
        left: 0.,
        right: 1.,
        cost: 0.,
        gap: 1.,
    };

    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let b = Observation {
        fraction: 0.625,
        ..a
    };
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let b = Observation {
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        invalid_checksum: false,
        fraction: 0.625,
        ..a
    };

    let mut width = Work::default();
    let mut budget = AssociationBudget::default();
    assert!(connected_density(
        ImageView::new(&pixels, 384, 3, 1, 384).unwrap(),
        m,
        a,
        b,
        &mut width,
        Some(&mut budget),
        &mut std::collections::HashMap::new()
    ));
    assert_eq!(width.continuity_rejects, 0);
    // A real white separating source row still rejects the connection.
    pixels[384..768].fill(255);
    let mut width = Work::default();
    assert!(!connected_density(
        ImageView::new(&pixels, 384, 3, 1, 384).unwrap(),
        m,
        a,
        b,
        &mut width,
        None,
        &mut std::collections::HashMap::new()
    ));
    assert!(width.continuity_rejects > 0);
}
