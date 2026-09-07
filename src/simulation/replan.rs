use super::engine::add;
use super::*;
use crate::{
    network::Network,
    observation::DecisionEvidence,
    scalable::{Journey, Request, SearchLimits},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Suffix {
    route: Route,
    departures: Option<Vec<u128>>,
    arrival: u128,
}
struct Work {
    payment: Payment,
    release: u128,
    deadline: u128,
    forbidden: Vec<String>,
    old: Option<Suffix>,
}
impl Work {
    fn request(&self) -> Request<'_> {
        Request {
            payment: &self.payment,
            release: self.release,
            deadline: self.deadline,
            forbidden: &self.forbidden,
        }
    }
}

fn work(p: &ActivePayment, minute: u128) -> Work {
    let fixed = p.fixed_hops();
    let mut payment = p.payment.clone();
    let forbidden: Vec<_> = p
        .route
        .iter()
        .flat_map(|r| r.hops[..fixed].iter().map(|h| h.sender.clone()))
        .collect();
    if fixed > 0 {
        payment.sender = p.route.as_ref().unwrap().hops[fixed - 1].receiver.clone();
    }
    let release = p.in_flight_until.unwrap_or(minute).max(minute);
    payment.max_delivery_minutes = Some((p.deadline.saturating_sub(release)) as u64);
    let old = p.has_complete_plan().then(|| {
        let route = p.route.as_ref().unwrap();
        let hops = route.hops[fixed..].to_vec();
        // Totals are reconstructed from the immutable network when assessing.
        let departures = p.planned_departures.as_ref().map(|t| t[fixed..].to_vec());
        Suffix {
            route: Route {
                hops,
                total_fee_cents: 0,
                total_settlement_minutes: 0,
            },
            departures,
            arrival: release,
        }
    });
    Work {
        payment,
        release,
        deadline: p.deadline,
        forbidden,
        old,
    }
}

fn totals(scenario: &Scenario, plan: &mut Suffix, release: u128) -> Result<(), SimulationError> {
    plan.route.total_fee_cents = 0;
    plan.route.total_settlement_minutes = 0;
    let mut ready = release;
    for (i, hop) in plan.route.hops.iter().enumerate() {
        let r = scenario
            .network
            .rails
            .iter()
            .find(|r| r.id == hop.rail_id)
            .unwrap();
        add(
            &mut plan.route.total_fee_cents,
            r.fee_cents.into(),
            "suffix fees",
        )?;
        add(
            &mut plan.route.total_settlement_minutes,
            r.settlement_minutes.into(),
            "suffix latency",
        )?;
        if let Some(times) = &plan.departures {
            ready = times[i];
        }
        add(&mut ready, r.settlement_minutes.into(), "suffix arrival")?;
    }
    plan.arrival = ready;
    Ok(())
}
fn from_journey(router: &Router, j: &Journey) -> Suffix {
    Suffix {
        route: router.route(j),
        departures: Some(j.steps.iter().map(|s| s.departure).collect()),
        arrival: j.steps.last().unwrap().arrival,
    }
}

/// Static search also excludes the immutable prefix, without changing the
/// original instruction or allowing cycles in the composed execution witness.
pub(super) fn static_suffix(
    mut view: Network,
    p: &ActivePayment,
    minute: u128,
    evidence: &mut Option<&mut DecisionEvidence>,
) -> Result<Option<Suffix>, SimulationError> {
    let w = work(p, minute);
    if w.release > w.deadline {
        return Ok(None);
    }
    for r in &mut view.rails {
        r.participants.retain(|n| !w.forbidden.contains(n));
    }
    view.rails.retain(|r| r.participants.len() >= 2);
    let route = crate::routing::route_payment_observed(&view, &w.payment, evidence.as_deref_mut())?;
    route
        .map(|route| {
            let arrival = w
                .release
                .checked_add(route.total_settlement_minutes)
                .ok_or(SimulationError::ArithmeticOverflow("static suffix arrival"))?;
            Ok(Suffix {
                route,
                departures: None,
                arrival,
            })
        })
        .transpose()
}

