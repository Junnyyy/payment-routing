#[path = "support/simulation_audit.rs"]
mod audit;
#[path = "support/simulation_fixture.rs"]
mod fixture;
use fixture::*;
use payment_routing::{scalable::SearchLimits, simulation::*};

fn reserved(c: &mut Scenario) {
    c.strategy = RoutingStrategy::Reserved {
        limits: SearchLimits::default(),
    };
}
fn update(id: &str, available: Option<bool>, cap: Option<Option<u64>>) -> RailUpdate {
    RailUpdate {
        rail_id: id.into(),
        available,
        capacity_per_minute_cents: cap,
    }
}
fn scheduled(c: &mut Scenario, minute: u128, update: RailUpdate) {
    c.disruptions.push(Disruption { minute, update });
}
fn recovery(count: u32, old_fee: u64, new_fee: u64) -> Scenario {
    let mut c = scenario(vec![
        rail("A", &["A", "B"], new_fee, 0),
        rail("B", &["A", "B"], old_fee, 0),
    ]);
    c.network.rails[0].available = false;
    c.arrivals.attempts_per_minute = count;
    sla(&mut c, 3);
    for s in &mut c.services {
        s.period_minutes = 10;
        s.offset_minutes = 3;
        s.open_minutes = 1;
    }
    reserved(&mut c);
    scheduled(&mut c, 1, update("A", Some(true), None));
    c
}
fn first_decision(c: Scenario, policy: ReoptimizationPolicy) -> Simulator {
    let mut s = Simulator::new(c, 42).unwrap();
    s.set_reoptimization_policy(policy);
    s.advance_ticks(2).unwrap();
    s
}

#[test]
fn equal_objective_recovery_avoids_twenty_four_unnecessary_route_changes() {
    let c = recovery(24, 5, 5);
    let adaptive = first_decision(c.clone(), Default::default());
    let full = first_decision(c, ReoptimizationPolicy::Recompute);
    let r = adaptive.last_reoptimization().unwrap();
    assert_eq!(r.preserve.previously_planned, 24);
    assert_eq!(
        (r.preserve.changed_assignments, r.recompute.changed_routes),
        (0, 24)
    );
    assert_eq!(
        (
            r.preserve.remaining_fee_cents,
            r.recompute.remaining_fee_cents
        ),
        (120, 120)
    );
    assert_eq!(
        (
            r.preserve.remaining_elapsed_minutes,
            r.recompute.remaining_elapsed_minutes
        ),
        (48, 48)
    );
    assert_eq!(r.selected, ReoptimizationChoice::Preserve);
    assert_eq!(full.adaptation_metrics().changed_assignments, 24);
    assert_eq!(adaptive.adaptation_metrics().changed_assignments, 0);
}

#[test]
fn expensive_preservation_exposes_the_fee_frontier_and_explicit_allowance() {
    let c = recovery(24, 100, 1);
    let adaptive = first_decision(c.clone(), Default::default());
    let preserve = first_decision(c.clone(), ReoptimizationPolicy::Preserve);
    let willing = first_decision(
        c,
        ReoptimizationPolicy::Adaptive {
            max_extra_fee_cents: 2376,
            max_extra_elapsed_minutes: 0,
            max_extra_hops: 0,
        },
    );
    let r = adaptive.last_reoptimization().unwrap();
    assert_eq!(
        (
            r.preserve.remaining_fee_cents,
            r.recompute.remaining_fee_cents
        ),
        (2400, 24)
    );
    assert_eq!(
        (
            r.preserve.changed_assignments,
            r.recompute.changed_assignments
        ),
        (0, 24)
    );
    assert_eq!(r.selected, ReoptimizationChoice::Recompute);
    assert_eq!(
        preserve.last_reoptimization().unwrap().selected,
        ReoptimizationChoice::Preserve
    );
    assert_eq!(
        willing.last_reoptimization().unwrap().selected,
        ReoptimizationChoice::Preserve
    );
}

