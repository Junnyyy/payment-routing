//! Opt-in diagnostic counters on the calling thread, reset explicitly by a driver.
//! Saturating counters cannot affect feasibility, ordering, or search termination.
//! Timing must use a build without `search-stats` to exclude counter overhead.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub solver_calls: u64,
    /// Recursive path states, including states immediately pruned.
    pub path_states: u64,
    /// Feasible destination paths; for batch solvers these are retained candidates.
    pub candidates: u64,
    /// Total hops across feasible destination paths, a storage proxy, not bytes.
    pub candidate_hops: u64,
    pub assignment_states: u64,
    pub complete_assignments: u64,
    pub bound_prunes: u64,
    pub deadline_prunes: u64,
    /// Failed joint assignment capacity checks (not path-enumeration checks).
    pub capacity_rejects: u64,
}

thread_local! {
    static COUNTERS: Cell<SearchStats> = Cell::new(SearchStats::default());
}

pub fn reset() {
    COUNTERS.set(SearchStats::default());
}

pub fn snapshot() -> SearchStats {
    COUNTERS.get()
}

pub(crate) fn record(f: impl FnOnce(&mut SearchStats)) {
    let mut value = COUNTERS.get();
    f(&mut value);
    COUNTERS.set(value);
}
