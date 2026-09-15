//! Inspect actual finder proposals; no reference decoder or ground-truth input.
use barcode_multiformat::{
    aztec_detect, binarization::Images, dm_detect, maxicode_detect, qr_detect,
};
use std::{env, fs};
fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 4, "finders GRAY WIDTH HEIGHT");
    let gray = fs::read(&args[1]).unwrap();
    let width: usize = args[2].parse().unwrap();
    let height: usize = args[3].parse().unwrap();
    assert_eq!(gray.len(), width * height);
    let mut images = Images::new(&gray, width, height);
    let modes: Vec<_> = (0..4)
        .map(|mode| {
            let bits = images.get(mode);
            serde_json::json!({
                "mode":mode,
                "qr":qr_detect::diagnostic_finders(bits,width,height),
                "dm":dm_detect::diagnostic_candidates(bits,width,height),
                "aztec":aztec_detect::diagnostic_centers(bits,width,height),
                "maxicode":maxicode_detect::diagnostic_centers(bits,width,height)
            })
        })
        .collect();
    println!("{}", serde_json::json!(modes));
}
