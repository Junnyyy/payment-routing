#[path = "support/simulation_fixture.rs"]
mod fixture;
use fixture::*;
use payment_routing::{evaluation::*, simulation::*};

fn compare(c: Scenario, arrivals: u64, drain: u64) -> Evaluation {
    evaluate(
        &[World {
            name: "test".into(),
            scenario: c,
        }],
        &[42],
        &[
            Strategy::named("static").unwrap(),
            Strategy::named("reserved").unwrap(),
        ],
        Config {
            arrival_minutes: arrivals,
            drain_minutes: drain,
            verify_replay: true,
        },
    )
    .unwrap()
}

#[test]
fn fixed_cohort_cost_sla_throughput_and_replay_are_exact() {
    let c = scenario(vec![rail("ab", &["A", "B"], 7, 2)]);
    let evaluation = compare(c.clone(), 3, 2);
    assert_eq!(evaluation, compare(c.clone(), 3, 2));
    for run in &evaluation.cases[0].runs {
        assert_eq!(run.status, Status::Complete);
        assert!(run.replay_verified);
        assert_eq!(
            (
                run.metrics.generated,
                run.on_time(),
                run.metrics.routing_cost_cents
            ),
            (3, 3, 21)
        );
        assert_eq!(
            (
                run.on_time_volume(),
                run.metrics.completed_elapsed_minutes,
                run.metrics.departed_hops
            ),
            (300, 6, 3)
        );
        assert_eq!(run.elapsed_percentile(95), Some(2));
        assert_eq!(run.at_arrival_cutoff.completed, 1);
        assert_eq!(run.pending(), 0);
        // Shared source preserves the original continuous simulator's exact demand/outcomes.
        let mut native = Simulator::new(c.clone(), 42).unwrap();
        native.advance_ticks(3).unwrap();
        assert_eq!(run.at_arrival_cutoff, *native.metrics());
    }
}

#[test]
fn censoring_is_visible_and_has_no_rank_or_false_failure() {
    let evaluation = compare(scenario(vec![rail("ab", &["A", "B"], 7, 5)]), 1, 0);
    for run in &evaluation.cases[0].runs {
        assert_eq!(run.status, Status::Censored);
        assert_eq!(run.score(), None);
        assert_eq!(
            (
                run.pending(),
                run.pending_volume(),
                run.metrics.sla_failures
            ),
            (1, 100, 0)
        );
        assert_eq!(run.elapsed_percentile(95), None);
        assert_eq!(run.metrics.routing_cost_cents, 7);
    }
}

#[test]
fn admission_divergence_cannot_change_later_demand() {
    let mut c = scenario(vec![
        rail("ab", &["A", "B"], 2, 1),
        rail("bc", &["B", "C"], 3, 2),
    ]);
    c.arrivals.flows[0].receiver = "C".into();
    c.services[1].period_minutes = 2;
    c.services[1].open_minutes = 1;
    c.max_active_payments = 2;
    c.arrivals.attempts_per_minute = 3;
    c.arrivals.probability_per_million = 750_000;
    c.arrivals.max_amount_cents = 300;
    c.arrivals.min_sla_minutes = 0;
    c.arrivals.max_sla_minutes = 3;
    let e = compare(c, 25, 8);
    let (a, b) = (&e.cases[0].runs[0], &e.cases[0].runs[1]);
    assert_ne!(a.metrics, b.metrics);
    assert_eq!(a.payments.len(), b.payments.len());
    for (id, p) in &a.payments {
        let q = &b.payments[id];
        assert_eq!(
            (&p.payment, p.arrived_at, p.deadline),
            (&q.payment, q.arrived_at, q.deadline)
        );
    }
    let mut reversed = e.strategies.clone();
    reversed.reverse();
    let reversed = evaluate(&e.worlds, &e.seeds, &reversed, e.config).unwrap();
    assert_eq!(a, &reversed.cases[0].runs[1]);
    assert_eq!(b, &reversed.cases[0].runs[0]);
}

#[test]
fn zero_fees_from_never_routing_are_not_success_and_rejection_is_separate() {
    let mut c = scenario(vec![]);
    c.max_active_payments = 1;
    c.arrivals.attempts_per_minute = 2;
    sla(&mut c, 0);
    let e = compare(c, 1, 1);
    for run in &e.cases[0].runs {
        assert_eq!(run.metrics.routing_cost_cents, 0);
        assert_eq!(
            (
                run.metrics.sla_failures,
                run.metrics.rejected,
                run.metrics.expired
            ),
            (2, 1, 1)
        );
        assert_eq!(run.never_routed_expired(), 1);
        assert_eq!(run.score().unwrap().not_on_time, 2);
    }
}

