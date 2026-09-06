#![allow(dead_code)]

use payment_routing::{
    network::{Institution, Network, Rail},
    simulation::*,
};

pub fn rail(id: &str, members: &[&str], fee: u64, latency: u32) -> Rail {
    Rail {
        id: id.into(),
        name: id.into(),
        participants: members.iter().map(|s| (*s).into()).collect(),
        fee_cents: fee,
        settlement_minutes: latency,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: None,
    }
}

pub fn scenario(rails: Vec<Rail>) -> Scenario {
    Scenario {
        services: rails
            .iter()
            .map(|r| RailService {
                rail_id: r.id.clone(),
                period_minutes: 1,
                offset_minutes: 0,
                open_minutes: 1,
                capacity_per_minute_cents: None,
            })
            .collect(),
        network: Network {
            name: "Simulation test".into(),
            institutions: ["A", "B", "C", "D"]
                .into_iter()
                .map(|id| Institution {
                    id: id.into(),
                    name: id.into(),
                    opening_balance_cents: 123,
                })
                .collect(),
            rails,
            payments: vec![],
        },
        arrivals: ArrivalProcess {
            attempts_per_minute: 1,
            probability_per_million: 1_000_000,
            flows: vec![PaymentFlow {
                sender: "A".into(),
                receiver: "B".into(),
            }],
            min_amount_cents: 100,
            max_amount_cents: 100,
            min_sla_minutes: 10,
            max_sla_minutes: 10,
        },
        strategy: RoutingStrategy::CheapestStatic,
        max_active_payments: 100,
        retained_events: 100,
    }
}

pub fn sla(config: &mut Scenario, minutes: u64) {
    config.arrivals.min_sla_minutes = minutes;
    config.arrivals.max_sla_minutes = minutes;
}

pub fn amount(config: &mut Scenario, cents: u64) {
    config.arrivals.min_amount_cents = cents;
    config.arrivals.max_amount_cents = cents;
}

pub fn generated(report: &TickReport) -> Vec<(u128, payment_routing::network::Payment, u128)> {
    report
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::Generated {
                sequence,
                payment,
                deadline,
            } => Some((*sequence, payment.clone(), *deadline)),
            _ => None,
        })
        .collect()
}
