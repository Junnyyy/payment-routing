//! One fresh-process benchmark worker; use scripts/benchmark.py for supervision.
#[path = "../benchmarks/audit.rs"]
mod audit;
#[path = "../benchmarks/fixtures.rs"]
mod fixtures;
use payment_routing::{
    batch::optimize_batch, routing::route_payment, scheduling::optimize_schedule, simulation::*,
};
use std::{collections::BTreeMap, io::Write, time::Instant};

#[derive(Default)]
struct Record(BTreeMap<String, String>);
impl Record {
    fn number(&mut self, key: &str, value: impl std::fmt::Display) {
        self.0.insert(key.into(), value.to_string());
    }
    fn string(&mut self, key: &str, value: impl AsRef<str>) {
        self.0.insert(key.into(), quote(value.as_ref()));
    }
    fn print(&self) {
        println!(
            "{{{}}}",
            self.0
                .iter()
                .map(|(k, v)| format!("{}:{v}", quote(k)))
                .collect::<Vec<_>>()
                .join(",")
        );
        std::io::stdout().flush().unwrap();
    }
}
fn quote(s: &str) -> String {
    let mut q = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => q.push_str("\\\""),
            '\\' => q.push_str("\\\\"),
            '\n' => q.push_str("\\n"),
            '\r' => q.push_str("\\r"),
            '\t' => q.push_str("\\t"),
            c if c < ' ' => q.push_str(&format!("\\u{:04x}", c as u32)),
            c => q.push(c),
        }
    }
    q.push('"');
    q
}
fn digest(s: &str) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}
fn stats(r: &mut Record) {
    r.number("instrumented", cfg!(feature = "search-stats"));
    #[cfg(feature = "search-stats")]
    {
        let s = payment_routing::search_stats::snapshot();
        r.number("solver_calls", s.solver_calls);
        r.number("path_states", s.path_states);
        r.number("candidates", s.candidates);
        r.number("candidate_hops", s.candidate_hops);
        r.number("assignment_states", s.assignment_states);
        r.number("complete_assignments", s.complete_assignments);
        r.number("bound_prunes", s.bound_prunes);
        r.number("deadline_prunes", s.deadline_prunes);
        r.number("capacity_rejects", s.capacity_rejects);
    }
}
fn reset() {
    #[cfg(feature = "search-stats")]
    payment_routing::search_stats::reset();
}
fn topology(r: &mut Record, n: &payment_routing::network::Network) {
    r.number("institutions", n.institutions.len());
    r.number("rails", n.rails.len());
    let arcs: usize = n
        .rails
        .iter()
        .map(|r| r.participants.len() * (r.participants.len() - 1))
        .sum();
    r.number("directed_rail_choices", arcs);
}
fn main() {
    let a: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        (3..=5).contains(&a.len()),
        "worker FAMILY SCALE SEED [TICKS] [--dump]"
    );
    let family = &a[0];
    let scale: usize = a[1].parse().unwrap();
    let seed: u64 = a[2].parse().unwrap();
    assert!(
        (2..=if family == "sim-history" {
            100000
        } else {
            10000
        })
            .contains(&scale)
    );
    let ticks: u64 = a
        .get(3)
        .filter(|s| s.as_str() != "--dump")
        .map(|s| s.parse().unwrap())
        .unwrap_or(1000);
    assert!((1..=100000).contains(&ticks));
    let dump = a.iter().any(|s| s == "--dump");
    let mut meta = Record::default();
    meta.string("kind", "input");
    meta.number("schema", 1);
    meta.string("family", family);
    meta.number("scale", scale);
    meta.number("seed", seed);
    if family.starts_with("sim-") {
        let config = fixtures::simulation_case(family, scale);
        let input = format!("{config:?};seed={seed};ticks={ticks}");
        topology(&mut meta, &config.network);
        meta.number("ticks", ticks);
        meta.number("active_limit", config.max_active_payments);
        meta.number("history_limit", config.retained_events);
        meta.string("input_digest", digest(&input));
        if dump {
            meta.string("fixture", &input);
        }
        meta.print();
        let mut sim = Simulator::new(config, seed).unwrap();
        let mut active = vec![];
        let mut queued = vec![];
        let mut waits = vec![];
        reset();
        let start = Instant::now();
        for _ in 0..ticks {
            sim.step().unwrap();
            let work = sim.active_payments();
            active.push(work.len());
            queued.push(work.iter().filter(|p| p.in_flight_until.is_none()).count());
            waits.push(
                work.iter()
                    .filter(|p| p.in_flight_until.is_none())
                    .map(|p| sim.next_minute() - 1 - p.arrived_at)
                    .max()
                    .unwrap_or(0),
            );
        }
        let elapsed = start.elapsed().as_nanos();
        let mut out = Record::default();
        stats(&mut out);
        sim.check_invariants().unwrap();
        let m = sim.metrics();
        out.string("kind", "result");
        out.string("status", "simulated");
        out.number("solve_ns", elapsed);
        out.number("generated", m.generated);
        out.number("completed", m.completed);
        out.number("completed_on_time", m.completed - m.completed_late);
        out.number("rejected", m.rejected);
        out.number("expired", m.expired);
        out.number("sla_failures", m.sla_failures);
        out.number("fee_cents", m.routing_cost_cents);
        out.number("completed_volume_cents", m.completed_volume_cents);
        out.number("completed_elapsed_minutes", m.completed_elapsed_minutes);
        out.number("departed_hops", m.departed_hops);
        out.number("active_end", sim.active_payments().len());
        out.number("active_peak", active.iter().max().unwrap());
        out.number("active_sum", active.iter().sum::<usize>());
        out.number("queue_sum", queued.iter().sum::<usize>());
        out.number("queue_peak", queued.iter().max().unwrap());
        queued.sort_unstable();
        out.number("queue_p95", queued[(queued.len() * 95).div_ceil(100) - 1]);
        out.number("oldest_queued_age_peak", waits.iter().max().unwrap());
        out.number("events", sim.event_count());
        out.number("retained_events", sim.recent_events().len());
        out.string("rail_metrics", format!("{:?}", sim.rail_states()));
        out.string(
            "result_digest",
            digest(&format!(
                "{sim:?};active={active:?};queued={queued:?};waits={waits:?}"
            )),
        );
        out.print();
        return;
    }
    let (case, provenance) = if family.starts_with("window-") {
        if family.starts_with("window-contended-") {
            fixtures::capture_window_capacity(scale, seed, 2)
        } else {
            fixtures::capture_window(scale, seed)
        }
    } else {
        (
            fixtures::static_case(family, scale),
            "constructed fixture v1".into(),
        )
    };
    if family.starts_with("window-") {
        let certificate = audit::window_reference(&case.network, &case.timed, &case.slots);
        meta.number("reference_feasible", certificate.is_some());
        if let Some(c) = certificate {
            meta.number("reference_fee_cents", c.total_fee_cents);
            meta.number("reference_elapsed_upper_bound", c.total_elapsed_minutes);
            if dump {
                meta.string("reference_witness", format!("{c:?}"));
            }
        }
    }
    let input = format!("{case:?}");
    meta.string("provenance", provenance);
    meta.string("input_digest", digest(&input));
    topology(&mut meta, &case.network);
    meta.number("payments", case.payments.len());
    meta.number(
        "principal_cents",
        case.payments
            .iter()
            .map(|p| u128::from(p.amount_cents))
            .sum::<u128>(),
    );
    meta.number("departure_slots", case.slots.len());
    if dump {
        meta.string("fixture", &input);
    }
    meta.print();
    let mut out = Record::default();
    out.string("kind", "result");
    reset();
    let (score, witness, elapsed) = if family.starts_with("single-")
        || (family.starts_with("window-") && family.ends_with("-static"))
    {
        let start = Instant::now();
        let routes: Vec<_> = case
            .payments
            .iter()
            .map(|p| route_payment(&case.network, p).unwrap())
            .collect();
        let elapsed = start.elapsed().as_nanos();
        stats(&mut out);
        let feasible = routes.iter().all(Option::is_some);
        let mut score = (0, 0, 0);
        for (p, r) in case.payments.iter().zip(&routes) {
            if let Some(r) = r {
                let (f, t, h) = audit::route(&case.network, p, r);
                score.0 += f;
                score.1 += t;
                score.2 += h;
            }
        }
        (feasible.then_some(score), format!("{routes:?}"), elapsed)
    } else if family.starts_with("batch-")
        || (family.starts_with("window-") && family.ends_with("-batch"))
    {
        let start = Instant::now();
        let plan = optimize_batch(&case.network, &case.payments).unwrap();
        let elapsed = start.elapsed().as_nanos();
        stats(&mut out);
        if let Some(p) = &plan {
            audit::batch(&case.network, &case.payments, p);
        }
        (
            plan.as_ref().map(|p| {
                (
                    p.total_fee_cents,
                    p.total_settlement_minutes,
                    p.assignments.iter().map(|a| a.route.hops.len()).sum(),
                )
            }),
            format!("{plan:?}"),
            elapsed,
        )
    } else {
        let start = Instant::now();
        let plan = optimize_schedule(&case.network, &case.timed, &case.slots).unwrap();
        let elapsed = start.elapsed().as_nanos();
        stats(&mut out);
        if let Some(p) = &plan {
            audit::schedule(&case.network, &case.timed, &case.slots, p);
        }
        (
            plan.as_ref().map(|p| {
                (
                    p.total_fee_cents,
                    p.total_elapsed_minutes,
                    p.assignments.iter().map(|a| a.route.hops.len()).sum(),
                )
            }),
            format!("{plan:?}"),
            elapsed,
        )
    };
    if case.expected_infeasible {
        assert!(score.is_none());
    }
    if let Some(expected) = case.expected_fee {
        assert_eq!(score.unwrap().0, expected);
        out.number("known_optimum_fee", expected);
    }
    out.string(
        "status",
        if score.is_some() {
            "optimal"
        } else {
            "infeasible"
        },
    );
    if let Some((f, t, h)) = score {
        out.number("fee_cents", f);
        out.number("objective_minutes", t);
        out.number("objective_hops", h);
    }
    out.number("solve_ns", elapsed);
    out.string("result_digest", digest(&witness));
    if dump {
        out.string("witness", witness);
    }
    out.print();
}