impl State {
    /// Replace only the future suffix. Initial acceptances and subsequent plan
    /// revisions have distinct events; accepted_routes remains a payment count.
    pub(super) fn install_suffix(
        &mut self,
        scenario: &Scenario,
        p: &mut ActivePayment,
        suffix: Option<Suffix>,
        events: &mut Vec<Event>,
        evidence: &mut Option<&mut DecisionEvidence>,
    ) -> Result<(), SimulationError> {
        let fixed = p.fixed_hops();
        let mut hops = p
            .route
            .as_ref()
            .map_or_else(Vec::new, |r| r.hops[..fixed].to_vec());
        let mut times = p.planned_departures.as_ref().map(|t| t[..fixed].to_vec());
        if let Some(suffix) = &suffix {
            hops.extend(suffix.route.hops.clone());
            if let Some(departures) = &suffix.departures {
                times.get_or_insert_with(Vec::new).extend(departures);
            }
        }
        let route = if hops.is_empty() {
            times = None;
            None
        } else {
            let mut result = Suffix {
                route: Route {
                    hops,
                    total_fee_cents: 0,
                    total_settlement_minutes: 0,
                },
                departures: times.clone(),
                arrival: 0,
            };
            totals(scenario, &mut result, p.arrived_at)?;
            Some(result.route)
        };
        if p.route == route && p.planned_departures == times {
            return Ok(());
        }
        p.route = route;
        p.planned_departures = times;
        if !p.ever_routed && suffix.is_some() {
            p.ever_routed = true;
            add(&mut self.metrics.accepted_routes, 1, "accepted routes")?;
            self.emit(
                events,
                EventKind::RouteAccepted {
                    sequence: p.sequence,
                    route: p.route.clone().unwrap(),
                },
            )?;
        } else {
            self.emit(
                events,
                EventKind::PlanRevised {
                    sequence: p.sequence,
                    route: p.route.clone(),
                    planned_departures: p.planned_departures.clone(),
                },
            )?;
        }
        if let (Some(evidence), Some(times)) = (evidence.as_deref_mut(), &p.planned_departures) {
            evidence
                .reserved_departures
                .insert(p.payment.id.clone(), times.clone());
        }
        Ok(())
    }

    pub(super) fn plan_pending(
        &mut self,
        scenario: &Scenario,
        router: &Router,
        limits: SearchLimits,
        events: &mut Vec<Event>,
        evidence: &mut Option<&mut DecisionEvidence>,
    ) -> Result<(), SimulationError> {
        let work: Vec<_> = self
            .active
            .iter()
            .filter(|p| !p.has_complete_plan() && !p.sla_failed)
            .map(|p| work(p, self.next_minute))
            .collect();
        if work.is_empty() {
            return Ok(());
        }
        let requests: Vec<_> = work.iter().map(Work::request).collect();
        let (plans, diagnostics) =
            router.allocate_observed(&requests, &self.reservations, limits, evidence);
        self.routing_diagnostics.plus(diagnostics);
        let mut plans = plans.into_iter();
        for mut p in std::mem::take(&mut self.active) {
            if !p.has_complete_plan()
                && !p.sla_failed
                && let Some(j) = plans.next().unwrap()
            {
                router.reserve(&mut self.reservations, &j, p.payment.amount_cents);
                self.install_suffix(
                    scenario,
                    &mut p,
                    Some(from_journey(router, &j)),
                    events,
                    evidence,
                )?;
            }
            self.active.push(p);
        }
        Ok(())
    }

