#[path = "support/batch_oracle.rs"]
mod oracle;
use oracle::*;
use payment_routing::{batch::optimize_batch, demo::demo_network, routing::route_payment};

#[test]
fn budget_is_shared_across_directions_and_distinct_member_pairs() {
    let mut net = network(
        &["A", "B", "C", "D"],
        vec![rail("shared", &["A", "B", "C", "D"], 0, 0, Some(2))],
    );
    let payments = vec![
        payment("P1", "A", "B", 1),
        payment("P2", "C", "D", 1),
        payment("P3", "B", "A", 1),
    ];
    let plan = optimize_batch(&net, &payments[..2]).unwrap().unwrap();
    assert_eq!(plan.rail_usage[0].principal_cents, 2);
    assert_eq!(optimize_batch(&net, &payments), Ok(None));
    net.rails[0].batch_capacity_cents = None;
    assert_eq!(
        optimize_batch(&net, &payments).unwrap().unwrap().rail_usage[0].principal_cents,
        3
    );
    net.rails[0].batch_capacity_cents = Some(0);
    assert_eq!(net.validate(), Ok(()));
    assert_eq!(optimize_batch(&net, &payments[..1]), Ok(None));
}

#[test]
fn multihop_usage_counts_each_rail_excludes_fees_and_keeps_ceilings_distinct() {
    let mut net = network(
        &["A", "B", "C"],
        vec![
            rail("ab", &["A", "B"], 500, 1, Some(200)),
            rail("bc", &["B", "C"], 700, 2, Some(100)),
        ],
    );
    net.rails[0].max_amount_cents = Some(100);
    let payments = vec![payment("P1", "A", "C", 100), payment("P2", "B", "A", 100)];
    let plan = optimize_batch(&net, &payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, 1700);
    assert_eq!(plan.total_settlement_minutes, 4);
    assert_eq!(
        plan.rail_usage
            .iter()
            .map(|r| r.principal_cents)
            .collect::<Vec<_>>(),
        [200, 100]
    );
    net.rails[0].batch_capacity_cents = Some(199);
    assert_eq!(optimize_batch(&net, &payments), Ok(None));
    net.rails[0].batch_capacity_cents = Some(200);
    net.rails[1].batch_capacity_cents = Some(99);
    assert_eq!(optimize_batch(&net, &payments), Ok(None));
    net.rails[1].batch_capacity_cents = Some(100);
    net.rails[0].max_amount_cents = Some(99);
    assert_eq!(optimize_batch(&net, &payments), Ok(None));
}

#[test]
fn tied_batch_prefers_global_rank_even_when_individual_latency_order_differs() {
    let mut net = network(
        &["A", "B"],
        vec![
            rail("z-fast", &["A", "B"], 0, 0, Some(1)),
            rail("a-slow", &["A", "B"], 0, 1, None),
        ],
    );
    let mut payments = vec![payment("P1", "A", "B", 1), payment("P2", "A", "B", 1)];
    let expected = optimize_batch(&net, &payments).unwrap().unwrap();
    assert_eq!(expected.total_settlement_minutes, 1);
    assert_eq!(expected.assignments[0].route.hops[0].rail_id, "a-slow");
    assert_eq!(expected.assignments[1].route.hops[0].rail_id, "z-fast");
    assert_eq!(exhaustive(&net, &payments).unwrap().paths[0][0].0, "a-slow");
    net.institutions.reverse();
    net.rails.reverse();
    payments.reverse();
    for r in &mut net.rails {
        r.participants.reverse();
    }
    assert_eq!(optimize_batch(&net, &payments), Ok(Some(expected)));
}

#[test]
fn deadline_can_reserve_fast_capacity_for_the_payment_that_needs_it() {
    let net = network(
        &["A", "B"],
        vec![
            rail("instant", &["A", "B"], 1, 0, Some(1)),
            rail("slow", &["A", "B"], 3, 2, None),
        ],
    );
    let mut payments = vec![payment("P1", "A", "B", 1), payment("P2", "B", "A", 1)];
    payments[0].max_delivery_minutes = Some(2);
    payments[1].max_delivery_minutes = Some(0);
    assert_eq!(greedy(&net, &payments), None);
    let plan = optimize_batch(&net, &payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, 4);
    assert_eq!(plan.assignments[0].route.hops[0].rail_id, "slow");
    assert_eq!(plan.assignments[1].route.hops[0].rail_id, "instant");
    payments[0].max_delivery_minutes = Some(1);
    assert_eq!(optimize_batch(&net, &payments), Ok(None));
}

#[test]
fn more_expensive_faster_prefix_can_be_required_inside_a_batch() {
    let net = network(
        &["A", "B", "C"],
        vec![
            rail("cheap", &["A", "B"], 1, 9, None),
            rail("fast", &["A", "B"], 5, 3, Some(2)),
            rail("bc", &["B", "C"], 1, 2, Some(2)),
        ],
    );
    let mut payments = vec![payment("P1", "A", "C", 1), payment("P2", "C", "A", 1)];
    for p in &mut payments {
        p.max_delivery_minutes = Some(5);
    }
    let plan = optimize_batch(&net, &payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, 12);
    assert_eq!(plan.total_settlement_minutes, 10);
    assert_eq!(exhaustive(&net, &payments).unwrap().fee, 12);
}

