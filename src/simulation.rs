//! Continuous deterministic synthetic execution. See `docs/simulation.md`.

mod config;
mod engine;
mod rng;

pub use config::{ArrivalProcess, PaymentFlow, RailService, RoutingStrategy, Scenario};

use std::{collections::VecDeque, error::Error, fmt};

use crate::{network::Payment, network::ValidationError, routing::Route, routing::RouteHop};
use rng::Random;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationError {
    Validation(ValidationError),
    ArithmeticOverflow(&'static str),
    Invariant(String),
}

impl fmt::Display for SimulationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(f),
            Self::ArithmeticOverflow(field) => {
                write!(f, "simulation arithmetic exhausted: {field}")
            }
            Self::Invariant(message) => write!(f, "simulation invariant violated: {message}"),
        }
    }
}

impl Error for SimulationError {}

impl From<ValidationError> for SimulationError {
    fn from(value: ValidationError) -> Self {
        Self::Validation(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metrics {
    pub generated: u128,
    pub generated_volume_cents: u128,
    pub accepted_routes: u128,
    pub rejected: u128,
    pub rejected_volume_cents: u128,
    pub expired: u128,
    pub expired_volume_cents: u128,
    /// Includes late final-hop settlements; every principal is counted once.
    pub completed: u128,
    pub completed_volume_cents: u128,
    pub completed_late: u128,
    pub completed_late_volume_cents: u128,
    /// Rejections + missed deadlines, counted once even while work drains.
    pub sla_failures: u128,
    pub sla_failed_volume_cents: u128,
    /// Actual executed fees, including work that ultimately fails its SLA.
    pub routing_cost_cents: u128,
    /// End-to-end elapsed minutes for completed payments, including waiting.
    pub completed_elapsed_minutes: u128,
    pub departed_hops: u128,
    pub settled_hops: u128,
    /// Hop volume can exceed end-to-end generated/completed volume.
    pub departed_principal_cents: u128,
    pub settled_principal_cents: u128,
}

/// Read-only through Simulator. A pinned route and at most one in-flight hop
/// replace a growing schedule/event heap. next_hop counts settled hops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivePayment {
    pub sequence: u128,
    pub payment: Payment,
    pub arrived_at: u128,
    pub deadline: u128,
    pub route: Option<Route>,
    pub next_hop: usize,
    pub in_flight_until: Option<u128>,
    pub sla_failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailState {
    pub rail_id: String,
    /// Availability sampled in the most recently processed minute.
    pub open: bool,
    pub used_this_minute_cents: u128,
    pub departed_principal_cents: u128,
    pub settled_principal_cents: u128,
    pub departed_hops: u128,
    pub settled_hops: u128,
    pub routing_cost_cents: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    RailTick {
        rail_id: String,
        open: bool,
        capacity_cents: Option<u64>,
    },
    Generated {
        sequence: u128,
        payment: Payment,
        deadline: u128,
    },
    Rejected {
        sequence: u128,
    },
    RouteAccepted {
        sequence: u128,
        route: Route,
    },
    HopDeparted {
        sequence: u128,
        hop: RouteHop,
        amount_cents: u64,
        fee_cents: u64,
        arrival_minute: u128,
    },
    HopSettled {
        sequence: u128,
        hop: RouteHop,
        amount_cents: u64,
    },
    DeadlineMissed {
        sequence: u128,
    },
    Expired {
        sequence: u128,
    },
    Completed {
        sequence: u128,
        late: bool,
        elapsed_minutes: u128,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Contiguous zero-based sequence across every event, including discarded history.
    pub sequence: u128,
    pub minute: u128,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickReport {
    pub minute: u128,
    /// Complete ordered events for this tick, independent of retained history.
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    next_minute: u128,
    next_event: u128,
    random: Random,
    metrics: Metrics,
    active: Vec<ActivePayment>,
    rails: Vec<RailState>,
}

/// A bounded-state deterministic model. Pacing belongs to the caller.
/// `step` is transactional; an error commits neither work, RNG nor history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Simulator {
    scenario: Scenario,
    seed: u64,
    running: bool,
    state: State,
    history: VecDeque<Event>,
}

impl Simulator {
    pub fn new(mut scenario: Scenario, seed: u64) -> Result<Self, SimulationError> {
        scenario.validate()?;
        // Align fixed-size rail state and service records with a canonical network.
        scenario
            .network
            .rails
            .sort_unstable_by(|a, b| a.id.cmp(&b.id));
        scenario
            .services
            .sort_unstable_by(|a, b| a.rail_id.cmp(&b.rail_id));
        let state = Self::initial_state(&scenario, seed);
        Ok(Self {
            scenario,
            seed,
            running: false,
            state,
            history: VecDeque::new(),
        })
    }

    fn initial_state(scenario: &Scenario, seed: u64) -> State {
        State {
            next_minute: 0,
            next_event: 0,
            random: Random(seed),
            metrics: Metrics::default(),
            active: vec![],
            rails: scenario
                .network
                .rails
                .iter()
                .map(|rail| RailState {
                    rail_id: rail.id.clone(),
                    open: false,
                    used_this_minute_cents: 0,
                    departed_principal_cents: 0,
                    settled_principal_cents: 0,
                    departed_hops: 0,
                    settled_hops: 0,
                    routing_cost_cents: 0,
                })
                .collect(),
        }
    }

    pub fn start(&mut self) {
        self.running = true;
    }
    pub fn pause(&mut self) {
        self.running = false;
    }
    pub fn is_running(&self) -> bool {
        self.running
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn next_minute(&self) -> u128 {
        self.state.next_minute
    }
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }
    pub fn metrics(&self) -> &Metrics {
        &self.state.metrics
    }
    pub fn active_payments(&self) -> &[ActivePayment] {
        &self.state.active
    }
    pub fn rail_states(&self) -> &[RailState] {
        &self.state.rails
    }
    pub fn recent_events(&self) -> &VecDeque<Event> {
        &self.history
    }
    pub fn event_count(&self) -> u128 {
        self.state.next_event
    }

    /// Restore exactly a fresh, paused run with the same scenario and this seed.
    pub fn restart(&mut self, seed: u64) {
        self.state = Self::initial_state(&self.scenario, seed);
        self.seed = seed;
        self.running = false;
        self.history.clear();
    }

    /// Driver tick: a paused simulator does not advance or consume randomness.
    pub fn tick(&mut self) -> Result<Option<TickReport>, SimulationError> {
        if self.running {
            self.step().map(Some)
        } else {
            Ok(None)
        }
    }

    /// One minute regardless of pause/speed. The first step processes minute zero.
    pub fn step(&mut self) -> Result<TickReport, SimulationError> {
        // Only bounded mutable state is copied. History and fixed configuration
        // are not cloned per tick. Errors discard the working transaction.
        let mut next = self.state.clone();
        let report = next.process_tick(&self.scenario)?;
        next.check_invariants(&self.scenario)?;
        self.state = next;
        for event in &report.events {
            if self.scenario.retained_events > 0 {
                if self.history.len() == self.scenario.retained_events {
                    self.history.pop_front();
                }
                self.history.push_back(event.clone());
            }
        }
        Ok(report)
    }

    /// Repeat manual steps without retaining their reports. Earlier successful
    /// ticks remain committed if a subsequent tick encounters numeric exhaustion.
    pub fn advance_ticks(&mut self, count: u64) -> Result<(), SimulationError> {
        for _ in 0..count {
            self.step()?;
        }
        Ok(())
    }

    /// Also run automatically before each tick commits.
    pub fn check_invariants(&self) -> Result<(), SimulationError> {
        self.state.check_invariants(&self.scenario)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_network;

    fn simulator() -> Simulator {
        let mut network = demo_network();
        for rail in &mut network.rails {
            rail.settlement_minutes = 0;
        }
        let services = network
            .rails
            .iter()
            .map(|r| RailService {
                rail_id: r.id.clone(),
                period_minutes: 1,
                offset_minutes: 0,
                open_minutes: 1,
                capacity_per_minute_cents: None,
            })
            .collect();
        let scenario = Scenario {
            arrivals: ArrivalProcess {
                attempts_per_minute: 1,
                probability_per_million: 1_000_000,
                flows: vec![PaymentFlow {
                    sender: network.institutions[0].id.clone(),
                    receiver: network.institutions[1].id.clone(),
                }],
                min_amount_cents: 1,
                max_amount_cents: 1,
                min_sla_minutes: 0,
                max_sla_minutes: 0,
            },
            network,
            services,
            strategy: RoutingStrategy::CheapestStatic,
            max_active_payments: 2,
            retained_events: 8,
        };
        Simulator::new(scenario, 0).unwrap()
    }

    #[test]
    fn ticks_cross_u64_time_without_a_horizon_or_wrapping() {
        let mut sim = simulator();
        sim.state.next_minute = u128::from(u64::MAX);
        let report = sim.step().unwrap();
        assert_eq!(report.minute, u128::from(u64::MAX));
        assert_eq!(sim.next_minute(), u128::from(u64::MAX) + 1);
        sim.step().unwrap();
        assert_eq!(sim.metrics().completed, 2);
    }

    #[test]
    fn numeric_exhaustion_rolls_back_rng_payments_metrics_and_events_for_the_whole_tick() {
        for field in 0..4 {
            let mut sim = simulator();
            match field {
                0 => sim.state.next_minute = u128::MAX,
                1 => sim.state.next_event = u128::MAX - 2,
                2 => sim.state.metrics.generated_volume_cents = u128::MAX,
                _ => sim.state.metrics.routing_cost_cents = u128::MAX,
            }
            let before = sim.clone();
            assert!(
                matches!(sim.step(), Err(SimulationError::ArithmeticOverflow(_))),
                "field {field}"
            );
            assert_eq!(sim, before, "field {field}");
        }
    }

    #[test]
    fn invariant_failure_rejects_the_tick_without_committing() {
        let mut sim = simulator();
        sim.state.metrics.generated = 1;
        let before = sim.clone();
        assert!(matches!(sim.step(), Err(SimulationError::Invariant(_))));
        assert_eq!(sim, before);
    }
}