#[test]
fn elapsed_allowance_is_independent_of_money() {
    let mut c = recovery(8, 5, 5);
    sla(&mut c, 9);
    c.services[0].offset_minutes = 2;
    c.services[1].offset_minutes = 8;
    let s = first_decision(c, Default::default());
    let r = s.last_reoptimization().unwrap();
    assert_eq!(
        (
            r.preserve.remaining_elapsed_minutes,
            r.recompute.remaining_elapsed_minutes
        ),
        (56, 8)
    );
    assert_eq!(r.selected, ReoptimizationChoice::Recompute);
}

#[test]
fn preserving_a_flexible_reservation_can_cause_an_avoidable_sla_failure() {
    let mut c = recovery(2, 1, 2);
    c.arrivals.min_amount_cents = 1;
    c.arrivals.max_amount_cents = 2;
    c.services[1].capacity_per_minute_cents = Some(2);
    c.network.rails[0].max_amount_cents = Some(1);
    c.services[0].capacity_per_minute_cents = Some(1);
    c.network.rails.push(rail("X", &["C", "A"], 1, 0));
    c.services.push(RailService {
        rail_id: "X".into(),
        period_minutes: 1,
        offset_minutes: 0,
        open_minutes: 1,
        capacity_per_minute_cents: None,
    });
    c.arrivals.flows.push(PaymentFlow {
        sender: "C".into(),
        receiver: "B".into(),
    });
    // The constrained flow needs a fee-bearing prefix; before recovery, the
    // cheaper flexible payment wins the shared slot when only one can be served.
    // Find the explicit deterministic demand [A->B 1, C->B 2].
    let seed = (0..1024)
        .find(|seed| {
            let mut s = Simulator::new(c.clone(), *seed).unwrap();
            let r = s.step().unwrap();
            generated(&r)
                .iter()
                .map(|(_, p, _)| (p.sender.clone(), p.amount_cents))
                .collect::<Vec<_>>()
                == vec![("A".into(), 1), ("C".into(), 2)]
        })
        .unwrap();
    let mut adaptive = Simulator::new(c.clone(), seed).unwrap();
    let mut preserve = adaptive.clone();
    preserve.set_reoptimization_policy(ReoptimizationPolicy::Preserve);
    adaptive.advance_ticks(2).unwrap();
    preserve.advance_ticks(2).unwrap();
    let r = adaptive.last_reoptimization().unwrap();
    assert_eq!((r.preserve.planned, r.recompute.planned), (1, 2));
    assert!(!r.same_planned_cohort);
    assert_eq!(r.selected, ReoptimizationChoice::Recompute);
    let a: Vec<_> = (0..2)
        .flat_map(|_| adaptive.step().unwrap().events)
        .collect();
    let b: Vec<_> = (0..2)
        .flat_map(|_| preserve.step().unwrap().events)
        .collect();
    for id in [1, 2] {
        assert!(a.iter().any(
            |e| matches!(e.kind, EventKind::Completed {sequence, late:false,..} if sequence==id)
        ));
    }
    assert!(
        b.iter()
            .any(|e| matches!(e.kind, EventKind::Expired { sequence: 2 }))
    );
}

