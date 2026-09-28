//! Lazy frame-wide threshold cache shared by opt-in matrix localizers.
//! A disabled format adds no thresholding work.
enum ContrastCut {
    Uncomputed,
    Unavailable,
    Value(u8),
}

pub struct Images<'a> {
    gray: &'a [u8],
    width: usize,
    height: usize,
    cached: [Option<Vec<bool>>; 8],
    row_offsets: [Option<RowOffsets>; 4],
    identical_thresholds: Option<bool>,
    qr_contrast_cut: ContrastCut,
    qr_upper_cut: ContrastCut,
}

// Cache transition positions only: runs can be derived exactly from adjacent
// offsets, while retaining every f32 conversion at the detector call site.
// Inverse threshold images have the same transition positions, so modes 0/2
// and 1/3 share one cache each.
const MAX_ROW_CACHE_BYTES: usize = 8 * 1024 * 1024;
const ROW_CACHE_ALLOCATOR_MARGIN: usize = 256 * 1024;
// This is a fixed cache policy, independent of height or the caller's step.
// It bounds row metadata even if a future caller requests a dense schedule.
const MAX_CACHED_ROWS: usize = 1_400;

struct CachedRow {
    y: usize,
    offsets: Box<[u32]>,
}

pub(crate) struct RowOffsets {
    rows: Vec<CachedRow>,
    payload_bytes: usize,
    // Retained capacity is budgeted; contents are cleared and reused for the
    // packed extractor while encoding one row at a time.
    scratch: Vec<usize>,
    max_bytes: usize,
    admission_exhausted: bool,
}

impl RowOffsets {
    fn new() -> Self {
        Self {
            rows: Vec::new(),
            payload_bytes: 0,
            scratch: Vec::new(),
            max_bytes: MAX_ROW_CACHE_BYTES - ROW_CACHE_ALLOCATOR_MARGIN,
            admission_exhausted: false,
        }
    }

    #[cfg(test)]
    fn with_budget(max_bytes: usize) -> Self {
        Self {
            rows: Vec::new(),
            payload_bytes: 0,
            scratch: Vec::new(),
            max_bytes,
            admission_exhausted: false,
        }
    }

    fn row_index(&self, y: usize) -> Result<usize, usize> {
        self.rows.binary_search_by_key(&y, |row| row.y)
    }

    fn populate(&mut self, bits: &[bool], width: usize, height: usize, step: usize) {
        if self.admission_exhausted || u32::try_from(width).is_err() || step == 0 {
            return;
        }
        // `transition_offsets_into` stores both 0 and the terminal width, so
        // an alternating row requires width + 1 offsets.
        let Some(required_offsets) = width.checked_add(1) else {
            self.admission_exhausted = true;
            return;
        };
        let Some(scratch_bytes) = required_offsets.checked_mul(std::mem::size_of::<usize>()) else {
            self.admission_exhausted = true;
            return;
        };
        if scratch_bytes > self.max_bytes {
            self.admission_exhausted = true;
            return;
        }
        let metadata = self
            .rows
            .capacity()
            .saturating_mul(std::mem::size_of::<CachedRow>());
        if self
            .payload_bytes
            .checked_add(metadata)
            .and_then(|total| total.checked_add(scratch_bytes))
            .is_none_or(|total| total > self.max_bytes)
        {
            self.admission_exhausted = true;
            return;
        }
        self.scratch.clear();
        if self.scratch.capacity() < required_offsets {
            self.scratch.reserve_exact(required_offsets);
        }
        let scratch_capacity_bytes = self
            .scratch
            .capacity()
            .saturating_mul(std::mem::size_of::<usize>());
        if self
            .payload_bytes
            .checked_add(metadata)
            .and_then(|total| total.checked_add(scratch_capacity_bytes))
            .is_none_or(|total| total > self.max_bytes)
        {
            self.scratch = Vec::new();
            self.admission_exhausted = true;
            return;
        }
        for y in (0..height).step_by(step) {
            if self.row_index(y).is_ok() {
                continue;
            }
            if self.rows.len() >= MAX_CACHED_ROWS {
                self.admission_exhausted = true;
                break;
            }
            let row = &bits[y * width..(y + 1) * width];
            self.scratch.clear();
            crate::qr_frontend::transition_offsets_into(row, &mut self.scratch);
            let payload = self
                .scratch
                .len()
                .saturating_mul(std::mem::size_of::<u32>());
            let capacity = if self.rows.len() == self.rows.capacity() {
                self.rows.capacity().max(1).saturating_mul(2)
            } else {
                self.rows.capacity()
            };
            let metadata = capacity.saturating_mul(std::mem::size_of::<CachedRow>());
            if payload
                .checked_add(self.payload_bytes)
                .and_then(|total| total.checked_add(metadata))
                .and_then(|total| total.checked_add(scratch_capacity_bytes))
                .is_none_or(|total| total > self.max_bytes)
            {
                self.admission_exhausted = true;
                break;
            }
            let Err(at) = self.row_index(y) else {
                continue;
            };
            self.rows.insert(
                at,
                CachedRow {
                    y,
                    offsets: self
                        .scratch
                        .iter()
                        .map(|&offset| u32::try_from(offset).expect("width checked above"))
                        .collect(),
                },
            );
            self.payload_bytes += payload;
        }
    }

