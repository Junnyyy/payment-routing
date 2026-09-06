#[allow(dead_code)]
#[path = "../benchmarks/audit.rs"]
mod audit;
#[allow(dead_code)]
#[path = "../benchmarks/fixtures.rs"]
mod fixtures;
use payment_routing::{scalable::*, scheduling::*, simulation::*};

#[test]
fn bounded_witnesses_obey_all_constraints_on_existing_families() {
    for family in [
        "schedule-ties",
        "schedule-contention",
        "schedule-deadline",
        "schedule-reverse-deadline",
        "schedule-multihop-slots",
        "schedule-nonfifo",
        "schedule-demo",
    ] {
        let c = fixtures::static_case(family, 5);
        let result =
            plan_schedule(&c.network, &c.timed, &c.slots, SearchLimits::default()).unwrap();
        if let Some(plan) = result.plan {
            audit::schedule(&c.network, &c.timed, &c.slots, &plan);
            assert_eq!(plan.total_fee_cents, c.expected_fee.unwrap());
        } else {
            assert_eq!(family, "schedule-reverse-deadline");
        }
    }
}
#[test]
fn reservations_avoid_the_pinned_path_trap_and_never_miss_accepted_deadlines() {
    let mut c = fixtures::simulation_case("sim-pinned", 2);
    c.strategy = RoutingStrategy::Reserved {
        limits: Default::default(),
    };
    let mut sim = Simulator::new(c, 42).unwrap();
    sim.advance_ticks(1000).unwrap();
    assert_eq!(sim.metrics().sla_failures, 0);
    assert_eq!(sim.metrics().completed_late, 0);
    assert_eq!(
        sim.metrics().generated,
        sim.metrics().completed + sim.active_payments().len() as u128
    );
}
#[test]
fn tiny_search_budget_fails_closed_and_never_claims_infeasibility() {
    let c = fixtures::static_case("schedule-multihop-slots", 4);
    let limits = SearchLimits {
        max_candidates: 1,
        ..Default::default()
    };
    let r = plan_schedule(&c.network, &c.timed, &c.slots, limits).unwrap();
    assert!(r.plan.is_none());
    assert_eq!(r.diagnostics.truncated_searches, 1);
    assert!(
        optimize_schedule(&c.network, &c.timed, &c.slots)
            .unwrap()
            .is_some()
    );
}
#[test]
fn validates_unused_slots_and_empty_inputs_before_returning() {
    let mut c = fixtures::static_case("schedule-ties", 2);
    c.slots[0].departure_minute = u64::MAX;
    c.slots[0].settlement_minutes = Some(1);
    assert!(plan_schedule(&c.network, &[], &c.slots, Default::default()).is_err());
    assert!(
        plan_schedule(
            &c.network,
            &[],
            &[],
            SearchLimits {
                labels_per_node: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn sparse_recurring_reservations_do_not_expand_a_huge_deadline() {
    let mut c = fixtures::simulation_case("sim-load", 2);
    c.arrivals.min_sla_minutes = u64::MAX;
    c.arrivals.max_sla_minutes = u64::MAX;
    c.services[0].period_minutes = u64::MAX;
    c.services[0].offset_minutes = u64::MAX - 1;
    c.services[0].open_minutes = 1;
    c.strategy = RoutingStrategy::Reserved {
        limits: Default::default(),
    };
    let mut s = Simulator::new(c, 42).unwrap();
    s.advance_ticks(8).unwrap();
    assert!(s.reservation_entries() <= 1);
    assert!(s.routing_diagnostics().candidates < 1000);
}

#[allow(dead_code)]
#[path = "../benchmarks/quality.rs"]
mod quality;
#[test]
fn pair_repair_fixes_measured_scarcity_trap_without_changing_fixed_capacity() {
    let c = quality::case("schedule-mixed", 5, 60);
    let before = plan_schedule(
        &c.network,
        &c.timed,
        &c.slots,
        SearchLimits {
            max_repairs: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(before.plan.unwrap().total_fee_cents, 213);
    let after = plan_schedule(&c.network, &c.timed, &c.slots, Default::default()).unwrap();
    assert!(after.diagnostics.repair_trials <= 16);
    let plan = after.plan.unwrap();
    audit::schedule(&c.network, &c.timed, &c.slots, &plan);
    assert_eq!(plan.total_fee_cents, 127);
    assert_eq!(
        optimize_schedule(&c.network, &c.timed, &c.slots)
            .unwrap()
            .unwrap()
            .total_fee_cents,
        127
    );
}
