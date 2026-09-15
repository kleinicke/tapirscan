//! Diagnostic only: sample a supplied reference quadrilateral. Never called by
//! the production scanner. Successful oracle reads diagnose localization loss.
use barcode_multiformat::{aztec, binarization::Images, datamatrix, qr, qr_detect};
use std::{env, fs};
fn rectified_oracle(
    gray: &[u8],
    width: usize,
    height: usize,
    quad: [[f32; 2]; 4],
    mask: u32,
) -> serde_json::Value {
    let mut attempts = 0;
    for rotation in 0..4 {
        let q = std::array::from_fn(|i| quad[(rotation + i) % 4]);
        let Some(t) = qr_detect::homography([[0., 0.], [1., 0.], [1., 1.], [0., 1.]], q) else {
            continue;
        };
        let w = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
        let h = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
        for margin in [0_f32, 4., 12.] {
            let dx = margin / w;
            let dy = margin / h;
            let expanded = [
                [-dx, -dy],
                [1. + dx, -dy],
                [1. + dx, 1. + dy],
                [-dx, 1. + dy],
            ]
            .map(|[x, y]| qr_detect::map(&t, x, y));
            let Some((pixels, cw, ch, _)) =
                barcode_multiformat::pdf_localize::rectify(gray, width, height, expanded)
            else {
                continue;
            };
            attempts += 1;
            let result = barcode_multiformat::scan(&pixels, cw, ch, mask, 1);
            if let Some(read) = result.barcodes.first() {
                return serde_json::json!({
                    "text": read.text, "format": read.format,
                    "readerInitialization": read.reader_initialization,
                    "structuredAppend": read.structured_append,
                    "rotation": rotation, "marginPixels": margin, "attempts": attempts,
                    "stage": "own scan of supplied rectified region"
                });
            }
        }
    }
    serde_json::json!({"failed":true,"attempts":attempts})
}
fn main() {
    let args: Vec<_> = env::args().collect();
    assert!(args.len() == 6, "oracle GRAY W H FORMAT QUAD_JSON");
    let gray = fs::read(&args[1]).unwrap();
    let width: usize = args[2].parse().unwrap();
    let height: usize = args[3].parse().unwrap();
    assert_eq!(gray.len(), width * height);
    let quad: [[f32; 2]; 4] = serde_json::from_str(&args[5]).unwrap();
    let linear_mask = match args[4].as_str() {
        "EAN13" => 1,
        "UPCA" => 2,
        "EAN8" => 4,
        "UPCE" => 8,
        "Code128" => 16,
        "Code39" => 32,
        "ITF" => 64,
        "Codabar" => 128,
        "Code93" => 256,
        "PDF417" => 2048,
        "DataBar" => 8192,
        "DataBarExpanded" => 16384,
        "MaxiCode" => 131_072,
        _ => 0,
    };
    if linear_mask != 0 {
        println!(
            "{}",
            rectified_oracle(&gray, width, height, quad, linear_mask)
        );
        return;
    }
    let dimensions: Vec<(usize, usize)> = match args[4].as_str() {
        "DataMatrix" => datamatrix::SIZES.iter().map(|s| (s.w, s.h)).collect(),
        "QRCode" => (1..=40).map(|v| (17 + 4 * v, 17 + 4 * v)).collect(),
        "Aztec" => {
            let mut values = vec![(11, 11)];
            values.extend((1..=4).map(|l| (11 + 4 * l, 11 + 4 * l)));
            values.extend((1..=32).map(|l| {
                let b = 14 + 4 * l;
                let n = b + 1 + 2 * ((b / 2 - 1) / 15);
                (n, n)
            }));
            values
        }
        _ => panic!("Unsupported oracle format"),
    };
    let mut images = Images::new(&gray, width, height);
    let mut attempts = 0;
    for mode in 0..4 {
        let bits = images.get(mode);
        for mirror in [false, true] {
            for rotation in 0..4 {
                let q =
                    std::array::from_fn(|i| quad[(rotation + if mirror { 4 - i } else { i }) % 4]);
                let Some(t) = qr_detect::homography([[0., 0.], [1., 0.], [1., 1.], [0., 1.]], q)
                else {
                    continue;
                };
                for &(cols, rows) in &dimensions {
                    for margin in [0_f32, 0.25, -0.25, 0.5, -0.5] {
                        let mut matrix = Vec::with_capacity(cols * rows);
                        for y in 0..rows {
                            for x in 0..cols {
                                let u = (x as f32 + 0.5 - margin) / (cols as f32 - 2. * margin);
                                let v = (y as f32 + 0.5 - margin) / (rows as f32 - 2. * margin);
                                let [xx, yy] = qr_detect::map(&t, u, v);
                                if xx >= 0.
                                    && yy >= 0.
                                    && xx < (width as f32)
                                    && yy < (height as f32)
                                {
                                    matrix.push(
                                        bits[yy.floor() as usize * width + xx.floor() as usize],
                                    );
                                }
                            }
                        }
                        if matrix.len() != cols * rows {
                            continue;
                        }
                        if args[4] == "DataMatrix" {
                            let errors = (0..cols)
                                .map(|x| {
                                    usize::from(matrix[x] != (x % 2 == 0))
                                        + usize::from(!matrix[(rows - 1) * cols + x])
                                })
                                .sum::<usize>()
                                + (0..rows)
                                    .map(|y| {
                                        usize::from(!matrix[y * cols])
                                            + usize::from(
                                                matrix[y * cols + cols - 1] != (y % 2 == 1),
                                            )
                                    })
                                    .sum::<usize>();
                            if errors * 5 > 2 * (cols + rows) {
                                continue;
                            }
                        }
                        attempts += 1;
                        let decoded = match args[4].as_str() {
                            "Aztec" => aztec::decode_matrix(&matrix, cols),
                            "DataMatrix" => datamatrix::decode_matrix(&matrix, cols, rows),
                            _ => qr::decode_matrix(&matrix, cols),
                        };
                        if let Some(read) = decoded {
                            println!(
                                "{}",
                                serde_json::json!({"text":read.text,"format":args[4],"width":cols,"height":rows,"mode":mode,"marginModules":margin,"rotation":rotation,"mirror":mirror,"corrected":read.corrected,"readerInitialization":read.reader_initialization,"structuredAppend":read.structured_append,"attempts":attempts})
                            );
                            return;
                        }
                    }
                }
            }
        }
    }
    println!("{}", serde_json::json!({"failed":true,"attempts":attempts}));
}
