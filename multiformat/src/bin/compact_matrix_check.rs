//! Module-level conformance check; not an image-scanner benchmark.
use barcode_multiformat::{micro_pdf417, rmqr};
use serde::Deserialize;
use std::{
    error::Error,
    io::{self, Read},
};
#[derive(Deserialize)]
struct Case {
    id: String,
    format: String,
    text: String,
    width: usize,
    height: usize,
    rows: Vec<String>,
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let cases: Vec<Case> = serde_json::from_str(&input)?;
    let mut failures = Vec::new();
    let mut damaged_checks = 0;
    let mut negative_checks = 0;
    let mut seed = 0x5a17_9c03_u32;
    for case in &cases {
        let matrix: Vec<_> = case
            .rows
            .iter()
            .flat_map(|r| r.bytes().map(|b| b == b'1'))
            .collect();
        let decode: fn(&[bool], usize, usize) -> Option<barcode_multiformat::qr::Payload> =
            match case.format.as_str() {
                "MicroPDF417" => micro_pdf417::decode_matrix,
                "RMQRCode" => rmqr::decode_matrix,
                _ => return Err("unsupported conformance format".into()),
            };
        for damaged in [false, true] {
            let mut modules = matrix.clone();
            if damaged {
                let index = if case.format == "MicroPDF417" {
                    10
                } else {
                    (case.height - 2) * case.width + case.width - 9
                };
                modules[index] ^= true;
                damaged_checks += 1;
            }
            let result = decode(&modules, case.width, case.height);
            if result.as_ref().map(|p| p.text.as_str()) != Some(case.text.as_str()) {
                failures.push(serde_json::json!({"id":case.id,"damaged":damaged,"expected":case.text,"actual":result.map(|p|p.text)}));
            }
        }
        for _ in 0..16 {
            let random: Vec<_> = (0..matrix.len())
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed & 1 != 0
                })
                .collect();
            negative_checks += 1;
            if decode(&random, case.width, case.height).is_some() {
                failures.push(serde_json::json!({"id":case.id,"failure":"random matrix accepted"}));
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"cases":cases.len(),"single_data_module_damage_checks":damaged_checks,"random_negative_checks":negative_checks,"failures":failures})
    );
    if !failures.is_empty() {
        return Err("conformance failures".into());
    }
    Ok(())
}
