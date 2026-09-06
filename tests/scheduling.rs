#[path = "support/batch_oracle.rs"]
mod fixtures;
use fixtures::{network, payment, rail};
#[path = "support/scheduling_oracle.rs"]
mod oracle;
use payment_routing::{
    batch::optimize_batch,
    demo::demo_network,
    scheduling::{RailDeparture, TimedPayment, optimize_schedule, validate_schedule},
};

fn timed(id: &str, release: u64, deadline: Option<u64>) -> TimedPayment {
    TimedPayment {
        payment: payment(id, "A", "B", 1),
        earliest_execution_minute: release,
        deadline_minute: deadline,
    }
}

fn slot(id: &str, minute: u64) -> RailDeparture {
    RailDeparture {
        rail_id: id.into(),
        departure_minute: minute,
        fee_cents: None,
        settlement_minutes: None,
        capacity_cents: None,
    }
}

#[test]
fn validates_the_whole_input_before_feasibility_or_empty_batch_shortcuts() {
    let mut net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 0, None)]);
    assert!(validate_schedule(&net, &[timed("P", 3, Some(2))], &[]).is_ok());
    assert!(validate_schedule(&net, &[], &[slot("missing", 0)]).is_err());
    assert!(validate_schedule(&net, &[], &[slot("r", 0), slot("r", 0)]).is_err());
    assert!(validate_schedule(&net, &[], &[slot("r", 0), slot("r", 1)]).is_ok());
    assert!(validate_schedule(&net, &[timed("P", 0, None), timed("P", 1, None)], &[]).is_err());
    let mut bad = timed("P2", 0, None);
    bad.payment.receiver = "missing".into();
    assert!(validate_schedule(&net, &[timed("P1", 0, None), bad], &[]).is_err());
    net.payments.push(payment("bad", "missing", "B", 1));
    assert!(validate_schedule(&net, &[], &[]).is_err());
    net.payments.clear();
    net.rails[0].max_amount_cents = Some(0);
    assert!(validate_schedule(&net, &[], &[]).is_err());
}

#[test]
fn timestamps_are_checked_without_wrapping_even_on_closed_or_unused_slots() {
    let mut net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 1, None)]);
    net.rails[0].available = false;
    assert!(validate_schedule(&net, &[], &[slot("r", u64::MAX)]).is_err());
    let mut last = slot("r", u64::MAX);
    last.settlement_minutes = Some(0);
    last.capacity_cents = Some(0);
    assert!(validate_schedule(&net, &[], &[last]).is_ok());
    assert!(validate_schedule(&net, &[], &[slot("r", u64::MAX - 1)]).is_ok());
}

#[test]
fn release_and_both_deadlines_are_inclusive_and_waiting_consumes_delivery_budget() {
    let net = network(&["A", "B"], vec![rail("r", &["A", "B"], 1, 1, None)]);
    let mut p = timed("P", 2, Some(3));
    p.payment.max_delivery_minutes = Some(1);
    let mut slots = vec![slot("r", 0), slot("r", 2)];
    assert_eq!(
        oracle::verify(&net, &[p.clone()], &slots)
            .unwrap()
            .assignments[0]
            .route
            .hops[0]
            .arrival_minute,
        3
    );
    p.deadline_minute = Some(2);
    assert_eq!(oracle::verify(&net, &[p.clone()], &slots), None);
    p.deadline_minute = None;
    slots[1].departure_minute = 3;
    assert_eq!(oracle::verify(&net, &[p.clone()], &slots), None);
    p.payment.max_delivery_minutes = Some(2);
    assert_eq!(
        oracle::verify(&net, &[p.clone()], &slots)
            .unwrap()
            .total_elapsed_minutes,
        2
    );
    p.earliest_execution_minute = 4;
    assert_eq!(oracle::verify(&net, &[p.clone()], &slots), None);
    p.deadline_minute = Some(3);
    assert_eq!(oracle::verify(&net, &[p], &slots), None);
}

#[test]
fn static_gates_and_both_principal_budgets_remain_hard_constraints() {
    let mut net = network(&["A", "B"], vec![rail("r", &["A", "B"], 1000, 1, Some(2))]);
    let mut p = timed("P", 0, None);
    p.payment.amount_cents = 2;
    let mut s = slot("r", 0);
    s.capacity_cents = Some(2);
    net.rails[0].max_amount_cents = Some(2);
    assert_eq!(
        oracle::verify(&net, &[p.clone()], &[s.clone()])
            .unwrap()
            .total_fee_cents,
        1000
    );
    net.rails[0].available = false;
    s.fee_cents = Some(0);
    s.settlement_minutes = Some(0);
    assert_eq!(oracle::verify(&net, &[p.clone()], &[s.clone()]), None);
    net.rails[0].available = true;
    net.rails[0].max_amount_cents = Some(1);
    assert_eq!(oracle::verify(&net, &[p.clone()], &[s.clone()]), None);
    net.rails[0].max_amount_cents = None;
    net.rails[0].batch_capacity_cents = Some(1);
    assert_eq!(oracle::verify(&net, &[p.clone()], &[s.clone()]), None);
    net.rails[0].batch_capacity_cents = None;
    s.capacity_cents = Some(1);
    assert_eq!(oracle::verify(&net, &[p.clone()], &[s.clone()]), None);
    s.capacity_cents = Some(0);
    assert_eq!(oracle::verify(&net, &[p.clone()], &[s.clone()]), None);
    s.capacity_cents = None;
    assert!(oracle::verify(&net, &[p], &[s]).is_some());
}

