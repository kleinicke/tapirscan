//! Factor signal normalization from edge geometry; no expected labels or fitting.
use barcode_research_core::{local_signal, multi_profile};
fn report(name: &str, r: multi_profile::Reads) {
    println!("{{\"model\":\"{}\",\"windows\":{},\"guardPass\":{},\"decoderCalls\":{},\"ambiguous\":{},\"truncated\":{},\"symbols\":{}}}",name,r.windows_examined,r.guard_pass,r.decoder_calls,r.ambiguous_intervals,r.truncated,r.symbols.len());
    for s in r.symbols {
        println!(
            "{{\"model\":\"{}\",\"digits\":{:?},\"left\":{},\"right\":{},\"cost\":{},\"gap\":{}}}",
            name, s.digits, s.left, s.right, s.cost, s.gap
        );
    }
    for (a, b) in r.rejected_intervals {
        println!("{{\"model\":\"{name}\",\"rejectedLeft\":{a},\"rejectedRight\":{b}}}");
    }
}
fn main() {
    let path = std::env::args().nth(1).expect("SIGNALS_FILE");
    let input = std::fs::read_to_string(path).unwrap();
    for (i, line) in input.lines().enumerate() {
        let p: Vec<f32> = line
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let local = local_signal::normalize(&p).unwrap();
        println!("{{\"signal\":{},\"samples\":{},\"originalTransitions\":{},\"localTransitions\":{},\"local\":{:?}}}",i,p.len(),p.windows(2).filter(|a|(a[0]>=0.5)!=(a[1]>=0.5)).count(),local.windows(2).filter(|a|(a[0]>=0.5)!=(a[1]>=0.5)).count(),local);
        for (name, s) in [("original", &p), ("local", &local)] {
            report(
                &format!("{name}_integer"),
                multi_profile::decode_many(s, 64).unwrap(),
            );
            report(
                &format!("{name}_fractional"),
                multi_profile::decode_fractional(s, 64).unwrap(),
            );
            report(
                &format!("{name}_guard"),
                multi_profile::decode_guard_bias(s, 64).unwrap(),
            );
            report(
                &format!("{name}_fractional_guard"),
                multi_profile::decode_fractional_guard_bias(s, 64).unwrap(),
            );
        }
        report(
            "local_merged",
            local_signal::decode_guard_bias(&p, 64).unwrap(),
        );
    }
}
