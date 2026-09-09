//! Paired finite-cohort evaluation. The world driver owns unrevealed demand.
//! Metric definitions and the information boundary are in `docs/evaluation.md`.
mod report;
pub mod scenarios;
pub use report::{Aggregate, CaseKey, PairComparison, Ratio};

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    network::{Payment, ValidationError},
    scalable::SearchDiagnostics,
    simulation::*,
};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    pub name: String,
    pub routing: RoutingStrategy,
    pub reoptimization: ReoptimizationPolicy,
}

impl Strategy {
    pub fn named(name: &str) -> Option<Self> {
        let routing = match name {
            "static" => RoutingStrategy::CheapestStatic,
            "reserved" | "preserve" | "recompute" => RoutingStrategy::Reserved {
                limits: Default::default(),
            },
            "tight" => RoutingStrategy::Reserved {
                limits: crate::scalable::SearchLimits {
                    labels_per_node: 1,
                    max_labels: 8,
                    max_expansions: 4,
                    max_candidates: 8,
                    max_repairs: 0,
                },
            },
            _ => return None,
        };
        Some(Self {
            name: name.into(),
            routing,
            reoptimization: match name {
                "preserve" => ReoptimizationPolicy::Preserve,
                "recompute" => ReoptimizationPolicy::Recompute,
                _ => ReoptimizationPolicy::default(),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub name: String,
    /// strategy is overridden by each Strategy; every other world field is shared.
    pub scenario: Scenario,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Generate at minutes 0..arrival_minutes, then stop admission for all runs.
    pub arrival_minutes: u64,
    /// Process exactly this many additional minutes, including surprise changes.
    pub drain_minutes: u64,
    /// Repeat each tick in a fresh twin and compare events and entire state.
    pub verify_replay: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            arrival_minutes: 60,
            drain_minutes: 60,
            verify_replay: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Rejected,
    Expired,
    Completed { late: bool, elapsed_minutes: u128 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentResult {
    pub sequence: u128,
    pub payment: Payment,
    pub arrived_at: u128,
    pub deadline: u128,
    pub terminal_at: Option<u128>,
    pub outcome: Outcome,
    pub ever_routed: bool,
    pub sla_failed: bool,
    pub actual_fee_cents: u128,
    pub departed_hops: u128,
}
impl PaymentResult {
    pub fn on_time(&self) -> bool {
        matches!(self.outcome, Outcome::Completed { late: false, .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Complete,
    Censored,
    Error { minute: u128, message: String },
}

/// Smaller is better, in this declared lexicographic order. Complete runs only.
/// This is an evaluation preference, never an online optimum certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score {
    pub not_on_time: u128,
    pub not_on_time_volume_cents: u128,
    pub not_completed: u128,
    pub not_completed_volume_cents: u128,
    pub actual_fee_cents: u128,
    pub completed_elapsed_minutes: u128,
    pub departed_hops: u128,
    pub changed_assignments: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub strategy: String,
    pub status: Status,
    pub processed_minutes: u128,
    pub replay_verified: bool,
    pub metrics: Metrics,
    pub at_arrival_cutoff: Metrics,
    pub adaptation: AdaptationMetrics,
    pub diagnostics: SearchDiagnostics,
    pub payments: BTreeMap<u128, PaymentResult>,
    pub peak_active: usize,
    /// Queue samples are after execution/expiry; in-flight work is separate.
    pub peak_queue: usize,
    pub queue_payment_minutes: u128,
}
impl RunResult {
    pub fn on_time(&self) -> u128 {
        self.metrics.completed - self.metrics.completed_late
    }
    pub fn on_time_volume(&self) -> u128 {
        self.metrics.completed_volume_cents - self.metrics.completed_late_volume_cents
    }
    pub fn pending(&self) -> u128 {
        self.metrics.generated
            - self.metrics.completed
            - self.metrics.expired
            - self.metrics.rejected
    }
    pub fn pending_volume(&self) -> u128 {
        self.metrics.generated_volume_cents
            - self.metrics.completed_volume_cents
            - self.metrics.expired_volume_cents
            - self.metrics.rejected_volume_cents
    }
    /// Observed expiry without any accepted route, not proof of infeasibility.
    pub fn never_routed_expired(&self) -> u128 {
        self.payments
            .values()
            .filter(|p| p.outcome == Outcome::Expired && !p.ever_routed)
            .count() as u128
    }
    pub fn score(&self) -> Option<Score> {
        (self.status == Status::Complete).then(|| Score {
            not_on_time: self.metrics.generated - self.on_time(),
            not_on_time_volume_cents: self.metrics.generated_volume_cents - self.on_time_volume(),
            not_completed: self.metrics.generated - self.metrics.completed,
            not_completed_volume_cents: self.metrics.generated_volume_cents
                - self.metrics.completed_volume_cents,
            actual_fee_cents: self.metrics.routing_cost_cents,
            completed_elapsed_minutes: self.metrics.completed_elapsed_minutes,
            departed_hops: self.metrics.departed_hops,
            changed_assignments: self.adaptation.changed_assignments,
        })
    }
    /// Includes late completions. None denotes an empty completed cohort.
    pub fn elapsed_percentile(&self, percent: u8) -> Option<u128> {
        assert!((1..=100).contains(&percent));
        let mut values: Vec<_> = self
            .payments
            .values()
            .filter_map(|p| match p.outcome {
                Outcome::Completed {
                    elapsed_minutes, ..
                } => Some(elapsed_minutes),
                _ => None,
            })
            .collect();
        values.sort_unstable();
        // ceil(n * percent / 100) - 1 without overflowing usize.
        let n = values.len();
        let rank =
            (n / 100) * usize::from(percent) + ((n % 100) * usize::from(percent)).div_ceil(100);
        rank.checked_sub(1).map(|i| values[i])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    pub world: String,
    pub seed: u64,
    pub offered: u128,
    pub offered_volume_cents: u128,
    pub runs: Vec<RunResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation {
    pub config: Config,
    /// Full inputs, including strategy limits, are retained for exact replay.
    pub worlds: Vec<World>,
    pub seeds: Vec<u64>,
    pub strategies: Vec<Strategy>,
    pub cases: Vec<CaseResult>,
}

fn invalid(message: &str) -> SimulationError {
    ValidationError(message.into()).into()
}
fn add(target: &mut u128, value: u128) -> Result<(), SimulationError> {
    *target = target
        .checked_add(value)
        .ok_or(SimulationError::ArithmeticOverflow("evaluation total"))?;
    Ok(())
}
fn ensure(condition: bool, message: &str) -> Result<(), SimulationError> {
    if condition {
        Ok(())
    } else {
        Err(SimulationError::Invariant(message.into()))
    }
}

/// Validate the entire benchmark before running anything. Runtime failures remain
/// explicit rows; invalid inputs and evaluation-accounting errors fail the call.
pub fn evaluate(
    worlds: &[World],
    seeds: &[u64],
    strategies: &[Strategy],
    config: Config,
) -> Result<Evaluation, SimulationError> {
    if worlds.is_empty() || seeds.is_empty() || strategies.is_empty() || config.arrival_minutes == 0
    {
        return Err(invalid(
            "evaluation needs worlds, seeds, strategies and positive arrival minutes",
        ));
    }
    if seeds.iter().collect::<BTreeSet<_>>().len() != seeds.len() {
        return Err(invalid("duplicate evaluation seed"));
    }
    for names in [
        worlds.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
        strategies.iter().map(|s| s.name.as_str()).collect(),
    ] {
        if names.iter().any(|name| name.trim().is_empty())
            || names.iter().collect::<BTreeSet<_>>().len() != names.len()
        {
            return Err(invalid("evaluation names must be nonempty and unique"));
        }
    }
    for world in worlds {
        for strategy in strategies {
            let mut scenario = world.scenario.clone();
            scenario.strategy = strategy.routing;
            scenario.validate()?;
        }
    }
    let mut cases = Vec::new();
    for world in worlds {
        for &seed in seeds {
            cases.push(run_case(world, seed, strategies, config)?);
        }
    }
    Ok(Evaluation {
        config,
        worlds: worlds.to_vec(),
        seeds: seeds.to_vec(),
        strategies: strategies.to_vec(),
        cases,
    })
}

struct Running {
    simulator: Simulator,
    twin: Option<Simulator>,
    result: RunResult,
}
impl Running {
    fn new(world: &World, strategy: &Strategy, config: Config) -> Result<Self, SimulationError> {
        let mut scenario = world.scenario.clone();
        scenario.strategy = strategy.routing;
        // The driver alone owns surprise events. Only due updates enter the run.
        scenario.disruptions.clear();
        // Injected arrivals never consult this simulator's RNG. Keep the real
        // world seed outside the run as well as the future event list.
        let mut simulator = Simulator::new(scenario, 0)?;
        simulator.set_reoptimization_policy(strategy.reoptimization);
        let twin = config.verify_replay.then(|| simulator.clone());
        Ok(Self {
            simulator,
            twin,
            result: RunResult {
                strategy: strategy.name.clone(),
                status: Status::Complete,
                processed_minutes: 0,
                replay_verified: config.verify_replay,
                metrics: Metrics::default(),
                at_arrival_cutoff: Metrics::default(),
                adaptation: AdaptationMetrics::default(),
                diagnostics: SearchDiagnostics::default(),
                payments: BTreeMap::new(),
                peak_active: 0,
                peak_queue: 0,
                queue_payment_minutes: 0,
            },
        })
    }
    fn step(
        &mut self,
        arrivals: &[ArrivalSample],
        changes: &[&Disruption],
        minute: u128,
    ) -> Result<(), SimulationError> {
        if matches!(self.result.status, Status::Error { .. }) {
            return Ok(());
        }
        for change in changes {
            self.simulator.queue_rail_update(change.update.clone())?;
            if let Some(twin) = &mut self.twin {
                twin.queue_rail_update(change.update.clone())?;
            }
        }
        let before = self.twin.as_ref().map(|_| self.simulator.clone());
        let report = self.simulator.step_with_arrivals(arrivals);
        if let Some(twin) = &mut self.twin {
            let repeated = twin.step_with_arrivals(arrivals);
            if report != repeated || self.simulator != *twin {
                self.simulator = before.unwrap();
                self.result.status = Status::Error {
                    minute,
                    message: "replay mismatch in events or state".into(),
                };
                self.result.replay_verified = false;
                return Ok(());
            }
        }
        match report {
            Ok(report) => self.observe(&report)?,
            Err(error) => {
                self.result.status = Status::Error {
                    minute,
                    message: error.to_string(),
                }
            }
        }
        Ok(())
    }
    fn observe(&mut self, report: &TickReport) -> Result<(), SimulationError> {
        let result = &mut self.result;
        for event in &report.events {
            let sequence = match &event.kind {
                EventKind::Generated {
                    sequence,
                    payment,
                    deadline,
                } => {
                    let previous = result.payments.insert(
                        *sequence,
                        PaymentResult {
                            sequence: *sequence,
                            payment: payment.clone(),
                            arrived_at: event.minute,
                            deadline: *deadline,
                            terminal_at: None,
                            outcome: Outcome::Pending,
                            ever_routed: false,
                            sla_failed: false,
                            actual_fee_cents: 0,
                            departed_hops: 0,
                        },
                    );
                    ensure(previous.is_none(), "duplicate evaluation payment")?;
                    continue;
                }
                EventKind::RouteAccepted { sequence, .. }
                | EventKind::HopDeparted { sequence, .. }
                | EventKind::Rejected { sequence }
                | EventKind::Expired { sequence }
                | EventKind::Completed { sequence, .. }
                | EventKind::DeadlineMissed { sequence } => *sequence,
                _ => continue,
            };
            let p = result
                .payments
                .get_mut(&sequence)
                .ok_or_else(|| invalid("event for unknown evaluation payment"))?;
            match event.kind {
                EventKind::RouteAccepted { .. } => p.ever_routed = true,
                EventKind::HopDeparted { fee_cents, .. } => {
                    add(&mut p.actual_fee_cents, fee_cents.into())?;
                    add(&mut p.departed_hops, 1)?;
                }
                EventKind::DeadlineMissed { .. } => p.sla_failed = true,
                EventKind::Rejected { .. } => {
                    p.outcome = Outcome::Rejected;
                    p.sla_failed = true;
                }
                EventKind::Expired { .. } => p.outcome = Outcome::Expired,
                EventKind::Completed {
                    late,
                    elapsed_minutes,
                    ..
                } => {
                    p.outcome = Outcome::Completed {
                        late,
                        elapsed_minutes,
                    }
                }
                _ => unreachable!(),
            }
            if p.outcome != Outcome::Pending {
                p.terminal_at = Some(event.minute);
            }
        }
        result.processed_minutes = self.simulator.next_minute();
        let active = self.simulator.active_payments();
        let queue = active
            .iter()
            .filter(|p| p.in_flight_until.is_none())
            .count();
        result.peak_active = result.peak_active.max(active.len());
        result.peak_queue = result.peak_queue.max(queue);
        add(&mut result.queue_payment_minutes, queue as u128)?;
        Ok(())
    }
    fn finish(mut self) -> Result<RunResult, SimulationError> {
        self.result.metrics = self.simulator.metrics().clone();
        self.result.adaptation = self.simulator.adaptation_metrics().clone();
        self.result.diagnostics = self.simulator.routing_diagnostics();
        if self.result.status == Status::Complete && self.result.pending() > 0 {
            self.result.status = Status::Censored;
        }
        // Reconstruct objectives from payment events rather than trusting display arithmetic.
        let mut audited = Metrics::default();
        for p in self.result.payments.values() {
            add(&mut audited.generated, 1)?;
            add(
                &mut audited.generated_volume_cents,
                p.payment.amount_cents.into(),
            )?;
            add(&mut audited.routing_cost_cents, p.actual_fee_cents)?;
            add(&mut audited.departed_hops, p.departed_hops)?;
            if p.ever_routed {
                add(&mut audited.accepted_routes, 1)?;
            }
            if p.sla_failed {
                add(&mut audited.sla_failures, 1)?;
                add(
                    &mut audited.sla_failed_volume_cents,
                    p.payment.amount_cents.into(),
                )?;
            }
            let volume = u128::from(p.payment.amount_cents);
            match p.outcome {
                Outcome::Rejected => {
                    add(&mut audited.rejected, 1)?;
                    add(&mut audited.rejected_volume_cents, volume)?;
                }
                Outcome::Expired => {
                    add(&mut audited.expired, 1)?;
                    add(&mut audited.expired_volume_cents, volume)?;
                }
                Outcome::Completed {
                    late,
                    elapsed_minutes,
                } => {
                    add(&mut audited.completed, 1)?;
                    add(&mut audited.completed_volume_cents, volume)?;
                    add(&mut audited.completed_elapsed_minutes, elapsed_minutes)?;
                    if late {
                        add(&mut audited.completed_late, 1)?;
                        add(&mut audited.completed_late_volume_cents, volume)?;
                    }
                }
                Outcome::Pending => {}
            }
        }
        // Hop settlement/volume has a separate simulator conservation audit.
        let m = &self.result.metrics;
        audited.departed_principal_cents = m.departed_principal_cents;
        audited.settled_principal_cents = m.settled_principal_cents;
        audited.settled_hops = m.settled_hops;
        ensure(
            audited == *m,
            "evaluation event accounting differs from simulator metrics",
        )?;
        Ok(self.result)
    }
}

fn run_case(
    world: &World,
    seed: u64,
    strategies: &[Strategy],
    config: Config,
) -> Result<CaseResult, SimulationError> {
    let mut running = strategies
        .iter()
        .map(|s| Running::new(world, s, config))
        .collect::<Result<Vec<_>, _>>()?;
    let mut random = Random(seed);
    let mut disruptions: Vec<_> = world.scenario.disruptions.iter().collect();
    disruptions.sort_by_key(|d| (d.minute, &d.update.rail_id));
    let mut cursor = 0;
    let mut offered = 0;
    let mut offered_volume_cents = 0;
    let horizon = u128::from(config.arrival_minutes) + u128::from(config.drain_minutes);
    for minute in 0..horizon {
        let arrivals = if minute < u128::from(config.arrival_minutes) {
            random.arrivals(&world.scenario.arrivals)
        } else {
            vec![]
        };
        add(&mut offered, arrivals.len() as u128)?;
        for a in &arrivals {
            add(&mut offered_volume_cents, a.amount.into())?;
        }
        let start = cursor;
        while cursor < disruptions.len() && disruptions[cursor].minute == minute {
            cursor += 1;
        }
        for run in &mut running {
            run.step(&arrivals, &disruptions[start..cursor], minute)?;
            if minute + 1 == u128::from(config.arrival_minutes) {
                run.result.at_arrival_cutoff = run.simulator.metrics().clone();
            }
            if !matches!(run.result.status, Status::Error { .. }) {
                ensure(
                    run.simulator.metrics().generated == offered
                        && run.simulator.metrics().generated_volume_cents == offered_volume_cents,
                    "strategies did not receive identical demand",
                )?;
            }
        }
    }
    let runs = running
        .into_iter()
        .map(Running::finish)
        .collect::<Result<Vec<_>, _>>()?;
    // Compare complete instructions, not just counts, including admission rejects.
    if let Some(first) = runs.first() {
        for other in &runs[1..] {
            for (id, p) in &first.payments {
                if let Some(q) = other.payments.get(id) {
                    ensure(
                        (&p.payment, p.arrived_at, p.deadline)
                            == (&q.payment, q.arrived_at, q.deadline),
                        "paired instruction mismatch",
                    )?;
                }
            }
        }
    }
    Ok(CaseResult {
        world: world.name.clone(),
        seed,
        offered,
        offered_volume_cents,
        runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_mismatch_keeps_only_committed_observations_and_is_unranked() {
        let world = scenarios::named("reservation-trap").unwrap();
        let strategy = Strategy::named("static").unwrap();
        let mut run = Running::new(&world, &strategy, Config::default()).unwrap();
        assert_eq!(run.simulator.seed(), 0);
        assert!(run.simulator.scenario().disruptions.is_empty());
        let before = run.simulator.clone();
        run.twin
            .as_mut()
            .unwrap()
            .queue_rail_update(RailUpdate {
                rail_id: "fast".into(),
                available: Some(false),
                capacity_per_minute_cents: None,
            })
            .unwrap();
        let arrivals = Random(42).arrivals(&world.scenario.arrivals);
        run.step(&arrivals, &[], 0).unwrap();
        assert_eq!(run.simulator, before);
        let result = run.finish().unwrap();
        assert!(matches!(result.status, Status::Error { minute: 0, .. }));
        assert!(!result.replay_verified);
        assert_eq!(result.processed_minutes, 0);
        assert!(result.payments.is_empty());
        assert_eq!(result.score(), None);
    }
}
