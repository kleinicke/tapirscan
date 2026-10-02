use super::*;
#[test]
fn exact_bounds_match_every_legacy_selected_index() {
    for n in [0, 64, 95, 512, 1905, 4096] {
        for (lo, hi) in [
            (-0.15, 1.15),
            (-1., 2.),
            (-2., -1.),
            (2., 3.),
            (0., 1.),
            (-0.000_000_01, 1.000_000_01),
            (-0.5, 0.5),
            (0.5, 1.5),
        ] {
            let selected: Vec<_> = (0..n)
                .filter(|&i| {
                    let u = lo
                        + (hi - lo) * (crate::numeric::usize_f64(i) + 0.5)
                            / crate::numeric::usize_f64(n);
                    (0.0..=1.0).contains(&u)
                })
                .collect();
            let (a, b) = interior_bounds(n, lo, hi);
            assert_eq!((a..b).collect::<Vec<_>>(), selected);
        }
    }
}
