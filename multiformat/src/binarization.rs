//! Lazy frame-wide threshold cache shared by opt-in matrix localizers.
//! A disabled format adds no thresholding work.
pub struct Images<'a> {
    gray: &'a [u8],
    width: usize,
    height: usize,
    cached: [Option<Vec<bool>>; 4],
    identical_thresholds: Option<bool>,
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
    let left = ((x - radius).floor() as isize).max(1) as usize;
    let right = ((x + radius).ceil() as isize).clamp(1, width as isize - 1) as usize;
    let top = ((y - radius).floor() as isize).max(1) as usize;
    let bottom = ((y + radius).ceil() as isize).clamp(1, height as isize - 1) as usize;
    let step = (radius as usize / 12).max(1);
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
    let energy = (xx + yy) as f32;
    energy < 12. || ((xx - yy) as f32).hypot((2 * xy) as f32) < energy * 0.90
}
impl<'a> Images<'a> {
    #[must_use]
    pub fn new(gray: &'a [u8], width: usize, height: usize) -> Self {
        Self {
            gray,
            width,
            height,
            cached: [None, None, None, None],
            identical_thresholds: None,
        }
    }
    /// Pixel-exact equality permits omitting deterministic repeated work.
    /// Callers with adaptive proposal grouping must also check their own state.
    ///
    /// # Panics
    ///
    /// May panic if the cached dimensions are inconsistent with the grayscale buffer.
    pub fn is_duplicate(&mut self, mode: usize) -> bool {
        if mode.is_multiple_of(2) {
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
    ///
    /// # Panics
    ///
    /// Panics if `mode` is outside the four cached slots. Local thresholding
    /// may also panic if the dimensions are inconsistent with the grayscale buffer.
    #[must_use]
    pub fn get(&mut self, mode: usize) -> &[bool] {
        if self.cached[mode].is_none() {
            let image = if mode < 2 {
                crate::qr_detect::binarize(self.gray, self.width, self.height, mode == 1)
            } else {
                self.get(mode - 2).iter().map(|b| !*b).collect()
            };
            self.cached[mode] = Some(image);
        }
        self.cached[mode].as_ref().unwrap()
    }
}