#[test]
fn departure_budgets_share_all_pairs_and_directions_and_do_not_model_occupancy() {
    let net = network(
        &["A", "B", "C", "D"],
        vec![rail("r", &["A", "B", "C", "D"], 2, 2, None)],
    );
    let p = vec![
        oracle::timed(payment("P1", "A", "B", 1), 0, Some(3)),
        oracle::timed(payment("P2", "C", "D", 1), 0, Some(3)),
    ];
    let slots = vec![
        oracle::slot("r", 0, None, None, Some(1)),
        oracle::slot("r", 1, None, None, Some(1)),
    ];
    let plan = oracle::verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.total_elapsed_minutes, 5); // Overlapping [0,2] and [1,3] transfers.
    assert_eq!(plan.rail_usage[0].principal_cents, 2);
    assert_eq!(oracle::verify(&net, &p, &slots[..1]), None);
    let mut p = p;
    p[1].payment.sender = "B".into();
    p[1].payment.receiver = "A".into();
    assert_eq!(oracle::verify(&net, &p, &slots[..1]), None);
    assert_eq!(
        oracle::verify(&net, &p, &slots)
            .unwrap()
            .total_elapsed_minutes,
        5
    );
}

#[test]
fn full_rank_ties_and_results_are_independent_of_every_input_order() {
    let mut net = network(
        &["A", "B"],
        vec![
            rail("z-fast", &["A", "B"], 0, 0, None),
            rail("a-slow", &["A", "B"], 0, 0, None),
        ],
    );
    let mut p = vec![timed("P1", 0, None), timed("P2", 0, None)];
    let mut slots = vec![
        oracle::slot("z-fast", 0, None, None, Some(1)),
        oracle::slot("a-slow", 1, None, None, Some(1)),
    ];
    let expected = oracle::verify(&net, &p, &slots).unwrap();
    assert_eq!(expected.total_elapsed_minutes, 1);
    assert_eq!(
        expected.assignments[0].route.hops[0].transfer.rail_id,
        "a-slow"
    );
    net.institutions.reverse();
    net.rails.reverse();
    p.reverse();
    slots.reverse();
    for r in &mut net.rails {
        r.participants.reverse();
    }
    assert_eq!(oracle::verify(&net, &p, &slots), Some(expected));
    net.rails.truncate(1); // a-slow remains: timestamps decide tied single-rail assignment.
    slots = vec![
        oracle::slot("a-slow", 1, None, None, Some(1)),
        oracle::slot("a-slow", 0, None, None, Some(1)),
    ];
    let plan = oracle::verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.assignments[0].route.hops[0].departure_minute, 0);
}

#[test]
fn wide_money_latency_and_sparse_absolute_times_never_wrap_or_expand_ticks() {
    let net = network(
        &["A", "B", "C"],
        vec![
            rail("ab", &["A", "B"], u64::MAX, u32::MAX, None),
            rail("bc", &["B", "C"], u64::MAX, u32::MAX, None),
        ],
    );
    let p = vec![
        oracle::timed(payment("P1", "A", "C", u64::MAX), 0, None),
        oracle::timed(payment("P2", "A", "C", u64::MAX), 0, None),
    ];
    let mut slots = vec![slot("ab", 0), slot("bc", u64::from(u32::MAX))];
    let plan = oracle::verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.total_fee_cents, u128::from(u64::MAX) * 4);
    assert_eq!(plan.total_elapsed_minutes, u128::from(u32::MAX) * 4);
    assert!(
        plan.rail_usage
            .iter()
            .all(|u| u.principal_cents == u128::from(u64::MAX) * 2)
    );
    slots[1].capacity_cents = Some(u64::MAX);
    assert_eq!(oracle::verify(&net, &p, &slots), None);
    assert!(oracle::verify(&net, &p[..1], &slots).is_some());
    let net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 0, None)]);
    let p = vec![timed("P1", 0, None), timed("P2", 0, None)];
    let plan = oracle::verify(&net, &p, &[slot("r", u64::MAX)]).unwrap();
    assert_eq!(plan.total_elapsed_minutes, u128::from(u64::MAX) * 2);
    let mut p = timed("P", u64::MAX, Some(u64::MAX));
    p.payment.max_delivery_minutes = Some(0);
    assert_eq!(
        oracle::verify(&net, &[p], &[slot("r", u64::MAX)])
            .unwrap()
            .total_elapsed_minutes,
        0
    );
}

