//! Decoder-only research adapter: pixels and supplied regions, never payload labels.
use barcode_research_core::ean;
#[allow(dead_code)]
#[path = "../src/sampling.rs"]
mod sampling;
use barcode_research_core::numeric;
#[path = "../src/lowres/photo.rs"]
mod photo;
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let effort: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1".into())
        .parse()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let f: Vec<_> = line.split_whitespace().collect();
        if f.len() != 11 {
            return Err("expected path width height quad".into());
        }
        let bytes = std::fs::read(f[0])?;
        let width = f[1].parse()?;
        let height = f[2].parse()?;
        let mut q = [[0.; 2]; 4];
        for (i, p) in q.iter_mut().flatten().enumerate() {
            *p = f[i + 3].parse()?;
        }
        let im = sampling::ImageView::new(&bytes, width, height, 1, width)?;
        let start = std::time::Instant::now();
        let quads = if std::env::args().nth(2).as_deref() == Some("regions") {
            photo::regions(im, q, true)
        } else {
            vec![q]
        };
        let mut rows = Vec::new();
        for q in quads {
            let r = photo::scan(im, q, effort == 6);
            let hypotheses:Vec<_>=r.hypotheses.iter().map(|h|serde_json::json!({"text":h.result.digits.iter().map(|d|char::from(b'0'+d)).collect::<String>(),"checksum":ean::checksum(&h.result.digits),"score":h.score,"cost":h.result.cost,"guard":h.result.guard,"gap":h.result.gap,"left":h.left,"right":h.right,"shear":h.shear,"bend":h.bend,"sharpen":h.sharpen,"reverse":h.reverse,"local":h.local,"quiet":h.quiet})).collect();
            rows.push(serde_json::json!({"quad":q,"hypotheses":hypotheses,"split_matches":r.split_matches,"split_cost":r.split_cost,"split_gap":r.split_gap,"digit_support_min":r.digit_support_min,"contradictions":r.contradictions,"accepted":photo::accepted(&r),"trials":r.trials,"pixels":r.pixels}));
        }
        println!(
            "{}",
            serde_json::json!({"regions":rows,"ms":start.elapsed().as_secs_f64()*1000.})
        );
    }
    Ok(())
}
