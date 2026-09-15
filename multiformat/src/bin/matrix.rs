use std::{env, fs};
fn main() {
    let a: Vec<_> = env::args().collect();
    let n: usize = a[2].parse().unwrap();
    let bytes = fs::read(&a[1]).unwrap();
    let matrix: Vec<bool> = bytes.iter().map(|&b| b != 0).collect();
    if a.get(3).is_some_and(|s| s == "maxicode-debug") {
        println!(
            "{}",
            barcode_multiformat::maxicode::diagnostic_words(&matrix)
        );
        return;
    }
    let decoded = if a.get(3).is_some_and(|s| s == "aztec") {
        barcode_multiformat::aztec::decode_matrix(&matrix, n)
    } else if a.get(3).is_some_and(|s| s == "dm") {
        barcode_multiformat::datamatrix::decode_matrix(&matrix, n, matrix.len() / n)
    } else if a.get(3).is_some_and(|s| s == "maxicode") {
        barcode_multiformat::maxicode::decode_matrix(&matrix)
    } else {
        barcode_multiformat::qr::decode_matrix(&matrix, n)
    };
    match decoded {
        Some(r) => println!(
            "{}",
            serde_json::json!({"text":r.text,"bytes":r.bytes,"corrected":r.corrected,"gs1":r.gs1,"readerInitialization":r.reader_initialization,"structuredAppend":r.structured_append})
        ),
        None => println!("null"),
    }
}
