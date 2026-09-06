#[path = "support/simulation_fixture.rs"]
mod fixture;
use fixture::*;
use payment_routing::simulation::*;

#[test]
fn validates_all_scenario_fields_before_any_execution_even_with_no_arrivals() {
    let base = scenario(vec![rail("r", &["A", "B"], 1, 0)]);
    assert!(Simulator::new(base.clone(), 0).is_ok());
    let mut cases = vec![];
    let mut bad = base.clone();
    bad.arrivals.probability_per_million = 1_000_001;
    cases.push(bad);
    let mut bad = base.clone();
    bad.arrivals.min_amount_cents = 0;
    cases.push(bad);
    let mut bad = base.clone();
    bad.arrivals.max_amount_cents = 99;
    cases.push(bad);
    let mut bad = base.clone();
    bad.arrivals.max_sla_minutes = 9;
    cases.push(bad);
    let mut bad = base.clone();
    bad.arrivals.flows.clear();
    cases.push(bad);
    let mut bad = base.clone();
    bad.arrivals.flows[0].receiver = "A".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad.arrivals.flows[0].receiver = "missing".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad.max_active_payments = 0;
    cases.push(bad);
    let mut bad = base.clone();
    bad.services.clear();
    cases.push(bad);
    let mut bad = base.clone();
    bad.services.push(bad.services[0].clone());
    cases.push(bad);
    let mut bad = base.clone();
    bad.services[0].rail_id = "missing".into();
    cases.push(bad);
    let mut bad = base.clone();
    bad.services[0].period_minutes = 0;
    cases.push(bad);
    let mut bad = base.clone();
    bad.services[0].offset_minutes = 1;
    cases.push(bad);
    let mut bad = base.clone();
    bad.services[0].open_minutes = 2;
    cases.push(bad);
    let mut bad = base.clone();
    bad.network.rails[0].batch_capacity_cents = Some(0);
    cases.push(bad);
    let mut bad = base.clone();
    bad.network.rails[0].max_amount_cents = Some(0);
    cases.push(bad);
    for (index, mut bad) in cases.into_iter().enumerate() {
        bad.arrivals.attempts_per_minute = 0;
        assert!(Simulator::new(bad, 0).is_err(), "case {index}");
    }
    let mut closed = base;
    closed.services[0].open_minutes = 0;
    closed.services[0].capacity_per_minute_cents = Some(0);
    closed.retained_events = 0;
    assert!(Simulator::new(closed, 0).is_ok());
}

#[test]
fn direct_execution_has_hand_calculated_latency_cost_and_capacity_totals() {
    let mut config = scenario(vec![rail("r", &["A", "B"], 3, 2)]);
    config.services[0].capacity_per_minute_cents = Some(100);
    sla(&mut config, 2);
    let original = config.clone();
    let mut sim = Simulator::new(config, 42).unwrap();
    let first = sim.step().unwrap();
    assert_eq!(first.minute, 0);
    assert_eq!(sim.metrics().completed, 0);
    assert_eq!(sim.active_payments()[0].in_flight_until, Some(2));
    sim.advance_ticks(4).unwrap();
    let m = sim.metrics();
    assert_eq!(
        (m.generated, m.completed, sim.active_payments().len()),
        (5, 3, 2)
    );
    assert_eq!(
        (m.generated_volume_cents, m.completed_volume_cents),
        (500, 300)
    );
    assert_eq!(
        (
            m.routing_cost_cents,
            m.completed_elapsed_minutes,
            m.sla_failures
        ),
        (15, 6, 0)
    );
    assert_eq!((m.departed_hops, m.settled_hops), (5, 3));
    assert_eq!(
        (m.departed_principal_cents, m.settled_principal_cents),
        (500, 300)
    );
    assert_eq!(sim.rail_states()[0].used_this_minute_cents, 100);
    assert_eq!(sim.scenario(), &original);
}

