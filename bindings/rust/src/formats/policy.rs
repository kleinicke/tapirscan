//! Which format-reader passes each compiled mode runs, and their limits.
//!
//! The mode is fixed at compile time, so every field is a constant and a disabled
//! pass costs nothing. Passes whose code exists in only some modes stay behind
//! `#[cfg(feature = ...)]` in `formats.rs`; this table holds the switches and limits
//! of the passes that several modes share.

/// A copy of a crop that the crop retries of pending matrix regions read, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CropCopy {
    Plain,
    Sharpened,
    /// Doubled in width and height.
    Doubled,
}

impl CropCopy {
    /// Every copy, in reading order.
    pub(crate) const ALL: &'static [Self] = &[Self::Plain, Self::Sharpened, Self::Doubled];
}

/// Mode-dependent switches and limits of the format-reader passes.
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each flag switches one independent pass; the named switches are the table"
)]
#[cfg_attr(
    feature = "low",
    expect(
        dead_code,
        reason = "Low compiles neither localized linear recovery nor the 2D recoveries these fields configure"
    )
)]
pub(super) struct ModePolicy {
    // Whole-image linear sweep (`scan_additional`).
    /// The sweep reads every selected linear format. Otherwise it keeps only the
    /// formats localized recovery cannot read (`SWEPT`); for the rest it only adds
    /// wrong reads and time.
    pub sweep_all_linear: bool,
    /// Sweep effort.
    pub sweep_effort: usize,
    /// Grayscale profiles across an EAN/UPC result on which the supplement reader
    /// (`addon_profile`, `read` and `require` policies) looks for one; two must agree. Low
    /// takes three, Medium five, High nine: the extra profiles cross supplements
    /// that only part of the symbol height shows sharply.
    pub supplement_profiles: usize,
    /// Effort of the retail-only supplement sweep that runs once where profiles saw
    /// supplement-like bars but confirmed no value.
    pub supplement_sweep_effort: usize,
    /// Code 128 stays in a sweep group of its own, with ITF without retail and with
    /// Code 39 on large frames with retail: retail boxes come from a 768-pixel
    /// localization raster, on which the many small fields of large label photos vanish.
    pub preserved_sweep: bool,
    /// With retail selected, Code 128 and Code 39 stay in that sweep on frames of any
    /// size, not only on large frames.
    pub preserved_sweep_any_size: bool,
    /// Effort of the preserved sweep group.
    pub preserved_sweep_effort: usize,
    /// The preserved sweep group also runs at effort 0 on the frames where Medium runs
    /// it: the effort-1 scanlines, twice as dense, lie between the effort-0 ones and
    /// can miss a symbol only those cross.
    pub coarse_preserved_sweep: bool,
    /// Strong sweep reads of other linear formats bound the primary retail retries.
    pub sweep_coverage: bool,
    /// EAN-8 and UPC-E are read by the primary retail scan instead of the sweep.
    pub shared_short_retail: bool,

