//! Bounded, feasible routing. Failure is unresolved, never a proof of infeasibility.
//! See `docs/scalable-routing.md`. Exact optimizers do not use this module.
use crate::{
    batch::RailUsage,
    network::{Network, Payment, Rail, ValidationError},
    routing::{Route, RouteHop},
    scheduling::*,
    simulation::RailService,
};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLimits {
    pub labels_per_node: usize,
    pub max_labels: usize,
    pub max_expansions: usize,
    pub max_candidates: usize,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            labels_per_node: 16,
            max_labels: 8192,
            max_expansions: 4096,
            max_candidates: 100_000,
        }
    }
}
impl SearchLimits {
    pub(crate) fn validate(self) -> Result<(), ValidationError> {
        if self.labels_per_node == 0
            || self.max_labels == 0
            || self.max_expansions == 0
            || self.max_candidates == 0
        {
            return Err(ValidationError("search limits must be positive".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchDiagnostics {
    pub searches: u128,
    pub expansions: u128,
    pub candidates: u128,
    pub truncated_searches: u128,
    pub unresolved: u128,
}
impl SearchDiagnostics {
    pub(crate) fn plus(&mut self, other: Self) {
        self.searches = self.searches.saturating_add(other.searches);
        self.expansions = self.expansions.saturating_add(other.expansions);
        self.candidates = self.candidates.saturating_add(other.candidates);
        self.truncated_searches = self
            .truncated_searches
            .saturating_add(other.truncated_searches);
        self.unresolved = self.unresolved.saturating_add(other.unresolved);
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningResult {
    /// None means no complete plan was found by the bounded heuristic.
    pub plan: Option<ScheduledBatchPlan>,
    pub diagnostics: SearchDiagnostics,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Reservations {
    pub slots: BTreeMap<(usize, u128), u128>,
    pub rails: BTreeMap<usize, u128>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Step {
    pub rail: usize,
    pub from: usize,
    pub to: usize,
    pub departure: u128,
    pub arrival: u128,
    pub fee: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Journey {
    pub steps: Vec<Step>,
    pub fee: u128,
}
impl Journey {
    fn rank(&self) -> (u128, u128, usize, &[Step]) {
        (
            self.fee,
            self.steps.last().unwrap().arrival,
            self.steps.len(),
            &self.steps,
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    departure: u128,
    arrival: u128,
    fee: u64,
    capacity: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Calendar {
    Finite(Vec<Vec<Slot>>),
    Recurring(Vec<RailService>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Router {
    nodes: Vec<String>,
    rails: Vec<Rail>,
    members: Vec<Vec<usize>>,
    outgoing: Vec<Vec<usize>>,
    calendar: Calendar,
}
#[derive(Clone)]
struct Label {
    node: usize,
    ready: u128,
    journey: Journey,
    visited: Vec<usize>,
    live: bool,
}
impl Router {
    fn base(network: &Network, calendar: Calendar) -> Self {
        let mut nodes: Vec<_> = network.institutions.iter().map(|n| n.id.clone()).collect();
        nodes.sort();
        let mut rails = network.rails.clone();
        rails.sort_by(|a, b| a.id.cmp(&b.id));
        let members: Vec<Vec<usize>> = rails
            .iter()
            .map(|r| {
                let mut v: Vec<_> = r
                    .participants
                    .iter()
                    .map(|n| nodes.binary_search(n).unwrap())
                    .collect();
                v.sort();
                v
            })
            .collect();
        let mut outgoing = vec![vec![]; nodes.len()];
        for (r, group) in members.iter().enumerate() {
            for &n in group {
                outgoing[n].push(r);
            }
        }
        Self {
            nodes,
            rails,
            members,
            outgoing,
            calendar,
        }
    }
    fn finite(network: &Network, departures: &[RailDeparture]) -> Self {
        let mut router = Self::base(network, Calendar::Finite(vec![]));
        let mut slots = vec![vec![]; router.rails.len()];
        for s in departures {
            let r = router
                .rails
                .binary_search_by(|r| r.id.cmp(&s.rail_id))
                .unwrap();
            slots[r].push(Slot {
                departure: s.departure_minute.into(),
                arrival: u128::from(s.departure_minute)
                    + u128::from(
                        s.settlement_minutes
                            .unwrap_or(router.rails[r].settlement_minutes),
                    ),
                fee: s.fee_cents.unwrap_or(router.rails[r].fee_cents),
                capacity: s.capacity_cents,
            });
        }
        for group in &mut slots {
            group.sort_by_key(|s| s.departure);
        }
        router.calendar = Calendar::Finite(slots);
        router
    }
    pub(crate) fn recurring(network: &Network, services: &[RailService]) -> Self {
        let mut services = services.to_vec();
        services.sort_by(|a, b| a.rail_id.cmp(&b.rail_id));
        Self::base(network, Calendar::Recurring(services))
    }
    fn slot_capacity(&self, rail: usize, minute: u128) -> Option<u64> {
        match &self.calendar {
            Calendar::Recurring(services) => services[rail].capacity_per_minute_cents,
            Calendar::Finite(slots) => {
                slots[rail][slots[rail]
                    .binary_search_by_key(&minute, |s| s.departure)
                    .unwrap()]
                .capacity
            }
        }
    }
    fn fits(
        &self,
        book: &Reservations,
        path: &Journey,
        rail: usize,
        slot: &Slot,
        amount: u64,
    ) -> bool {
        let amount = u128::from(amount);
        let same_rail = path.steps.iter().filter(|s| s.rail == rail).count() as u128;
        let same_slot = path
            .steps
            .iter()
            .filter(|s| s.rail == rail && s.departure == slot.departure)
            .count() as u128;
        self.rails[rail].batch_capacity_cents.is_none_or(|c| {
            book.rails.get(&rail).copied().unwrap_or(0) + amount * (same_rail + 1) <= u128::from(c)
        }) && slot.capacity.is_none_or(|c| {
            book.slots
                .get(&(rail, slot.departure))
                .copied()
                .unwrap_or(0)
                + amount * (same_slot + 1)
                <= u128::from(c)
        })
    }
    // Safe dominance requires enough freedom to reproduce every suffix. In
    // particular, fee/arrival alone cannot dominate different resource use.
    fn dominates(&self, a: &Label, b: &Label) -> bool {
        if a.ready > b.ready
            || a.journey.fee > b.journey.fee
            || a.journey.steps.len() > b.journey.steps.len()
            || !a.visited.iter().all(|n| b.visited.contains(n))
        {
            return false;
        }
        for s in &a.journey.steps {
            if self.rails[s.rail].batch_capacity_cents.is_some()
                && a.journey.steps.iter().filter(|h| h.rail == s.rail).count()
                    > b.journey.steps.iter().filter(|h| h.rail == s.rail).count()
            {
                return false;
            }
            if self.slot_capacity(s.rail, s.departure).is_some()
                && a.journey
                    .steps
                    .iter()
                    .filter(|h| h.rail == s.rail && h.departure == s.departure)
                    .count()
                    > b.journey
                        .steps
                        .iter()
                        .filter(|h| h.rail == s.rail && h.departure == s.departure)
                        .count()
            {
                return false;
            }
        }
        // Preserve lexical ties; earlier ready time can disappear in a wait.
        a.journey.fee < b.journey.fee
            || a.journey.steps.len() < b.journey.steps.len()
            || a.journey.steps <= b.journey.steps
    }
    pub(crate) fn find(
        &self,
        payment: &Payment,
        release: u128,
        deadline: u128,
        book: &Reservations,
        limits: SearchLimits,
    ) -> (Option<Journey>, SearchDiagnostics) {
        let source = self.nodes.binary_search(&payment.sender).unwrap();
        let target = self.nodes.binary_search(&payment.receiver).unwrap();
        let mut stats = SearchDiagnostics {
            searches: 1,
            ..Default::default()
        };
        let mut labels = vec![Label {
            node: source,
            ready: release,
            journey: Journey {
                steps: vec![],
                fee: 0,
            },
            visited: vec![source],
            live: true,
        }];
        let mut per_node = vec![vec![]; self.nodes.len()];
        per_node[source].push(0);
        let mut heap = BinaryHeap::from([Reverse((0u128, release, 0usize, 0usize))]);
        let mut best: Option<Journey> = None;
        let mut truncated = false;
        'search: while let Some(Reverse((_, _, _, index))) = heap.pop() {
            if !labels[index].live {
                continue;
            }
            if stats.expansions >= limits.max_expansions as u128 {
                truncated = true;
                break;
            }
            let label = labels[index].clone();
            if label.ready > deadline {
                continue;
            }
            if best.as_ref().is_some_and(|b| {
                (
                    label.journey.fee,
                    label.ready,
                    label.journey.steps.len() + 1,
                ) > (b.fee, b.steps.last().unwrap().arrival, b.steps.len())
            }) {
                continue;
            }
            stats.expansions += 1;
            for &r in &self.outgoing[label.node] {
                let rail = &self.rails[r];
                if !rail.available
                    || rail
                        .max_amount_cents
                        .is_some_and(|c| payment.amount_cents > c)
                    || rail
                        .batch_capacity_cents
                        .is_some_and(|c| payment.amount_cents > c)
                {
                    continue;
                }
                let mut choices = vec![];
                match &self.calendar {
                    Calendar::Finite(slots) => {
                        for s in slots[r]
                            .iter()
                            .skip(slots[r].partition_point(|s| s.departure < label.ready))
                        {
                            if s.departure > deadline {
                                break;
                            }
                            stats.candidates += 1;
                            if stats.candidates > limits.max_candidates as u128 {
                                truncated = true;
                                break 'search;
                            }
                            if s.arrival <= deadline
                                && self.fits(book, &label.journey, r, s, payment.amount_cents)
                            {
                                choices.push(s.clone());
                            }
                        }
                    }
                    Calendar::Recurring(services) => {
                        let service = &services[r];
                        if service.open_minutes == 0
                            || service
                                .capacity_per_minute_cents
                                .is_some_and(|c| payment.amount_cents > c)
                        {
                            continue;
                        }
                        let mut ready = label.ready;
                        loop {
                            stats.candidates += 1;
                            if stats.candidates > limits.max_candidates as u128 {
                                truncated = true;
                                break 'search;
                            }
                            let period = u128::from(service.period_minutes);
                            let offset = u128::from(service.offset_minutes);
                            let phase = ready % period;
                            let wait = if phase < offset {
                                offset - phase
                            } else if phase >= offset + u128::from(service.open_minutes) {
                                period - phase + offset
                            } else {
                                0
                            };
                            let Some(departure) = ready.checked_add(wait) else {
                                break;
                            };
                            let Some(arrival) =
                                departure.checked_add(u128::from(rail.settlement_minutes))
                            else {
                                break;
                            };
                            if arrival > deadline {
                                break;
                            }
                            let s = Slot {
                                departure,
                                arrival,
                                fee: rail.fee_cents,
                                capacity: service.capacity_per_minute_cents,
                            };
                            if self.fits(book, &label.journey, r, &s, payment.amount_cents) {
                                choices.push(s);
                                break;
                            }
                            let Some(next) = departure.checked_add(1) else {
                                break;
                            };
                            ready = next;
                        }
                    }
                }
                for slot in choices {
                    // Visit the receiver first to obtain an incumbent promptly.
                    let neighbors = std::iter::once(target)
                        .filter(|n| self.members[r].binary_search(n).is_ok())
                        .chain(self.members[r].iter().copied().filter(|&n| n != target));
                    for next in neighbors {
                        stats.candidates += 1;
                        if stats.candidates > limits.max_candidates as u128 {
                            truncated = true;
                            break 'search;
                        }
                        if label.visited.contains(&next) {
                            continue;
                        }
                        let mut candidate = label.clone();
                        candidate.node = next;
                        candidate.ready = slot.arrival;
                        candidate.visited.push(next);
                        candidate.journey.fee += u128::from(slot.fee);
                        candidate.journey.steps.push(Step {
                            rail: r,
                            from: label.node,
                            to: next,
                            departure: slot.departure,
                            arrival: slot.arrival,
                            fee: slot.fee,
                        });
                        if next == target {
                            if best
                                .as_ref()
                                .is_none_or(|b| candidate.journey.rank() < b.rank())
                            {
                                best = Some(candidate.journey);
                            }
                            continue;
                        }
                        if per_node[next]
                            .iter()
                            .any(|&i| labels[i].live && self.dominates(&labels[i], &candidate))
                        {
                            continue;
                        }
                        for &i in &per_node[next] {
                            if labels[i].live && self.dominates(&candidate, &labels[i]) {
                                labels[i].live = false;
                            }
                        }
                        per_node[next].retain(|&i| labels[i].live);
                        if per_node[next].len() >= limits.labels_per_node {
                            truncated = true;
                            continue;
                        }
                        if labels.len() >= limits.max_labels {
                            truncated = true;
                            break 'search;
                        }
                        let i = labels.len();
                        heap.push(Reverse((
                            candidate.journey.fee,
                            candidate.ready,
                            candidate.journey.steps.len(),
                            i,
                        )));
                        labels.push(candidate);
                        per_node[next].push(i);
                    }
                }
            }
        }
        stats.truncated_searches = u128::from(truncated);
        stats.unresolved = u128::from(best.is_none());
        (best, stats)
    }
    pub(crate) fn reserve(&self, book: &mut Reservations, journey: &Journey, amount: u64) {
        for s in &journey.steps {
            if self.rails[s.rail].batch_capacity_cents.is_some() {
                *book.rails.entry(s.rail).or_default() += u128::from(amount);
            }
            if self.slot_capacity(s.rail, s.departure).is_some() {
                *book.slots.entry((s.rail, s.departure)).or_default() += u128::from(amount);
            }
        }
    }
    pub(crate) fn route(&self, journey: &Journey) -> Route {
        Route {
            hops: journey.steps.iter().map(|s| self.hop(s)).collect(),
            total_fee_cents: journey.fee,
            total_settlement_minutes: journey.steps.iter().map(|s| s.arrival - s.departure).sum(),
        }
    }
    fn hop(&self, s: &Step) -> RouteHop {
        RouteHop {
            rail_id: self.rails[s.rail].id.clone(),
            sender: self.nodes[s.from].clone(),
            receiver: self.nodes[s.to].clone(),
        }
    }
    fn plan(
        &self,
        payments: &[TimedPayment],
        journeys: &[Journey],
        departures: &[RailDeparture],
    ) -> ScheduledBatchPlan {
        let mut assignments = vec![];
        let mut used = BTreeMap::<(String, u64), u128>::new();
        for (p, j) in payments.iter().zip(journeys) {
            let hops: Vec<_> = j
                .steps
                .iter()
                .map(|s| {
                    *used
                        .entry((self.rails[s.rail].id.clone(), s.departure as u64))
                        .or_default() += u128::from(p.payment.amount_cents);
                    ScheduledHop {
                        transfer: self.hop(s),
                        departure_minute: s.departure as u64,
                        arrival_minute: s.arrival as u64,
                        fee_cents: s.fee,
                    }
                })
                .collect();
            assignments.push(ScheduledPaymentRoute {
                payment_id: p.payment.id.clone(),
                route: ScheduledRoute {
                    elapsed_minutes: hops.last().unwrap().arrival_minute
                        - p.earliest_execution_minute,
                    total_fee_cents: j.fee,
                    hops,
                },
            });
        }
        assignments.sort_by(|a, b| a.payment_id.cmp(&b.payment_id));
        let mut departure_usage: Vec<_> = departures
            .iter()
            .map(|s| DepartureUsage {
                rail_id: s.rail_id.clone(),
                departure_minute: s.departure_minute,
                principal_cents: used
                    .get(&(s.rail_id.clone(), s.departure_minute))
                    .copied()
                    .unwrap_or(0),
            })
            .collect();
        departure_usage.sort_by(|a, b| {
            (&a.rail_id, a.departure_minute).cmp(&(&b.rail_id, b.departure_minute))
        });
        ScheduledBatchPlan {
            total_fee_cents: assignments.iter().map(|a| a.route.total_fee_cents).sum(),
            total_elapsed_minutes: assignments
                .iter()
                .map(|a| u128::from(a.route.elapsed_minutes))
                .sum(),
            rail_usage: self
                .rails
                .iter()
                .map(|r| RailUsage {
                    rail_id: r.id.clone(),
                    principal_cents: used
                        .iter()
                        .filter(|((id, _), _)| id == &r.id)
                        .map(|(_, v)| v)
                        .sum(),
                })
                .collect(),
            departure_usage,
            assignments,
        }
    }
}
/// Multiple deterministic orders of bounded allocation. A full result obeys every schedule constraint; None
/// means unresolved, including when a greedy allocation blocks a feasible batch.
pub fn plan_schedule(
    network: &Network,
    payments: &[TimedPayment],
    departures: &[RailDeparture],
    limits: SearchLimits,
) -> Result<PlanningResult, ValidationError> {
    validate_schedule(network, payments, departures)?;
    limits.validate()?;
    let router = Router::finite(network, departures);
    let requests: Vec<_> = payments
        .iter()
        .map(|p| Request {
            payment: &p.payment,
            release: p.earliest_execution_minute.into(),
            deadline: u128::from(p.deadline_minute.unwrap_or(u64::MAX))
                .min(
                    u128::from(p.earliest_execution_minute)
                        + u128::from(p.payment.max_delivery_minutes.unwrap_or(u64::MAX)),
                )
                .min(u128::from(u64::MAX)),
        })
        .collect();
    let (journeys, diagnostics) = router.allocate(&requests, &Reservations::default(), limits);
    let full: Option<Vec<_>> = journeys.into_iter().collect();
    Ok(PlanningResult {
        plan: full.map(|j| router.plan(payments, &j, departures)),
        diagnostics,
    })
}

/// One decision in a shared residual calendar. Earlier commitments remain fixed.
pub(crate) struct Request<'a> {
    pub payment: &'a Payment,
    pub release: u128,
    pub deadline: u128,
}

impl Router {
    pub(crate) fn allocate(
        &self,
        requests: &[Request<'_>],
        initial: &Reservations,
        limits: SearchLimits,
    ) -> (Vec<Option<Journey>>, SearchDiagnostics) {
        let base: Vec<_> = (0..requests.len()).collect();
        let mut orders = vec![base.clone()];
        let mut deadline = base.clone();
        deadline.sort_by_key(|&i| {
            (
                requests[i].deadline,
                requests[i].payment.amount_cents,
                &requests[i].payment.id,
            )
        });
        let mut amount = base.clone();
        amount.sort_by_key(|&i| {
            (
                requests[i].payment.amount_cents,
                requests[i].deadline,
                &requests[i].payment.id,
            )
        });
        let reverse: Vec<_> = base.into_iter().rev().collect();
        for order in [deadline, amount, reverse] {
            if !orders.contains(&order) {
                orders.push(order);
            }
        }
        let mut best: Option<Vec<Option<Journey>>> = None;
        let mut diagnostics = SearchDiagnostics::default();
        let score = |plans: &[Option<Journey>]| {
            let served = plans.iter().filter(|p| p.is_some()).count();
            let fee: u128 = plans.iter().flatten().map(|p| p.fee).sum();
            let elapsed: u128 = plans
                .iter()
                .zip(requests)
                .filter_map(|(p, r)| {
                    p.as_ref()
                        .map(|p| p.steps.last().unwrap().arrival - r.release)
                })
                .sum();
            let hops: usize = plans.iter().flatten().map(|p| p.steps.len()).sum();
            (Reverse(served), fee, elapsed, hops)
        };
        for order in orders {
            let mut book = initial.clone();
            let mut plans = vec![None; requests.len()];
            for i in order {
                let r = &requests[i];
                let (journey, stats) = self.find(r.payment, r.release, r.deadline, &book, limits);
                diagnostics.plus(stats);
                if let Some(ref j) = journey {
                    self.reserve(&mut book, j, r.payment.amount_cents);
                }
                plans[i] = journey;
            }
            if best
                .as_ref()
                .is_none_or(|b| (score(&plans), &plans) < (score(b), b))
            {
                best = Some(plans);
            }
        }
        (best.unwrap_or_default(), diagnostics)
    }
}
