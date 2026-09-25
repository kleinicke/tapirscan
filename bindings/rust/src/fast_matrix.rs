//! Private matrix acceleration which preserves all non-background source pixels.
use barcode_multiformat::Scan;

pub(crate) fn scan(pixels: &[u8], width: usize, height: usize, mask: u32, effort: usize) -> Scan {
    let background = pixels[0];
    let mut bounds = [width, height, 0, 0];
    for (y, row) in pixels[..width * height].chunks_exact(width).enumerate() {
        if let Some(left) = row.iter().position(|&p| p != background) {
            let right = row.iter().rposition(|&p| p != background).unwrap_or(left);
            bounds[0] = bounds[0].min(left);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(right + 1);
            bounds[3] = y + 1;
            // Bounds only grow. A busy frame cannot become eligible later.
            if (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]) > 512 {
                return barcode_multiformat::scan(pixels, width, height, mask, effort);
            }
        }
    }
    if bounds[0] == width {
        return Scan {
            barcodes: vec![],
            regions: vec![],
            unfinished: false,
            lines: 0,
        };
    }
    let padding = ((bounds[2] - bounds[0]).max(bounds[3] - bounds[1]) / 4).max(32);
    let x0 = bounds[0].saturating_sub(padding);
    let y0 = bounds[1].saturating_sub(padding);
    let x1 = bounds[2].saturating_add(padding).min(width);
    let y1 = bounds[3].saturating_add(padding).min(height);
    let w = x1 - x0;
    let h = y1 - y0;
    // A failed retry should cost only a small fraction of the source search.
    if w.max(h) > 512 || w * h > width * height / 4 {
        return barcode_multiformat::scan(pixels, width, height, mask, effort);
    }
    let mut cropped = Vec::with_capacity(w * h);
    for row in pixels[y0 * width..y1 * width].chunks_exact(width) {
        cropped.extend_from_slice(&row[x0..x1]);
    }
    let mut result = barcode_multiformat::scan(&cropped, w, h, mask, effort);
    // A failed or visibly incomplete crop must not replace the source scan.
    // The crop contains ALL foreground, independent of payload or format.
    if result.barcodes.is_empty() || !result.regions.is_empty() {
        return barcode_multiformat::scan(pixels, width, height, mask, effort);
    }
    let offset = [
        barcode_multiformat::numeric::usize_f32(x0),
        barcode_multiformat::numeric::usize_f32(y0),
    ];
    for polygon in result
        .barcodes
        .iter_mut()
        .map(|r| &mut r.polygon)
        .chain(result.regions.iter_mut().map(|r| &mut r.polygon))
    {
        for point in polygon {
            point[0] += offset[0];
            point[1] += offset[1];
        }
    }
    // Cropping changes threshold context and row scheduling, though no foreground is omitted.
    result.unfinished = true;
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn constant_frames_have_no_matrix_evidence() {
        for value in [0, 64, 128, 192, 255] {
            let pixels = vec![value; 129 * 67];
            let actual = super::scan(&pixels, 129, 67, 512 | 1024 | 2048 | 4096 | 131_072, 0);
            assert!(actual.barcodes.is_empty());
            assert!(actual.regions.is_empty());
            assert!(!actual.unfinished);
        }
    }
}
