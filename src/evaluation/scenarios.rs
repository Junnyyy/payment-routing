//! Version 1 synthetic evaluation suite. These are counterexamples, not forecasts.
use super::World;
use crate::{
    network::{Institution, Network, Rail},
    operations::Preset,
    simulation::*,
};

pub const NAMES: &[&str] = &[
    "balanced",
    "pressure",
    "outage",
    "limited",
    "disruptions",
    "missed-connection",
    "reservation-trap",
    "disconnected",
    "capacity-cliff",
];

pub fn named(name: &str) -> Option<World> {
    if let Some(preset) = Preset::ALL.iter().find(|p| p.name() == name) {
        return Some(World {
            name: name.into(),
            scenario: preset.scenario(RoutingStrategy::CheapestStatic),
        });
    }
    let mut c = small();
    match name {
        "missed-connection" => {
            c.network.rails = vec![
                rail("ab", &["A", "B"], 2, 1),
                rail("bc", &["B", "C"], 3, 2),
                rail("ac", &["A", "C"], 20, 0),
            ];
            c.arrivals.min_sla_minutes = 3;
            c.arrivals.max_sla_minutes = 3;
        }
        "reservation-trap" => {
            // A cheap window lures reservations. An unannounced closure makes
            // immediate expensive static execution the better service decision.
            c.network.rails = vec![
                rail("cheap", &["A", "C"], 0, 0),
                rail("fast", &["A", "C"], 20, 0),
            ];
            c.arrivals.min_sla_minutes = 6;
            c.arrivals.max_sla_minutes = 6;
            c.disruptions = vec![
                Disruption {
                    minute: 1,
                    update: RailUpdate {
                        rail_id: "cheap".into(),
                        available: Some(false),
                        capacity_per_minute_cents: None,
                    },
                },
                Disruption {
                    minute: 1,
                    update: RailUpdate {
                        rail_id: "fast".into(),
                        available: Some(false),
                        capacity_per_minute_cents: None,
                    },
                },
                Disruption {
                    minute: 8,
                    update: RailUpdate {
                        rail_id: "cheap".into(),
                        available: Some(true),
                        capacity_per_minute_cents: None,
                    },
                },
                Disruption {
                    minute: 8,
                    update: RailUpdate {
                        rail_id: "fast".into(),
                        available: Some(true),
                        capacity_per_minute_cents: None,
                    },
                },
            ];
        }
        "disconnected" => {
            c.network.rails = vec![rail("ab", &["A", "B"], 0, 0)];
            c.max_active_payments = 3;
            c.arrivals.attempts_per_minute = 4;
        }
        "capacity-cliff" => {
            c.network.rails = vec![
                rail("cheap", &["A", "C"], 1, 2),
                rail("fast", &["A", "C"], 30, 0),
            ];
            c.arrivals.attempts_per_minute = 6;
            c.arrivals.min_amount_cents = 50;
            c.arrivals.max_amount_cents = 200;
            c.arrivals.min_sla_minutes = 0;
            c.arrivals.max_sla_minutes = 5;
            c.max_active_payments = 8;
            for (minute, cap) in [(3, 0), (9, 200), (15, 50), (21, 200)] {
                c.disruptions.push(Disruption {
                    minute,
                    update: RailUpdate {
                        rail_id: "cheap".into(),
                        available: None,
                        capacity_per_minute_cents: Some(Some(cap)),
                    },
                });
            }
        }
        _ => return None,
    }
    c.services = c
        .network
        .rails
        .iter()
        .map(|r| RailService {
            rail_id: r.id.clone(),
            period_minutes: 1,
            offset_minutes: 0,
            open_minutes: 1,
            capacity_per_minute_cents: (name == "capacity-cliff").then_some(200),
        })
        .collect();
    if name == "missed-connection" {
        c.services[1].period_minutes = 2;
        c.services[1].open_minutes = 1;
    }
    if name == "reservation-trap" {
        c.services[0].period_minutes = 8;
        c.services[0].offset_minutes = 4;
        c.services[0].open_minutes = 1;
    }
    Some(World {
        name: name.into(),
        scenario: c,
    })
}

pub fn all() -> Vec<World> {
    NAMES.iter().map(|name| named(name).unwrap()).collect()
}

fn rail(id: &str, participants: &[&str], fee: u64, latency: u32) -> Rail {
    Rail {
        id: id.into(),
        name: id.into(),
        participants: participants.iter().map(|p| (*p).into()).collect(),
        fee_cents: fee,
        settlement_minutes: latency,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: None,
    }
}
fn small() -> Scenario {
    Scenario {
        network: Network {
            name: "Evaluation adversary v1".into(),
            institutions: ["A", "B", "C"]
                .iter()
                .map(|id| Institution {
                    id: (*id).into(),
                    name: (*id).into(),
                    opening_balance_cents: 0,
                })
                .collect(),
            rails: vec![],
            payments: vec![],
        },
        arrivals: ArrivalProcess {
            attempts_per_minute: 2,
            probability_per_million: 800_000,
            flows: vec![PaymentFlow {
                sender: "A".into(),
                receiver: "C".into(),
            }],
            min_amount_cents: 100,
            max_amount_cents: 100,
            min_sla_minutes: 2,
            max_sla_minutes: 4,
        },
        services: vec![],
        strategy: RoutingStrategy::CheapestStatic,
        max_active_payments: 16,
        retained_events: 0,
        disruptions: vec![],
    }
}
