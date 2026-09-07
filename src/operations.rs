//! Terminal-independent operator sessions: twin deterministic runs and bounded dossiers.
use std::collections::{BTreeMap, VecDeque};

use crate::{
    demo::demo_network,
    network::Payment,
    observation::{EVIDENCE_PER_PAYMENT, PaymentEvidence},
    routing::Route,
    scalable::SearchLimits,
    simulation::*,
};

pub const RECENT_PAYMENTS: usize = 128;
pub const PAYMENT_EVENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Balanced,
    Pressure,
    Outage,
    Limited,
}
impl Preset {
    pub const ALL: [Self; 4] = [Self::Balanced, Self::Pressure, Self::Outage, Self::Limited];
    pub fn name(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Pressure => "pressure",
            Self::Outage => "outage",
            Self::Limited => "limited",
        }
    }
    pub fn scenario(self, strategy: RoutingStrategy) -> Scenario {
        let mut network = demo_network();
        // This execution fixture is separate from demo_network's static contract.
        network.payments.clear();
        for rail in &mut network.rails {
            rail.settlement_minutes = match rail.id.as_str() {
                "ACH" => 5,
                "FEDWIRE" => 2,
                _ => 0,
            };
            if self == Self::Outage && rail.id == "ACH" {
                rail.available = false;
            }
        }
        let flows = network
            .institutions
            .iter()
            .flat_map(|sender| {
                network
                    .institutions
                    .iter()
                    .filter(|receiver| sender.id != receiver.id)
                    .map(|receiver| PaymentFlow {
                        sender: sender.id.clone(),
                        receiver: receiver.id.clone(),
                    })
            })
            .collect();
        let services = network
            .rails
            .iter()
            .map(|rail| {
                let (period, open, capacity) = match rail.id.as_str() {
                    "ACH" => (8, 3, 200_000),
                    "FEDWIRE" => (4, 3, 200_000),
                    _ => (1, 1, 100_000),
                };
                RailService {
                    rail_id: rail.id.clone(),
                    period_minutes: period,
                    offset_minutes: 0,
                    open_minutes: open,
                    capacity_per_minute_cents: Some(capacity),
                }
            })
            .collect();
        Scenario {
            network,
            services,
            arrivals: ArrivalProcess {
                attempts_per_minute: if self == Self::Pressure { 12 } else { 3 },
                probability_per_million: 650_000,
                flows,
                min_amount_cents: 10_000,
                max_amount_cents: 150_000,
                min_sla_minutes: 0,
                max_sla_minutes: 12,
            },
            strategy,
            max_active_payments: if self == Self::Pressure { 16 } else { 64 },
            retained_events: 128,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatus {
    Queued,
    Waiting,
    InFlight,
    Draining,
    Completed,
    Late,
    Expired,
    Rejected,
}
impl PaymentStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "UNROUTED",
            Self::Waiting => "WAIT SLOT",
            Self::InFlight => "IN FLIGHT",
            Self::Draining => "DRAIN SLA",
            Self::Completed => "DONE",
            Self::Late => "LATE",
            Self::Expired => "EXPIRED",
            Self::Rejected => "OVERLOAD",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Late | Self::Expired | Self::Rejected
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dossier {
    pub sequence: u128,
    pub payment: Payment,
    pub arrived_at: u128,
    pub deadline: u128,
    pub status: PaymentStatus,
    pub route: Option<Route>,
    pub actual_fee_cents: u128,
    pub departed_hops: usize,
    pub completed_elapsed_minutes: Option<u128>,
    pub decision_minute: Option<u128>,
    pub evidence: PaymentEvidence,
    pub planned_departures: Option<Vec<u128>>,
    pub events: VecDeque<Event>,
    pub omitted_events: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub minute: u128,
    pub queued: usize,
    pub in_flight: usize,
    pub completed: u128,
    pub sla_failures: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedRun {
    pub simulator: Simulator,
    pub payments: BTreeMap<u128, Dossier>,
    pub samples: VecDeque<Sample>,
}
impl ObservedRun {
    fn new(scenario: Scenario, seed: u64) -> Result<Self, SimulationError> {
        Ok(Self {
            simulator: Simulator::new(scenario, seed)?,
            payments: BTreeMap::new(),
            samples: VecDeque::new(),
        })
    }
    fn step(&mut self) -> Result<(), SimulationError> {
        let (report, mut evidence) = self.simulator.step_observed()?;
        for event in report.events {
            let sequence = match &event.kind {
                EventKind::RailTick { .. } => continue,
                EventKind::Generated {
                    sequence,
                    payment,
                    deadline,
                } => {
                    self.payments.insert(
                        *sequence,
                        Dossier {
                            sequence: *sequence,
                            payment: payment.clone(),
                            arrived_at: event.minute,
                            deadline: *deadline,
                            status: PaymentStatus::Queued,
                            route: None,
                            actual_fee_cents: 0,
                            departed_hops: 0,
                            completed_elapsed_minutes: None,
                            decision_minute: None,
                            evidence: Default::default(),
                            planned_departures: None,
                            events: VecDeque::new(),
                            omitted_events: 0,
                        },
                    );
                    *sequence
                }
                EventKind::Rejected { sequence }
                | EventKind::RouteAccepted { sequence, .. }
                | EventKind::HopDeparted { sequence, .. }
                | EventKind::HopSettled { sequence, .. }
                | EventKind::DeadlineMissed { sequence }
                | EventKind::Expired { sequence }
                | EventKind::Completed { sequence, .. } => *sequence,
            };
            let p = self
                .payments
                .get_mut(&sequence)
                .expect("active dossier retained");
            match &event.kind {
                EventKind::RouteAccepted { route, .. } => {
                    p.route = Some(route.clone());
                    p.decision_minute = Some(event.minute);
                }
                EventKind::Rejected { .. } => p.status = PaymentStatus::Rejected,
                EventKind::Expired { .. } => p.status = PaymentStatus::Expired,
                EventKind::HopDeparted { fee_cents, .. } => {
                    p.actual_fee_cents += u128::from(*fee_cents);
                    p.departed_hops += 1;
                }
                EventKind::Completed {
                    late,
                    elapsed_minutes,
                    ..
                } => {
                    p.completed_elapsed_minutes = Some(*elapsed_minutes);
                    p.status = if *late {
                        PaymentStatus::Late
                    } else {
                        PaymentStatus::Completed
                    }
                }
                _ => {}
            }
            if p.events.len() == PAYMENT_EVENTS {
                p.events.pop_front();
                p.omitted_events += 1;
            }
            p.events.push_back(event);
        }
        for p in self.payments.values_mut() {
            if let Some(trace) = evidence.payments.remove(&p.payment.id) {
                p.evidence = trace;
                p.decision_minute = Some(report.minute);
                debug_assert!(p.evidence.entries.len() <= EVIDENCE_PER_PAYMENT);
            }
        }
        for active in self.simulator.active_payments() {
            let p = self
                .payments
                .get_mut(&active.sequence)
                .expect("active dossier retained");
            p.planned_departures.clone_from(&active.planned_departures);
            p.status = if active.sla_failed {
                PaymentStatus::Draining
            } else if active.in_flight_until.is_some() {
                PaymentStatus::InFlight
            } else if active.route.is_some() {
                PaymentStatus::Waiting
            } else {
                PaymentStatus::Queued
            };
        }
        let remove = self
            .payments
            .values()
            .filter(|p| p.status.terminal())
            .count()
            .saturating_sub(RECENT_PAYMENTS);
        let ids: Vec<_> = self
            .payments
            .iter()
            .filter(|(_, p)| p.status.terminal())
            .take(remove)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.payments.remove(&id);
        }
        if self.samples.len() == 120 {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            minute: report.minute,
            queued: self
                .simulator
                .active_payments()
                .iter()
                .filter(|p| p.in_flight_until.is_none())
                .count(),
            in_flight: self
                .simulator
                .active_payments()
                .iter()
                .filter(|p| p.in_flight_until.is_some())
                .count(),
            completed: self.simulator.metrics().completed,
            sla_failures: self.simulator.metrics().sla_failures,
        });
        Ok(())
    }
}

/// Both strategies consume identical demand at the same simulated minute.
/// A failure commits neither run nor its observations; pacing lives in the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operations {
    pub preset: Preset,
    pub runs: [ObservedRun; 2],
}
impl Operations {
    pub fn new(preset: Preset, seed: u64) -> Result<Self, SimulationError> {
        let limits = if preset == Preset::Limited {
            SearchLimits {
                max_candidates: 2,
                max_expansions: 1,
                ..Default::default()
            }
        } else {
            Default::default()
        };
        Ok(Self {
            preset,
            runs: [
                ObservedRun::new(preset.scenario(RoutingStrategy::CheapestStatic), seed)?,
                ObservedRun::new(preset.scenario(RoutingStrategy::Reserved { limits }), seed)?,
            ],
        })
    }
    pub fn step(&mut self) -> Result<(), SimulationError> {
        let mut next = self.clone();
        for run in &mut next.runs {
            run.step()?;
        }
        debug_assert_eq!(
            next.runs[0].simulator.next_minute(),
            next.runs[1].simulator.next_minute()
        );
        debug_assert_eq!(
            next.runs[0].simulator.metrics().generated,
            next.runs[1].simulator.metrics().generated
        );
        *self = next;
        Ok(())
    }
    pub fn restart(&mut self, seed: u64) -> Result<(), SimulationError> {
        *self = Self::new(self.preset, seed)?;
        Ok(())
    }
}
