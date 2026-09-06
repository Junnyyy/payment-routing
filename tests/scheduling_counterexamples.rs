#[path = "support/batch_oracle.rs"]
mod fixtures;
#[path = "support/scheduling_oracle.rs"]
mod oracle;
use fixtures::{network, payment, rail};
use oracle::*;

#[test]
fn delaying_flexible_payment_beats_locally_cheapest_scheduling() {
    let mut net = network(
        &["A", "B"],
        vec![
            rail("cheap", &["A", "B"], 1, 0, None),
            rail("fallback", &["A", "B"], 10, 0, None),
        ],
    );
    let p = vec![
        timed(payment("P1", "A", "B", 1), 0, Some(1)),
        timed(payment("P2", "B", "A", 1), 0, Some(0)),
    ];
    let mut slots = vec![
        slot("cheap", 0, None, None, Some(1)),
        slot("cheap", 1, None, None, Some(1)),
        slot("fallback", 0, None, None, None),
        slot("fallback", 1, None, None, None),
    ];
    let ordered: Vec<_> = p.iter().collect();
    assert_eq!(walks(&net, &p[0], &slots).len(), 4);
    assert_eq!(walks(&net, &p[1], &slots).len(), 2);
    assert_eq!(product(&net, &ordered, &slots).len(), 8);
    assert_eq!(greedy(&net, &p, &slots).unwrap().fee, 11);
    let independent: Vec<_> = p
        .iter()
        .map(|p| {
            walks(&net, p, &slots)
                .into_iter()
                .min_by_key(|w| (w.fee, w.elapsed))
                .unwrap()
        })
        .collect();
    assert_eq!(rank(&independent).fee, 2);
    assert!(!fits(&net, &ordered, &slots, &independent));
    let plan = verify(&net, &p, &slots).unwrap();
    assert_eq!((plan.total_fee_cents, plan.total_elapsed_minutes), (2, 1));
    assert_eq!(plan.assignments[0].route.hops[0].departure_minute, 1);
    assert_eq!(plan.assignments[1].route.hops[0].departure_minute, 0);
    let no_wait = product(&net, &ordered, &slots)
        .into_iter()
        .filter(|paths| paths.iter().all(|w| w.hops[0].3 == 0))
        .filter(|paths| fits(&net, &ordered, &slots, paths))
        .map(|p| rank(&p))
        .min()
        .unwrap();
    assert_eq!(no_wait.fee, 11);
    slots.retain(|s| s.rail_id == "cheap");
    net.rails.pop();
    assert_eq!(greedy(&net, &p, &slots), None);
    assert_eq!(verify(&net, &p, &slots).unwrap().total_fee_cents, 2);
    // Slot budgets don't replenish the whole-batch budget.
    net.rails[0].batch_capacity_cents = Some(1);
    assert_eq!(verify(&net, &p, &slots), None);
}

#[test]
fn waiting_for_a_cheaper_prefix_misses_the_only_connection() {
    let net = network(
        &["A", "B", "C"],
        vec![
            rail("ab", &["A", "B"], 4, 1, None),
            rail("bc", &["B", "C"], 1, 1, None),
        ],
    );
    let p = vec![timed(payment("P", "A", "C", 1), 0, Some(4))];
    let mut slots = vec![
        slot("ab", 0, None, None, None),
        slot("ab", 2, Some(1), None, None),
        slot("bc", 1, None, None, None),
    ];
    assert_eq!(verify(&net, &p, &slots).unwrap().total_fee_cents, 5);
    let mut late = p.clone();
    late[0].earliest_execution_minute = 2;
    assert_eq!(verify(&net, &late, &slots), None);
    // Reopening the downstream connection makes the delayed cheap path valid.
    slots[2].departure_minute = 3;
    let plan = verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.total_fee_cents, 2);
    assert_eq!(plan.assignments[0].route.hops[0].departure_minute, 2);
}

#[test]
fn later_departure_with_shorter_latency_can_be_the_only_feasible_prefix() {
    let net = network(
        &["A", "B", "C"],
        vec![
            rail("ab", &["A", "B"], 1, 5, None),
            rail("bc", &["B", "C"], 1, 1, None),
        ],
    );
    let p = vec![timed(payment("P", "A", "C", 1), 0, Some(4))];
    let slots = vec![
        slot("ab", 0, None, None, None),
        slot("ab", 1, Some(3), Some(1), None),
        slot("bc", 3, None, None, None),
    ];
    let plan = verify(&net, &p, &slots).unwrap();
    assert_eq!((plan.total_fee_cents, plan.total_elapsed_minutes), (4, 4));
    assert_eq!(plan.assignments[0].route.hops[0].departure_minute, 1);
    assert_eq!(plan.assignments[0].route.hops[0].arrival_minute, 2);
    assert_eq!(plan.assignments[0].route.hops[1].departure_minute, 3);
    let mut p = p;
    p[0].payment.max_delivery_minutes = Some(3);
    assert_eq!(verify(&net, &p, &slots), None); // Transit=2, but elapsed with waits=4.
}

#[test]
fn choosing_the_cheapest_departure_can_steal_a_later_arrivals_only_capacity() {
    let net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 0, None)]);
    let p = vec![
        timed(payment("P1", "A", "B", 1), 0, Some(1)),
        timed(payment("P2", "A", "B", 1), 1, Some(2)),
    ];
    let slots = vec![
        slot("r", 0, Some(3), None, Some(1)),
        slot("r", 1, Some(1), None, Some(1)),
        slot("r", 2, Some(10), None, Some(1)),
    ];
    assert_eq!(greedy(&net, &p, &slots).unwrap().fee, 11);
    let plan = verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.total_fee_cents, 4);
    assert_eq!(plan.assignments[0].route.hops[0].departure_minute, 0);
    assert_eq!(plan.assignments[1].route.hops[0].departure_minute, 1);
}

#[test]
fn joint_routing_can_require_an_expensive_faster_prefix_to_free_a_later_slot() {
    let net = network(
        &["A", "B", "C"],
        vec![
            rail("ab", &["A", "B"], 0, 1, None),
            rail("bc", &["B", "C"], 1, 0, None),
        ],
    );
    let p = vec![
        timed(payment("P1", "A", "C", 1), 0, Some(2)),
        timed(payment("P2", "C", "B", 1), 2, Some(2)),
    ];
    let slots = vec![
        slot("ab", 0, Some(3), None, None),
        slot("ab", 1, Some(0), None, None),
        slot("bc", 1, None, None, Some(1)),
        slot("bc", 2, None, None, Some(1)),
    ];
    assert_eq!(greedy(&net, &p, &slots), None);
    let plan = verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.total_fee_cents, 5);
    assert_eq!(
        plan.assignments[0]
            .route
            .hops
            .iter()
            .map(|h| h.departure_minute)
            .collect::<Vec<_>>(),
        [0, 1]
    );
}
