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
