//! Versioned, deterministic synthetic fixtures. Never calls a production solver.
use payment_routing::{demo::demo_network, network::*, scheduling::*, simulation::*};

#[derive(Clone, Debug)]
pub struct StaticCase {
    pub network: Network,
    pub payments: Vec<Payment>,
    pub timed: Vec<TimedPayment>,
    pub slots: Vec<RailDeparture>,
    pub expected_fee: Option<u128>,
    pub expected_infeasible: bool,
}

pub fn network(n: usize) -> Network {
    Network {
        name: "Benchmark synthetic USD v1".into(),
        institutions: (0..n)
            .map(|i| Institution {
                id: format!("N{i:03}"),
                name: format!("Institution {i}"),
                opening_balance_cents: 0,
            })
            .collect(),
        rails: vec![],
        payments: vec![],
    }
}

pub fn rail(id: usize, members: Vec<String>, fee: u64, latency: u32) -> Rail {
    Rail {
        id: format!("R{id:03}"),
        name: format!("Synthetic rail {id}"),
        participants: members,
        fee_cents: fee,
        settlement_minutes: latency,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: None,
    }
}

pub fn payment(i: usize, to: usize) -> Payment {
    Payment {
        id: format!("P{i:06}"),
        sender: "N000".into(),
        receiver: format!("N{to:03}"),
        amount_cents: 1,
        max_delivery_minutes: None,
    }
}

