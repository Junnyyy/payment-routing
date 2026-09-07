use payment_routing::{
    observation::EVIDENCE_PER_PAYMENT,
    operations::{Operations, PAYMENT_EVENTS, Preset, RECENT_PAYMENTS},
    simulation::{RoutingStrategy, Simulator},
};

#[test]
fn observing_actual_searches_preserves_every_event_and_state() {
    for preset in Preset::ALL {
        for strategy in [
            RoutingStrategy::CheapestStatic,
            RoutingStrategy::Reserved {
                limits: Default::default(),
            },
        ] {
            let mut plain = Simulator::new(preset.scenario(strategy), 42).unwrap();
            let mut observed = plain.clone();
            let mut evidence_count = 0;
            for _ in 0..160 {
                let expected = plain.step().unwrap();
                let (report, evidence) = observed.step_observed().unwrap();
                assert_eq!(expected, report);
                assert_eq!(plain, observed);
                for p in evidence.payments.values() {
                    assert!(p.entries.len() <= EVIDENCE_PER_PAYMENT);
                    evidence_count += p.entries.len();
                }
            }
            assert!(evidence_count > 0);
        }
    }
}

#[test]
fn sessions_replay_both_strategies_at_one_time_and_bound_retained_dossiers() {
    for preset in Preset::ALL {
        let mut ops = Operations::new(preset, 42).unwrap();
        for _ in 0..300 {
            ops.step().unwrap();
        }
        for run in &ops.runs {
            run.simulator.check_invariants().unwrap();
            assert_eq!(run.simulator.next_minute(), 300);
            assert!(
                run.payments.len()
                    <= RECENT_PAYMENTS + run.simulator.scenario().max_active_payments
            );
            assert_eq!(run.samples.len(), 120);
            for p in run.payments.values() {
                assert!(p.events.len() <= PAYMENT_EVENTS);
            }
            for active in run.simulator.active_payments() {
                assert!(run.payments.contains_key(&active.sequence));
            }
        }
        assert_eq!(
            ops.runs[0].simulator.metrics().generated_volume_cents,
            ops.runs[1].simulator.metrics().generated_volume_cents
        );
        let expected = ops.clone();
        ops.restart(42).unwrap();
        assert!(
            ops.runs
                .iter()
                .all(|r| r.payments.is_empty() && r.samples.is_empty())
        );
        for _ in 0..300 {
            ops.step().unwrap();
        }
        assert_eq!(ops, expected);
        match preset {
            Preset::Pressure => assert!(ops.runs[0].simulator.metrics().rejected > 0),
            Preset::Outage => assert!(ops.runs[0].simulator.metrics().expired > 0),
            Preset::Limited => assert!(
                ops.runs[1]
                    .simulator
                    .routing_diagnostics()
                    .truncated_searches
                    > 0
            ),
            _ => {}
        }
        println!(
            "{}: static {:?}; reserved {:?}",
            preset.name(),
            ops.runs[0].simulator.metrics(),
            ops.runs[1].simulator.metrics()
        );
    }
}

#[test]
fn evidence_includes_real_rejected_alternatives_and_selected_witnesses() {
    let mut ops = Operations::new(Preset::Balanced, 42).unwrap();
    for _ in 0..40 {
        ops.step().unwrap();
    }
    assert!(ops.runs[0].payments.values().any(|p| {
        p.route.is_some()
            && p.evidence
                .entries
                .iter()
                .any(|e| e.reason.starts_with("Rejected"))
    }));
    assert!(ops.runs[1].payments.values().any(|p| {
        p.evidence
            .entries
            .iter()
            .any(|e| e.reason.contains("capacity conflict"))
    }));
    for run in &ops.runs {
        assert!(
            run.payments
                .values()
                .any(|p| p.status.terminal() && p.route.is_some())
        );
    }
}
