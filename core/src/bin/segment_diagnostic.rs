//! One source segment, separately normalized; no expected values.
use barcode_research_core::{experiment::Experiment, sampling::ImageView};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(
        a.len(),
        18,
        "RGB WIDTH HEIGHT QUAD8 AXIS FRACTION LO HI SAMPLES INTERIOR"
    );
    let data = std::fs::read(&a[1]).unwrap();
    let w: usize = a[2].parse().unwrap();
    let h: usize = a[3].parse().unwrap();
    let im = ImageView::new(&data, w, h, 3, w * 3).unwrap();
    let q = std::array::from_fn(|i| [a[4 + i * 2].parse().unwrap(), a[5 + i * 2].parse().unwrap()]);
    let reads = Experiment::default()
        .diagnostic_segment_reads(
            im,
            q,
            a[12].parse().unwrap(),
            a[13].parse().unwrap(),
            a[14].parse().unwrap(),
            a[15].parse().unwrap(),
            a[16].parse().unwrap(),
            a[17] == "1",
        )
        .unwrap();
    for r in reads {
        println!("{{\"digits\":{:?},\"ambiguous\":{},\"fraction\":{},\"left\":{},\"right\":{},\"shortQuiet\":{}}}",r.digits,r.ambiguous,r.fraction,r.left,r.right,r.short_quiet);
    }
}