    // 2D reader groups and their recoveries (`scan_additional`).
    /// Effort of reader groups that contain QR.
    pub qr_effort: usize,
    /// Unresolved QR regions are resampled on their projected module grid.
    pub qr_grid_projection: bool,
    /// The foreground-threshold QR pass also runs for mixed 2D selections on frames
    /// above `SMALL_FRAME_PIXELS`; QR-only selections always take it.
    pub qr_foreground_mixed_any_size: bool,
    /// The foreground-threshold QR pass also runs on frames that left a QR region.
    pub qr_foreground_with_region: bool,
    /// With QR sharing the scan with other formats, the foreground-threshold and
    /// equalized/inverted QR recoveries run on frames above `SMALL_FRAME_PIXELS` only
    /// where a QR candidate region stayed unread.
    pub qr_mixed_recovery_needs_region: bool,
    /// Largest frame, in pixels, whose unread foreground pass is retried sharpened.
    pub qr_restoration_pixels: usize,
    /// Effort of the sharpened QR retry.
    pub qr_restoration_effort: usize,
    /// Reduced copies (long side, coarsest first) for unread QR selections; each finer
    /// scale runs only when the coarser one read nothing.
    pub qr_reduced_scales: &'static [usize],
    /// Mixed selections run the reduced QR copies on frames above `SMALL_FRAME_PIXELS`
    /// only where a low-resolution copy shows a large, square area of module-like
    /// blocks (`matrix_texture`). The copies rescue large symbols whose finder patterns
    /// the full-resolution passes miss; without such an area they would only cost time.
    pub qr_reduced_needs_texture: bool,
    /// Pixel allowance for localized crop retries of pending matrix regions.
    pub matrix_crop_pixels: usize,
    /// Effort of the crop retries of pending matrix regions.
    pub matrix_crop_effort: usize,
    /// Pending `MaxiCode` regions get crop retries too. Their recoveries come from the
    /// sharpened and doubled copies, which would cost Medium's `MaxiCode` frames a few
    /// milliseconds each.
    pub maxi_code_crop_retries: bool,
    /// One QR region per frame whose timing patterns read without error, and whose crop is
    /// larger than what is left of `matrix_crop_pixels`, may take a crop of up to this many
    /// pixels outside that shared allowance.
    pub qr_located_crop_pixels: usize,
    /// Located QR regions too large for either crop allowance retried per frame on a
    /// reduced copy (`signal_recovery::reduced_retry`).
    pub qr_reduced_region_retries: usize,
    /// Crop copies read of a fair Data Matrix region (`fair_data_matrix`). Texture with a
    /// fair border fails every copy, each a whole scan.
    pub fair_data_matrix_copies: &'static [CropCopy],
    /// Fair Data Matrix regions cropped per frame.
    pub fair_data_matrix_crops: usize,
    /// Long sides of the area-averaged Data Matrix copies, in order; empty in modes
    /// without them.
    pub data_matrix_averaged_scales: &'static [usize],
    /// On mixed selections' frames from `SMALL_FRAME_PIXELS` up to `CLOSE_UP_PIXELS`
    /// without a Data Matrix region, an averaged copy that leaves no Data Matrix
    /// candidate ends the copies. Frames of `CLOSE_UP_PIXELS` and more always try both:
    /// 12-megapixel close-ups of large symbols are read by the finer copy without any
    /// candidate on the coarse one.
    pub data_matrix_averaged_needs_candidate: bool,

    // Localized linear recovery (`recover_linear_regions`, not compiled into Low).
    /// Exact source profiles are cached within one frame.
    pub source_profile_cache: bool,
    /// The stripe-box fallback, which always decodes the sparse stripe search's boxes,
    /// also decodes the boxes of the full stripe search that the sparse one did not
    /// propose. Neither search finds every box the other does: the sparse boxes alone
    /// read 16 Code 128 fields on large label photos that the full boxes missed.
    pub full_stripe_fallback: bool,
    /// Large frames localize again on one half-scale raster.
    pub half_scale_proposals: bool,
    /// Large frames also localize on overlapping `WINDOW_SIDE` windows.
    pub window_proposals: bool,
    /// Large batch frames (`RETAIL_BATCH_READS` EAN-13/UPC-A reads) decode the retail
    /// boxes of one half-scale raster.
    pub retail_batch_half_scale: bool,
    /// The short-retail contrast hypothesis merges its reads and withholds repeated
    /// equal payloads: an equal value alone cannot establish physical ownership.
    pub short_retail_withholds_repeats: bool,
    /// Unresolved proposals retried with contrast hypotheses for Code 128, Code 39,
    /// ITF and Code 93.
    pub checksum_free_recovery_proposals: usize,
}

