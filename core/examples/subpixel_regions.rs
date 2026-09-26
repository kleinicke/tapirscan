//! Supplied-region diagnostic: stdin rows are path width height and eight quad coordinates.
use barcode_research_core::{multi_scan::Policy, region_scan::RegionScanner, sampling::ImageView};
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scanner = RegionScanner::default();
    for line in io::stdin().lock().lines() {
        let line = line?;
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 11 {
            return Err("expected eleven fields".into());
        }
        let bytes = std::fs::read(fields[0])?;
        let width = fields[1].parse()?;
        let height = fields[2].parse()?;
        let mut quad = [[0.; 2]; 4];
        for (i, p) in quad.iter_mut().flatten().enumerate() {
            *p = fields[3 + i].parse()?;
        }
        let image = ImageView::new(&bytes, width, height, 1, width)?;
        let start = std::time::Instant::now();
        let result = scanner.scan(image, &[quad], Policy::default())?;
        let reads: std::collections::BTreeSet<String> = result
            .frame
            .barcodes
            .iter()
            .map(|b| {
                b.detection
                    .digits
                    .iter()
                    .map(|d| char::from(b'0' + d))
                    .collect()
            })
            .collect();
        println!(
            "{}\t{}",
            reads.into_iter().collect::<Vec<_>>().join(","),
            start.elapsed().as_secs_f64() * 1e6
        );
    }
    Ok(())
}
