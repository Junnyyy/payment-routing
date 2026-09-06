#[path = "support/simulation_audit.rs"]
mod audit;
#[path = "support/simulation_fixture.rs"]
mod fixture;
use fixture::*;
use payment_routing::simulation::*;

#[test]
fn varied_scenarios_replay_exactly_and_every_event_passes_independent_accounting() {
    for case in 0..128u64 {
        let mut config = scenario(vec![
            rail("ab", &["A", "B"], case % 3, (case % 3) as u32),
            rail("bc", &["B", "C"], (case + 1) % 5, ((case / 3) % 3) as u32),
            rail("ac", &["A", "C"], (case + 2) % 4, ((case / 5) % 3) as u32),
            rail("cd", &["C", "D"], case % 7, ((case / 7) % 3) as u32),
            rail("da", &["D", "A", "B"], case % 2, ((case / 11) % 3) as u32),
        ]);
        for (i, service) in config.services.iter_mut().enumerate() {
            let variant = case + i as u64;
            service.period_minutes = 1 + variant % 5;
            service.offset_minutes = variant % service.period_minutes;
            service.open_minutes = if variant.is_multiple_of(7) {
                0
            } else {
                service.period_minutes - service.offset_minutes
            };
            service.capacity_per_minute_cents =
                [None, Some(0), Some(1), Some(3), Some(8)][variant as usize % 5];
            config.network.rails[i].max_amount_cents = if variant.is_multiple_of(4) {
                Some(2)
            } else {
                None
            };
            config.network.rails[i].available = !variant.is_multiple_of(13);
        }
        config.arrivals.flows = [("A", "B"), ("A", "C"), ("D", "B"), ("B", "A"), ("C", "D")]
            .into_iter()
            .map(|(s, r)| PaymentFlow {
                sender: s.into(),
                receiver: r.into(),
            })
            .collect();
        config.arrivals.attempts_per_minute = (1 + case % 3) as u32;
        config.arrivals.probability_per_million = [0, 350_000, 1_000_000][case as usize % 3];
        config.arrivals.min_amount_cents = 1;
        config.arrivals.max_amount_cents = 1 + case % 4;
        config.arrivals.min_sla_minutes = 0;
        config.arrivals.max_sla_minutes = case % 7;
        config.max_active_payments = 1 + case as usize % 9;
        config.retained_events = case as usize % 17;
        let mut left = Simulator::new(config.clone(), case).unwrap();
        let mut right = left.clone();
        let mut check = audit::Audit::default();
        for minute in 0..80 {
            // Driver pacing, pause frequency and rendering reads are irrelevant.
            right.start();
            if minute % 3 == 0 {
                right.pause();
                assert_eq!(right.tick().unwrap(), None);
                right.start();
            }
            let report = left.step().unwrap();
            assert_eq!(
                report,
                right.tick().unwrap().unwrap(),
                "case {case} minute {minute}"
            );
            right.pause();
            assert_eq!(left, right, "case {case} minute {minute}");
            check.check(&config, &report, &left);
        }
        let finished = left.clone();
        left.restart(case);
        left.advance_ticks(13).unwrap();
        left.advance_ticks(0).unwrap();
        left.advance_ticks(67).unwrap();
        assert_eq!(left, finished, "grouped restart case {case}");
    }
}

#[test]
fn different_seeds_change_generated_demand_while_identical_seeds_match() {
    let mut config = scenario(vec![]);
    config.arrivals.attempts_per_minute = 3;
    config.arrivals.probability_per_million = 500_000;
    config.arrivals.min_amount_cents = 1;
    config.arrivals.max_amount_cents = 1_000_000;
    let mut a = Simulator::new(config.clone(), 1).unwrap();
    let mut b = Simulator::new(config, 2).unwrap();
    let (mut first, mut second) = (vec![], vec![]);
    for _ in 0..20 {
        first.extend(generated(&a.step().unwrap()));
        second.extend(generated(&b.step().unwrap()));
    }
    assert_ne!(first, second);
}

#[test]
fn sustained_overload_and_execution_keep_persistent_state_bounded() {
    let mut config = scenario(vec![rail("r", &["A", "B"], 2, 1)]);
    config.arrivals.attempts_per_minute = 3;
    amount(&mut config, 50);
    sla(&mut config, 3);
    config.services[0].capacity_per_minute_cents = Some(50);
    config.max_active_payments = 16;
    config.retained_events = 17;
    let mut executing = Simulator::new(config.clone(), 10).unwrap();
    config.network.rails.clear();
    config.services.clear();
    config.max_active_payments = 8;
    sla(&mut config, u64::MAX);
    let mut congested = Simulator::new(config, 10).unwrap();
    let (mut max_executing, mut max_congested) = (0, 0);
    for _ in 0..100_000 {
        executing.step().unwrap();
        congested.step().unwrap();
        max_executing = max_executing.max(executing.active_payments().len());
        max_congested = max_congested.max(congested.active_payments().len());
        assert!(executing.recent_events().len() <= 17 && congested.recent_events().len() <= 17);
    }
    assert!(max_executing <= 16);
    assert_eq!(max_congested, 8);
    assert_eq!(
        (executing.metrics().generated, executing.metrics().completed),
        (300_000, 99_999)
    );
    assert_eq!(executing.metrics().routing_cost_cents, 200_000);
    assert_eq!(
        (
            congested.metrics().generated,
            congested.metrics().rejected,
            congested.active_payments().len()
        ),
        (300_000, 299_992, 8)
    );
    assert_eq!(congested.metrics().sla_failures, 299_992);
    assert_eq!(
        (
            executing.recent_events().len(),
            congested.recent_events().len()
        ),
        (17, 17)
    );
    println!(
        "100,000 ticks each: executing peak {max_executing} active; disconnected peak {max_congested}; both retain 17 events"
    );
}