#[test]
fn zero_latency_multihop_executes_in_order_with_full_principal_and_each_fee() {
    let mut config = scenario(vec![
        rail("ab", &["A", "B"], 2, 0),
        rail("bc", &["B", "C"], 3, 0),
    ]);
    config.arrivals.flows[0].receiver = "C".into();
    sla(&mut config, 0);
    let mut sim = Simulator::new(config, 0).unwrap();
    let report = sim.step().unwrap();
    assert_eq!(
        (sim.metrics().completed, sim.metrics().routing_cost_cents),
        (1, 5)
    );
    assert_eq!(
        (
            sim.metrics().departed_principal_cents,
            sim.metrics().completed_volume_cents
        ),
        (200, 100)
    );
    assert!(sim.active_payments().is_empty());
    let mut hops = report.events.iter().filter_map(|e| match &e.kind {
        EventKind::HopDeparted { hop, .. } => Some(("depart", hop.rail_id.as_str())),
        EventKind::HopSettled { hop, .. } => Some(("settle", hop.rail_id.as_str())),
        _ => None,
    });
    assert_eq!(
        hops.by_ref().collect::<Vec<_>>(),
        [
            ("depart", "ab"),
            ("settle", "ab"),
            ("depart", "bc"),
            ("settle", "bc")
        ]
    );
}

#[test]
fn shared_capacity_is_fifo_across_directions_resets_and_never_refunds_on_settlement() {
    let mut config = scenario(vec![rail("r", &["A", "B", "C"], 7, 0)]);
    config.arrivals.attempts_per_minute = 3;
    config.arrivals.flows.extend([
        PaymentFlow {
            sender: "B".into(),
            receiver: "A".into(),
        },
        PaymentFlow {
            sender: "C".into(),
            receiver: "B".into(),
        },
    ]);
    config.services[0].capacity_per_minute_cents = Some(200);
    sla(&mut config, 0);
    let mut sim = Simulator::new(config, 15).unwrap();
    for minute in 0..5 {
        let report = sim.step().unwrap();
        let completed: Vec<_> = report
            .events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::Completed { sequence, .. } => Some(sequence),
                _ => None,
            })
            .collect();
        assert_eq!(completed, [minute * 3 + 1, minute * 3 + 2]);
        assert_eq!(sim.rail_states()[0].used_this_minute_cents, 200);
    }
    assert_eq!(
        (
            sim.metrics().generated,
            sim.metrics().completed,
            sim.metrics().expired
        ),
        (15, 10, 5)
    );
    assert_eq!(sim.metrics().routing_cost_cents, 70);
    assert_eq!(sim.metrics().sla_failures, 5);
}

#[test]
fn recurring_windows_and_hard_static_gates_are_enforced() {
    let mut config = scenario(vec![rail("r", &["A", "B"], 1, 0)]);
    config.services[0].period_minutes = 4;
    config.services[0].offset_minutes = 2;
    config.services[0].open_minutes = 1;
    sla(&mut config, 0);
    let mut sim = Simulator::new(config.clone(), 0).unwrap();
    sim.advance_ticks(8).unwrap();
    assert_eq!((sim.metrics().completed, sim.metrics().expired), (2, 6));
    for gate in 0..3 {
        let mut closed = config.clone();
        match gate {
            0 => closed.network.rails[0].available = false,
            1 => closed.network.rails[0].max_amount_cents = Some(99),
            _ => closed.services[0].capacity_per_minute_cents = Some(99),
        }
        let mut sim = Simulator::new(closed, 0).unwrap();
        sim.advance_ticks(8).unwrap();
        assert_eq!(
            (
                sim.metrics().completed,
                sim.metrics().expired,
                sim.metrics().routing_cost_cents
            ),
            (0, 8, 0)
        );
    }
}

#[test]
fn queued_work_can_depart_at_its_inclusive_deadline_and_expiry_is_once() {
    let mut config = scenario(vec![rail("r", &["A", "B"], 1, 0)]);
    config.services[0].period_minutes = 3;
    config.services[0].offset_minutes = 1;
    config.services[0].open_minutes = 1;
    config.services[0].capacity_per_minute_cents = Some(100);
    sla(&mut config, 1);
    let mut sim = Simulator::new(config, 0).unwrap();
    sim.step().unwrap();
    assert_eq!(sim.active_payments()[0].route, None);
    let second = sim.step().unwrap();
    assert!(second.events.iter().any(|e| matches!(
        e.kind,
        EventKind::Completed {
            sequence: 1,
            late: false,
            elapsed_minutes: 1
        }
    )));
    sim.step().unwrap();
    assert_eq!((sim.metrics().expired, sim.metrics().sla_failures), (1, 1));
    let mut disconnected = scenario(vec![]);
    sla(&mut disconnected, 1);
    let mut sim = Simulator::new(disconnected, 0).unwrap();
    sim.advance_ticks(5).unwrap();
    assert_eq!(
        (
            sim.metrics().expired,
            sim.metrics().sla_failures,
            sim.active_payments().len()
        ),
        (4, 4, 1)
    );
}

