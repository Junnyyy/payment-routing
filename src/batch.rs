//! Exact, read-only joint routing for small synthetic USD payment batches.

use crate::{
    network::{Network, Payment, ValidationError, unique_ids},
    routing::{Route, RouteHop},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRoute {
    pub payment_id: String,
    pub route: Route,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailUsage {
    pub rail_id: String,
    /// Sum of full payment principals over every hop using this rail.
    pub principal_cents: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchPlan {
    /// Exactly one unsplit route per supplied payment, sorted by payment ID.
    pub assignments: Vec<PaymentRoute>,
    /// All rails, including unused ones, sorted by rail ID.
    pub rail_usage: Vec<RailUsage>,
    pub total_fee_cents: u128,
    /// Sum of route latencies, not a batch completion time or schedule.
    pub total_settlement_minutes: u128,
}

/// Route every supplied payment at globally minimum total fee, or return `None`
/// if the entire batch cannot fit. Invalid networks/instructions (including
/// duplicate batch payment IDs) return an error. An empty valid batch costs zero.
/// Only `payments` consumes capacity; `network.payments` is still validated.
///
/// Every hop consumes the full principal from its rail's static batch budget,
/// shared across all member pairs and both directions. Budgets exclude fees and
/// never replenish or net. None is unlimited; zero permits no batch use. Existing
/// availability, per-hop ceilings and end-to-end deadlines also apply. Opening
/// balances are descriptive. No input is changed, reserved, debited or settled.
///
/// Ties minimize summed latency, total hop count, then the sequences of
/// `(rail_id, sender, receiver)` in ascending payment-ID order. Input order does
/// not determine the answer. A one-payment batch can differ from `route_payment`
/// because that API deliberately ignores batch capacity.
///
/// We enumerate every feasible simple path for each payment, then backtrack over
/// joint assignments with shared capacity accounting and an optimistic remaining
/// fee bound. Cycle removal cannot raise
/// fee, latency or capacity consumption and improves hop count, so an optimum
/// exists among simple paths. No cheapest-route or cheapest-prefix reduction is
/// valid in general: a more expensive route can conserve a contested rail.
/// Search is exponential, with no heuristic cutoff or external solver.
///
/// ```
/// use payment_routing::{batch::optimize_batch, demo::demo_network};
/// let network = demo_network();
/// let plan = optimize_batch(&network, &network.payments)?.unwrap();
/// assert_eq!(plan.assignments.len(), 12);
/// assert_eq!(plan.total_fee_cents, 60);
/// # Ok::<(), payment_routing::network::ValidationError>(())
/// ```
pub fn optimize_batch(
    network: &Network,
    payments: &[Payment],
) -> Result<Option<BatchPlan>, ValidationError> {
    crate::count_search!(solver_calls, 1);
    network.validate()?;
    unique_ids("batch payment", payments.iter().map(|p| p.id.as_str()))?;
    for payment in payments {
        payment.validate(network)?;
    }
    let mut ordered: Vec<_> = payments.iter().collect();
    ordered.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    let mut candidates = vec![];
    for payment in &ordered {
        let mut paths = PathSearch {
            network,
            payment,
            visited: vec![payment.sender.as_str()],
            route: Route {
                hops: vec![],
                total_fee_cents: 0,
                total_settlement_minutes: 0,
            },
            usage: vec![0; network.rails.len()],
            answers: vec![],
        };
        paths.visit(&payment.sender);
        if paths.answers.is_empty() {
            return Ok(None);
        }
        paths
            .answers
            .sort_unstable_by(|a, b| route_rank(&a.route).cmp(&route_rank(&b.route)));
        candidates.push(paths.answers);
    }
    // Candidates are fee-sorted. Ignoring competition gives a lower bound on
    // every completion, never a promise that those cheapest routes fit together.
    let mut remaining_fee = vec![0; candidates.len() + 1];
    for index in (0..candidates.len()).rev() {
        remaining_fee[index] =
            remaining_fee[index + 1] + candidates[index][0].route.total_fee_cents;
    }
    let mut search = AssignmentSearch {
        candidates: &candidates,
        remaining_fee,
        capacities: network
            .rails
            .iter()
            .map(|r| r.batch_capacity_cents)
            .collect(),
        used: vec![0; network.rails.len()],
        chosen: vec![],
        best: None,
    };
    search.visit(0, Score::default());
    let Some(best) = search.best else {
        return Ok(None);
    };
    let mut usage = vec![0; network.rails.len()];
    let assignments = best
        .indices
        .iter()
        .enumerate()
        .map(|(i, &choice)| {
            let candidate = &candidates[i][choice];
            for (total, amount) in usage.iter_mut().zip(&candidate.usage) {
                *total += amount;
            }
            PaymentRoute {
                payment_id: ordered[i].id.clone(),
                route: candidate.route.clone(),
            }
        })
        .collect();
    let mut rail_usage: Vec<_> = network
        .rails
        .iter()
        .zip(usage)
        .map(|(r, principal_cents)| RailUsage {
            rail_id: r.id.clone(),
            principal_cents,
        })
        .collect();
    rail_usage.sort_unstable_by(|a, b| a.rail_id.cmp(&b.rail_id));
    Ok(Some(BatchPlan {
        assignments,
        rail_usage,
        total_fee_cents: best.score.fee,
        total_settlement_minutes: best.score.minutes,
    }))
}

fn route_rank(route: &Route) -> (u128, u128, usize, &[RouteHop]) {
    (
        route.total_fee_cents,
        route.total_settlement_minutes,
        route.hops.len(),
        &route.hops,
    )
}

struct Candidate {
    route: Route,
    usage: Vec<u128>,
}

struct PathSearch<'a> {
    network: &'a Network,
    payment: &'a Payment,
    visited: Vec<&'a str>,
    route: Route,
    usage: Vec<u128>,
    answers: Vec<Candidate>,
}

impl<'a> PathSearch<'a> {
    fn visit(&mut self, at: &'a str) {
        crate::count_search!(path_states, 1);
        if at == self.payment.receiver {
            crate::count_search!(candidates, 1);
            crate::count_search!(candidate_hops, self.route.hops.len() as u64);
            self.answers.push(Candidate {
                route: self.route.clone(),
                usage: self.usage.clone(),
            });
            return;
        }
        for (index, rail) in self.network.rails.iter().enumerate() {
            if !rail.available
                || !rail.participants.iter().any(|id| id == at)
                || rail
                    .max_amount_cents
                    .is_some_and(|c| self.payment.amount_cents > c)
            {
                continue;
            }
            let amount = u128::from(self.payment.amount_cents);
            if rail
                .batch_capacity_cents
                .is_some_and(|c| self.usage[index] + amount > u128::from(c))
            {
                continue;
            }
            let elapsed = self.route.total_settlement_minutes + u128::from(rail.settlement_minutes);
            if self
                .payment
                .max_delivery_minutes
                .is_some_and(|d| elapsed > u128::from(d))
            {
                crate::count_search!(deadline_prunes, 1);
                continue;
            }
            for next in &rail.participants {
                if self.visited.contains(&next.as_str()) {
                    continue;
                }
                self.visited.push(next);
                self.route.hops.push(RouteHop {
                    rail_id: rail.id.clone(),
                    sender: at.into(),
                    receiver: next.clone(),
                });
                self.route.total_fee_cents += u128::from(rail.fee_cents);
                self.route.total_settlement_minutes = elapsed;
                self.usage[index] += amount;
                self.visit(next);
                self.usage[index] -= amount;
                self.route.total_settlement_minutes -= u128::from(rail.settlement_minutes);
                self.route.total_fee_cents -= u128::from(rail.fee_cents);
                self.route.hops.pop();
                self.visited.pop();
            }
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Score {
    fee: u128,
    minutes: u128,
    hops: usize,
}

struct Selection {
    score: Score,
    indices: Vec<usize>,
}

struct AssignmentSearch<'a> {
    candidates: &'a [Vec<Candidate>],
    remaining_fee: Vec<u128>,
    capacities: Vec<Option<u64>>,
    used: Vec<u128>,
    chosen: Vec<usize>,
    best: Option<Selection>,
}

impl AssignmentSearch<'_> {
    fn visit(&mut self, index: usize, score: Score) {
        crate::count_search!(assignment_states, 1);
        // Even unconstrained cheapest remaining routes cannot rescue this prefix.
        // Equality must remain searchable to preserve the documented tie-breaks.
        if self
            .best
            .as_ref()
            .is_some_and(|best| score.fee + self.remaining_fee[index] > best.score.fee)
        {
            crate::count_search!(bound_prunes, 1);
            return;
        }
        if index == self.candidates.len() {
            crate::count_search!(complete_assignments, 1);
            let improves = self.best.as_ref().is_none_or(|best| {
                score < best.score
                    || (score == best.score && self.lexically_less(&self.chosen, &best.indices))
            });
            if improves {
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
                crate::count_search!(capacity_rejects, 1);
                continue;
            }
            let next = Score {
                fee: score.fee + candidate.route.total_fee_cents,
                minutes: score.minutes + candidate.route.total_settlement_minutes,
                hops: score.hops + candidate.route.hops.len(),
            };
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

    fn lexically_less(&self, a: &[usize], b: &[usize]) -> bool {
        for (index, (&left, &right)) in a.iter().zip(b).enumerate() {
            let paths = &self.candidates[index];
            match paths[left].route.hops.cmp(&paths[right].route.hops) {
                std::cmp::Ordering::Less => return true,
                std::cmp::Ordering::Greater => return false,
                std::cmp::Ordering::Equal => {}
            }
        }
        false
    }
}