    /// Copy a cached row's transition offsets, or calculate the exact same
    /// offsets into the caller scratch buffer after the bounded cache is full.
    pub(crate) fn copy_or_extract(&self, y: usize, row: &[bool], out: &mut Vec<usize>) {
        out.clear();
        if let Ok(index) = self.row_index(y) {
            out.extend(
                self.rows[index]
                    .offsets
                    .iter()
                    .map(|&offset| usize::try_from(offset).expect("u32 fits target usize")),
            );
        } else {
            crate::qr_frontend::transition_offsets_into(row, out);
        }
    }
}

/// Reject a putative matrix finder when its neighborhood contains almost only
/// parallel edges. Both square finders and bullseyes need two edge directions.
/// Low-energy neighborhoods remain eligible for the normal template checks.
pub(crate) fn has_two_directions(
    bits: &[bool],
    width: usize,
    height: usize,
    x: f32,
    y: f32,
    radius: f32,
) -> bool {
    if width < 3 || height < 3 {
        return true;
    }
    let left =
        (barcode_multiformat::numeric::f32_isize((x - radius).floor()).max(1)).cast_unsigned();
    let right = (barcode_multiformat::numeric::f32_isize((x + radius).ceil())
        .clamp(1, (width).cast_signed() - 1))
    .cast_unsigned();
    let top =
        (barcode_multiformat::numeric::f32_isize((y - radius).floor()).max(1)).cast_unsigned();
    let bottom = (barcode_multiformat::numeric::f32_isize((y + radius).ceil())
        .clamp(1, (height).cast_signed() - 1))
    .cast_unsigned();
    let step = (barcode_multiformat::numeric::f32_usize(radius) / 12).max(1);
    let (mut xx, mut yy, mut xy) = (0_i32, 0_i32, 0_i32);
    for y in (top..bottom).step_by(step) {
        for x in (left..right).step_by(step) {
            let at = y * width + x;
            let dx = i32::from(bits[at + 1]) - i32::from(bits[at - 1]);
            let dy = i32::from(bits[at + width]) - i32::from(bits[at - width]);
            xx += dx * dx;
            yy += dy * dy;
            xy += dx * dy;
        }
    }
    let energy = barcode_multiformat::numeric::f64_f32(f64::from(xx + yy));
    energy < 12.
        || barcode_multiformat::numeric::f64_f32(f64::from(xx - yy))
            .hypot(barcode_multiformat::numeric::f64_f32(f64::from(2 * xy)))
            < energy * 0.90
}
impl<'a> Images<'a> {
    #[must_use]
    pub fn new(gray: &'a [u8], width: usize, height: usize) -> Self {
        Self {
            gray,
            width,
            height,
            cached: [None, None, None, None, None, None, None, None],
            row_offsets: [None, None, None, None],
            identical_thresholds: None,
            qr_contrast_cut: ContrastCut::Uncomputed,
            qr_upper_cut: ContrastCut::Uncomputed,
        }
    }
    /// A second foreground split recovers low-contrast ink surrounded by a bright background.
    pub(crate) fn has_qr_contrast(&mut self) -> bool {
        if matches!(self.qr_contrast_cut, ContrastCut::Uncomputed) {
            let histogram = crate::qr_frontend::histogram(self.gray);
            let cut = otsu_cut(&histogram);
            let nested = cut.and_then(|cut| {
                let lower = &histogram[..=cut];
                let min = lower.iter().position(|&count| count > 0)?;
                let max = lower.iter().rposition(|&count| count > 0)?;
                if max - min < 6 {
                    return None;
                }
                otsu_cut(lower).map(|value| u8::try_from(value).expect("histogram has 256 bins"))
            });
            self.qr_contrast_cut = nested.map_or(ContrastCut::Unavailable, ContrastCut::Value);
            let upper = cut.and_then(|cut| {
                let values = &histogram[cut + 1..];
                let min = values.iter().position(|&count| count > 0)?;
                let max = values.iter().rposition(|&count| count > 0)?;
                if max - min < 6 {
                    return None;
                }
                otsu_cut(values)
                    .map(|value| u8::try_from(value + cut + 1).expect("histogram has 256 bins"))
            });
            self.qr_upper_cut = upper.map_or(ContrastCut::Unavailable, ContrastCut::Value);
        }
        matches!(self.qr_contrast_cut, ContrastCut::Value(_))
    }
    pub(crate) fn has_qr_upper_contrast(&mut self) -> bool {
        let _ = self.has_qr_contrast();
        matches!(self.qr_upper_cut, ContrastCut::Value(_))
    }
    /// Return the checked scalar cut without allocating threshold pixels.
    pub(crate) fn foreground_cut(&mut self, mode: usize) -> Option<u8> {
        let present = match mode {
            4 => self.has_qr_contrast(),
            6 => self.has_qr_upper_contrast(),
            _ => false,
        };
        if !present {
            return None;
        }
        match if mode == 4 {
            &self.qr_contrast_cut
        } else {
            &self.qr_upper_cut
        } {
            ContrastCut::Value(cut) => Some(*cut),
            _ => None,
        }
    }
    /// Pixel-exact equality permits omitting deterministic repeated work.
    /// Callers with adaptive proposal grouping must also check their own state.
    ///
    /// # Panics
    ///
    /// May panic if the cached dimensions are inconsistent with the grayscale buffer.
    pub fn is_duplicate(&mut self, mode: usize) -> bool {
        if mode >= 4 || mode.is_multiple_of(2) {
            return false;
        }
        if self.identical_thresholds.is_none() {
            let _ = self.get(0);
            let _ = self.get(1);
            self.identical_thresholds = Some(self.cached[0] == self.cached[1]);
        }
        self.identical_thresholds == Some(true)
    }
    /// Return global/local threshold pixels (modes 0/1) or their inverse (2/3).
    /// Internal QR recovery additionally uses checked lower (4/5) and upper (6/7) foreground splits.
    ///
    /// # Panics
    ///
    /// Panics if `mode` is outside the eight cached slots. Local thresholding
    /// may also panic if the dimensions are inconsistent with the grayscale buffer.
    #[must_use]
    pub fn get(&mut self, mode: usize) -> &[bool] {
        if self.cached[mode].is_none() {
            let image = if mode == 4 || mode == 6 {
                let ContrastCut::Value(cut) = (if mode == 4 {
                    &self.qr_contrast_cut
                } else {
                    &self.qr_upper_cut
                }) else {
                    unreachable!("QR contrast availability checked before use");
                };
                self.gray.iter().map(|&pixel| pixel <= *cut).collect()
            } else if mode == 5 || mode == 7 {
                self.get(mode - 1).iter().map(|bit| !*bit).collect()
            } else if mode < 2 {
                crate::qr_frontend::qr_detect::binarize(
                    self.gray,
                    self.width,
                    self.height,
                    mode == 1,
                )
            } else {
                self.get(mode - 2).iter().map(|b| !*b).collect()
            };
            self.cached[mode] = Some(image);
        }
        self.cached[mode].as_ref().unwrap()
    }