#[test]
fn accepted_path_waits_and_a_late_final_hop_drains_without_double_sla_failure() {
    let mut config = scenario(vec![
        rail("ab", &["A", "B"], 2, 1),
        rail("bc", &["B", "C"], 3, 2),
    ]);
    config.arrivals.flows[0].receiver = "C".into();
    config.services[1].period_minutes = 3;
    config.services[1].open_minutes = 1;
    sla(&mut config, 3);
    let mut sim = Simulator::new(config, 0).unwrap();
    let reports: Vec<_> = (0..6).map(|_| sim.step().unwrap()).collect();
    assert!(
        reports[1]
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::HopSettled { sequence: 1, .. }))
    );
    assert!(reports[3].events.iter().any(|e| matches!(
        e.kind,
        EventKind::HopDeparted {
            sequence: 1,
            arrival_minute: 5,
            ..
        }
    )));
    assert!(
        reports[3]
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::DeadlineMissed { sequence: 1 }))
    );
    assert!(reports[5].events.iter().any(|e| matches!(
        e.kind,
        EventKind::Completed {
            sequence: 1,
            late: true,
            elapsed_minutes: 5
        }
    )));
    assert_eq!(
        reports
            .iter()
            .flat_map(|r| &r.events)
            .filter(|e| matches!(e.kind, EventKind::DeadlineMissed { sequence: 1 }))
            .count(),
        1
    );
    assert!(
        !reports
            .iter()
            .flat_map(|r| &r.events)
            .any(|e| matches!(e.kind, EventKind::Expired { sequence: 1 }))
    );
}

