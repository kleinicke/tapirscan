//! Supplied source rows through the real retry decoder, for diagnostics only.
use barcode_research_core::{experiment::Experiment, sampling::ImageView};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(a.len(), 13, "RGB WIDTH HEIGHT QUAD8 FRACTIONS");
    let data = std::fs::read(&a[1]).unwrap();
    let w: usize = a[2].parse().unwrap();
    let h: usize = a[3].parse().unwrap();
    let im = ImageView::new(&data, w, h, 3, w * 3).unwrap();
    let q = std::array::from_fn(|i| [a[4 + i * 2].parse().unwrap(), a[5 + i * 2].parse().unwrap()]);
    let fractions: Vec<f64> = a[12].split(',').map(|s| s.parse().unwrap()).collect();
    eprintln!(
        "{{\"diagnostic\":\"retry\",\"trace\":{},\"aperture\":{},\"green\":{},\"phase\":{}}}",
        cfg!(feature = "diagnostic-retry-trace"),
        cfg!(feature = "experimental-profile-aperture"),
        cfg!(feature = "experimental-green-luminance"),
        cfg!(feature = "experimental-identity-phase")
    );
    println!(
        "{:?}",
        Experiment::default()
            .diagnostic_retry_rows(im, q, &fractions)
            .unwrap()
    );
}
