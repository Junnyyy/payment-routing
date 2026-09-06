#[path = "../benchmarks/audit.rs"]
mod audit;
#[path = "support/batch_oracle.rs"]
mod batch_oracle;
#[path = "../benchmarks/fixtures.rs"]
mod fixtures;
#[path = "support/scheduling_oracle.rs"]
mod schedule_oracle;
use payment_routing::{
    batch::optimize_batch, routing::route_payment, scheduling::optimize_schedule,
};

#[test]
fn constructed_cases_match_independent_oracles_including_full_tie_rank() {
    for family in [
        "batch-volume",
        "batch-ties",
        "batch-latency-ties",
        "batch-choices",
        "batch-scarce",
        "batch-infeasible",
        "batch-ceiling",
        "batch-trap",
        "batch-trap-infeasible-greedy",
        "batch-density",
    ] {
        let c = fixtures::static_case(family, 3);
        let plan = optimize_batch(&c.network, &c.payments).unwrap();
        let oracle = batch_oracle::exhaustive(&c.network, &c.payments);
        assert_eq!(
            plan.as_ref().map(|p| p.total_fee_cents),
            oracle.as_ref().map(|p| p.fee),
            "{family}"
        );
        if let Some(p) = &plan {
            audit::batch(&c.network, &c.payments, p);
        }
        if let Some(expected) = c.expected_fee {
            assert_eq!(plan.unwrap().total_fee_cents, expected);
        }
        if c.expected_infeasible {
            assert!(oracle.is_none());
        }
    }
    for family in [
        "schedule-ties",
        "schedule-latency-ties",
        "schedule-reverse-deadline",
        "schedule-multihop-slots",
        "schedule-contention",
        "schedule-deadline",
        "schedule-slots",
        "schedule-nonfifo",
    ] {
        let c = fixtures::static_case(family, 3);
        let plan = schedule_oracle::verify(&c.network, &c.timed, &c.slots);
        if let Some(p) = plan {
            audit::schedule(&c.network, &c.timed, &c.slots, &p);
            assert_eq!(p.total_fee_cents, c.expected_fee.unwrap());
        }
    }
}

#[test]
fn adversarial_controls_have_known_quality_and_feasibility() {
    for family in [
        "single-positive",
        "single-zero",
        "single-deadline",
        "single-disconnected",
        "single-chain",
    ] {
        let c = fixtures::static_case(family, 5);
        let p = route_payment(&c.network, &c.payments[0]).unwrap();
        assert_eq!(p.as_ref().map(|r| r.total_fee_cents), c.expected_fee);
        if let Some(p) = p {
            audit::route(&c.network, &c.payments[0], &p);
        }
    }
    let c = fixtures::static_case("batch-trap", 2);
    assert_eq!(
        batch_oracle::greedy(&c.network, &c.payments).unwrap().fee,
        11
    );
    let c = fixtures::static_case("batch-trap-infeasible-greedy", 2);
    assert!(batch_oracle::greedy(&c.network, &c.payments).is_none());
    assert_eq!(
        batch_oracle::exhaustive(&c.network, &c.payments)
            .unwrap()
            .fee,
        4
    );
}

#[test]
fn captured_windows_replay_and_obey_the_independent_timetable_oracle() {
    let (c, p) = fixtures::capture_window(1, 42);
    assert_eq!(format!("{c:?}{p}"), {
        let (c, p) = fixtures::capture_window(1, 42);
        format!("{c:?}{p}")
    });
    assert_ne!(
        format!("{:?}", c.payments),
        format!("{:?}", fixtures::capture_window(1, 43).0.payments)
    );
    assert!(c.timed.iter().all(|p| p.earliest_execution_minute == 8));
    let plan = schedule_oracle::verify(&c.network, &c.timed, &c.slots).unwrap();
    audit::schedule(&c.network, &c.timed, &c.slots, &plan);
    let ordinary = optimize_schedule(&c.network, &c.timed, &c.slots)
        .unwrap()
        .unwrap();
    assert_eq!(plan, ordinary);
}

#[test]
fn witness_audit_rejects_a_corrupted_objective_and_budget() {
    let c = fixtures::static_case("batch-scarce", 3);
    let mut p = optimize_batch(&c.network, &c.payments).unwrap().unwrap();
    p.total_fee_cents += 1;
    assert!(std::panic::catch_unwind(|| audit::batch(&c.network, &c.payments, &p)).is_err());
    p.total_fee_cents -= 1;
    let mut n = c.network;
    n.rails[0].batch_capacity_cents = Some(0);
    assert!(std::panic::catch_unwind(|| audit::batch(&n, &c.payments, &p)).is_err());
}

#[test]
fn future_closed_hop_is_a_quality_failure_even_with_zero_queue() {
    use payment_routing::simulation::Simulator;
    let mut trap = Simulator::new(fixtures::simulation_case("sim-pinned", 2), 42).unwrap();
    let mut control =
        Simulator::new(fixtures::simulation_case("sim-pinned-direct", 2), 42).unwrap();
    trap.advance_ticks(100).unwrap();
    control.advance_ticks(100).unwrap();
    assert_eq!(trap.metrics().generated, control.metrics().generated);
    assert!(trap.metrics().expired > 0);
    assert_eq!(control.metrics().completed, control.metrics().generated);
    assert_eq!(control.metrics().sla_failures, 0);
}
