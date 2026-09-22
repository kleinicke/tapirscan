//! Emit all visual parity rankings for supplied run widths, without repair.
use barcode_research_core::run_ean;
fn main() {
    let path = std::env::args().nth(1).expect("WIDTHS_FILE");
    let text = std::fs::read_to_string(path).unwrap();
    for line in text.lines() {
        let w: Vec<f32> = line
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        let evidence = run_ean::decode_evidence(&w);
        println!(
            "{{\"accepted\":{}}}",
            evidence
                .map(|v| v.digits)
                .map_or("null".into(), |v| format!("{v:?}"))
        );
        for r in run_ean::diagnostic_parities(&w).unwrap() {
            println!("{{\"digits\":{:?},\"cost\":{},\"maxCost\":{},\"minGap\":{},\"digitCosts\":{:?},\"checksum\":{}}}",r.digits,r.cost,r.maximum_digit_cost,r.minimum_digit_gap,r.digit_costs,r.checksum_valid);
        }
    }
}
