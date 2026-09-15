//! Native supplied-region example: `scan_pgm` image.pgm x0 y0 ... x3 y3 [more quads].
//! Input is binary P5 gray8 PGM. No detector, learned runtime, or `ZXing` is used.
use barcode_research_core::{sampling::ImageView, scan::Scanner};
fn token<'a>(b: &'a [u8], i: &mut usize) -> Result<&'a str, Box<dyn std::error::Error>> {
    loop {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
        if b.get(*i) == Some(&b'#') {
            while *i < b.len() && b[*i] != b'\n' {
                *i += 1;
            }
        } else {
            break;
        }
    }
    let start = *i;
    while *i < b.len() && !b[*i].is_ascii_whitespace() {
        *i += 1;
    }
    if start == *i {
        return Err("Missing PGM header token".into());
    }
    Ok(std::str::from_utf8(&b[start..*i])?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let bands = args.first().is_some_and(|a| a == "--bands");
    if bands {
        args.remove(0);
    }
    if args.is_empty() || (args.len() - 1) % 8 != 0 {
        return Err("Usage: scan_pgm [--bands] image.pgm [x0 y0 x1 y1 x2 y2 x3 y3 ...]".into());
    }
    if std::fs::metadata(&args[0])?.len() > 128 * 1024 * 1024 + 4096 {
        return Err("PGM file too large".into());
    }
    let bytes = std::fs::read(&args[0])?;
    let mut i = 0;
    if token(&bytes, &mut i)? != "P5" {
        return Err("Expected P5".into());
    }
    let w: usize = token(&bytes, &mut i)?.parse()?;
    let h: usize = token(&bytes, &mut i)?.parse()?;
    if token(&bytes, &mut i)? != "255" {
        return Err("Expected gray8 maxval255".into());
    }
    if !bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        return Err("Missing pixel delimiter".into());
    }
    if bytes.get(i) == Some(&b'\r') && bytes.get(i + 1) == Some(&b'\n') {
        i += 2;
    } else {
        i += 1;
    }
    let image = ImageView::new(&bytes[i..], w, h, 1, w)?;
    let mut candidates = Vec::new();
    for values in args[1..].chunks(8) {
        let mut q = [[0.; 2]; 4];
        for j in 0..8 {
            q[j / 2][j % 2] = values[j].parse()?;
        }
        candidates.push(q);
    }
    if candidates.is_empty() {
        candidates.push([
            [0., 0.],
            [w as f64, 0.],
            [w as f64, h as f64],
            [0., h as f64],
        ]);
    }
    if bands {
        let mut scanner = barcode_research_core::bands::BandScanner::default();
        for (index, q) in candidates.iter().enumerate() {
            let started = std::time::Instant::now();
            match scanner.scan(image, *q) {
                Ok(reads) => {
                    for r in reads {
                        let text: String = r.digits.iter().map(|d| char::from(b'0' + d)).collect();
                        println!(
                            "candidate={} text={} decoded_polygon={:?}",
                            index, text, r.polygon
                        );
                    }
                }
                Err(e) => eprintln!("candidate={index} error={e:?}"),
            }
            eprintln!(
                "candidate={} coverage={:?} paths={} truncated={} native_band_ms={:.3}",
                index,
                q,
                scanner.attempts(),
                scanner.truncated(),
                started.elapsed().as_secs_f64() * 1000.
            );
        }
        return Ok(());
    }
    let mut scanner = Scanner::default();
    let started = std::time::Instant::now();
    let results = scanner.scan_regions(image, &candidates)?;
    let elapsed = started.elapsed();
    for r in results {
        let text = r
            .read
            .map(|x| {
                x.digits
                    .iter()
                    .map(|d| char::from(b'0' + d))
                    .collect::<String>()
            })
            .unwrap_or_default();
        println!(
            "candidate={} rank={} text={} paths={} error={:?} coverage={:?} decoded_polygon={:?}",
            r.candidate_index,
            r.rank,
            text,
            r.paths_attempted,
            r.error,
            r.coverage,
            r.read.map(|x| x.polygon)
        );
    }
    eprintln!(
        "native candidate decoding only: {:.3} ms",
        elapsed.as_secs_f64() * 1000.
    );
    Ok(())
}