    pub(super) fn reoptimize(
        &mut self,
        scenario: &Scenario,
        router: &Router,
        policy: ReoptimizationPolicy,
        events: &mut Vec<Event>,
        evidence: &mut Option<&mut DecisionEvidence>,
    ) -> Result<(), SimulationError> {
        let indices: Vec<_> = self
            .active
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                !p.sla_failed
                    && !(p.has_complete_plan()
                        && p.fixed_hops() == p.route.as_ref().unwrap().hops.len())
            })
            .map(|(i, _)| i)
            .collect();
        let mut work: Vec<_> = indices
            .iter()
            .map(|&i| work(&self.active[i], self.next_minute))
            .collect();
        for w in &mut work {
            if let Some(old) = &mut w.old {
                totals(scenario, old, w.release)?;
            }
        }
        let mut retained = vec![None; work.len()];
        let mut book = Reservations::default();
        for (i, w) in work.iter().enumerate() {
            if let Some(old) = &w.old {
                match scenario.strategy {
                    RoutingStrategy::Reserved { .. } => {
                        if let Some(j) = router.retain_journey(
                            &w.request(),
                            &old.route,
                            old.departures.as_ref().unwrap(),
                            &book,
                        ) {
                            router.reserve(&mut book, &j, w.payment.amount_cents);
                            retained[i] = Some(from_journey(router, &j));
                        }
                    }
                    RoutingStrategy::CheapestStatic => {
                        if old.arrival <= w.deadline
                            && old.route.hops.iter().all(|h| {
                                let r = self.rail_index(&h.rail_id);
                                scenario.network.rails[r].available
                                    && scenario.services[r].open_minutes > 0
                                    && scenario.services[r]
                                        .capacity_per_minute_cents
                                        .is_none_or(|c| w.payment.amount_cents <= c)
                            })
                        {
                            retained[i] = Some(old.clone());
                        }
                    }
                }
            }
        }
        let (preserve, preserve_diagnostics) =
            self.complete_candidate(scenario, router, &indices, &work, retained, book, evidence)?;
        let (recompute, recompute_diagnostics) = self.complete_candidate(
            scenario,
            router,
            &indices,
            &work,
            vec![None; work.len()],
            Reservations::default(),
            evidence,
        )?;
        let same_planned_cohort = preserve
            .iter()
            .zip(&recompute)
            .all(|(a, b)| a.is_some() == b.is_some());
        let a = assess(&work, &preserve, self.next_minute)?;
        let b = assess(&work, &recompute, self.next_minute)?;
        let keep = match policy {
            ReoptimizationPolicy::Preserve => true,
            ReoptimizationPolicy::Recompute => false,
            ReoptimizationPolicy::Adaptive {
                max_extra_fee_cents,
                max_extra_elapsed_minutes,
                max_extra_hops,
            } => {
                if a.planned != b.planned {
                    a.planned > b.planned
                } else if !same_planned_cohort {
                    a.withdrawn <= b.withdrawn
                } else {
                    a.remaining_fee_cents.saturating_sub(b.remaining_fee_cents)
                        <= max_extra_fee_cents
                        && a.remaining_elapsed_minutes
                            .saturating_sub(b.remaining_elapsed_minutes)
                            <= max_extra_elapsed_minutes
                        && a.remaining_hops.saturating_sub(b.remaining_hops) <= max_extra_hops
                }
            }
        };
        let report = ReoptimizationReport {
            minute: self.next_minute,
            policy,
            selected: if keep {
                ReoptimizationChoice::Preserve
            } else {
                ReoptimizationChoice::Recompute
            },
            preserve: a,
            recompute: b,
            same_planned_cohort,
            reserved: matches!(scenario.strategy, RoutingStrategy::Reserved { .. }),
            preserve_diagnostics,
            recompute_diagnostics,
        };
        let selected = report.selected_assessment();
        let m = &mut self.adaptation_metrics;
        add(&mut m.reoptimizations, 1, "reoptimizations")?;
        add(
            &mut m.assignment_comparisons,
            selected.previously_planned as u128,
            "assignment comparisons",
        )?;
        add(
            &mut m.changed_assignments,
            selected.changed_assignments as u128,
            "assignment churn",
        )?;
        add(
            &mut m.changed_routes,
            selected.changed_routes as u128,
            "route churn",
        )?;
        add(
            &mut m.retimed_only,
            selected.retimed_only as u128,
            "retimings",
        )?;
        add(&mut m.withdrawn, selected.withdrawn as u128, "withdrawals")?;
        self.routing_diagnostics.plus(preserve_diagnostics);
        self.routing_diagnostics.plus(recompute_diagnostics);
        self.last_reoptimization = Some(report.clone());
        self.emit(events, EventKind::Reoptimized(Box::new(report)))?;
        self.reservations = Reservations::default();
        let selected = if keep { preserve } else { recompute };
        let mut selected = indices.into_iter().zip(selected).peekable();
        for (i, mut p) in std::mem::take(&mut self.active).into_iter().enumerate() {
            if selected.peek().is_some_and(|(index, _)| *index == i) {
                let (_, plan) = selected.next().unwrap();
                if let Some(plan) = &plan
                    && let Some(times) = &plan.departures
                {
                    let w = work_for_reservation(&p, self.next_minute);
                    let j = router
                        .retain_journey(&w.request(), &plan.route, times, &self.reservations)
                        .ok_or_else(|| {
                            SimulationError::Invariant("selected repair is not feasible".into())
                        })?;
                    router.reserve(&mut self.reservations, &j, p.payment.amount_cents);
                }
                self.install_suffix(scenario, &mut p, plan, events, evidence)?;
            }
            self.active.push(p);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_candidate(
        &self,
        scenario: &Scenario,
        router: &Router,
        indices: &[usize],
        work: &[Work],
        mut plans: Vec<Option<Suffix>>,
        book: Reservations,
        evidence: &mut Option<&mut DecisionEvidence>,
    ) -> Result<(Vec<Option<Suffix>>, SearchDiagnostics), SimulationError> {
        let pending: Vec<_> = (0..work.len()).filter(|&i| plans[i].is_none()).collect();
        match scenario.strategy {
            RoutingStrategy::Reserved { limits } => {
                let requests: Vec<_> = pending.iter().map(|&i| work[i].request()).collect();
                let (new, stats) = router.allocate_observed(&requests, &book, limits, evidence);
                for (i, plan) in pending.into_iter().zip(new) {
                    plans[i] = plan.as_ref().map(|j| from_journey(router, j));
                }
                Ok((plans, stats))
            }
            RoutingStrategy::CheapestStatic => {
                for i in pending {
                    let mut view = scenario.network.clone();
                    for (r, rail) in view.rails.iter_mut().enumerate() {
                        rail.available &= scenario.services[r].is_open(work[i].release)
                            && scenario.services[r]
                                .capacity_per_minute_cents
                                .is_none_or(|c| work[i].payment.amount_cents <= c);
                    }
                    plans[i] =
                        static_suffix(view, &self.active[indices[i]], self.next_minute, evidence)?;
                }
                Ok((plans, SearchDiagnostics::default()))
            }
        }
    }
}
fn work_for_reservation(p: &ActivePayment, minute: u128) -> Work {
    work(p, minute)
}

