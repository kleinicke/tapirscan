//! Native row/localization diagnostic using only our reader.
use barcode_multiformat::{pdf417, pdf_localize};
use std::{env, fs};
fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 5, "pdf_probe GRAY WIDTH HEIGHT OUT_PREFIX");
    let gray = fs::read(&args[1]).unwrap();
    let w: usize = args[2].parse().unwrap();
    let h: usize = args[3].parse().unwrap();
    assert_eq!(gray.len(), w * h);
    let (proposals, limited) = pdf_localize::proposals(&gray, w, h);
    let mut output = Vec::new();
    for (i, (quad, support)) in proposals.into_iter().enumerate() {
        if let Some((pixels, width, height, _)) = pdf_localize::rectify(&gray, w, h, quad) {
            let path = format!("{}-{i}.gray", args[4]);
            fs::write(&path, &pixels).unwrap();
            output.push(serde_json::json!({"path":path,"width":width,"height":height,
                "quad":quad,"support":support,"rows":pdf417::diagnostic_rows(&pixels,width,height)}));
        }
    }
    println!(
        "{}",
        serde_json::json!({"limited":limited,"proposals":output})
    );
}