pub fn static_case(family: &str, scale: usize) -> StaticCase {
    assert!(scale >= 2);
    let mut case = StaticCase {
        network: network(2),
        payments: vec![],
        timed: vec![],
        slots: vec![],
        expected_fee: None,
        expected_infeasible: false,
    };
    match family {
        "single-positive"
        | "single-zero"
        | "single-deadline"
        | "single-disconnected"
        | "batch-density" => {
            let disconnected = family == "single-disconnected";
            case.network = network(scale + usize::from(disconnected));
            let members = case.network.institutions[..scale]
                .iter()
                .map(|i| i.id.clone())
                .collect();
            let fee = u64::from(family == "single-positive" || family == "batch-density");
            case.network.rails.push(rail(0, members, fee, 1));
            let mut p = payment(0, case.network.institutions.len() - 1);
            if family == "single-deadline" {
                p.max_delivery_minutes = Some(2);
            }
            case.payments.push(p);
            case.expected_fee = (!disconnected).then_some(u128::from(fee));
            case.expected_infeasible = disconnected;
        }
        "single-chain" => {
            case.network = network(scale);
            for i in 0..scale - 1 {
                case.network.rails.push(rail(
                    i,
                    vec![format!("N{i:03}"), format!("N{:03}", i + 1)],
                    1,
                    1,
                ));
            }
            case.payments.push(payment(0, scale - 1));
            case.expected_fee = Some((scale - 1) as u128);
        }
        "batch-volume"
        | "batch-ties"
        | "batch-scarce"
        | "batch-infeasible"
        | "batch-ceiling"
        | "schedule-ties"
        | "schedule-contention"
        | "schedule-deadline"
        | "schedule-slots" => {
            let members = vec!["N000".into(), "N001".into()];
            let tied = family.ends_with("ties");
            case.network.rails = vec![
                rail(0, members.clone(), 1, 0),
                rail(1, members, if tied { 1 } else { 5 }, 0),
            ];
            case.payments = (0..scale).map(|i| payment(i, 1)).collect();
            case.expected_fee = Some(scale as u128);
            if family == "batch-scarce" {
                case.network.rails[0].batch_capacity_cents = Some((scale / 2) as u64);
                case.expected_fee = Some((scale / 2 + 5 * (scale - scale / 2)) as u128);
            }
            if family == "batch-infeasible" {
                case.network.rails[0].batch_capacity_cents = Some((scale / 2) as u64);
                case.network.rails[1].batch_capacity_cents = Some((scale - 1 - scale / 2) as u64);
                case.expected_fee = None;
                case.expected_infeasible = true;
            }
            if family == "batch-ceiling" {
                case.network.rails[0].max_amount_cents = Some(1);
                for p in &mut case.payments {
                    p.amount_cents = 2;
                }
                case.expected_fee = Some(5 * scale as u128);
            }
            if family == "schedule-contention"
                || family == "schedule-deadline"
                || family == "schedule-slots"
            {
                case.network.rails.pop();
                if family == "schedule-slots" {
                    case.payments.truncate(1);
                    case.expected_fee = Some(1);
                }
                for t in 0..scale {
                    case.slots
                        .push(slot(&case.network.rails[0], t as u64, Some(1)));
                }
            } else if family == "schedule-ties" {
                case.slots = case
                    .network
                    .rails
                    .iter()
                    .map(|r| slot(r, 0, None))
                    .collect();
            }
        }
        "batch-trap" | "batch-trap-infeasible-greedy" => {
            let members = vec!["N000".into(), "N001".into()];
            let mut cheap = rail(0, members.clone(), 1, 0);
            cheap.batch_capacity_cents = Some(1);
            case.network.rails = vec![cheap, rail(1, members.clone(), 3, 2)];
            if family == "batch-trap" {
                case.network.rails.push(rail(2, members, 10, 0));
            }
            case.payments = vec![payment(0, 1), payment(1, 1)];
            case.payments[1].max_delivery_minutes = Some(0);
            case.expected_fee = Some(4);
        }
        "schedule-nonfifo" => {
            case.network = network(3);
            case.network.rails = vec![
                rail(0, vec!["N000".into(), "N001".into()], 1, 5),
                rail(1, vec!["N001".into(), "N002".into()], 1, 1),
            ];
            let mut p = payment(0, 2);
            p.max_delivery_minutes = Some(4);
            case.payments = vec![p];
            case.slots = vec![
                slot(&case.network.rails[0], 0, None),
                slot(&case.network.rails[0], 1, None),
                slot(&case.network.rails[1], 3, None),
            ];
            case.slots[1].fee_cents = Some(3);
            case.slots[1].settlement_minutes = Some(1);
            case.expected_fee = Some(4);
        }
        "batch-demo" | "schedule-demo" => {
            case.network = demo_network();
            case.payments = case.network.payments.clone();
            case.expected_fee = Some(60);
            case.slots = case
                .network
                .rails
                .iter()
                .map(|r| slot(r, 0, None))
                .collect();
        }
        _ => panic!("unknown static family: {family}"),
    }
    case.timed = case
        .payments
        .iter()
        .enumerate()
        .map(|(i, p)| TimedPayment {
            payment: p.clone(),
            earliest_execution_minute: 0,
            deadline_minute: if family == "schedule-deadline" {
                Some(i as u64)
            } else {
                p.max_delivery_minutes
            },
        })
        .collect();
    case
}

pub fn slot(rail: &Rail, time: u64, capacity: Option<u64>) -> RailDeparture {
    RailDeparture {
        rail_id: rail.id.clone(),
        departure_minute: time,
        fee_cents: None,
        settlement_minutes: None,
        capacity_cents: capacity,
    }
}

