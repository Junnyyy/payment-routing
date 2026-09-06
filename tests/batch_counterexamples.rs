#[path = "support/batch_oracle.rs"]
mod oracle;
use oracle::*;
use payment_routing::batch::optimize_batch;

#[test]
fn exhaustive_enumeration_exposes_greedy_and_independent_failures() {
    let (net, payments) = greedy_trap();
    // Three choices for P1 and two for P2: exactly six joint assignments.
    assert_eq!(walks(&net, &payments[0]).len(), 3);
    assert_eq!(walks(&net, &payments[1]).len(), 2);
    let optimal = exhaustive(&net, &payments).unwrap();
    assert_eq!(optimal.fee, 4);
    assert_eq!(
        optimize_batch(&net, &payments)
            .unwrap()
            .unwrap()
            .total_fee_cents,
        optimal.fee
    );
    assert_eq!(greedy(&net, &payments).unwrap().fee, 11);
    let independent: Vec<_> = payments
        .iter()
        .map(|p| walks(&net, p).into_iter().min_by_key(|w| w.fee).unwrap())
        .collect();
    assert_eq!(answer(&independent).fee, 2);
    assert!(!capacity_fits(
        &net,
        &payments.iter().collect::<Vec<_>>(),
        &independent
    ));
}

#[test]
fn greedy_can_report_infeasible_when_the_batch_is_feasible() {
    let (mut net, payments) = greedy_trap();
    net.rails.pop();
    assert_eq!(greedy(&net, &payments), None);
    assert_eq!(exhaustive(&net, &payments).unwrap().fee, 4);
    assert_eq!(
        optimize_batch(&net, &payments)
            .unwrap()
            .unwrap()
            .total_fee_cents,
        4
    );
}

#[test]
fn cheap_multihop_route_must_yield_a_contested_rail_to_an_urgent_payment() {
    let (net, payments) = multihop_trap();
    // The independent bounded-walk oracle also includes cyclic witnesses;
    // production needs only the three/two simple route alternatives.
    assert_eq!(greedy(&net, &payments).unwrap().fee, 11);
    assert_eq!(exhaustive(&net, &payments).unwrap().fee, 4);
    let plan = optimize_batch(&net, &payments).unwrap().unwrap();
    assert_eq!(plan.total_fee_cents, 4);
    assert_eq!(plan.assignments[0].route.hops[0].rail_id, "backup-ad");
    assert_eq!(plan.assignments[1].route.hops[0].rail_id, "cheap-bc");
}