#[test]
fn free_waiting_replaces_cycles_and_zero_latency_connections_are_allowed() {
    let net = network(
        &["A", "B", "C", "D"],
        vec![
            rail("ab", &["A", "B"], 0, 0, None),
            rail("ad", &["A", "D"], 0, 0, None),
            rail("dc", &["D", "C"], 0, 0, None),
        ],
    );
    let p = vec![oracle::timed(payment("P", "A", "D", 1), 0, Some(2))];
    let slots = vec![slot("ab", 0), slot("ab", 1), slot("ad", 2), slot("dc", 2)];
    let walks = oracle::walks(&net, &p[0], &slots);
    assert!(
        walks
            .iter()
            .any(|w| w.hops.len() == 3 && w.hops[1].2 == "A")
    );
    assert_eq!(
        oracle::verify(&net, &p, &slots).unwrap().assignments[0]
            .route
            .hops
            .len(),
        1
    );
    let p = vec![oracle::timed(payment("P", "A", "C", 1), 2, Some(2))];
    let plan = oracle::verify(&net, &p, &slots).unwrap();
    assert_eq!(plan.assignments[0].route.hops.len(), 2);
    assert_eq!(plan.total_elapsed_minutes, 0);
}

#[test]
fn empty_missing_timetable_external_instructions_and_validation_stay_distinct() {
    let mut net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 0, Some(1))]);
    let slots = vec![slot("r", 0)];
    net.payments.push(payment("stored", "A", "B", 1));
    assert_eq!(
        oracle::verify(&net, &[], &slots).unwrap().departure_usage[0].principal_cents,
        0
    );
    let p = vec![timed("external", 0, None)];
    assert!(oracle::verify(&net, &p, &slots).is_some());
    assert_eq!(oracle::verify(&net, &p, &[]), None);
    let mut bad = p.clone();
    bad[0].payment.amount_cents = 0;
    assert!(optimize_schedule(&net, &bad, &[]).is_err());
    assert!(optimize_schedule(&net, &[], &[slot("unknown", 0)]).is_err());
    assert!(optimize_schedule(&net, &[], &[slot("r", 0), slot("r", 0)]).is_err());
    for i in &mut net.institutions {
        i.opening_balance_cents = u64::MAX;
    }
    assert_eq!(oracle::verify(&net, &p, &slots).unwrap().total_fee_cents, 0);
    net.payments[0].receiver = "missing".into();
    assert!(optimize_schedule(&net, &[], &slots).is_err());
    let empty = network(&[], vec![]);
    let plan = oracle::verify(&empty, &[], &[]).unwrap();
    assert_eq!((plan.total_fee_cents, plan.total_elapsed_minutes), (0, 0));
    assert!(plan.rail_usage.is_empty() && plan.departure_usage.is_empty());
}

#[test]
fn unchanged_twelve_payment_demo_has_an_exact_finite_timetable_plan() {
    let net = demo_network();
    let before = net.clone();
    let payments: Vec<_> = net
        .payments
        .iter()
        .map(|p| oracle::timed(p.clone(), 0, None))
        .collect();
    // Explicit finite timetable: each rail opens at minute zero. ACH connects all
    // members directly. Every payment's cost lower bound is the 5-cent ACH fee.
    let slots: Vec<_> = net.rails.iter().map(|r| slot(&r.id, 0)).collect();
    let plan = optimize_schedule(&net, &payments, &slots).unwrap().unwrap();
    let static_plan = optimize_batch(&net, &net.payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, 60);
    assert_eq!(plan.total_fee_cents, static_plan.total_fee_cents);
    assert_eq!(plan.total_elapsed_minutes, 12 * 1440);
    assert_eq!(plan.rail_usage, static_plan.rail_usage);
    assert_eq!(plan.assignments.len(), 12);
    for (p, a) in payments.iter().zip(&plan.assignments) {
        assert_eq!(p.payment.id, a.payment_id);
        assert_eq!(a.route.hops.len(), 1);
        let h = &a.route.hops[0];
        assert_eq!(h.transfer.rail_id, "ACH");
        assert_eq!(
            (&h.transfer.sender, &h.transfer.receiver),
            (&p.payment.sender, &p.payment.receiver)
        );
        assert_eq!(
            (h.departure_minute, h.arrival_minute, h.fee_cents),
            (0, 1440, 5)
        );
    }
    assert_eq!(net, before);
}