#[test]
fn late_completion_keeps_sunk_cost_and_counts_sla_failure_once() {
    let mut c = scenario(vec![
        rail("ab", &["A", "B"], 2, 1),
        rail("bc", &["B", "C"], 3, 2),
    ]);
    c.arrivals.flows[0].receiver = "C".into();
    c.services[1].period_minutes = 2;
    c.services[1].open_minutes = 1;
    sla(&mut c, 3);
    let e = compare(c, 1, 4);
    let r = &e.cases[0].runs[0];
    assert_eq!(r.status, Status::Complete);
    assert_eq!(
        (
            r.metrics.completed_late,
            r.metrics.sla_failures,
            r.metrics.expired
        ),
        (1, 1, 0)
    );
    assert_eq!(r.metrics.routing_cost_cents, 5);
    assert_eq!(r.metrics.completed_elapsed_minutes, 4);
    assert_eq!(r.on_time(), 0);
    assert_eq!(r.score().unwrap().not_on_time, 1);
}

#[test]
fn unrevealed_surprises_cannot_change_prior_decisions_even_after_restart() {
    for name in ["static", "reserved", "preserve", "recompute", "tight"] {
        let mut c = scenario(vec![rail("ab", &["A", "B"], 2, 1)]);
        c.strategy = Strategy::named(name).unwrap().routing;
        let mut changed = c.clone();
        changed.disruptions.push(Disruption {
            minute: 4,
            update: RailUpdate {
                rail_id: "ab".into(),
                available: Some(false),
                capacity_per_minute_cents: None,
            },
        });
        let mut a = Simulator::new(c.clone(), 42).unwrap();
        let mut b = Simulator::new(changed.clone(), 42).unwrap();
        assert!(b.effective_scenario().disruptions.is_empty());
        for _ in 0..4 {
            assert_eq!(a.step().unwrap(), b.step().unwrap());
        }
        b.restart(42);
        assert!(b.effective_scenario().disruptions.is_empty());
        let config = Config {
            arrival_minutes: 3,
            drain_minutes: 1,
            verify_replay: true,
        };
        let s = [Strategy::named(name).unwrap()];
        let x = evaluate(
            &[World {
                name: "x".into(),
                scenario: c,
            }],
            &[42],
            &s,
            config,
        )
        .unwrap();
        let y = evaluate(
            &[World {
                name: "x".into(),
                scenario: changed,
            }],
            &[42],
            &s,
            config,
        )
        .unwrap();
        assert_eq!(x.cases, y.cases);
    }
}

#[test]
fn empty_demand_and_invalid_benchmarks_are_explicit() {
    let mut c = scenario(vec![]);
    c.arrivals.probability_per_million = 0;
    let e = compare(c, 2, 0);
    assert_eq!(e.cases[0].offered, 0);
    assert!(
        e.cases[0]
            .runs
            .iter()
            .all(|r| r.status == Status::Complete && r.score().unwrap().not_on_time == 0)
    );
    assert!(evaluate(&e.worlds, &[42, 42], &e.strategies, e.config).is_err());
    assert!(
        evaluate(
            &e.worlds,
            &[42],
            &[e.strategies[0].clone(), e.strategies[0].clone()],
            e.config
        )
        .is_err()
    );
    assert!(
        evaluate(
            &e.worlds,
            &[42],
            &e.strategies,
            Config {
                arrival_minutes: 0,
                ..e.config
            }
        )
        .is_err()
    );
}

#[test]
fn aggregates_use_only_identical_complete_cases_and_keep_exclusions() {
    let e = compare(scenario(vec![rail("ab", &["A", "B"], 7, 0)]), 3, 0);
    let mut mixed = e.clone();
    let mut incomplete = mixed.cases[0].clone();
    incomplete.seed = 7;
    incomplete.runs[0].status = Status::Error {
        minute: 2,
        message: "injected error".into(),
    };
    incomplete.runs[1].status = Status::Censored;
    mixed.cases.push(incomplete);
    for (index, a) in mixed.aggregates().unwrap().iter().enumerate() {
        assert_eq!((a.total_cases, a.matched_cases), (2, 1));
        assert_eq!(
            (a.error_cases, a.censored_cases),
            if index == 0 { (1, 0) } else { (0, 1) }
        );
        assert_eq!(a.metrics, e.cases[0].runs[index].metrics);
        assert_eq!(a.tied_best, 1);
    }
    assert!(mixed.has_incomplete_runs());
    assert!(mixed.to_csv().unwrap().contains("injected error"));
    assert!(mixed.to_text().unwrap().contains("ERROR minute=2"));
    let mut overflowing = mixed;
    overflowing.cases[1] = overflowing.cases[0].clone();
    overflowing.cases[0].runs[0].metrics.routing_cost_cents = u128::MAX;
    assert!(overflowing.aggregates().is_err());
}