impl ModePolicy {
    const LOW: Self = Self {
        sweep_all_linear: true,
        sweep_effort: 0,
        supplement_profiles: 3,
        supplement_sweep_effort: 0,
        preserved_sweep: false,
        preserved_sweep_any_size: false,
        preserved_sweep_effort: 0,
        coarse_preserved_sweep: false,
        sweep_coverage: false,
        shared_short_retail: false,
        qr_effort: 0,
        qr_grid_projection: crate::LOW_FAST_PATH,
        qr_foreground_mixed_any_size: false,
        qr_foreground_with_region: false,
        qr_mixed_recovery_needs_region: false,
        qr_restoration_pixels: super::SMALL_FRAME_PIXELS,
        qr_restoration_effort: 1,
        qr_reduced_scales: &[],
        qr_reduced_needs_texture: false,
        matrix_crop_pixels: 0,
        matrix_crop_effort: 0,
        maxi_code_crop_retries: false,
        qr_located_crop_pixels: 0,
        qr_reduced_region_retries: 0,
        fair_data_matrix_copies: &[],
        fair_data_matrix_crops: 0,
        data_matrix_averaged_scales: &[],
        data_matrix_averaged_needs_candidate: false,
        source_profile_cache: false,
        full_stripe_fallback: false,
        half_scale_proposals: false,
        window_proposals: false,
        retail_batch_half_scale: false,
        short_retail_withholds_repeats: false,
        checksum_free_recovery_proposals: 0,
    };

    /// Medium keeps its cost on camera frames: costly recoveries need evidence.
    const MEDIUM: Self = Self {
        sweep_all_linear: false,
        supplement_profiles: 5,
        supplement_sweep_effort: 1,
        preserved_sweep: true,
        sweep_coverage: true,
        shared_short_retail: true,
        qr_effort: 1,
        qr_grid_projection: true,
        matrix_crop_pixels: 65_536,
        matrix_crop_effort: 1,
        qr_located_crop_pixels: 131_072,
        qr_reduced_region_retries: 1,
        fair_data_matrix_copies: &[CropCopy::Sharpened],
        fair_data_matrix_crops: 1,
        source_profile_cache: true,
        retail_batch_half_scale: true,
        checksum_free_recovery_proposals: 8,
        ..Self::LOW
    };

    /// High adds fixed-cost recoveries that do not wait for candidate evidence.
    const HIGH: Self = Self {
        sweep_effort: 2,
        supplement_profiles: 9,
        supplement_sweep_effort: 2,
        preserved_sweep_any_size: true,
        preserved_sweep_effort: 1,
        coarse_preserved_sweep: true,
        qr_effort: 2,
        qr_foreground_mixed_any_size: true,
        qr_foreground_with_region: true,
        qr_mixed_recovery_needs_region: true,
        qr_restoration_pixels: super::QR_RECOVERY_MAX_PIXELS,
        qr_restoration_effort: 2,
        qr_reduced_scales: &[1200, 1280],
        qr_reduced_needs_texture: true,
        matrix_crop_pixels: 131_072,
        matrix_crop_effort: 2,
        maxi_code_crop_retries: true,
        fair_data_matrix_copies: CropCopy::ALL,
        fair_data_matrix_crops: usize::MAX,
        // The cheap coarse copy runs first; the finer one only after it left a candidate.
        data_matrix_averaged_scales: &[256, 400],
        data_matrix_averaged_needs_candidate: true,
        source_profile_cache: false,
        full_stripe_fallback: true,
        half_scale_proposals: true,
        retail_batch_half_scale: false,
        short_retail_withholds_repeats: true,
        ..Self::MEDIUM
    };

    /// Very High reads what High reads plus finer scales and windows.
    const VERY_HIGH: Self = Self {
        qr_effort: 3,
        qr_reduced_scales: &[1200, 1280, 1600],
        matrix_crop_pixels: 262_144,
        matrix_crop_effort: 3,
        data_matrix_averaged_scales: &[400, 1008, 800, 256],
        // Its finer copies read eight Data Matrix symbols that a coarse candidate
        // would not have admitted.
        data_matrix_averaged_needs_candidate: false,
        window_proposals: true,
        checksum_free_recovery_proposals: 16,
        ..Self::HIGH
    };

    const fn for_mode(mode: u32) -> Self {
        match mode {
            0 => Self::LOW,
            1 => Self::MEDIUM,
            2 => Self::HIGH,
            _ => Self::VERY_HIGH,
        }
    }
}

/// The policy of the compiled mode.
pub(super) const POLICY: ModePolicy = ModePolicy::for_mode(crate::MODE_ID);
