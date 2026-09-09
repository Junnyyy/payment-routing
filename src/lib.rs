//! Payment-network data and deterministic fixtures, independent of terminal rendering.

pub mod batch;
pub mod demo;
pub mod evaluation;
pub mod network;
pub mod observation;
pub mod operations;
pub mod routing;
pub mod scalable;
pub mod scheduling;
pub mod simulation;

#[cfg(feature = "search-stats")]
pub mod search_stats;

// Compiles away entirely in ordinary builds, including the value expression.
macro_rules! count_search {
    ($field:ident, $value:expr) => {
        #[cfg(feature = "search-stats")]
        crate::search_stats::record(|stats| {
            stats.$field = stats.$field.saturating_add($value);
        });
    };
}
pub(crate) use count_search;
