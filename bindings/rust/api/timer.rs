use std::time::Duration;

#[cfg(not(target_family = "wasm"))]
pub(crate) struct Timer(std::time::Instant);
#[cfg(target_family = "wasm")]
pub(crate) struct Timer;

impl Timer {
    pub(crate) fn start() -> Self {
        #[cfg(not(target_family = "wasm"))]
        {
            Self(std::time::Instant::now())
        }
        #[cfg(target_family = "wasm")]
        {
            Self
        }
    }

    // JavaScript supplies WASM timing; keep the same interface as the native timer.
    #[cfg_attr(target_family = "wasm", allow(clippy::unused_self))]
    pub(crate) fn elapsed(&self) -> Duration {
        #[cfg(not(target_family = "wasm"))]
        {
            self.0.elapsed()
        }
        #[cfg(target_family = "wasm")]
        {
            Duration::ZERO
        }
    }
}
