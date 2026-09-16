use tapirscan::{Formats, Image, ScanOptions, Scanner};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 9 || args.len() == 10,
        "mode width height channels stride pixels multiple include_regions [format_mask]"
    );
    let formats = Formats::try_from(args.get(9).map_or(1, |mask| mask.parse().unwrap())).unwrap();
    let data = std::fs::read(&args[6]).unwrap();
    let image = Image {
        data: &data,
        width: args[2].parse().unwrap(),
        height: args[3].parse().unwrap(),
        channels: args[4].parse().unwrap(),
        stride: args[5].parse().unwrap(),
    };
    let result = Scanner::default()
        .scan_formats(
            image,
            ScanOptions {
                finish_candidates: false,
                multiple: args[7] == "1",
                include_regions: args[8] == "1",
            },
            formats,
        )
        .unwrap();
    assert_eq!(
        result.image_size(),
        [
            args[2].parse::<usize>().unwrap(),
            args[3].parse::<usize>().unwrap()
        ]
    );
    assert_eq!(result.debug().is_some(), args[8] == "1");
    let raw = result.json()["scan"]["barcodes"].as_array().unwrap();
    assert_eq!(
        result.values().collect::<Vec<_>>(),
        raw.iter()
            .map(|b| b["text"].as_str().unwrap())
            .collect::<Vec<_>>()
    );
    for (typed, raw) in result.barcodes().iter().zip(raw) {
        assert_eq!(
            typed.polygon,
            serde_json::from_value::<[[f64; 2]; 4]>(raw["polygon"].clone()).unwrap()
        );
    }
    if let Some(best) = result.best() {
        assert_eq!(
            best.text,
            raw.iter()
                .enumerate()
                .max_by_key(|(i, b)| (b["support"].as_u64().unwrap(), std::cmp::Reverse(*i)))
                .unwrap()
                .1["text"]
        );
    } else {
        assert!(raw.is_empty());
    }
    println!("{}", result.json());
}
