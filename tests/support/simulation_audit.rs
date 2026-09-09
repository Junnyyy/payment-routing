//! Independent event accounting: reconstruct lifecycle, fees, capacity and
//! metrics from witnesses. No simulation helpers or routing search are shared.
use payment_routing::{network::Payment, routing::Route, simulation::*};
use std::collections::{BTreeMap, BTreeSet};

struct Record {
    payment: Payment,
    release: u128,
    deadline: u128,
    at: String,
    route: Option<Route>,
    next_hop: usize,
    in_flight: Option<u128>,
    failed: bool,
}

#[derive(Default)]
pub struct Audit {
    next_event: u128,
    next_minute: u128,
    live: BTreeMap<u128, Record>,
    metrics: Metrics,
    rails: BTreeMap<String, RailState>,
    conditions: BTreeMap<String, RailConditions>,
}

impl Audit {
    pub fn check(&mut self, scenario: &Scenario, report: &TickReport, sim: &Simulator) {
        assert_eq!(report.minute, self.next_minute);
        self.next_minute += 1;
        let minute = report.minute;
        let mut reset = BTreeSet::new();
        for event in &report.events {
            if let EventKind::DisruptionApplied(change) = &event.kind {
                let rail = scenario
                    .network
                    .rails
                    .iter()
                    .find(|r| r.id == change.rail_id)
                    .unwrap();
                let service = scenario
                    .services
                    .iter()
                    .find(|s| s.rail_id == change.rail_id)
                    .unwrap();
                let before =
                    self.conditions
                        .get(&change.rail_id)
                        .copied()
                        .unwrap_or(RailConditions {
                            available: rail.available,
                            capacity_per_minute_cents: service.capacity_per_minute_cents,
                        });
                assert_eq!(before, change.before);
                assert_ne!(before, change.after);
                self.conditions.insert(change.rail_id.clone(), change.after);
            }
        }
        let open = |id: &str| {
            let rail = scenario.network.rails.iter().find(|r| r.id == id).unwrap();
            let service = scenario.services.iter().find(|s| s.rail_id == id).unwrap();
            let phase = minute % u128::from(service.period_minutes);
            self.conditions
                .get(id)
                .map_or(rail.available, |c| c.available)
                && (u128::from(service.offset_minutes)
                    ..u128::from(service.offset_minutes) + u128::from(service.open_minutes))
                    .contains(&phase)
        };
        for event in &report.events {
            assert_eq!((event.sequence, event.minute), (self.next_event, minute));
            self.next_event += 1;
            if !matches!(event.kind, EventKind::RailTick { .. }) {
                assert_eq!(reset.len(), scenario.network.rails.len());
            }
            match &event.kind {
                EventKind::RailTick {
                    rail_id,
                    open: available,
                    capacity_cents,
                } => {
                    assert!(reset.insert(rail_id.clone()));
                    assert_eq!(*available, open(rail_id));
                    let service = scenario
                        .services
                        .iter()
                        .find(|s| &s.rail_id == rail_id)
                        .unwrap();
                    assert_eq!(
                        *capacity_cents,
                        self.conditions
                            .get(rail_id)
                            .map_or(service.capacity_per_minute_cents, |c| c
                                .capacity_per_minute_cents)
                    );
                    let state = self.rails.entry(rail_id.clone()).or_insert(RailState {
                        rail_id: rail_id.clone(),
                        open: false,
                        used_this_minute_cents: 0,
                        departed_principal_cents: 0,
                        settled_principal_cents: 0,
                        departed_hops: 0,
                        settled_hops: 0,
                        routing_cost_cents: 0,
                    });
                    state.open = *available;
                    state.used_this_minute_cents = 0;
                }
                EventKind::DisruptionApplied(_) | EventKind::Reoptimized(_) => {}
                EventKind::PlanRevised {
                    sequence,
                    route,
                    planned_departures,
                } => {
                    let p = self.live.get_mut(sequence).unwrap();
                    let fixed = p.next_hop + usize::from(p.in_flight.is_some());
                    let old_prefix = p.route.as_ref().map_or(&[][..], |r| &r.hops[..fixed]);
                    let new_hops = route.as_ref().map_or(&[][..], |r| r.hops.as_slice());
                    assert!(new_hops.len() >= fixed);
                    assert_eq!(&new_hops[..fixed], old_prefix);
                    if let Some(times) = planned_departures {
                        assert_eq!(times.len(), new_hops.len());
                    }
                    let mut at = p.payment.sender.clone();
                    let mut visited = BTreeSet::from([at.clone()]);
                    let (mut fee, mut latency) = (0, 0);
                    for h in new_hops {
                        let r = scenario
                            .network
                            .rails
                            .iter()
                            .find(|r| r.id == h.rail_id)
                            .unwrap();
                        assert_eq!(h.sender, at);
                        assert!(
                            r.participants.contains(&h.sender)
                                && r.participants.contains(&h.receiver)
                        );
                        assert!(visited.insert(h.receiver.clone()));
                        assert!(
                            r.max_amount_cents
                                .is_none_or(|c| p.payment.amount_cents <= c)
                        );
                        at = h.receiver.clone();
                        fee += u128::from(r.fee_cents);
                        latency += u128::from(r.settlement_minutes);
                    }
                    assert!(new_hops.len() == fixed || at == p.payment.receiver);
                    if let Some(r) = route {
                        assert_eq!(
                            (r.total_fee_cents, r.total_settlement_minutes),
                            (fee, latency)
                        );
                    }
                    p.route = route.clone();
                }
                EventKind::Generated {
                    sequence,
                    payment,
                    deadline,
                } => {
                    self.metrics.generated += 1;
                    self.metrics.generated_volume_cents += u128::from(payment.amount_cents);
                    assert_eq!(*sequence, self.metrics.generated);
                    assert_eq!(payment.id, format!("SIM-{sequence}"));
                    let a = &scenario.arrivals;
                    assert!(
                        (a.min_amount_cents..=a.max_amount_cents).contains(&payment.amount_cents)
                    );
                    assert!(
                        (u128::from(a.min_sla_minutes)..=u128::from(a.max_sla_minutes))
                            .contains(&(deadline - minute))
                    );
                    assert!(
                        a.flows
                            .iter()
                            .any(|f| f.sender == payment.sender && f.receiver == payment.receiver)
                    );
                    assert_eq!(
                        payment.max_delivery_minutes.map(u128::from),
                        Some(deadline - minute)
                    );
                    assert!(
                        self.live
                            .insert(
                                *sequence,
                                Record {
                                    payment: payment.clone(),
                                    release: minute,
                                    deadline: *deadline,
                                    at: payment.sender.clone(),
                                    route: None,
                                    next_hop: 0,
                                    in_flight: None,
                                    failed: false,
                                }
                            )
                            .is_none()
                    );
                    assert!(self.live.len() <= scenario.max_active_payments + 1);
                }
                EventKind::Rejected { sequence } => {
                    assert_eq!(self.live.len(), scenario.max_active_payments + 1);
                    let p = self.live.remove(sequence).unwrap();
                    assert_eq!(p.release, minute);
                    assert!(p.route.is_none() && p.in_flight.is_none());
                    self.metrics.rejected += 1;
                    self.metrics.rejected_volume_cents += u128::from(p.payment.amount_cents);
                    self.metrics.sla_failures += 1;
                    self.metrics.sla_failed_volume_cents += u128::from(p.payment.amount_cents);
                }
                EventKind::RouteAccepted { sequence, route } => {
                    let p = self.live.get_mut(sequence).unwrap();
                    assert!(p.route.is_none() && p.in_flight.is_none() && !p.failed);
                    assert!(!route.hops.is_empty());
                    let mut at = p.payment.sender.clone();
                    let mut visited = BTreeSet::from([at.clone()]);
                    let (mut fee, mut latency) = (0, 0);
                    for hop in &route.hops {
                        let r = scenario
                            .network
                            .rails
                            .iter()
                            .find(|r| r.id == hop.rail_id)
                            .unwrap();
                        assert_eq!(hop.sender, at);
                        assert!(
                            r.participants.contains(&hop.sender)
                                && r.participants.contains(&hop.receiver)
                        );
                        assert!(visited.insert(hop.receiver.clone()));
                        assert!(
                            self.conditions
                                .get(&r.id)
                                .map_or(r.available, |c| c.available)
                        );
                        if scenario.strategy == RoutingStrategy::CheapestStatic {
                            assert!(open(&hop.rail_id));
                        }
                        assert!(
                            r.max_amount_cents
                                .is_none_or(|c| p.payment.amount_cents <= c)
                        );
                        at.clone_from(&hop.receiver);
                        fee += u128::from(r.fee_cents);
                        latency += u128::from(r.settlement_minutes);
                    }
                    assert_eq!(at, p.payment.receiver);
                    assert_eq!(
                        (route.total_fee_cents, route.total_settlement_minutes),
                        (fee, latency)
                    );
                    assert!(minute + latency <= p.deadline);
                    p.route = Some(route.clone());
                    self.metrics.accepted_routes += 1;
                }
                EventKind::HopDeparted {
                    sequence,
                    hop,
                    amount_cents,
                    fee_cents,
                    arrival_minute,
                } => {
                    let p = self.live.get_mut(sequence).unwrap();
                    assert!(!p.failed && p.in_flight.is_none() && minute <= p.deadline);
                    assert_eq!(p.route.as_ref().unwrap().hops[p.next_hop], *hop);
                    assert_eq!(hop.sender, p.at);
                    assert_eq!(*amount_cents, p.payment.amount_cents);
                    let r = scenario
                        .network
                        .rails
                        .iter()
                        .find(|r| r.id == hop.rail_id)
                        .unwrap();
                    assert_eq!(*fee_cents, r.fee_cents);
                    assert_eq!(*arrival_minute, minute + u128::from(r.settlement_minutes));
                    if matches!(scenario.strategy, RoutingStrategy::Reserved { .. }) {
                        assert!(*arrival_minute <= p.deadline);
                    }
                    assert!(open(&hop.rail_id));
                    assert!(r.max_amount_cents.is_none_or(|c| *amount_cents <= c));
                    let amount = u128::from(*amount_cents);
                    let state = self.rails.get_mut(&hop.rail_id).unwrap();
                    state.used_this_minute_cents += amount;
                    let s = scenario
                        .services
                        .iter()
                        .find(|s| s.rail_id == hop.rail_id)
                        .unwrap();
                    assert!(
                        self.conditions
                            .get(&hop.rail_id)
                            .map_or(s.capacity_per_minute_cents, |c| c.capacity_per_minute_cents)
                            .is_none_or(|c| state.used_this_minute_cents <= u128::from(c))
                    );
                    state.departed_hops += 1;
                    state.departed_principal_cents += amount;
                    state.routing_cost_cents += u128::from(*fee_cents);
                    self.metrics.departed_hops += 1;
                    self.metrics.departed_principal_cents += amount;
                    self.metrics.routing_cost_cents += u128::from(*fee_cents);
                    p.in_flight = Some(*arrival_minute);
                }
                EventKind::HopSettled {
                    sequence,
                    hop,
                    amount_cents,
                } => {
                    let p = self.live.get_mut(sequence).unwrap();
                    assert_eq!(p.in_flight.take(), Some(minute));
                    assert_eq!(p.route.as_ref().unwrap().hops[p.next_hop], *hop);
                    assert_eq!(*amount_cents, p.payment.amount_cents);
                    assert_eq!(p.at, hop.sender);
                    p.at.clone_from(&hop.receiver);
                    p.next_hop += 1;
                    let state = self.rails.get_mut(&hop.rail_id).unwrap();
                    state.settled_hops += 1;
                    state.settled_principal_cents += u128::from(*amount_cents);
                    self.metrics.settled_hops += 1;
                    self.metrics.settled_principal_cents += u128::from(*amount_cents);
                }
                EventKind::DeadlineMissed { sequence } => {
                    let p = self.live.get_mut(sequence).unwrap();
                    assert!(!p.failed);
                    assert_eq!(minute, p.deadline);
                    assert_ne!(p.at, p.payment.receiver);
                    p.failed = true;
                    self.metrics.sla_failures += 1;
                    self.metrics.sla_failed_volume_cents += u128::from(p.payment.amount_cents);
                }
                EventKind::Expired { sequence } => {
                    let p = self.live.remove(sequence).unwrap();
                    assert!(p.failed && p.in_flight.is_none());
                    assert_ne!(p.at, p.payment.receiver);
                    self.metrics.expired += 1;
                    self.metrics.expired_volume_cents += u128::from(p.payment.amount_cents);
                }
                EventKind::Completed {
                    sequence,
                    late,
                    elapsed_minutes,
                } => {
                    let p = self.live.remove(sequence).unwrap();
                    assert_eq!(p.at, p.payment.receiver);
                    assert!(p.in_flight.is_none());
                    assert_eq!(p.next_hop, p.route.as_ref().unwrap().hops.len());
                    assert_eq!(*late, minute > p.deadline);
                    assert_eq!(p.failed, *late);
                    assert_eq!(*elapsed_minutes, minute - p.release);
                    self.metrics.completed += 1;
                    self.metrics.completed_volume_cents += u128::from(p.payment.amount_cents);
                    self.metrics.completed_elapsed_minutes += elapsed_minutes;
                    if *late {
                        self.metrics.completed_late += 1;
                        self.metrics.completed_late_volume_cents +=
                            u128::from(p.payment.amount_cents);
                    }
                }
            }
        }
        assert_eq!(reset.len(), scenario.network.rails.len());
        if matches!(scenario.strategy, RoutingStrategy::Reserved { .. }) {
            let mut future = BTreeMap::<(String, u128), u128>::new();
            for p in sim.active_payments() {
                if let Some(route) = &p.route {
                    let times = p.planned_departures.as_ref().unwrap();
                    assert_eq!(times.len(), route.hops.len());
                    let fixed = p.next_hop + usize::from(p.in_flight_until.is_some());
                    let mut ready = p.in_flight_until.unwrap_or(sim.next_minute());
                    for (h, &departure) in route.hops.iter().zip(times).skip(fixed) {
                        let r = scenario
                            .network
                            .rails
                            .iter()
                            .find(|r| r.id == h.rail_id)
                            .unwrap();
                        let service = scenario
                            .services
                            .iter()
                            .find(|s| s.rail_id == h.rail_id)
                            .unwrap();
                        let condition =
                            self.conditions
                                .get(&h.rail_id)
                                .copied()
                                .unwrap_or(RailConditions {
                                    available: r.available,
                                    capacity_per_minute_cents: service.capacity_per_minute_cents,
                                });
                        assert!(condition.available);
                        assert!(departure >= ready);
                        let phase = departure % u128::from(service.period_minutes);
                        assert!(
                            phase >= u128::from(service.offset_minutes)
                                && phase - u128::from(service.offset_minutes)
                                    < u128::from(service.open_minutes)
                        );
                        assert!(
                            r.max_amount_cents
                                .is_none_or(|c| p.payment.amount_cents <= c)
                        );
                        ready = departure + u128::from(r.settlement_minutes);
                        if let Some(cap) = condition.capacity_per_minute_cents {
                            let used = future.entry((h.rail_id.clone(), departure)).or_default();
                            *used += u128::from(p.payment.amount_cents);
                            assert!(*used <= u128::from(cap));
                        }
                    }
                    if route.hops.last().unwrap().receiver == p.payment.receiver {
                        assert!(ready <= p.deadline);
                    }
                }
            }
            let current = self
                .rails
                .iter()
                .filter(|(id, state)| {
                    let service = scenario
                        .services
                        .iter()
                        .find(|s| &s.rail_id == *id)
                        .unwrap();
                    state.used_this_minute_cents > 0
                        && self
                            .conditions
                            .get(*id)
                            .map_or(service.capacity_per_minute_cents, |c| {
                                c.capacity_per_minute_cents
                            })
                            .is_some()
                })
                .count();
            assert_eq!(future.len() + current, sim.reservation_entries());
        }
        assert_eq!(&self.metrics, sim.metrics());
        assert_eq!(
            self.rails.values().collect::<Vec<_>>(),
            sim.rail_states().iter().collect::<Vec<_>>()
        );
        assert_eq!(self.live.len(), sim.active_payments().len());
        for ((sequence, p), actual) in self.live.iter().zip(sim.active_payments()) {
            assert_eq!(*sequence, actual.sequence);
            assert_eq!(p.payment, actual.payment);
            assert_eq!(
                (p.release, p.deadline, p.next_hop, p.failed, p.in_flight),
                (
                    actual.arrived_at,
                    actual.deadline,
                    actual.next_hop,
                    actual.sla_failed,
                    actual.in_flight_until
                )
            );
            assert_eq!(p.route, actual.route);
            assert!(
                p.deadline > minute
                    || (p.failed && p.in_flight.is_some_and(|arrival| arrival > minute))
            );
        }
    }
}
