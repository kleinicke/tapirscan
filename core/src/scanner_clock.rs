//! Optional native diagnostics. Portable builds never invoke an OS clock.
#[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
pub(crate) struct Timer(std::time::Instant);
#[cfg(not(all(feature = "native-timing", not(target_arch = "wasm32"))))]
pub(crate) struct Timer;
impl Timer {
    pub(crate) fn now() -> Self {
        #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
        {
            Self(std::time::Instant::now())
        }
        #[cfg(not(all(feature = "native-timing", not(target_arch = "wasm32"))))]
        {
            Self
        }
    }
    #[cfg_attr(
        not(all(feature = "native-timing", not(target_arch = "wasm32"))),
        expect(
            clippy::unused_self,
            reason = "Portable timer has the same instance API as the native elapsed timer but intentionally reports zero."
        )
    )]
    pub(crate) fn ms(&self) -> f64 {
        #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
        {
            self.0.elapsed().as_secs_f64() * 1000.
        }
        #[cfg(not(all(feature = "native-timing", not(target_arch = "wasm32"))))]
        {
            0.
        }
    }
}