#[test]
fn overdue_intermediate_hop_settles_then_expires_without_dispatching_more_work() {
    let mut config = scenario(vec![
        rail("ab", &["A", "B"], 2, 1),
        rail("bc", &["B", "C"], 3, 3),
        rail("cd", &["C", "D"], 4, 3),
    ]);
    config.arrivals.flows[0].receiver = "D".into();
    config.services[1].period_minutes = 6;
    config.services[1].open_minutes = 1;
    sla(&mut config, 7);
    let mut sim = Simulator::new(config, 0).unwrap();
    let reports: Vec<_> = (0..10).map(|_| sim.step().unwrap()).collect();
    assert!(
        reports[7]
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::DeadlineMissed { sequence: 1 }))
    );
    assert!(
        reports[9]
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Expired { sequence: 1 }))
    );
    let departures: Vec<_> = reports
        .iter()
        .flat_map(|r| &r.events)
        .filter_map(|e| match &e.kind {
            EventKind::HopDeparted {
                sequence: 1, hop, ..
            } => Some(hop.rail_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(departures, ["ab", "bc"]);
    assert!(!sim.active_payments().iter().any(|p| p.sequence == 1));
}

#[test]
fn paused_ticks_do_nothing_manual_steps_ignore_speed_and_restart_is_fresh() {
    let config = scenario(vec![rail("r", &["A", "B"], 1, 2)]);
    let mut sim = Simulator::new(config.clone(), 11).unwrap();
    let initial = sim.clone();
    assert_eq!(sim.tick().unwrap(), None);
    assert_eq!(sim, initial);
    sim.step().unwrap();
    assert!(!sim.is_running());
    sim.start();
    assert_eq!(sim.tick().unwrap().unwrap().minute, 1);
    sim.pause();
    let paused = sim.clone();
    for _ in 0..20 {
        assert_eq!(sim.tick().unwrap(), None);
    }
    assert_eq!(sim, paused);
    let mut grouped = initial.clone();
    grouped.advance_ticks(2).unwrap();
    assert_eq!(sim, grouped);
    sim.restart(11);
    assert_eq!(sim, initial);
    sim.restart(123);
    assert_eq!(sim, Simulator::new(config, 123).unwrap());
}

#[test]
fn overload_is_explicit_and_does_not_change_the_seeded_demand_stream() {
    let mut config = scenario(vec![]);
    config.arrivals.attempts_per_minute = 3;
    config.arrivals.probability_per_million = 700_000;
    config.arrivals.min_amount_cents = 1;
    config.arrivals.max_amount_cents = u64::MAX;
    config.arrivals.min_sla_minutes = 0;
    config.arrivals.max_sla_minutes = u64::MAX;
    let mut roomy = Simulator::new(config.clone(), 70).unwrap();
    config.max_active_payments = 1;
    let mut small = Simulator::new(config, 70).unwrap();
    for _ in 0..30 {
        assert_eq!(
            generated(&roomy.step().unwrap()),
            generated(&small.step().unwrap())
        );
    }
    assert!(small.metrics().rejected > 0);
    assert_eq!(small.metrics().rejected, small.metrics().sla_failures);
    assert!(small.active_payments().len() <= 1);
    assert_eq!(roomy.metrics().rejected, 0);
}

#[test]
fn wide_totals_history_limits_and_zero_probability_are_exact() {
    let mut config = scenario(vec![rail("r", &["A", "B"], u64::MAX, 0)]);
    amount(&mut config, u64::MAX);
    config.arrivals.attempts_per_minute = 2;
    config.retained_events = 1;
    let mut sim = Simulator::new(config.clone(), u64::MAX).unwrap();
    let report = sim.step().unwrap();
    assert_eq!(
        sim.metrics().completed_volume_cents,
        2 * u128::from(u64::MAX)
    );
    assert_eq!(sim.metrics().routing_cost_cents, 2 * u128::from(u64::MAX));
    assert_eq!(sim.recent_events().len(), 1);
    assert_eq!(sim.recent_events().back(), report.events.last());
    config.retained_events = 0;
    config.arrivals.probability_per_million = 0;
    let mut sim = Simulator::new(config, 0).unwrap();
    sim.advance_ticks(20).unwrap();
    assert_eq!(sim.metrics(), &Metrics::default());
    assert!(sim.recent_events().is_empty());
    assert_eq!(sim.event_count(), 20);
}

#[test]
fn unused_capacity_is_discarded_between_consecutive_open_minutes() {
    let mut config = scenario(vec![rail("r", &["A", "B"], 1, 0)]);
    amount(&mut config, 60);
    config.services[0].period_minutes = 4;
    config.services[0].offset_minutes = 2;
    config.services[0].open_minutes = 2;
    config.services[0].capacity_per_minute_cents = Some(100);
    let mut sim = Simulator::new(config, 0).unwrap();
    sim.advance_ticks(3).unwrap();
    assert_eq!(sim.metrics().completed, 1);
    assert_eq!(sim.rail_states()[0].used_this_minute_cents, 60);
    sim.step().unwrap();
    // Carrying the previous 40 cents forward would incorrectly allow two hops.
    assert_eq!(sim.metrics().completed, 2);
    assert_eq!(sim.rail_states()[0].used_this_minute_cents, 60);
    assert_eq!(sim.active_payments().len(), 2);
}

#[test]
fn expiry_at_an_intermediate_retains_executed_fees_without_counting_completion() {
    let mut config = scenario(vec![
        rail("ab", &["A", "B"], 2, 1),
        rail("bc", &["B", "C"], 3, 1),
    ]);
    config.arrivals.flows[0].receiver = "C".into();
    config.services[1].period_minutes = 3;
    config.services[1].open_minutes = 1;
    sla(&mut config, 2);
    let mut sim = Simulator::new(config, 0).unwrap();
    sim.advance_ticks(2).unwrap();
    assert_eq!(sim.active_payments()[0].next_hop, 1);
    assert_eq!(sim.active_payments()[0].in_flight_until, None);
    let report = sim.step().unwrap();
    assert!(
        report
            .events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Expired { sequence: 1 }))
    );
    assert_eq!(
        (
            sim.metrics().routing_cost_cents,
            sim.metrics().departed_hops
        ),
        (2, 1)
    );
    assert_eq!(
        (
            sim.metrics().completed,
            sim.metrics().expired,
            sim.metrics().sla_failures
        ),
        (0, 1, 1)
    );
    assert_eq!(sim.metrics().expired_volume_cents, 100);
}

#[test]
fn terminal_settlement_frees_admission_space_before_new_arrivals() {
    let mut config = scenario(vec![rail("r", &["A", "B"], 1, 1)]);
    config.max_active_payments = 1;
    config.arrivals.attempts_per_minute = 2;
    let mut sim = Simulator::new(config, 0).unwrap();
    sim.advance_ticks(4).unwrap();
    assert_eq!(
        (
            sim.metrics().generated,
            sim.metrics().completed,
            sim.metrics().rejected
        ),
        (8, 3, 4)
    );
    assert_eq!(sim.active_payments()[0].sequence, 7);
    assert_eq!(sim.metrics().sla_failures, 4);
}