#[test]
fn exact_fraction_order_and_empty_percentiles_do_not_hide_tail_failures() {
    for a in 0..15 {
        for b in 1..15 {
            for c in 0..15 {
                for d in 1..15 {
                    assert_eq!(
                        Ratio::new(a, b).unwrap().compare(Ratio::new(c, d).unwrap()),
                        (a * d).cmp(&(c * b))
                    );
                }
            }
        }
    }
    assert!(
        Ratio::new(u128::MAX - 1, u128::MAX)
            .unwrap()
            .compare(Ratio::new(u128::MAX - 2, u128::MAX - 1).unwrap())
            .is_gt()
    );
    assert_eq!(Ratio::new(0, 0), None);
    let mut e = compare(scenario(vec![rail("ab", &["A", "B"], 7, 0)]), 1, 0);
    let mut disconnected = scenario(vec![]);
    sla(&mut disconnected, 0);
    let mut bad = compare(disconnected, 1, 0).cases.remove(0);
    bad.world = "catastrophe".into();
    for _ in 0..19 {
        e.cases.push(e.cases[0].clone());
    }
    e.cases.push(bad);
    for a in e.aggregates().unwrap() {
        assert_eq!(a.worst_case.unwrap().world, "catastrophe");
        assert_eq!(a.worst_not_on_time.unwrap().percent(), "100.00%");
        // 1 bad / 21 cases is below the top 5%; worst remains visible beside p95.
        assert_eq!(a.p95_not_on_time.unwrap().percent(), "0.00%");
    }
}

#[test]
fn adversarial_suite_breaks_blanket_dominance_and_matches_payment_ids() {
    let strategies = [
        Strategy::named("static").unwrap(),
        Strategy::named("reserved").unwrap(),
    ];
    let config = Config {
        arrival_minutes: 1,
        drain_minutes: 8,
        verify_replay: true,
    };
    let mut trap = scenarios::named("reservation-trap").unwrap();
    trap.scenario.arrivals.probability_per_million = 1_000_000;
    let e = evaluate(&[trap], &[42], &strategies, config).unwrap();
    let case = &e.cases[0];
    assert_eq!((case.runs[0].on_time(), case.runs[1].on_time()), (2, 0));
    assert!(case.runs[0].score() < case.runs[1].score());
    assert!(case.runs[0].metrics.routing_cost_cents > case.runs[1].metrics.routing_cost_cents);
    let p = &case.pairs().unwrap()[0];
    assert_eq!(p.only_left_on_time, [1, 2]);
    assert_eq!(p.common_on_time, 0);
    let a = e.aggregates_for_world("reservation-trap").unwrap();
    assert_eq!(a[1].worst_service_shortfall.unwrap().percent(), "100.00%");
    assert_eq!(a[1].worst_shortfall_case.as_ref().unwrap().seed, 42);
    assert!(e.aggregates_for_world("unknown").is_err());
    assert!(e.to_text().unwrap().contains("DIFFERENT COHORTS"));
    let mut connection = scenarios::named("missed-connection").unwrap();
    connection.scenario.arrivals.probability_per_million = 1_000_000;
    let e = evaluate(&[connection], &[42], &strategies, config).unwrap();
    assert_eq!(
        (e.cases[0].runs[0].on_time(), e.cases[0].runs[1].on_time()),
        (0, 2)
    );
    assert!(e.cases[0].runs[1].score() < e.cases[0].runs[0].score());
}

#[test]
fn default_suite_reproduces_with_or_without_twin_checks_and_reordered_inputs() {
    let worlds = scenarios::all();
    let strategies = [
        Strategy::named("static").unwrap(),
        Strategy::named("reserved").unwrap(),
        Strategy::named("tight").unwrap(),
    ];
    let config = Config {
        arrival_minutes: 8,
        drain_minutes: 16,
        verify_replay: true,
    };
    let e = evaluate(&worlds, &[0, 42], &strategies, config).unwrap();
    assert!(!e.has_incomplete_runs());
    let mut reversed = worlds.clone();
    reversed.reverse();
    let mut repeated = evaluate(
        &reversed,
        &[42, 0],
        &strategies,
        Config {
            verify_replay: false,
            ..config
        },
    )
    .unwrap();
    for case in &e.cases {
        let other = repeated
            .cases
            .iter_mut()
            .find(|c| c.world == case.world && c.seed == case.seed)
            .unwrap();
        for r in &mut other.runs {
            r.replay_verified = true;
        }
        assert_eq!(case, other);
    }
    assert!(e.to_csv().unwrap().starts_with("\"record\",\"version\""));
    assert!(e.payments_csv().starts_with("\"version\",\"world\""));
}