#[test]
fn capacity_loss_rebooks_shared_slots_and_preserves_inflight_principal() {
    let mut c = scenario(vec![
        rail("X", &["A", "C"], 0, 3),
        rail("Y", &["C", "B"], 1, 0),
        rail("W", &["C", "B"], 5, 0),
        rail("Z", &["A", "B"], 2, 1),
    ]);
    reserved(&mut c);
    scheduled(&mut c, 1, update("Y", None, Some(Some(0))));
    let mut s = Simulator::new(c.clone(), 42).unwrap();
    let mut audit = audit::Audit::default();
    for _ in 0..5 {
        let report = s.step().unwrap();
        audit.check(&c, &report, &s);
        if report.minute == 1 {
            let p = &s.active_payments()[0];
            assert_eq!(p.in_flight_until, Some(3));
            assert_eq!(
                p.route
                    .as_ref()
                    .unwrap()
                    .hops
                    .iter()
                    .map(|h| h.rail_id.as_str())
                    .collect::<Vec<_>>(),
                vec!["X", "W"]
            );
            assert_eq!(p.planned_departures, Some(vec![0, 3]));
            // Independently measure the documented model tradeoff: if the
            // immutable-prefix restriction were relaxed, backtracking is cheaper.
            let mut relaxed = s.effective_scenario().network.clone();
            relaxed
                .rails
                .iter_mut()
                .find(|r| r.id == "Y")
                .unwrap()
                .available = false;
            let mut instruction = p.payment.clone();
            instruction.sender = "C".into();
            instruction.max_delivery_minutes = Some(7);
            let alternative = payment_routing::routing::route_payment(&relaxed, &instruction)
                .unwrap()
                .unwrap();
            assert_eq!(
                (
                    alternative.total_fee_cents,
                    alternative.total_settlement_minutes
                ),
                (2, 4)
            );
            assert_eq!(
                alternative
                    .hops
                    .iter()
                    .map(|h| h.rail_id.as_str())
                    .collect::<Vec<_>>(),
                vec!["X", "Z"]
            );
        }
    }
}

#[test]
fn unrepairable_suffix_queues_at_intermediate_node_then_recovers_without_double_acceptance() {
    for strategy in [
        RoutingStrategy::CheapestStatic,
        RoutingStrategy::Reserved {
            limits: Default::default(),
        },
    ] {
        let mut c = scenario(vec![
            rail("X", &["A", "C"], 1, 2),
            rail("Y", &["C", "B"], 1, 0),
        ]);
        c.strategy = strategy;
        scheduled(&mut c, 1, update("Y", Some(false), None));
        scheduled(&mut c, 4, update("Y", Some(true), None));
        let mut s = Simulator::new(c.clone(), 42).unwrap();
        let mut audit = audit::Audit::default();
        for _ in 0..6 {
            let report = s.step().unwrap();
            audit.check(&c, &report, &s);
            if report.minute == 2 {
                let p = &s.active_payments()[0];
                assert_eq!(p.next_hop, 1);
                assert!(!p.has_complete_plan());
                assert_eq!(p.in_flight_until, None);
                assert_eq!(s.metrics().completed, 0);
            }
            if report.minute == 4 {
                assert!(report.events.iter().any(|e| matches!(
                    e.kind,
                    EventKind::Completed {
                        sequence: 1,
                        late: false,
                        ..
                    }
                )));
            }
        }
        assert!(s.metrics().accepted_routes <= s.metrics().generated);
    }
}

#[test]
fn same_minute_zero_latency_capacity_is_additive_after_a_reduction() {
    let mut c = recovery(2, 5, 9);
    amount(&mut c, 1);
    c.services[1].capacity_per_minute_cents = Some(2);
    c.disruptions = vec![Disruption {
        minute: 1,
        update: update("B", None, Some(Some(1))),
    }];
    let mut s = first_decision(c, Default::default());
    let r = s.last_reoptimization().unwrap();
    assert_eq!((r.preserve.planned, r.preserve.withdrawn), (1, 1));
    s.advance_ticks(3).unwrap();
    assert!(s.metrics().expired > 0);
}

