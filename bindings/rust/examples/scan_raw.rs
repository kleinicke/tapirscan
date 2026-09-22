mod support;
use tapirscan::Image;
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
    let mut scanner = support::scanner(support::compiled_mode());
    let image = match channels {
        1 => Image::gray(&data, width, height),
        3 => Image::rgb(&data, width, height),
        4 => Image::rgba(&data, width, height),
        _ => panic!("unsupported channel count"),
    }
    .with_stride(stride);
    let result = support::scan_legacy(&mut scanner, image, 1, true, true);
    println!("{}", result["scan"]);
}
