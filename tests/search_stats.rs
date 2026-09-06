#![cfg(feature = "search-stats")]

#[path = "support/batch_oracle.rs"]
mod fixtures;

use payment_routing::{batch::optimize_batch, search_stats};

#[test]
fn counters_explain_a_complete_binary_assignment_tree_and_are_thread_local() {
    let net = fixtures::network(
        &["A", "B"],
        vec![
            fixtures::rail("a", &["A", "B"], 0, 0, None),
            fixtures::rail("b", &["A", "B"], 0, 0, None),
        ],
    );
    let payments: Vec<_> = (0..3)
        .map(|i| fixtures::payment(&format!("P{i}"), "A", "B", 1))
        .collect();
    search_stats::reset();
    let plan = optimize_batch(&net, &payments).unwrap().unwrap();
    let stats = search_stats::snapshot();
    assert_eq!(plan.total_fee_cents, 0);
    assert_eq!(stats.solver_calls, 1);
    assert_eq!(stats.path_states, 9);
    assert_eq!(stats.candidates, 6);
    assert_eq!(stats.candidate_hops, 6);
    assert_eq!(stats.assignment_states, 15);
    assert_eq!(stats.complete_assignments, 8);
    assert_eq!(stats.bound_prunes, 0);
    std::thread::spawn(|| assert_eq!(search_stats::snapshot(), Default::default()))
        .join()
        .unwrap();
    assert_eq!(search_stats::snapshot(), stats);
    search_stats::reset();
    assert_eq!(search_stats::snapshot(), Default::default());
}