#[test]
fn controls_merge_bound_storage_and_restart_replays_scheduled_changes() {
    let c = recovery(3, 5, 5);
    let mut s = Simulator::new(c.clone(), 42).unwrap();
    let before = s.clone();
    assert!(
        s.queue_rail_update(update("missing", Some(false), None))
            .is_err()
    );
    assert_eq!(s, before);
    for _ in 0..100 {
        s.queue_rail_update(update("B", Some(false), None)).unwrap();
    }
    s.queue_rail_update(update("B", None, Some(Some(0))))
        .unwrap();
    assert_eq!(s.pending_rail_updates(), 1);
    assert_eq!(s.next_minute(), 0);
    assert!(s.tick().unwrap().is_none());
    s.step().unwrap();
    assert!(!s.effective_scenario().network.rails[1].available);
    assert_eq!(
        s.effective_scenario().services[1].capacity_per_minute_cents,
        Some(0)
    );
    assert_eq!(s.scenario().services[1].capacity_per_minute_cents, None);
    s.restart(42);
    assert_eq!(s, Simulator::new(c, 42).unwrap());
    s.advance_ticks(5).unwrap();
    let expected = s.clone();
    s.restart(42);
    s.advance_ticks(5).unwrap();
    assert_eq!(s, expected);
}

#[test]
fn control_override_and_noop_are_deterministic_and_do_not_count_churn() {
    let mut s = Simulator::new(recovery(2, 5, 5), 42).unwrap();
    s.step().unwrap();
    s.queue_rail_update(update("A", Some(false), None)).unwrap();
    let r = s.step().unwrap();
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e.kind, EventKind::DisruptionApplied(_)))
    );
    assert_eq!(s.adaptation_metrics().rail_changes, 0);
    s.queue_rail_update(update("A", Some(true), Some(None)))
        .unwrap();
    s.step().unwrap();
    assert_eq!(s.adaptation_metrics().rail_changes, 1);
}

#[test]
fn changing_networks_replay_with_independent_accounting_and_bounded_storage() {
    let (mut decisions, mut higher_churn, mut lost_coverage, mut incomparable) = (0, 0, 0, 0);
    for seed in 0..24 {
        for strategy in [
            RoutingStrategy::CheapestStatic,
            RoutingStrategy::Reserved {
                limits: SearchLimits {
                    max_candidates: if seed % 3 == 0 { 8 } else { 1000 },
                    ..Default::default()
                },
            },
        ] {
            for policy in [
                ReoptimizationPolicy::Preserve,
                ReoptimizationPolicy::Recompute,
                ReoptimizationPolicy::default(),
            ] {
                let mut c = scenario(vec![
                    rail("R", &["A", "B", "C", "D"], 1, 1),
                    rail("S", &["A", "C"], 2, 0),
                    rail("T", &["B", "C", "D"], 3, 2),
                ]);
                c.strategy = strategy;
                c.max_active_payments = 12;
                c.retained_events = 7;
                c.arrivals.attempts_per_minute = 3;
                c.arrivals.probability_per_million = 700_000;
                c.arrivals.min_amount_cents = 1;
                c.arrivals.max_amount_cents = 3;
                c.arrivals.min_sla_minutes = 0;
                c.arrivals.max_sla_minutes = 8;
                c.arrivals.flows.push(PaymentFlow {
                    sender: "C".into(),
                    receiver: "D".into(),
                });
                c.arrivals.flows.push(PaymentFlow {
                    sender: "B".into(),
                    receiver: "A".into(),
                });
                for (i, s) in c.services.iter_mut().enumerate() {
                    s.period_minutes = 3;
                    s.open_minutes = 2;
                    s.offset_minutes = (i % 2) as u64;
                    s.capacity_per_minute_cents = Some(4);
                }
                for minute in [1, 3, 5, 8, 9, 13, 19, 23, 31, 37, 44, 50] {
                    let id = ["R", "S", "T"][(minute as usize + seed as usize) % 3];
                    scheduled(
                        &mut c,
                        minute,
                        update(
                            id,
                            Some((minute + seed) % 4 != 0),
                            Some(match (minute + seed) % 4 {
                                0 => Some(0),
                                1 => Some(2),
                                2 => Some(6),
                                _ => None,
                            }),
                        ),
                    );
                }
                let mut a = Simulator::new(c.clone(), seed as u64).unwrap();
                a.set_reoptimization_policy(policy);
                let mut b = a.clone();
                let mut audit = audit::Audit::default();
                for minute in 0..60 {
                    let ar = a.step().unwrap_or_else(|e| {
                        panic!("seed {seed} minute {minute} {strategy:?} {policy:?}: {e}")
                    });
                    for e in &ar.events {
                        if let EventKind::Reoptimized(r) = &e.kind {
                            decisions += 1;
                            higher_churn += usize::from(
                                r.same_planned_cohort
                                    && r.preserve.changed_assignments
                                        > r.recompute.changed_assignments,
                            );
                            lost_coverage += usize::from(r.preserve.planned < r.recompute.planned);
                            incomparable += usize::from(!r.same_planned_cohort);
                        }
                    }
                    let (br, _) = b.step_observed().unwrap();
                    assert_eq!(ar, br);
                    assert_eq!(a, b);
                    audit.check(&c, &ar, &a);
                    assert!(a.recent_events().len() <= 7);
                    assert!(
                        a.reservation_entries()
                            <= a.active_payments().len() * c.network.institutions.len()
                                + c.network.rails.len()
                    );
                }
                b.restart(seed as u64);
                b.advance_ticks(60).unwrap();
                assert_eq!(a, b);
            }
        }
    }
    println!(
        "decisions={decisions}, preserve_higher_churn_same_cohort={higher_churn}, preserve_lower_coverage={lost_coverage}, incomparable_cohorts={incomparable}"
    );
}

