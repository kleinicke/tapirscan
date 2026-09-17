#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

// Private snapshots retain entry points used by other language bindings.
#[allow(dead_code, unused_imports, missing_docs)]
#[path = "../generated/mod.rs"]
mod engine;
mod pixels;
mod types;
pub use pixels::Image;
use std::time::Instant;
pub use types::*;

/// Scan with default configuration and return decoded instances and work status.
///
/// # Errors
/// Returns an error for invalid pixels or an engine failure.
pub fn scan<'a>(image: impl Into<Image<'a>>) -> Result<ScanResult, Error> {
    Scanner::default().scan(image)
}

/// Scan an image with default scanner configuration and per-image options.
/// Returns all decoded instances, undecoded proposals and reported work limits.
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

/// Reusable scanner. Inputs are borrowed only during the call; results own their data.
/// Dropping the scanner releases its resources. Calls scan every selected candidate
/// within the engine's work limits; [`ScanResult::best`] does not change that work.
pub struct Scanner {
    options: ScannerOptions,
    engine: Engine,
}

enum Engine {
    Low(engine::low::Scanner),
    Medium(engine::medium::Scanner),
    High(engine::high::Scanner),
    VeryHigh(engine::very_high::Scanner),
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
    /// for Medium effort, EAN-13 and ignored supplements. Construction is local
    /// and infallible. Input validation happens during scanning.
    #[must_use]
    pub fn new(options: ScannerOptions) -> Self {
        let engine = match options.mode {
            Mode::Low => Engine::Low(engine::low::Scanner::default()),
            Mode::Medium => Engine::Medium(engine::medium::Scanner::default()),
            Mode::High => Engine::High(engine::high::Scanner::default()),
            Mode::VeryHigh => Engine::VeryHigh(engine::very_high::Scanner::default()),
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

    /// Scan with per-image overrides, unread regions, timing and work status.
    ///
    /// Overrides apply only to this call. Raw diagnostics require `debug: true`;
    /// unread geometry is always included. The call blocks without a wall-clock
    /// timeout. False `unfinished` does not guarantee exhaustive scanning.
    ///
    /// # Errors
    /// Rejects invalid pixels. Extended budgets are accepted for every format.
    /// Internal reader failures are errors, never empty results.
    pub fn scan_with_options<'a>(
        &mut self,
        image: impl Into<Image<'a>>,
        options: ScanOptions,
    ) -> Result<ScanResult, Error> {
        self.run(image.into(), options)
    }

    fn run(&mut self, image: Image<'_>, options: ScanOptions) -> Result<ScanResult, Error> {
        let start = Instant::now();
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
                    finish_candidates: options.extended_budget && formats.bits() & 3 != 0,
                };
                let policy = match addons {
                    EanAddOnPolicy::Ignore => selected::formats::EanAddOnPolicy::Ignore,
                    EanAddOnPolicy::Read => selected::formats::EanAddOnPolicy::Read,
                    EanAddOnPolicy::Require => selected::formats::EanAddOnPolicy::Require,
                };
                let raw = $scanner
                    .scan_formats_json_with_addons(input, settings, formats.bits(), policy)
                    .map_err(|error| Error::Engine(error.to_string()))?;
                ScanResult::from_raw(
                    raw,
                    image,
                    self.options.mode,
                    start.elapsed(),
                    options.debug,
                )?
            }};
        }
        let mut result = match &mut self.engine {
            Engine::Low(scanner) => run!(scanner, low),
            Engine::Medium(scanner) => run!(scanner, medium),
            Engine::High(scanner) => run!(scanner, high),
            Engine::VeryHigh(scanner) => run!(scanner, very_high),
        };
        result.elapsed = start.elapsed();
        Ok(result)
    }
}
