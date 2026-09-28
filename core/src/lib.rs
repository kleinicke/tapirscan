//! Production scanner kernels. Select one effort mode at compile time.
//! Algorithm edits belong in this tree; historical recipes are never applied here.
#[cfg(not(any(
    feature = "mode-low",
    feature = "mode-medium",
    feature = "mode-high",
    feature = "mode-very-high"
)))]
compile_error!("Select one core mode; use mode-medium for the default policy.");
#[cfg(any(
    all(feature = "mode-low", feature = "mode-medium"),
    all(feature = "mode-low", feature = "mode-high"),
    all(feature = "mode-low", feature = "mode-very-high"),
    all(feature = "mode-medium", feature = "mode-high"),
    all(feature = "mode-medium", feature = "mode-very-high"),
    all(feature = "mode-high", feature = "mode-very-high")
))]
compile_error!(
    "Core modes are mutually exclusive; pass --no-default-features when selecting a mode."
);
pub mod association;
pub mod bands;
pub mod continuity;
pub mod contrast;
pub mod ean;
#[doc(hidden)]
pub mod numeric;
pub mod oriented;
pub mod profile;
pub mod run_ean;
pub mod run_profile;
pub mod sampling;
pub mod scan;

// Selected find-all scanner and physical-instance reconciliation.
pub mod experiment;
pub mod frame;
pub mod identity;

pub(crate) mod invalid_visual;
pub mod local_signal;
pub mod multi_profile;
pub mod multi_scan;
pub mod region_json;
pub mod region_scan;
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
pub(crate) mod retail_pipeline;
mod scanner_clock;
pub mod shear;

pub mod stripes;
pub mod transition;

pub mod fast_profile;

#[cfg(feature = "mode-very-high")]
mod subpixel;

pub mod qr_frontend;

pub mod qr_grid;

#[cfg(not(feature = "mode-low"))]
mod lowres;

pub mod aztec_frontend;