fn assess(
    work: &[Work],
    plans: &[Option<Suffix>],
    minute: u128,
) -> Result<PlanAssessment, SimulationError> {
    let mut a = PlanAssessment {
        eligible_payments: work.len(),
        ..Default::default()
    };
    for (w, plan) in work.iter().zip(plans) {
        if let Some(plan) = plan {
            a.planned += 1;
            add(
                &mut a.remaining_fee_cents,
                plan.route.total_fee_cents,
                "assessed fee",
            )?;
            add(
                &mut a.remaining_elapsed_minutes,
                plan.arrival - minute,
                "assessed elapsed",
            )?;
            a.remaining_hops = a
                .remaining_hops
                .checked_add(plan.route.hops.len())
                .ok_or(SimulationError::ArithmeticOverflow("assessed hops"))?;
        } else {
            a.unplanned += 1;
        }
        if let Some(old) = &w.old {
            a.previously_planned += 1;
            if let Some(plan) = plan {
                if old.route.hops != plan.route.hops {
                    a.changed_routes += 1;
                    a.changed_assignments += 1;
                } else if old.departures != plan.departures {
                    a.retimed_only += 1;
                    a.changed_assignments += 1;
                }
            } else {
                a.withdrawn += 1;
                a.changed_assignments += 1;
            }
        } else if plan.is_some() {
            a.newly_planned += 1;
        }
    }
    Ok(a)
}
