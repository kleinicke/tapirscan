use tapirscan::{Format, Image, ScanOptions, Scanner};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 4 || args.len() == 6,
        "usage: scan_raw width height pixels [channels stride]"
    );
    let width: usize = args[1].parse().unwrap();
    let height: usize = args[2].parse().unwrap();
    let data = std::fs::read(&args[3]).unwrap();
    let channels = if args.len() == 6 {
        args[4].parse().unwrap()
    } else {
        1
    };
    let stride = if args.len() == 6 {
        args[5].parse().unwrap()
    } else {
        width
    };
    let result = Scanner::default()
        .scan_formats(
            Image {
                data: &data,
                width,
                height,
                channels,
                stride,
            },
            ScanOptions {
                finish_candidates: false,
                multiple: true,
                include_regions: true,
            },
            Format::Ean13,
        )
        .unwrap();
    println!("{}", result.json()["scan"]);
}
