//! Which format-reader passes each compiled mode runs, and their limits.
//!
//! The mode is fixed at compile time, so every field is a constant and a disabled
//! pass costs nothing. Passes whose code exists in only some modes stay behind
//! `#[cfg(feature = ...)]` in `formats.rs`; this table holds the switches and limits
//! of the passes that several modes share.

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
    /// formats localized recovery cannot read (`SWEPT`); for the rest it only added
    /// wrong reads and time. EAN add-on reading always sweeps every format.
    pub sweep_all_linear: bool,
    /// Sweep effort without EAN add-on reading.
    pub sweep_effort: usize,
    /// Sweep effort with EAN add-on reading.
    pub addon_sweep_effort: usize,
    /// Code 128, Code 39 and ITF stay in a sweep group of their own: retail boxes come
    /// from a 768-pixel localization raster, on which the many small fields of large
    /// label photos vanish.
    pub preserved_sweep: bool,
    /// With retail selected, Code 128 and Code 39 stay in that sweep on frames of any
    /// size, not only on large frames.
    pub preserved_sweep_any_size: bool,
    /// Effort of the preserved sweep group.
    pub preserved_sweep_effort: usize,
    /// Strong sweep reads of other linear formats bound the primary retail retries.
    pub sweep_coverage: bool,
    /// EAN-8 and UPC-E are read by the primary retail scan instead of the sweep.
    pub shared_short_retail: bool,

    // 2D reader groups and their recoveries (`scan_additional`).
    /// Effort of reader groups that contain QR.
    pub qr_effort: usize,
    /// Unresolved QR regions are resampled on their projected module grid.
    pub qr_grid_projection: bool,
    /// The QR group of an EAN add-on selection runs the QR-only reader, as a QR-only
    /// scan does. High's QR-only reader adds block thresholds on every unread frame,
    /// which made add-on retail scans 5% slower, so High keeps the group reader there.
    pub qr_addon_group_as_qr_only: bool,
    /// The foreground-threshold QR pass also runs for mixed 2D selections on frames
    /// above `SMALL_FRAME_PIXELS`; QR-only selections always take it.
    pub qr_foreground_mixed_any_size: bool,
    /// The foreground-threshold QR pass also runs on frames that left a QR region.
    pub qr_foreground_with_region: bool,
    /// Largest frame, in pixels, whose unread foreground pass is retried sharpened.
    pub qr_restoration_pixels: usize,
    /// Effort of the sharpened QR retry.
    pub qr_restoration_effort: usize,
    /// Reduced copies (long side, coarsest first) for unread QR selections; each finer
    /// scale runs only when the coarser one read nothing.
    pub qr_reduced_scales: &'static [usize],
    /// Source-sample allowance for grid recovery of pending matrix regions.
    pub matrix_grid_samples: usize,
    /// Pixel allowance for localized crop retries of pending matrix regions.
    pub matrix_crop_pixels: usize,
    /// Effort of those crop retries.
    pub matrix_crop_effort: usize,
    /// Area-averaged Data Matrix copies also run on frames without a Data Matrix
    /// region, not only after one was found.
    pub data_matrix_averaged_without_region: bool,
    /// Long sides of the area-averaged Data Matrix copies, in order.
    pub data_matrix_averaged_scales: &'static [usize],

    // Localized linear recovery (`recover_linear_regions`, not compiled into Low).
    /// Exact source profiles are cached within one frame.
    pub source_profile_cache: bool,
    /// The stripe-box fallback uses the sparse stripe search instead of the full one.
    pub sparse_stripe_fallback: bool,
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
    /// Unread short-retail frames retry rejected narrow localizer groups.
    pub short_retail_narrow_groups: bool,
    /// Unresolved proposals retried with contrast hypotheses for Code 128, Code 39,
    /// ITF and Code 93.
    pub checksum_free_recovery_proposals: usize,
}

impl ModePolicy {
    const LOW: Self = Self {
        sweep_all_linear: true,
        sweep_effort: 0,
        addon_sweep_effort: 0,
        preserved_sweep: false,
        preserved_sweep_any_size: false,
        preserved_sweep_effort: 0,
        sweep_coverage: false,
        shared_short_retail: false,
        qr_effort: 0,
        qr_grid_projection: crate::LOW_FAST_PATH,
        qr_addon_group_as_qr_only: true,
        qr_foreground_mixed_any_size: false,
        qr_foreground_with_region: false,
        qr_restoration_pixels: super::SMALL_FRAME_PIXELS,
        qr_restoration_effort: 1,
        qr_reduced_scales: &[],
        matrix_grid_samples: 0,
        matrix_crop_pixels: 0,
        matrix_crop_effort: 0,
        data_matrix_averaged_without_region: false,
        data_matrix_averaged_scales: &[400, 256],
        source_profile_cache: false,
        sparse_stripe_fallback: false,
        half_scale_proposals: false,
        window_proposals: false,
        retail_batch_half_scale: false,
        short_retail_withholds_repeats: false,
        short_retail_narrow_groups: false,
        checksum_free_recovery_proposals: 0,
    };

    /// Medium keeps its cost on camera frames: costly recoveries need evidence.
    const MEDIUM: Self = Self {
        sweep_all_linear: false,
        addon_sweep_effort: 1,
        preserved_sweep: true,
        sweep_coverage: true,
        shared_short_retail: true,
        qr_effort: 1,
        qr_grid_projection: true,
        matrix_grid_samples: 250_000,
        matrix_crop_pixels: 65_536,
        matrix_crop_effort: 1,
        source_profile_cache: true,
        sparse_stripe_fallback: true,
        retail_batch_half_scale: true,
        short_retail_narrow_groups: true,
        checksum_free_recovery_proposals: 8,
        ..Self::LOW
    };

    /// High adds fixed-cost recoveries that do not wait for candidate evidence.
    const HIGH: Self = Self {
        sweep_effort: 2,
        addon_sweep_effort: 2,
        preserved_sweep_any_size: true,
        preserved_sweep_effort: 1,
        qr_effort: 2,
        qr_addon_group_as_qr_only: false,
        qr_foreground_mixed_any_size: true,
        qr_foreground_with_region: true,
        qr_restoration_pixels: super::QR_RECOVERY_MAX_PIXELS,
        qr_restoration_effort: 2,
        qr_reduced_scales: &[1200, 1280],
        matrix_grid_samples: 2_000_000,
        matrix_crop_pixels: 131_072,
        matrix_crop_effort: 2,
        data_matrix_averaged_without_region: true,
        source_profile_cache: false,
        sparse_stripe_fallback: false,
        half_scale_proposals: true,
        retail_batch_half_scale: false,
        short_retail_withholds_repeats: true,
        short_retail_narrow_groups: false,
        ..Self::MEDIUM
    };

    /// Very High reads what High reads plus finer scales and windows.
    const VERY_HIGH: Self = Self {
        qr_effort: 3,
        qr_reduced_scales: &[1200, 1280, 1600],
        matrix_grid_samples: 4_000_000,
        matrix_crop_pixels: 262_144,
        matrix_crop_effort: 3,
        data_matrix_averaged_scales: &[400, 1008, 800, 256],
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