    /// Return threshold pixels plus lazily populated transition rows. The
    /// requested `step` adds only its missing rows; cache budget exhaustion
    /// leaves later rows to the exact uncached extraction path.
    ///
    /// # Panics
    ///
    /// Has the same dimension and mode requirements as [`Self::get`].
    pub(crate) fn get_with_runs(&mut self, mode: usize, step: usize) -> (&[bool], &RowOffsets) {
        let _ = self.get(mode);
        let base = if mode >= 4 {
            2 + (mode - 4) / 2
        } else {
            mode % 2
        };
        let (width, height) = (self.width, self.height);
        let (cached, row_offsets) = (&self.cached, &mut self.row_offsets);
        let bits = cached[mode].as_ref().unwrap();
        let offsets = row_offsets[base].get_or_insert_with(RowOffsets::new);
        offsets.populate(bits, width, height, step);
        (bits, offsets)
    }
}

fn otsu_cut(histogram: &[usize]) -> Option<usize> {
    let count: usize = histogram.iter().sum();
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            barcode_multiformat::numeric::usize_f64(i) * barcode_multiformat::numeric::usize_f64(n)
        })
        .sum();
    let mut mass = 0usize;
    let mut left = 0.;
    let mut best = 0.;
    let mut cut = None;
    for (i, &n) in histogram.iter().enumerate() {
        if n == 0 {
            continue;
        }
        mass += n;
        left +=
            barcode_multiformat::numeric::usize_f64(i) * barcode_multiformat::numeric::usize_f64(n);
        if mass == count {
            break;
        }
        let delta = left / barcode_multiformat::numeric::usize_f64(mass)
            - (sum - left) / barcode_multiformat::numeric::usize_f64(count - mass);
        let score = barcode_multiformat::numeric::usize_f64(mass)
            * barcode_multiformat::numeric::usize_f64(count - mass)
            * delta
            * delta;
        if score > best {
            best = score;
            cut = Some(i);
        }
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::{CachedRow, Images, RowOffsets};

    fn uncached(row: &[bool]) -> Vec<usize> {
        let mut offsets = Vec::new();
        crate::qr_frontend::transition_offsets_into(row, &mut offsets);
        offsets
    }

    #[test]
    fn transition_cache_matches_inverse_and_differing_steps() {
        let gray = vec![
            0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 0, 255,
            255, 0, 255,
        ];
        let mut images = Images::new(&gray, 4, 6);
        let (global, rows) = images.get_with_runs(0, 2);
        let global = global.to_vec();
        let mut offsets = Vec::new();
        for y in (0..6).step_by(2) {
            rows.copy_or_extract(y, &global[y * 4..(y + 1) * 4], &mut offsets);
            assert_eq!(offsets, uncached(&global[y * 4..(y + 1) * 4]));
        }
        let (inverse, rows) = images.get_with_runs(2, 3);
        let inverse = inverse.to_vec();
        assert_eq!(inverse, global.iter().map(|bit| !bit).collect::<Vec<_>>());
        assert!(rows.row_index(3).is_ok());
        for y in 0..6 {
            rows.copy_or_extract(y, &inverse[y * 4..(y + 1) * 4], &mut offsets);
            assert_eq!(offsets, uncached(&inverse[y * 4..(y + 1) * 4]));
        }
    }

    #[test]
    fn budget_exhaustion_uses_exact_uncached_offsets() {
        let row = [false, true, false, true, false, true, false, true];
        let mut rows = RowOffsets::with_budget(row.len() * std::mem::size_of::<usize>());
        rows.populate(&row, row.len(), 1, 1);
        assert!(rows.admission_exhausted);
        let mut offsets = Vec::new();
        rows.copy_or_extract(0, &row, &mut offsets);
        assert_eq!(offsets, uncached(&row));
    }

    #[test]
    fn alternating_rows_reserve_the_terminal_offset_before_admission() {
        let row = [false, true, false, true, false, true, false, true];
        let budget = row.len() * std::mem::size_of::<usize>()
            + (row.len() + 1) * std::mem::size_of::<u32>()
            + std::mem::size_of::<CachedRow>();
        let mut rows = RowOffsets::with_budget(budget);
        rows.populate(&row, row.len(), 1, 1);
        let retained = rows.payload_bytes
            + rows.rows.capacity() * std::mem::size_of::<CachedRow>()
            + rows.scratch.capacity() * std::mem::size_of::<usize>();
        assert!(retained <= budget);
        let mut offsets = Vec::new();
        rows.copy_or_extract(0, &row, &mut offsets);
        assert_eq!(offsets, uncached(&row));
    }
}

