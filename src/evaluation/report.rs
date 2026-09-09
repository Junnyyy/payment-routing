use super::*;
use std::{cmp::Ordering, fmt::Write};

/// Exact fraction. Undefined ratios (zero demand) are represented by None.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ratio {
    pub numerator: u128,
    pub denominator: u128,
}
impl Ratio {
    pub fn new(numerator: u128, denominator: u128) -> Option<Self> {
        (denominator > 0).then_some(Self {
            numerator,
            denominator,
        })
    }
    /// Compare without overflowing cross-products or floating-point ranking.
    pub fn compare(self, other: Self) -> Ordering {
        let (mut a, mut b, mut c, mut d) = (
            self.numerator,
            self.denominator,
            other.numerator,
            other.denominator,
        );
        assert!(b > 0 && d > 0);
        let mut reverse = false;
        loop {
            let order = (a / b).cmp(&(c / d));
            if order != Ordering::Equal {
                return if reverse { order.reverse() } else { order };
            }
            let (r, s) = (a % b, c % d);
            if r == 0 || s == 0 {
                let order = r.cmp(&s);
                return if reverse { order.reverse() } else { order };
            }
            (a, b, c, d) = (b, r, d, s);
            reverse = !reverse;
        }
    }
    pub fn percent(self) -> String {
        format!(
            "{:.2}%",
            100.0 * self.numerator as f64 / self.denominator as f64
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseKey {
    pub world: String,
    pub seed: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggregate {
    pub strategy: String,
    pub total_cases: usize,
    /// Only cases complete for EVERY requested strategy contribute totals/ranks.
    pub matched_cases: usize,
    pub censored_cases: usize,
    pub error_cases: usize,
    pub wins: usize,
    pub tied_best: usize,
    pub metrics: Metrics,
    pub adaptation: AdaptationMetrics,
    pub diagnostics: SearchDiagnostics,
    pub never_routed_expired: u128,
    pub queue_payment_minutes: u128,
    pub peak_queue: usize,
    pub peak_active: usize,
    /// Nearest-rank p95 across nonempty matched cases, not across payments.
    pub p95_not_on_time: Option<Ratio>,
    pub worst_case: Option<CaseKey>,
    pub worst_not_on_time: Option<Ratio>,
    /// Largest on-time count shortfall / offered count versus a tested strategy.
    /// This compares observed policies, never a clairvoyant optimum.
    pub worst_service_shortfall: Option<Ratio>,
    pub worst_shortfall_case: Option<CaseKey>,
    pub replay_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairComparison {
    pub left: String,
    pub right: String,
    pub only_left_on_time: Vec<u128>,
    pub only_right_on_time: Vec<u128>,
    pub common_on_time: u128,
    pub common_left_fee_cents: u128,
    pub common_right_fee_cents: u128,
}
impl CaseResult {
    /// Payment-ID matched evidence; partial/error runs are deliberately excluded.
    pub fn pairs(&self) -> Result<Vec<PairComparison>, SimulationError> {
        let mut pairs = Vec::new();
        for (i, left) in self.runs.iter().enumerate() {
            for right in &self.runs[i + 1..] {
                if left.status != Status::Complete || right.status != Status::Complete {
                    continue;
                }
                let mut p = PairComparison {
                    left: left.strategy.clone(),
                    right: right.strategy.clone(),
                    only_left_on_time: vec![],
                    only_right_on_time: vec![],
                    common_on_time: 0,
                    common_left_fee_cents: 0,
                    common_right_fee_cents: 0,
                };
                for (&id, a) in &left.payments {
                    let b = &right.payments[&id];
                    match (a.on_time(), b.on_time()) {
                        (true, true) => {
                            add(&mut p.common_on_time, 1)?;
                            add(&mut p.common_left_fee_cents, a.actual_fee_cents)?;
                            add(&mut p.common_right_fee_cents, b.actual_fee_cents)?;
                        }
                        (true, false) => p.only_left_on_time.push(id),
                        (false, true) => p.only_right_on_time.push(id),
                        _ => {}
                    }
                }
                pairs.push(p);
            }
        }
        Ok(pairs)
    }
}

fn sum_metrics(a: &mut Metrics, b: &Metrics) -> Result<(), SimulationError> {
    macro_rules! sum { ($($field:ident),* $(,)?) => { $(add(&mut a.$field, b.$field)?;)* }; }
    sum!(
        generated,
        generated_volume_cents,
        accepted_routes,
        rejected,
        rejected_volume_cents,
        expired,
        expired_volume_cents,
        completed,
        completed_volume_cents,
        completed_late,
        completed_late_volume_cents,
        sla_failures,
        sla_failed_volume_cents,
        routing_cost_cents,
        completed_elapsed_minutes,
        departed_hops,
        settled_hops,
        departed_principal_cents,
        settled_principal_cents
    );
    Ok(())
}
fn sum_adaptation(a: &mut AdaptationMetrics, b: &AdaptationMetrics) -> Result<(), SimulationError> {
    macro_rules! sum { ($($field:ident),* $(,)?) => { $(add(&mut a.$field, b.$field)?;)* }; }
    sum!(
        rail_changes,
        reoptimizations,
        assignment_comparisons,
        changed_assignments,
        changed_routes,
        retimed_only,
        withdrawn
    );
    Ok(())
}
fn sum_diagnostics(
    a: &mut SearchDiagnostics,
    b: &SearchDiagnostics,
) -> Result<(), SimulationError> {
    macro_rules! sum { ($($field:ident),* $(,)?) => { $(add(&mut a.$field, b.$field)?;)* }; }
    sum!(
        searches,
        expansions,
        candidates,
        truncated_searches,
        unresolved,
        repair_trials
    );
    Ok(())
}

impl Evaluation {
    pub fn has_incomplete_runs(&self) -> bool {
        self.cases
            .iter()
            .any(|c| c.runs.iter().any(|r| r.status != Status::Complete))
    }
    pub fn aggregates(&self) -> Result<Vec<Aggregate>, SimulationError> {
        self.aggregate_selected(None)
    }
    pub fn aggregates_for_world(&self, world: &str) -> Result<Vec<Aggregate>, SimulationError> {
        if !self.worlds.iter().any(|w| w.name == world) {
            return Err(invalid("unknown aggregate world"));
        }
        self.aggregate_selected(Some(world))
    }
    fn aggregate_groups(&self) -> Result<Vec<(String, Vec<Aggregate>)>, SimulationError> {
        let mut groups = vec![(String::new(), self.aggregates()?)];
        for world in &self.worlds {
            groups.push((world.name.clone(), self.aggregates_for_world(&world.name)?));
        }
        Ok(groups)
    }
    fn aggregate_selected(&self, world: Option<&str>) -> Result<Vec<Aggregate>, SimulationError> {
        let cases: Vec<_> = self
            .cases
            .iter()
            .filter(|c| world.is_none_or(|w| c.world == w))
            .collect();
        let mut result = Vec::new();
        for (index, strategy) in self.strategies.iter().enumerate() {
            let mut a = Aggregate {
                strategy: strategy.name.clone(),
                total_cases: cases.len(),
                matched_cases: 0,
                censored_cases: 0,
                error_cases: 0,
                wins: 0,
                tied_best: 0,
                metrics: Metrics::default(),
                adaptation: AdaptationMetrics::default(),
                diagnostics: SearchDiagnostics::default(),
                never_routed_expired: 0,
                queue_payment_minutes: 0,
                peak_queue: 0,
                peak_active: 0,
                p95_not_on_time: None,
                worst_case: None,
                worst_not_on_time: None,
                worst_service_shortfall: None,
                worst_shortfall_case: None,
                replay_verified: self.config.verify_replay,
            };
            let mut rates = Vec::new();
            for case in &cases {
                let run = &case.runs[index];
                a.replay_verified &= run.replay_verified;
                match run.status {
                    Status::Censored => a.censored_cases += 1,
                    Status::Error { .. } => a.error_cases += 1,
                    Status::Complete => {}
                }
                if case.runs.iter().any(|r| r.status != Status::Complete) {
                    continue;
                }
                a.matched_cases += 1;
                let best = case.runs.iter().filter_map(RunResult::score).min().unwrap();
                if run.score() == Some(best) {
                    if case.runs.iter().filter(|r| r.score() == Some(best)).count() == 1 {
                        a.wins += 1;
                    } else {
                        a.tied_best += 1;
                    }
                }
                sum_metrics(&mut a.metrics, &run.metrics)?;
                sum_adaptation(&mut a.adaptation, &run.adaptation)?;
                sum_diagnostics(&mut a.diagnostics, &run.diagnostics)?;
                add(&mut a.never_routed_expired, run.never_routed_expired())?;
                add(&mut a.queue_payment_minutes, run.queue_payment_minutes)?;
                a.peak_queue = a.peak_queue.max(run.peak_queue);
                a.peak_active = a.peak_active.max(run.peak_active);
                let best_on_time = case.runs.iter().map(RunResult::on_time).max().unwrap();
                if let Some(shortfall) = Ratio::new(best_on_time - run.on_time(), case.offered)
                    && a.worst_service_shortfall
                        .is_none_or(|old| shortfall.compare(old).is_gt())
                {
                    a.worst_service_shortfall = Some(shortfall);
                    a.worst_shortfall_case = Some(CaseKey {
                        world: case.world.clone(),
                        seed: case.seed,
                    });
                }
                if let Some(rate) = Ratio::new(case.offered - run.on_time(), case.offered) {
                    rates.push(rate);
                    if a.worst_not_on_time
                        .is_none_or(|old| rate.compare(old).is_gt())
                    {
                        a.worst_not_on_time = Some(rate);
                        a.worst_case = Some(CaseKey {
                            world: case.world.clone(),
                            seed: case.seed,
                        });
                    }
                }
            }
            rates.sort_by(|a, b| a.compare(*b));
            let n = rates.len();
            if n > 0 {
                a.p95_not_on_time =
                    Some(rates[(n / 100) * 95 + ((n % 100) * 95).div_ceil(100) - 1]);
            }
            result.push(a);
        }
        Ok(result)
    }

    pub fn to_text(&self) -> Result<String, SimulationError> {
        let mut out = format!(
            "Payment routing evaluation v{FORMAT_VERSION} (synthetic USD)\narrival_minutes={} drain_minutes={} replay={}\nScore: minimize not-on-time count, volume; not-completed count, volume; actual fees; completed elapsed; hops; churn.\nAll fees include failed work. Censored/error rows are unranked. Percentages with zero demand are n/a.\n",
            self.config.arrival_minutes, self.config.drain_minutes, self.config.verify_replay
        );
        for s in &self.strategies {
            writeln!(
                out,
                "strategy {}: {:?}; {:?}",
                s.name, s.routing, s.reoptimization
            )
            .unwrap();
        }
        for case in &self.cases {
            writeln!(
                out,
                "\n{} seed={} offered={} volume={}c",
                case.world, case.seed, case.offered, case.offered_volume_cents
            )
            .unwrap();
            writeln!(out, "strategy        status     on-time  SLA-fail reject expire late pending fees(c) latency-p95 queue-peak churn/comp trunc").unwrap();
            for r in &case.runs {
                let m = &r.metrics;
                writeln!(out, "{:<15} {:<10} {:>7} {:>9} {:>6} {:>6} {:>4} {:>7} {:>7} {:>11} {:>10} {}/{} {}",
                    r.strategy, status(&r.status), r.on_time(), m.sla_failures, m.rejected, m.expired,
                    m.completed_late, r.pending(), m.routing_cost_cents, optional(r.elapsed_percentile(95)),
                    r.peak_queue, r.adaptation.changed_assignments, r.adaptation.assignment_comparisons,
                    r.diagnostics.truncated_searches).unwrap();
                writeln!(out, "  on-time={} volume={}c; completed={} volume={}c; throughput={}/{} payments/min; cutoff-completed={}; elapsed={}m max={}m; hops={}; never-routed-expired={}; pending-volume={}c; queue-area={} payment-min; peak-active={}",
                    rate(r.on_time(), case.offered), r.on_time_volume(), m.completed, m.completed_volume_cents,
                    m.completed, r.processed_minutes, r.at_arrival_cutoff.completed, m.completed_elapsed_minutes,
                    optional(r.elapsed_percentile(100)), m.departed_hops, r.never_routed_expired(), r.pending_volume(),
                    r.queue_payment_minutes, r.peak_active).unwrap();
                if let Status::Error { minute, message } = &r.status {
                    writeln!(out, "  ERROR minute={minute}: {message}").unwrap();
                }
                let mut failures: Vec<_> = r.payments.values().filter(|p| !p.on_time()).collect();
                failures.sort_by_key(|p| (std::cmp::Reverse(p.actual_fee_cents), p.sequence));
                for p in failures.iter().take(3) {
                    writeln!(out, "  failure {} {}->{} amount={}c arrival={} deadline={} outcome={:?} fee={}c hops={}",
                        p.payment.id, p.payment.sender, p.payment.receiver, p.payment.amount_cents,
                        p.arrived_at, p.deadline, p.outcome, p.actual_fee_cents, p.departed_hops).unwrap();
                }
            }
            for p in case.pairs()? {
                writeln!(out, "  pair {}/{}: exclusive on-time IDs {:?}/{:?} ({} / {} total); common={} common fees={}c/{}c{}",
                    p.left, p.right, &p.only_left_on_time[..p.only_left_on_time.len().min(5)],
                    &p.only_right_on_time[..p.only_right_on_time.len().min(5)], p.only_left_on_time.len(),
                    p.only_right_on_time.len(), p.common_on_time, p.common_left_fee_cents, p.common_right_fee_cents,
                    if !p.only_left_on_time.is_empty() || !p.only_right_on_time.is_empty() { "; DIFFERENT COHORTS: total fee difference is not an optimality gap" } else { "" }).unwrap();
            }
        }
        writeln!(out, "\nAGGREGATE: identical complete case intersection only; exclusions remain visible above.").unwrap();
        for (world, aggregates) in self.aggregate_groups()? {
            writeln!(
                out,
                "scope={}",
                if world.is_empty() { "all" } else { &world }
            )
            .unwrap();
            writeln!(out, "strategy        matched/all errors censored wins ties pooled-on-time SLA-fail fees(c) p95-case-loss worst-case-loss worst world/seed").unwrap();
            for a in aggregates {
                writeln!(
                    out,
                    "{:<15} {}/{} {} {} {} {} {} {} {} {} {} {}",
                    a.strategy,
                    a.matched_cases,
                    a.total_cases,
                    a.error_cases,
                    a.censored_cases,
                    a.wins,
                    a.tied_best,
                    rate(
                        a.metrics.completed - a.metrics.completed_late,
                        a.metrics.generated
                    ),
                    a.metrics.sla_failures,
                    a.metrics.routing_cost_cents,
                    a.p95_not_on_time.map_or("n/a".into(), Ratio::percent),
                    a.worst_not_on_time.map_or("n/a".into(), Ratio::percent),
                    a.worst_case
                        .map_or("n/a".into(), |k| format!("{}/{}", k.world, k.seed))
                )
                .unwrap();
                writeln!(out, "  largest service shortfall vs tested best={} at {}; completed={} volume={}c elapsed={}m hops={} queue-area={} churn={}/{} trunc={}",
                    a.worst_service_shortfall.map_or("n/a".into(), Ratio::percent),
                    a.worst_shortfall_case.map_or("n/a".into(), |k| format!("{}/{}", k.world, k.seed)),
                    a.metrics.completed, a.metrics.completed_volume_cents, a.metrics.completed_elapsed_minutes,
                    a.metrics.departed_hops, a.queue_payment_minutes, a.adaptation.changed_assignments,
                    a.adaptation.assignment_comparisons, a.diagnostics.truncated_searches).unwrap();
            }
        }
        Ok(out)
    }

    /// One rectangular, quoted CSV with case and aggregate records. Exact raw
    /// metrics and full input manifests make rounding/display choices reversible.
    pub fn to_csv(&self) -> Result<String, SimulationError> {
        let mut out = String::new();
        let header = [
            "record",
            "version",
            "world",
            "seed",
            "strategy",
            "status",
            "arrival_minutes",
            "drain_minutes",
            "replay_verified",
            "processed_minutes",
            "offered",
            "offered_volume_cents",
            "matched_cases",
            "total_cases",
            "error_cases",
            "censored_cases",
            "wins",
            "tied_best",
            "on_time",
            "on_time_volume_cents",
            "pending",
            "pending_volume_cents",
            "never_routed_expired",
            "elapsed_p95",
            "elapsed_max",
            "peak_active",
            "peak_queue",
            "queue_payment_minutes",
            "cutoff_completed",
            "worst_world",
            "worst_seed",
            "worst_loss_numerator",
            "worst_loss_denominator",
            "p95_loss_numerator",
            "p95_loss_denominator",
            "generated",
            "generated_volume_cents",
            "accepted_routes",
            "rejected",
            "rejected_volume_cents",
            "expired",
            "expired_volume_cents",
            "completed",
            "completed_volume_cents",
            "completed_late",
            "completed_late_volume_cents",
            "sla_failures",
            "sla_failed_volume_cents",
            "routing_cost_cents",
            "completed_elapsed_minutes",
            "departed_hops",
            "settled_hops",
            "departed_principal_cents",
            "settled_principal_cents",
            "rail_changes",
            "reoptimizations",
            "assignment_comparisons",
            "changed_assignments",
            "changed_routes",
            "retimed_only",
            "withdrawn",
            "searches",
            "expansions",
            "candidates",
            "truncated_searches",
            "unresolved_search_attempts",
            "repair_trials",
            "error",
            "strategy_config",
            "world_config",
            "shortfall_world",
            "shortfall_seed",
            "shortfall_numerator",
            "shortfall_denominator",
        ];
        csv_row(&mut out, header.iter().map(|s| (*s).into()).collect());
        for case in &self.cases {
            let world = self.worlds.iter().find(|w| w.name == case.world).unwrap();
            for (index, r) in case.runs.iter().enumerate() {
                let mut row = vec![
                    "case".into(),
                    FORMAT_VERSION.to_string(),
                    case.world.clone(),
                    case.seed.to_string(),
                    r.strategy.clone(),
                    status(&r.status).into(),
                    self.config.arrival_minutes.to_string(),
                    self.config.drain_minutes.to_string(),
                    r.replay_verified.to_string(),
                    r.processed_minutes.to_string(),
                    case.offered.to_string(),
                    case.offered_volume_cents.to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    r.on_time().to_string(),
                    r.on_time_volume().to_string(),
                    r.pending().to_string(),
                    r.pending_volume().to_string(),
                    r.never_routed_expired().to_string(),
                    csv_optional(r.elapsed_percentile(95)),
                    csv_optional(r.elapsed_percentile(100)),
                    r.peak_active.to_string(),
                    r.peak_queue.to_string(),
                    r.queue_payment_minutes.to_string(),
                    r.at_arrival_cutoff.completed.to_string(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ];
                append_metrics(&mut row, &r.metrics, &r.adaptation, &r.diagnostics);
                row.push(match &r.status {
                    Status::Error { minute, message } => format!("minute={minute}: {message}"),
                    _ => String::new(),
                });
                row.push(format!("{:?}", self.strategies[index]));
                row.push(format!("{:?}", world.scenario));
                row.extend([String::new(), String::new(), String::new(), String::new()]);
                assert_eq!(row.len(), header.len());
                csv_row(&mut out, row);
            }
        }
        for (world, aggregates) in self.aggregate_groups()? {
            for (index, a) in aggregates.iter().enumerate() {
                let mut row = vec![
                    if world.is_empty() {
                        "aggregate"
                    } else {
                        "world-aggregate"
                    }
                    .into(),
                    FORMAT_VERSION.to_string(),
                    world.clone(),
                    String::new(),
                    a.strategy.clone(),
                    if a.matched_cases == a.total_cases {
                        "complete"
                    } else {
                        "partial"
                    }
                    .into(),
                    self.config.arrival_minutes.to_string(),
                    self.config.drain_minutes.to_string(),
                    a.replay_verified.to_string(),
                    String::new(),
                    a.metrics.generated.to_string(),
                    a.metrics.generated_volume_cents.to_string(),
                    a.matched_cases.to_string(),
                    a.total_cases.to_string(),
                    a.error_cases.to_string(),
                    a.censored_cases.to_string(),
                    a.wins.to_string(),
                    a.tied_best.to_string(),
                    (a.metrics.completed - a.metrics.completed_late).to_string(),
                    (a.metrics.completed_volume_cents - a.metrics.completed_late_volume_cents)
                        .to_string(),
                    "0".into(),
                    "0".into(),
                    a.never_routed_expired.to_string(),
                    String::new(),
                    String::new(),
                    a.peak_active.to_string(),
                    a.peak_queue.to_string(),
                    a.queue_payment_minutes.to_string(),
                    String::new(),
                    a.worst_case
                        .as_ref()
                        .map_or(String::new(), |k| k.world.clone()),
                    a.worst_case
                        .as_ref()
                        .map_or(String::new(), |k| k.seed.to_string()),
                    a.worst_not_on_time
                        .map_or(String::new(), |r| r.numerator.to_string()),
                    a.worst_not_on_time
                        .map_or(String::new(), |r| r.denominator.to_string()),
                    a.p95_not_on_time
                        .map_or(String::new(), |r| r.numerator.to_string()),
                    a.p95_not_on_time
                        .map_or(String::new(), |r| r.denominator.to_string()),
                ];
                append_metrics(&mut row, &a.metrics, &a.adaptation, &a.diagnostics);
                row.push(String::new());
                row.push(format!("{:?}", self.strategies[index]));
                row.push(String::new());
                row.push(
                    a.worst_shortfall_case
                        .as_ref()
                        .map_or(String::new(), |k| k.world.clone()),
                );
                row.push(
                    a.worst_shortfall_case
                        .as_ref()
                        .map_or(String::new(), |k| k.seed.to_string()),
                );
                row.push(
                    a.worst_service_shortfall
                        .map_or(String::new(), |r| r.numerator.to_string()),
                );
                row.push(
                    a.worst_service_shortfall
                        .map_or(String::new(), |r| r.denominator.to_string()),
                );
                assert_eq!(row.len(), header.len());
                csv_row(&mut out, row);
            }
        }
        Ok(out)
    }

    pub fn payments_csv(&self) -> String {
        let mut out = String::new();
        csv_row(
            &mut out,
            [
                "version",
                "world",
                "seed",
                "strategy",
                "run_status",
                "sequence",
                "payment_id",
                "sender",
                "receiver",
                "amount_cents",
                "arrival",
                "deadline",
                "terminal_at",
                "outcome",
                "ever_routed",
                "sla_failed",
                "actual_fee_cents",
                "departed_hops",
            ]
            .map(str::to_string)
            .to_vec(),
        );
        for case in &self.cases {
            for r in &case.runs {
                for p in r.payments.values() {
                    csv_row(
                        &mut out,
                        vec![
                            FORMAT_VERSION.to_string(),
                            case.world.clone(),
                            case.seed.to_string(),
                            r.strategy.clone(),
                            status(&r.status).into(),
                            p.sequence.to_string(),
                            p.payment.id.clone(),
                            p.payment.sender.clone(),
                            p.payment.receiver.clone(),
                            p.payment.amount_cents.to_string(),
                            p.arrived_at.to_string(),
                            p.deadline.to_string(),
                            csv_optional(p.terminal_at),
                            format!("{:?}", p.outcome),
                            p.ever_routed.to_string(),
                            p.sla_failed.to_string(),
                            p.actual_fee_cents.to_string(),
                            p.departed_hops.to_string(),
                        ],
                    );
                }
            }
        }
        out
    }
}
fn append_metrics(
    row: &mut Vec<String>,
    m: &Metrics,
    a: &AdaptationMetrics,
    d: &SearchDiagnostics,
) {
    macro_rules! fields { ($source:ident; $($field:ident),* $(,)?) => { $(row.push($source.$field.to_string());)* }; }
    fields!(m; generated, generated_volume_cents, accepted_routes, rejected, rejected_volume_cents, expired, expired_volume_cents, completed, completed_volume_cents, completed_late, completed_late_volume_cents, sla_failures, sla_failed_volume_cents, routing_cost_cents, completed_elapsed_minutes, departed_hops, settled_hops, departed_principal_cents, settled_principal_cents);
    fields!(a; rail_changes, reoptimizations, assignment_comparisons, changed_assignments, changed_routes, retimed_only, withdrawn);
    fields!(d; searches, expansions, candidates, truncated_searches, unresolved, repair_trials);
}
fn csv_row(out: &mut String, row: Vec<String>) {
    for (i, value) in row.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&value.replace('"', "\"\""));
        out.push('"');
    }
    out.push('\n');
}
fn status(status: &Status) -> &'static str {
    match status {
        Status::Complete => "complete",
        Status::Censored => "censored",
        Status::Error { .. } => "error",
    }
}
fn optional(value: Option<u128>) -> String {
    value.map_or("n/a".into(), |v| v.to_string())
}
fn csv_optional(value: Option<u128>) -> String {
    value.map_or(String::new(), |v| v.to_string())
}
fn rate(n: u128, d: u128) -> String {
    Ratio::new(n, d).map_or("n/a".into(), Ratio::percent)
}
