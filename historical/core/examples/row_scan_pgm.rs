//! Native full-frame experimental EAN-13 scanner: `row_scan_pgm` image.pgm.
use barcode_research_core::{row_scan::Scanner, sampling::ImageView};
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
    let file = std::env::args()
        .nth(1)
        .ok_or("Usage: row_scan_pgm image.pgm")?;
    if std::fs::metadata(&file)?.len() > 32 * 1024 * 1024 + 4096 {
        return Err("PGM too large".into());
    }
    let bytes = std::fs::read(file)?;
    let mut i = 0;
    if token(&bytes, &mut i)? != "P5" {
        return Err("Expected P5".into());
    }
    let w: usize = token(&bytes, &mut i)?.parse()?;
    let h: usize = token(&bytes, &mut i)?.parse()?;
    if token(&bytes, &mut i)? != "255" {
        return Err("Expected gray8".into());
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
    let mut scanner = Scanner::default();
    let start = std::time::Instant::now();
    scanner.scan(image)?;
    let ms = start.elapsed().as_secs_f64() * 1000.;
    print!("{{\"candidates\":[");
    for (i, r) in scanner.results().iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        let text = r
            .digits
            .map(|ds| ds.iter().map(|d| char::from(b'0' + d)).collect::<String>())
            .unwrap_or_default();
        print!(
            "{{\"text\":\"{}\",\"polygon\":{:?},\"support\":{}}}",
            text, r.polygon, r.support
        );
    }
    println!(
        "],\"rows\":{},\"hypotheses\":{},\"truncated\":{},\"native_scan_ms\":{}}}",
        scanner.rows, scanner.hypotheses, scanner.truncated, ms
    );
    Ok(())
}