#[cfg(test)]
mod foreground_tests {
    #[test]
    fn two_tone_inputs_have_no_new_foreground_split() {
        for value in [0u8, 127, 255] {
            let pixels = vec![value; 64];
            let mut images = super::Images::new(&pixels, 8, 8);
            assert!(!images.has_qr_contrast());
            assert!(!images.has_qr_upper_contrast());
        }
        let pixels: Vec<_> = (0..64).map(|i| if i % 2 == 0 { 0 } else { 255 }).collect();
        let mut images = super::Images::new(&pixels, 8, 8);
        assert!(!images.has_qr_contrast());
        assert!(!images.has_qr_upper_contrast());
    }
    #[test]
    fn lower_and_upper_splits_keep_independent_inverse_run_caches() {
        let pixels: Vec<_> = (0..64)
            .map(|i| [0, 10, 20, 30, 220, 230, 240, 250][i % 8])
            .collect();
        let mut images = super::Images::new(&pixels, 8, 8);
        assert!(images.has_qr_contrast());
        assert!(images.has_qr_upper_contrast());
        let lower = images.get(4).to_vec();
        let upper = images.get(6).to_vec();
        assert_ne!(lower, upper);
        for (mode, bits) in [(4, lower), (6, upper)] {
            let (normal, rows) = images.get_with_runs(mode, 2);
            assert_eq!(normal, bits);
            let mut offsets = Vec::new();
            rows.copy_or_extract(0, &bits[..8], &mut offsets);
            let (inverse, rows) = images.get_with_runs(mode + 1, 3);
            assert_eq!(inverse, bits.iter().map(|bit| !*bit).collect::<Vec<_>>());
            let mut inverted_offsets = Vec::new();
            rows.copy_or_extract(0, &inverse[..8], &mut inverted_offsets);
            assert_eq!(offsets, inverted_offsets);
        }
    }
}