#[path = "../benchmarks/disruptions.rs"]
mod comparisons;
#[test]
fn small_full_recomputations_match_exact_complete_cohort_calendar_optima() {
    for (_, c, seed) in comparisons::cases()
        .into_iter()
        .filter(|(_, c, _)| c.arrivals.attempts_per_minute <= 4)
    {
        let mut sim = Simulator::new(c, seed).unwrap();
        sim.step().unwrap();
        let cohort = sim.active_payments().to_vec();
        sim.step().unwrap();
        let r = sim.last_reoptimization().unwrap();
        match comparisons::exact_at_one(sim.effective_scenario(), &cohort).unwrap() {
            Some(plan) => {
                assert_eq!(r.recompute.planned, cohort.len());
                assert_eq!(r.recompute.remaining_fee_cents, plan.total_fee_cents);
                assert_eq!(
                    r.recompute.remaining_elapsed_minutes,
                    plan.total_elapsed_minutes
                );
                assert_eq!(
                    r.recompute.remaining_hops,
                    plan.assignments
                        .iter()
                        .map(|p| p.route.hops.len())
                        .sum::<usize>()
                );
            }
            None => assert!(r.recompute.unplanned > 0),
        }
    }
}

#[test]
fn reduced_capacity_can_retime_without_changing_the_route() {
    let mut c = recovery(2, 5, 9);
    amount(&mut c, 1);
    sla(&mut c, 5);
    c.services[1].open_minutes = 3;
    c.services[1].capacity_per_minute_cents = Some(2);
    c.disruptions[0].update = update("B", None, Some(Some(1)));
    let s = first_decision(c, Default::default());
    let a = &s.last_reoptimization().unwrap().preserve;
    assert_eq!(
        (
            a.previously_planned,
            a.changed_assignments,
            a.retimed_only,
            a.changed_routes,
            a.withdrawn
        ),
        (2, 1, 1, 0, 0)
    );
}

#[test]
fn closure_does_not_cancel_or_reprice_a_final_hop_already_in_flight() {
    let mut c = scenario(vec![rail("X", &["A", "B"], 7, 3)]);
    reserved(&mut c);
    sla(&mut c, 3);
    scheduled(&mut c, 1, update("X", Some(false), Some(Some(0))));
    let mut s = Simulator::new(c.clone(), 42).unwrap();
    let mut audit = audit::Audit::default();
    for minute in 0..4 {
        let r = s.step().unwrap();
        audit.check(&c, &r, &s);
        if minute == 1 {
            assert_eq!(s.adaptation_metrics().changed_assignments, 0);
            assert_eq!(s.active_payments()[0].in_flight_until, Some(3));
        }
        if minute == 3 {
            assert!(r.events.iter().any(|e| matches!(
                e.kind,
                EventKind::Completed {
                    sequence: 1,
                    late: false,
                    ..
                }
            )));
        }
    }
    assert_eq!(s.metrics().routing_cost_cents, 7);
    assert_eq!(s.metrics().completed, 1);
}