#[test]
fn batch_validation_rejects_duplicates_and_all_malformed_instructions_before_search() {
    let (mut net, mut payments) = greedy_trap();
    payments[1].id = payments[0].id.clone();
    assert!(
        optimize_batch(&net, &payments)
            .unwrap_err()
            .to_string()
            .contains("duplicate batch payment")
    );
    payments[1].id = "P2".into();
    net.rails.clear(); // First payment unreachable must not hide invalid second payment.
    for malformed in [
        payment("P2", "missing", "B", 1),
        payment("P2", "A", "missing", 1),
        payment("P2", "A", "A", 1),
        payment("P2", "A", "B", 0),
        payment(" P2", "A", "B", 1),
        payment("", "A", "B", 1),
    ] {
        payments[1] = malformed;
        assert!(optimize_batch(&net, &payments).is_err());
    }
    net.payments = vec![payment("bad", "A", "missing", 1)];
    assert!(optimize_batch(&net, &[]).is_err());
}

#[test]
fn malformed_network_is_rejected_even_for_an_empty_batch() {
    let (net, _) = greedy_trap();
    let mut bad = net.clone();
    bad.rails[0].max_amount_cents = Some(0);
    assert!(optimize_batch(&bad, &[]).is_err());
    let mut bad = net.clone();
    bad.rails[0].participants.push("missing".into());
    assert!(optimize_batch(&bad, &[]).is_err());
    let mut bad = net.clone();
    bad.rails.push(net.rails[0].clone());
    assert!(optimize_batch(&bad, &[]).is_err());
}

#[test]
fn empty_batches_and_unreachable_payments_have_distinct_results() {
    let empty = network(&[], vec![]);
    let plan = optimize_batch(&empty, &[]).unwrap().unwrap();
    assert!(plan.assignments.is_empty());
    assert!(plan.rail_usage.is_empty());
    assert_eq!(
        (plan.total_fee_cents, plan.total_settlement_minutes),
        (0, 0)
    );
    let net = network(
        &["A", "B", "C"],
        vec![rail("ab", &["A", "B"], 0, 0, Some(0))],
    );
    assert_eq!(
        optimize_batch(&net, &[]).unwrap().unwrap().rail_usage[0].principal_cents,
        0
    );
    assert_eq!(optimize_batch(&net, &[payment("P", "A", "C", 1)]), Ok(None));
}

#[test]
fn only_explicit_batch_consumes_capacity_and_inputs_remain_unchanged() {
    let mut net = network(&["A", "B"], vec![rail("ab", &["A", "B"], 0, 0, Some(1))]);
    net.payments = vec![payment("stored", "A", "B", 1)];
    let batch = vec![payment("external", "B", "A", 1)];
    let before = net.clone();
    let expected = optimize_batch(&net, &batch).unwrap().unwrap();
    assert_eq!(optimize_batch(&net, &batch), Ok(Some(expected.clone())));
    assert_eq!(net, before);
    for i in &mut net.institutions {
        i.opening_balance_cents = u64::MAX;
    }
    assert_eq!(optimize_batch(&net, &batch), Ok(Some(expected)));
    let mut together = batch;
    together.extend(net.payments.clone());
    assert_eq!(optimize_batch(&net, &together), Ok(None));
}

#[test]
fn wide_principal_fees_and_latency_do_not_wrap_or_saturate() {
    let mut net = network(
        &["A", "B", "C"],
        vec![
            rail("ab", &["A", "B"], u64::MAX, u32::MAX, None),
            rail("bc", &["B", "C"], u64::MAX, u32::MAX, None),
        ],
    );
    let payments = vec![
        payment("P1", "A", "C", u64::MAX),
        payment("P2", "C", "A", u64::MAX),
    ];
    let plan = optimize_batch(&net, &payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, u128::from(u64::MAX) * 4);
    assert_eq!(plan.total_settlement_minutes, u128::from(u32::MAX) * 4);
    assert_eq!(plan.rail_usage[0].principal_cents, u128::from(u64::MAX) * 2);
    assert_eq!(plan.rail_usage[1].principal_cents, u128::from(u64::MAX) * 2);
    net.rails[0].batch_capacity_cents = Some(u64::MAX);
    assert_eq!(optimize_batch(&net, &payments), Ok(None));
    assert!(optimize_batch(&net, &payments[..1]).unwrap().is_some());
}

#[test]
fn unlimited_demo_matches_independent_routes_and_preserves_the_viewer_data() {
    let net = demo_network();
    let before = net.clone();
    let plan = optimize_batch(&net, &net.payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, 60);
    assert_eq!(plan.assignments.len(), net.payments.len());
    for (p, a) in net.payments.iter().zip(&plan.assignments) {
        assert_eq!(a.payment_id, p.id);
        assert_eq!(Some(a.route.clone()), route_payment(&net, p).unwrap());
    }
    assert_eq!(net, before);
    assert_eq!(
        plan.rail_usage
            .iter()
            .find(|r| r.rail_id == "ACH")
            .unwrap()
            .principal_cents,
        22_500_150
    );
}
