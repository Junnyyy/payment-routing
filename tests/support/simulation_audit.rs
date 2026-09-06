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
}

impl Audit {
    pub fn check(&mut self, scenario: &Scenario, report: &TickReport, sim: &Simulator) {
        assert_eq!(report.minute, self.next_minute);
        self.next_minute += 1;
        let minute = report.minute;
        let mut reset = BTreeSet::new();
        let open = |id: &str| {
            let rail = scenario.network.rails.iter().find(|r| r.id == id).unwrap();
            let service = scenario.services.iter().find(|s| s.rail_id == id).unwrap();
            let phase = minute % u128::from(service.period_minutes);
            rail.available
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
                    assert_eq!(*capacity_cents, service.capacity_per_minute_cents);
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
                        assert!(r.available);
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
                        s.capacity_per_minute_cents
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
