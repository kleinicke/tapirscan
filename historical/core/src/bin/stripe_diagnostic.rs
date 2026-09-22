use barcode_research_core::{sampling::ImageView, stripes};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(a.len(), 4, "stripe_diagnostic RGB WIDTH HEIGHT");
    let data = std::fs::read(&a[1]).unwrap();
    let w: usize = a[2].parse().unwrap();
    let h: usize = a[3].parse().unwrap();
    let im = ImageView::new(&data, w, h, 3, w * 3).unwrap();
    let r=stripes::detect_with_observer(im,|g|println!("{{\"index\":{},\"tiles\":{},\"edges\":{},\"angle\":{},\"initial_angle\":{},\"bounds\":{:?},\"reason\":\"{}\"}}",g.index,g.tiles,g.edges,g.angle,g.initial_angle,g.bounds,g.reason)).unwrap();
    eprintln!(
        "proposals={} omitted={} limited={} trace={:?}",
        r.proposals.len(),
        r.omitted,
        r.limited,
        r.trace
    );
}
