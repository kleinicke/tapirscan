//! Host budgets for the four compiled modes. Algorithm differences stay in the core.
#[derive(Clone, Copy)]
pub(super) struct Effort {
    pub fit_limit: usize,
    pub proposal_limit: usize,
    #[cfg(not(feature = "low"))]
    pub recovery_directions: usize,
}
pub(super) const SELECTED: Effort = [
    Effort {
        fit_limit: 0,
        proposal_limit: 32,
        #[cfg(not(feature = "low"))]
        recovery_directions: 0,
    },
    Effort {
        fit_limit: 1,
        proposal_limit: 32,
        #[cfg(not(feature = "low"))]
        recovery_directions: 1,
    },
    Effort {
        fit_limit: 4,
        proposal_limit: 32,
        #[cfg(not(feature = "low"))]
        recovery_directions: 2,
    },
    Effort {
        fit_limit: 1,
        proposal_limit: 63,
        #[cfg(not(feature = "low"))]
        recovery_directions: 2,
    },
][super::MODE_ID as usize];
