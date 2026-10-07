#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

// Internal engine; its items are documented in core/README.md, not rustdoc.
#[allow(missing_docs)]
#[path = "../generated/mod.rs"]
mod engine;
mod format;
mod pixels;
mod timer;
mod types;
pub use format::Format;
pub use pixels::Image;
pub use types::*;

/// Scan with default configuration and return decoded barcodes with positions.
///
/// # Errors
/// Returns an error for invalid pixels or an engine failure.
pub fn scan<'a>(image: impl Into<Image<'a>>) -> Result<ScanResult, Error> {
    Scanner::default().scan(image)
}

/// Scan an image with default scanner configuration and per-image options.
/// Returns all decoded instances with source-image positions.
/// Reuse [`Scanner`] for successive images.
///
/// # Errors
/// Rejects invalid pixels, incompatible options or engine failures.
pub fn scan_with_options<'a>(
    image: impl Into<Image<'a>>,
    options: ScanOptions,
) -> Result<ScanResult, Error> {
    Scanner::default().scan_with_options(image, options)
}

/// Inspect with default scanner configuration and per-image options.
/// # Errors
/// Returns an error for invalid pixels or an engine failure.
pub fn inspect_with_options<'a>(
    image: impl Into<Image<'a>>,
    options: ScanOptions,
) -> Result<InspectionResult, Error> {
    Scanner::default().inspect_with_options(image, options)
}

/// Inspect with default configuration, including work status and diagnostics.
/// # Errors
/// Returns an error for invalid pixels or an engine failure.
pub fn inspect<'a>(image: impl Into<Image<'a>>) -> Result<InspectionResult, Error> {
    Scanner::default().inspect(image)
}

/// Reusable scanner. Inputs are borrowed only during the call; results own their data.
/// Dropping the scanner releases its resources. Calls scan every selected candidate
/// within the engine's work limits; [`InspectionResult::best`] does not change that work.
pub struct Scanner {
    options: ScannerOptions,
    engine: Engine,
}

// The long-code variants keep comparable reusable scratch (about 9 KiB).
// Keep construction inline rather than adding heap indirection for that small difference.
#[allow(clippy::large_enum_variant)]
enum Engine {
    #[cfg(tapirscan_mode_low)]
    Low(engine::low::Scanner),
    #[cfg(tapirscan_mode_medium)]
    Medium(engine::medium::Scanner),
    #[cfg(tapirscan_mode_high)]
    High(engine::high::Scanner),
    #[cfg(tapirscan_mode_very_high)]
    VeryHigh(engine::very_high::Scanner),
    Unavailable,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new(ScannerOptions::default())
    }
}

impl Scanner {
    /// Configure effort, formats and supplement policy once.
    ///
    /// `options` fixes the scanner's defaults; use [`ScannerOptions::default()`]
    /// for Medium effort, retail formats and ignored supplements. Construction is local
    /// and infallible. Input validation happens during scanning.
    #[must_use]
    pub fn new(options: ScannerOptions) -> Self {
        let engine = match options.mode {
            #[cfg(tapirscan_mode_low)]
            Mode::Low => Engine::Low(engine::low::Scanner::default()),
            #[cfg(tapirscan_mode_medium)]
            Mode::Medium => Engine::Medium(engine::medium::Scanner::default()),
            #[cfg(tapirscan_mode_high)]
            Mode::High => Engine::High(engine::high::Scanner::default()),
            #[cfg(tapirscan_mode_very_high)]
            Mode::VeryHigh => Engine::VeryHigh(engine::very_high::Scanner::default()),
            #[allow(unreachable_patterns)]
            _ => Engine::Unavailable,
        };
        Self { options, engine }
    }

    /// Return the configuration used to construct this scanner.
    #[must_use]
    pub fn options(&self) -> ScannerOptions {
        self.options
    }

    /// Scan using this scanner's configuration and default per-image options.
    ///
    /// # Errors
    /// Returns an error for invalid pixels or an engine failure.
    pub fn scan<'a>(&mut self, image: impl Into<Image<'a>>) -> Result<ScanResult, Error> {
        self.scan_with_options(image, ScanOptions::default())
    }

    /// Scan with per-image overrides and return decoded barcodes.
    ///
    /// Overrides apply only to this call. Use [`Self::inspect`] for work status
    /// and diagnostic evidence. The call has no wall-clock timeout.
    ///
    /// # Errors
    /// Rejects invalid pixels. Extended budgets are accepted for every format.
    /// Internal reader failures are errors, never empty results.
    pub fn scan_with_options<'a>(
        &mut self,
        image: impl Into<Image<'a>>,
        options: ScanOptions,
    ) -> Result<ScanResult, Error> {
        self.run(image.into(), options, false)
            .map(|result| ScanResult {
                barcodes: result.barcodes,
            })
    }

    /// Inspect one image, including unread regions, work status and engine diagnostics.
    /// # Errors
    /// Returns an error for invalid pixels or an engine failure.
    pub fn inspect<'a>(&mut self, image: impl Into<Image<'a>>) -> Result<InspectionResult, Error> {
        self.inspect_with_options(image, ScanOptions::default())
    }

    /// Inspect with per-image overrides. Diagnostic schemas are unstable.
    /// # Errors
    /// Returns an error for invalid pixels or an engine failure.
    pub fn inspect_with_options<'a>(
        &mut self,
        image: impl Into<Image<'a>>,
        options: ScanOptions,
    ) -> Result<InspectionResult, Error> {
        self.run(image.into(), options, true)
    }

    fn run(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        diagnostics: bool,
    ) -> Result<InspectionResult, Error> {
        let start = timer::Timer::start();
        image.validate()?;
        let formats = options.formats.unwrap_or(self.options.formats);
        let addons = self.options.ean_add_on_policy;
        // Each arm calls the exact selected host/core pair.
        macro_rules! run {
            ($scanner:expr, $module:ident) => {{
                use engine::$module as selected;
                let input = selected::Image {
                    data: image.data,
                    width: image.width,
                    height: image.height,
                    channels: image.channels,
                    stride: image.stride,
                };
                let settings = selected::ScanOptions {
                    multiple: true,
                    include_regions: true,
                    retain_diagnostics: diagnostics,
                    finish_candidates: false,
                };
                let policy = match addons {
                    EanAddOnPolicy::Ignore => selected::formats::EanAddOnPolicy::Ignore,
                    EanAddOnPolicy::Read => selected::formats::EanAddOnPolicy::Read,
                    EanAddOnPolicy::Require => selected::formats::EanAddOnPolicy::Require,
                };
                let output = $scanner
                    .scan_formats_typed_with_addons(input, settings, formats.bits(), policy)
                    .map_err(|error| Error::Engine(error.to_string()))?;
                InspectionResult::from_engine(output, image, self.options.mode, start.elapsed())
            }};
        }
        let mut result = match &mut self.engine {
            #[cfg(tapirscan_mode_low)]
            Engine::Low(scanner) => run!(scanner, low),
            #[cfg(tapirscan_mode_medium)]
            Engine::Medium(scanner) => run!(scanner, medium),
            #[cfg(tapirscan_mode_high)]
            Engine::High(scanner) => run!(scanner, high),
            #[cfg(tapirscan_mode_very_high)]
            Engine::VeryHigh(scanner) => run!(scanner, very_high),
            Engine::Unavailable => {
                return Err(Error::InvalidOptions(
                    "selected scanner mode is not enabled in this build",
                ));
            }
        };
        result.elapsed = start.elapsed();
        Ok(result)
    }
}
