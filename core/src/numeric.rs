//! Explicit numeric semantics used by scanner geometry and diagnostic tools: the
//! multi-format crate's helpers, so both crates convert identically.
//!
//! Geometry stays in its original floating-point representation. Float-to-integer
//! conversions truncate toward zero and saturate (NaN becomes zero), as required
//! by Rust's cast semantics. Integer extraction keeps low bits where requested.
//! These functions do not add input limits, validation failures or rounding steps.

pub use barcode_multiformat::numeric::*;
