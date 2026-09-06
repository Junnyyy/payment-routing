//! Exact routing and execution planning over a finite synthetic timetable.
//! See `docs/time-model.md` for the time, capacity and optimality contract.

use crate::{
    batch::RailUsage,
    network::{Network, Payment, ValidationError, unique_ids},
    routing::RouteHop,
};

/// Arrival/release time is the first minute the payment can execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedPayment {
    pub payment: Payment,
    pub earliest_execution_minute: u64,
    /// Inclusive absolute completion deadline; None adds no absolute deadline.
    /// The underlying max_delivery_minutes also limits elapsed time from release,
    /// including waiting at the sender and intermediaries.
    pub deadline_minute: Option<u64>,
}

/// One rail-wide departure opportunity. Missing minutes are closed: there is no
/// implicit timetable or horizon expansion. All member pairs share this slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailDeparture {
    pub rail_id: String,
    pub departure_minute: u64,
    /// None inherits the rail's fee. The effective fee is charged per hop.
    pub fee_cents: Option<u64>,
    /// None inherits the rail's latency. Later departures may arrive earlier.
    pub settlement_minutes: Option<u32>,
    /// Principal budget for this departure, shared across all pairs/directions.
    /// None is unlimited; Some(0) closes the slot. The static batch budget applies
    /// additionally, across all slots, and never replenishes.
    pub capacity_cents: Option<u64>,
}

