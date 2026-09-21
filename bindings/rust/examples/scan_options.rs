use tapirscan::{Image, ScanOptions, Scanner};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 9 || args.len() == 10,
        "mode width height channels stride pixels multiple include_regions [format_mask]"
    );
    let formats = args.get(9).map_or(1, |mask| mask.parse().unwrap());
    let data = std::fs::read(&args[6]).unwrap();
    let image = Image {
        data: &data,
        width: args[2].parse().unwrap(),
        height: args[3].parse().unwrap(),
        channels: args[4].parse().unwrap(),
        stride: args[5].parse().unwrap(),
    };
    let result = Scanner::default()
        .scan_formats_json(
            image,
            ScanOptions {
                finish_candidates: false,
                multiple: args[7] == "1",
                include_regions: args[8] == "1",
            },
            formats,
        )
        .unwrap();
    assert_eq!(result["localization"].is_object(), args[8] == "1");
    println!("{result}");
}
