mod support;
use tapirscan::Image;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 9 || args.len() == 10,
        "mode width height channels stride pixels multiple include_regions [format_mask]"
    );
    let formats = args.get(9).map_or(1, |mask| mask.parse().unwrap());
    let data = std::fs::read(&args[6]).unwrap();
    let width = args[2].parse().unwrap();
    let height = args[3].parse().unwrap();
    let channels = args[4].parse().unwrap();
    let image = match channels {
        1 => Image::gray(&data, width, height),
        3 => Image::rgb(&data, width, height),
        4 => Image::rgba(&data, width, height),
        _ => panic!("unsupported channel count"),
    }
    .with_stride(args[5].parse().unwrap());
    let mut scanner = support::scanner(support::mode(&args[1]));
    let result = support::scan_legacy(&mut scanner, image, formats, args[7] == "1", args[8] == "1");
    println!("{result}");
}