/// Effective inputs and exact timestamps make the returned plan replayable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScheduledHop {
    pub transfer: RouteHop,
    pub departure_minute: u64,
    pub arrival_minute: u64,
    pub fee_cents: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRoute {
    pub hops: Vec<ScheduledHop>,
    pub total_fee_cents: u128,
    /// Final arrival minus release, including every wait. Not summed hop latency.
    pub elapsed_minutes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledPaymentRoute {
    pub payment_id: String,
    pub route: ScheduledRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartureUsage {
    pub rail_id: String,
    pub departure_minute: u64,
    pub principal_cents: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledBatchPlan {
    /// Exactly the supplied payments, sorted by payment ID.
    pub assignments: Vec<ScheduledPaymentRoute>,
    /// Every network rail, including unused rails, sorted by rail ID.
    pub rail_usage: Vec<RailUsage>,
    /// Every supplied slot, including unused slots, sorted by rail ID then time.
    pub departure_usage: Vec<DepartureUsage>,
    pub total_fee_cents: u128,
    pub total_elapsed_minutes: u128,
}

/// Validate all inputs even for empty or infeasible batches. A deadline before
/// release is valid but infeasible. Timetables cannot reopen unavailable rails.
/// Unknown rails, duplicate (rail, minute) slots and overflowing arrivals are
/// malformed. Only the supplied timed payments will consume planning capacity.
pub fn validate_schedule(
    network: &Network,
    payments: &[TimedPayment],
    departures: &[RailDeparture],
) -> Result<(), ValidationError> {
    network.validate()?;
    unique_ids(
        "timed payment",
        payments.iter().map(|p| p.payment.id.as_str()),
    )?;
    for payment in payments {
        payment.payment.validate(network)?;
    }
    let mut seen = vec![];
    for slot in departures {
        let Some(rail) = network.rails.iter().find(|r| r.id == slot.rail_id) else {
            return Err(ValidationError(format!(
                "departure references unknown rail {}",
                slot.rail_id
            )));
        };
        let key = (&slot.rail_id, slot.departure_minute);
        if seen.contains(&key) {
            return Err(ValidationError(format!(
                "duplicate departure for rail {} at minute {}",
                slot.rail_id, slot.departure_minute
            )));
        }
        seen.push(key);
        if slot
            .departure_minute
            .checked_add(u64::from(
                slot.settlement_minutes.unwrap_or(rail.settlement_minutes),
            ))
            .is_none()
        {
            return Err(ValidationError(format!(
                "arrival overflows u64 for rail {} at minute {}",
                slot.rail_id, slot.departure_minute
            )));
        }
    }
    Ok(())
}

/// Find a globally minimum-fee full execution plan within the supplied timetable.
/// `Ok(None)` means infeasible; malformed input returns a validation error. Empty
/// valid batches return a zero plan. No input is mutated and no payment executes.
///
/// Ties minimize summed elapsed minutes from release, total hops, then timed-hop
/// sequences in payment-ID order. All feasible simple paths and departure choices
/// are retained before exact joint selection. Free waiting allows cycle removal
/// without changing downstream slots, so an optimal simple witness always exists.
/// Search is exponential; there is no timeout, heuristic cutoff or approximation.
///
/// ```
/// use payment_routing::{demo::demo_network, scheduling::*};
/// let network = demo_network();
/// let payment = TimedPayment {
///     payment: network.payments[0].clone(),
///     earliest_execution_minute: 2,
///     deadline_minute: Some(5),
/// };
/// let slot = RailDeparture {
///     rail_id: "ACH".into(), departure_minute: 4,
///     fee_cents: Some(1), settlement_minutes: Some(1), capacity_cents: None,
/// };
/// let plan = optimize_schedule(&network, &[payment], &[slot])?.unwrap();
/// assert_eq!((plan.total_fee_cents, plan.total_elapsed_minutes), (1, 3));
/// assert_eq!(plan.assignments[0].route.hops[0].arrival_minute, 5);
/// # Ok::<(), payment_routing::network::ValidationError>(())
/// ```
pub fn optimize_schedule(
    network: &Network,
    payments: &[TimedPayment],
    departures: &[RailDeparture],
) -> Result<Option<ScheduledBatchPlan>, ValidationError> {
    validate_schedule(network, payments, departures)?;
    let mut ordered: Vec<_> = payments.iter().collect();
    ordered.sort_unstable_by(|a, b| a.payment.id.cmp(&b.payment.id));
    let slots: Vec<_> = departures
        .iter()
        .map(|slot| {
            let rail_index = network
                .rails
                .iter()
                .position(|r| r.id == slot.rail_id)
                .unwrap();
            let rail = &network.rails[rail_index];
            Slot {
                rail_index,
                departure: slot.departure_minute,
                // Checked by validation, including slots on unavailable rails.
                arrival: slot.departure_minute
                    + u64::from(slot.settlement_minutes.unwrap_or(rail.settlement_minutes)),
                fee: slot.fee_cents.unwrap_or(rail.fee_cents),
            }
        })
        .collect();
    // One resource per whole-batch rail budget, then one per departure budget.
    let capacities: Vec<_> = network
        .rails
        .iter()
        .map(|r| r.batch_capacity_cents)
        .chain(departures.iter().map(|s| s.capacity_cents))
        .collect();
    let mut candidates = vec![];
    for payment in &ordered {
        let mut paths = TimedPathSearch {
            network,
            payment,
            slots: &slots,
            capacities: &capacities,
            used: vec![0; capacities.len()],
            visited: vec![payment.payment.sender.as_str()],
            hops: vec![],
            answers: vec![],
        };
        paths.visit(
            &payment.payment.sender,
            payment.earliest_execution_minute,
            0,
        );
        if paths.answers.is_empty() {
            return Ok(None);
        }
        paths
            .answers
            .sort_unstable_by(|a, b| (a.score(), &a.route.hops).cmp(&(b.score(), &b.route.hops)));
        candidates.push(paths.answers);
    }
    // The sum of unconstrained lexicographic minima is an optimistic score for
    // every completion. Slot competition can only worsen it. Strict pruning
    // preserves equal-score candidates that can win the final lexical tie.
    let mut remaining = vec![Score::default(); candidates.len() + 1];
    for i in (0..candidates.len()).rev() {
        remaining[i] = remaining[i + 1].plus(candidates[i][0].score());
    }
    let mut search = JointSearch {
        candidates: &candidates,
        capacities: &capacities,
        remaining: &remaining,
        used: vec![0; capacities.len()],
        chosen: vec![],
        best: None,
    };
    search.visit(0, Score::default());
    let Some(best) = search.best else {
        return Ok(None);
    };
    let mut used = vec![0; capacities.len()];
    let assignments = best
        .indices
        .iter()
        .enumerate()
        .map(|(i, &choice)| {
            let candidate = &candidates[i][choice];
            for (total, amount) in used.iter_mut().zip(&candidate.usage) {
                *total += amount;
            }
            ScheduledPaymentRoute {
                payment_id: ordered[i].payment.id.clone(),
                route: candidate.route.clone(),
            }
        })
        .collect();
    let mut rail_usage: Vec<_> = network
        .rails
        .iter()
        .enumerate()
        .map(|(i, rail)| RailUsage {
            rail_id: rail.id.clone(),
            principal_cents: used[i],
        })
        .collect();
    rail_usage.sort_unstable_by(|a, b| a.rail_id.cmp(&b.rail_id));
    let mut departure_usage: Vec<_> = departures
        .iter()
        .enumerate()
        .map(|(i, slot)| DepartureUsage {
            rail_id: slot.rail_id.clone(),
            departure_minute: slot.departure_minute,
            principal_cents: used[network.rails.len() + i],
        })
        .collect();
    departure_usage.sort_unstable_by(|a, b| {
        (&a.rail_id, a.departure_minute).cmp(&(&b.rail_id, b.departure_minute))
    });
    Ok(Some(ScheduledBatchPlan {
        assignments,
        rail_usage,
        departure_usage,
        total_fee_cents: best.score.fee,
        total_elapsed_minutes: best.score.elapsed,
    }))
}

struct Slot {
    rail_index: usize,
    departure: u64,
    arrival: u64,
    fee: u64,
}

struct Candidate {
    route: ScheduledRoute,
    usage: Vec<u128>,
}

impl Candidate {
    fn score(&self) -> Score {
        Score {
            fee: self.route.total_fee_cents,
            elapsed: u128::from(self.route.elapsed_minutes),
            hops: self.route.hops.len(),
        }
    }
}

struct TimedPathSearch<'a> {
    network: &'a Network,
    payment: &'a TimedPayment,
    slots: &'a [Slot],
    capacities: &'a [Option<u64>],
    used: Vec<u128>,
    visited: Vec<&'a str>,
    hops: Vec<ScheduledHop>,
    answers: Vec<Candidate>,
}

impl<'a> TimedPathSearch<'a> {
    fn visit(&mut self, at: &'a str, ready: u64, fee: u128) {
        let elapsed = ready - self.payment.earliest_execution_minute;
        if self.payment.deadline_minute.is_some_and(|d| ready > d)
            || self
                .payment
                .payment
                .max_delivery_minutes
                .is_some_and(|d| elapsed > d)
        {
            return;
        }
        if at == self.payment.payment.receiver {
            self.answers.push(Candidate {
                route: ScheduledRoute {
                    hops: self.hops.clone(),
                    total_fee_cents: fee,
                    elapsed_minutes: elapsed,
                },
                usage: self.used.clone(),
            });
            return;
        }
        for (i, slot) in self.slots.iter().enumerate() {
            let rail = &self.network.rails[slot.rail_index];
            if slot.departure < ready
                || !rail.available
                || !rail.participants.iter().any(|id| id == at)
                || rail
                    .max_amount_cents
                    .is_some_and(|c| self.payment.payment.amount_cents > c)
            {
                continue;
            }
            let amount = u128::from(self.payment.payment.amount_cents);
            let resources = [slot.rail_index, self.network.rails.len() + i];
            if resources
                .iter()
                .any(|&r| self.capacities[r].is_some_and(|c| self.used[r] + amount > u128::from(c)))
            {
                continue;
            }
            for next in &rail.participants {
                if self.visited.contains(&next.as_str()) {
                    continue;
                }
                self.visited.push(next);
                self.hops.push(ScheduledHop {
                    transfer: RouteHop {
                        rail_id: rail.id.clone(),
                        sender: at.into(),
                        receiver: next.clone(),
                    },
                    departure_minute: slot.departure,
                    arrival_minute: slot.arrival,
                    fee_cents: slot.fee,
                });
                for r in resources {
                    self.used[r] += amount;
                }
                self.visit(next, slot.arrival, fee + u128::from(slot.fee));
                for r in resources {
                    self.used[r] -= amount;
                }
                self.hops.pop();
                self.visited.pop();
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Score {
    fee: u128,
    elapsed: u128,
    hops: usize,
}

impl Score {
    fn plus(self, other: Self) -> Self {
        Self {
            fee: self.fee + other.fee,
            elapsed: self.elapsed + other.elapsed,
            hops: self.hops + other.hops,
        }
    }
}

struct Selection {
    score: Score,
    indices: Vec<usize>,
}

struct JointSearch<'a> {
    candidates: &'a [Vec<Candidate>],
    capacities: &'a [Option<u64>],
    remaining: &'a [Score],
    used: Vec<u128>,
    chosen: Vec<usize>,
    best: Option<Selection>,
}

impl JointSearch<'_> {
    fn visit(&mut self, index: usize, score: Score) {
        if self
            .best
            .as_ref()
            .is_some_and(|b| score.plus(self.remaining[index]) > b.score)
        {
            return;
        }
        if index == self.candidates.len() {
            if self.best.as_ref().is_none_or(|b| {
                score < b.score || (score == b.score && self.lexically_less(&b.indices))
            }) {
                self.best = Some(Selection {
                    score,
                    indices: self.chosen.clone(),
                });
            }
            return;
        }
        for choice in 0..self.candidates[index].len() {
            let candidate = &self.candidates[index][choice];
            if self
                .capacities
                .iter()
                .zip(&self.used)
                .zip(&candidate.usage)
                .any(|((cap, used), amount)| cap.is_some_and(|c| used + amount > u128::from(c)))
            {
                continue;
            }
            let next = score.plus(candidate.score());
            for (used, amount) in self.used.iter_mut().zip(&candidate.usage) {
                *used += amount;
            }
            self.chosen.push(choice);
            self.visit(index + 1, next);
            self.chosen.pop();
            for (used, amount) in self
                .used
                .iter_mut()
                .zip(&self.candidates[index][choice].usage)
            {
                *used -= amount;
            }
        }
    }

    fn lexically_less(&self, other: &[usize]) -> bool {
        for (i, (&a, &b)) in self.chosen.iter().zip(other).enumerate() {
            match self.candidates[i][a]
                .route
                .hops
                .cmp(&self.candidates[i][b].route.hops)
            {
                std::cmp::Ordering::Less => return true,
                std::cmp::Ordering::Greater => return false,
                std::cmp::Ordering::Equal => {}
            }
        }
        false
    }
}