pub fn simulation_case(family: &str, scale: usize) -> Scenario {
    let mut net = network(2);
    net.rails = vec![rail(0, vec!["N000".into(), "N001".into()], 1, 1)];
    let mut attempts = scale as u32;
    let mut sla = 8;
    let mut cap = Some(10);
    let mut period = 1;
    let mut open = 1;
    let mut max_active = 64;
    let mut history = 32;
    match family {
        "sim-load" | "window-static" | "window-batch" | "window-schedule" => {}
        "sim-deadline" => {
            attempts = 16;
            sla = scale as u64;
        }
        "sim-outage" => {
            attempts = 8;
            period = scale as u64;
        }
        "sim-disconnected" => {
            attempts = 8;
            open = 0;
            max_active = scale;
        }
        "sim-history" => {
            attempts = 8;
            history = scale;
        }
        "sim-density" => {
            net = network(scale);
            net.rails = vec![rail(
                0,
                net.institutions.iter().map(|i| i.id.clone()).collect(),
                0,
                0,
            )];
            attempts = 1;
            sla = 0;
            cap = None;
        }
        "sim-pinned" => {
            // The downstream service is open when a path is pinned but closed on arrival.
            net = network(3);
            net.rails = vec![
                rail(0, vec!["N000".into(), "N001".into()], 1, 1),
                rail(1, vec!["N001".into(), "N002".into()], 1, 0),
                rail(2, vec!["N000".into(), "N002".into()], 5, 0),
            ];
            attempts = 1;
            sla = 1;
            cap = None;
            period = scale as u64;
        }
        _ => panic!("unknown simulation family {family}"),
    }
    let services = net
        .rails
        .iter()
        .map(|r| RailService {
            rail_id: r.id.clone(),
            period_minutes: if family == "sim-pinned" && r.id != "R001" {
                1
            } else {
                period
            },
            offset_minutes: 0,
            open_minutes: open,
            capacity_per_minute_cents: cap,
        })
        .collect();
    Scenario {
        arrivals: ArrivalProcess {
            attempts_per_minute: attempts,
            probability_per_million: 750_000,
            flows: vec![PaymentFlow {
                sender: "N000".into(),
                receiver: net.institutions.last().unwrap().id.clone(),
            }],
            min_amount_cents: 1,
            max_amount_cents: 1,
            min_sla_minutes: sla,
            max_sla_minutes: sla,
        },
        network: net,
        services,
        strategy: RoutingStrategy::CheapestStatic,
        max_active_payments: max_active,
        retained_events: history,
    }
}

/// A retrospective isolated cohort: full capacity, no carry-in/background work.
/// The timetable covers every release through the cohort's maximum deadline.
/// Aggregate static capacities are a relaxation of the same timetable.
pub fn capture_window(width: usize, seed: u64) -> (StaticCase, String) {
    let config = simulation_case("window-schedule", 4);
    let mut sim = Simulator::new(config.clone(), seed).unwrap();
    let start = 8_u64;
    let mut timed = vec![];
    for _ in 0..start + width as u64 {
        for e in sim.step().unwrap().events {
            if e.minute >= u128::from(start) {
                if let EventKind::Generated {
                    payment, deadline, ..
                } = e.kind
                {
                    timed.push(TimedPayment {
                        payment,
                        earliest_execution_minute: e.minute as u64,
                        deadline_minute: Some(deadline as u64),
                    });
                }
            }
        }
    }
    let end = timed
        .iter()
        .map(|p| p.deadline_minute.unwrap())
        .max()
        .unwrap_or(start);
    let mut net = config.network.clone();
    let mut slots = vec![];
    for (r, s) in net.rails.iter_mut().zip(&config.services) {
        let mut budget = 0_u64;
        for t in start..=end {
            let phase = t % s.period_minutes;
            if phase >= s.offset_minutes && phase - s.offset_minutes < s.open_minutes {
                slots.push(slot(r, t, s.capacity_per_minute_cents));
                budget += s.capacity_per_minute_cents.unwrap();
            }
        }
        r.batch_capacity_cents = Some(budget);
    }
    let provenance = format!(
        "seed={seed}; warmup={start}; releases=[{start},{}); timetable=[{start},{end}]; full generated cohort including rejections; isolated from carry-in and background",
        start + width as u64
    );
    (
        StaticCase {
            payments: timed.iter().map(|p| p.payment.clone()).collect(),
            timed,
            network: net,
            slots,
            expected_fee: None,
            expected_infeasible: false,
        },
        provenance,
    )
}
