//! Whole-frame retry events using localizer-supplied quadrilaterals; no labels.
use barcode_research_core::{experiment::Experiment, multi_scan::Policy, sampling::ImageView};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(a.len(), 5, "RGB WIDTH HEIGHT QUADS_TEXT");
    let data = std::fs::read(&a[1]).unwrap();
    let w: usize = a[2].parse().unwrap();
    let h: usize = a[3].parse().unwrap();
    let im = ImageView::new(&data, w, h, 3, w * 3).unwrap();
    let values: Vec<f64> = std::fs::read_to_string(&a[4])
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(values.len() % 8, 0);
    assert!(values.len() / 8 <= 64);
    let qs: Vec<_> = values
        .chunks_exact(8)
        .map(|v| std::array::from_fn(|i| [v[i * 2], v[i * 2 + 1]]))
        .collect();
    let policy = Policy {
        transition_cleanup: true,
        source_identity: true,
        interior_normalization: true,
        guard_bias: true,
        ..Policy::default()
    };
    println!(
        "{:?}",
        Experiment::default().scan_frame(im, &qs, policy).unwrap()
    );
}
