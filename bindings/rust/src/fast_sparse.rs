//! Private sparse-frame acceleration preserving every non-background source pixel.
use barcode_multiformat::Scan;

struct Crop {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
    offset: [f32; 2],
}

/// Prepare once so additional linear and matrix readers share the same bounds and pixels.
pub(crate) struct Prepared<'a> {
    pixels: &'a [u8],
    width: usize,
    height: usize,
    constant: bool,
    crop: Option<Crop>,
}

impl<'a> Prepared<'a> {
    pub(crate) fn new(pixels: &'a [u8], width: usize, height: usize) -> Self {
        let mut prepared = Self {
            pixels,
            width,
            height,
            constant: false,
            crop: None,
        };
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
                    return prepared;
                }
            }
        }
        if bounds[0] == width {
            prepared.constant = true;
            return prepared;
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
            return prepared;
        }
        let mut cropped = Vec::with_capacity(w * h);
        for row in pixels[y0 * width..y1 * width].chunks_exact(width) {
            cropped.extend_from_slice(&row[x0..x1]);
        }
        prepared.crop = Some(Crop {
            pixels: cropped,
            width: w,
            height: h,
            offset: [
                barcode_multiformat::numeric::usize_f32(x0),
                barcode_multiformat::numeric::usize_f32(y0),
            ],
        });
        prepared
    }

    pub(crate) fn scan(&self, mask: u32, effort: usize) -> Scan {
        if self.constant {
            return Scan {
                barcodes: vec![],
                regions: vec![],
                unfinished: false,
                lines: 0,
            };
        }
        let source =
            || barcode_multiformat::scan(self.pixels, self.width, self.height, mask, effort);
        let Some(crop) = &self.crop else {
            return source();
        };
        let mut result =
            barcode_multiformat::scan(&crop.pixels, crop.width, crop.height, mask, effort);
        // Recovery remains independent for each reader group. Finding a matrix symbol
        // must never suppress the full-source linear search, or vice versa.
        if result.barcodes.is_empty() || !result.regions.is_empty() {
            return source();
        }
        for polygon in result
            .barcodes
            .iter_mut()
            .map(|r| &mut r.polygon)
            .chain(result.regions.iter_mut().map(|r| &mut r.polygon))
        {
            for point in polygon {
                point[0] += crop.offset[0];
                point[1] += crop.offset[1];
            }
        }
        // Cropping changes threshold context and row scheduling, though no foreground is omitted.
        result.unfinished = true;
        result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn constant_frames_have_no_symbol_evidence() {
        for value in [0, 64, 128, 192, 255] {
            let pixels = vec![value; 129 * 67];
            let prepared = super::Prepared::new(&pixels, 129, 67);
            for mask in [512 | 1024 | 2048 | 4096 | 131_072, 128 | 256 | 8192 | 16384] {
                let actual = prepared.scan(mask, 0);
                assert!(actual.barcodes.is_empty());
                assert!(actual.regions.is_empty());
                assert!(!actual.unfinished);
            }
        }
    }
}