#[test]
fn blocked_intermediate_suffix_expires_once_without_losing_executed_costs() {
    let mut c = scenario(vec![
        rail("X", &["A", "C"], 7, 2),
        rail("Y", &["C", "B"], 1, 0),
    ]);
    reserved(&mut c);
    sla(&mut c, 3);
    scheduled(&mut c, 1, update("Y", Some(false), None));
    let mut s = Simulator::new(c.clone(), 42).unwrap();
    let mut events = vec![];
    for _ in 0..6 {
        events.extend(s.step().unwrap().events);
    }
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Expired { sequence: 1 }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::DeadlineMissed { sequence: 1 }))
            .count(),
        1
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Completed { sequence: 1, .. }))
    );
    assert_eq!(s.metrics().routing_cost_cents, 7);
}

#[test]
fn existing_plan_survives_an_unresolved_full_search() {
    let mut c = recovery(1, 5, 9);
    // The new first lexical rail is a dead end, consuming the full search budget
    // before the old useful rail. Retention validates the known witness directly.
    c.network.rails[0].participants = vec!["A".into(), "C".into()];
    c.strategy = RoutingStrategy::Reserved {
        limits: SearchLimits {
            max_candidates: 2,
            ..Default::default()
        },
    };
    let s = first_decision(c, Default::default());
    let r = s.last_reoptimization().unwrap();
    assert_eq!((r.preserve.planned, r.recompute.planned), (1, 0));
    assert!(r.recompute_diagnostics.truncated_searches > 0);
    assert_eq!(r.selected, ReoptimizationChoice::Preserve);
}

#[test]
fn zero_optimum_and_invalid_scheduled_events_have_explicit_outcomes() {
    let s = first_decision(recovery(4, 0, 0), Default::default());
    let r = s.last_reoptimization().unwrap();
    assert_eq!(
        (
            r.preserve.remaining_fee_cents,
            r.recompute.remaining_fee_cents
        ),
        (0, 0)
    );
    assert_eq!(r.preserve.changed_assignments, 0);
    for bad in [
        update("unknown", Some(false), None),
        update("A", None, None),
    ] {
        let mut c = recovery(0, 0, 0);
        c.disruptions[0].update = bad;
        assert!(Simulator::new(c, 42).is_err());
    }
    let mut c = recovery(0, 0, 0);
    c.disruptions.push(c.disruptions[0].clone());
    assert!(Simulator::new(c, 42).is_err());
}

#[test]
fn extra_hops_require_an_explicit_stability_allowance() {
    let c = comparisons::hop_recovery();
    let zero = first_decision(c.clone(), Default::default());
    let allowed = first_decision(
        c,
        ReoptimizationPolicy::Adaptive {
            max_extra_fee_cents: 0,
            max_extra_elapsed_minutes: 0,
            max_extra_hops: 1,
        },
    );
    let r = zero.last_reoptimization().unwrap();
    assert_eq!(
        (r.preserve.remaining_hops, r.recompute.remaining_hops),
        (2, 1)
    );
    assert_eq!(
        (
            r.preserve.remaining_fee_cents,
            r.recompute.remaining_fee_cents
        ),
        (1, 1)
    );
    assert_eq!(
        (
            r.preserve.changed_assignments,
            r.recompute.changed_assignments
        ),
        (0, 1)
    );
    assert_eq!(r.selected, ReoptimizationChoice::Recompute);
    assert_eq!(
        allowed.last_reoptimization().unwrap().selected,
        ReoptimizationChoice::Preserve
    );
}
