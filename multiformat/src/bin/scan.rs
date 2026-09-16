use std::{env, fs, time::Instant};
fn main() {
    let a: Vec<String> = env::args().collect();
    assert!(a.len() >= 5, "scan GRAY W H MASK [EFFORT] [--profile]");
    let image = fs::read(&a[1]).unwrap();
    let width = a[2].parse().unwrap();
    let height = a[3].parse().unwrap();
    let mask = a[4].parse().unwrap();
    let effort = a.get(5).map_or(1, |s| s.parse().unwrap());
    let t = Instant::now();
    let mut stages = Vec::new();
    let scan = if a.get(6).is_some_and(|v| v == "--profile") {
        let mut previous = t;
        barcode_multiformat::scan_observed(&image, width, height, mask, effort, |name, limited| {
            let now = Instant::now();
            stages.push(serde_json::json!({"stage":name,"ms":(now-previous).as_secs_f64()*1000.,"cumulativeLimited":limited}));
            previous = Instant::now();
        })
    } else {
        barcode_multiformat::scan(&image, width, height, mask, effort)
    };
    let mut result = serde_json::to_value(scan).unwrap();
    result["scanMs"] = serde_json::json!(t.elapsed().as_secs_f64() * 1000.);
    if !stages.is_empty() {
        result["stageProfile"] = serde_json::json!(stages);
    }
    println!("{result}");
}
