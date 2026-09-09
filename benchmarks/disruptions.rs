//! Small synthetic disruption families with independently inspectable optima.
use payment_routing::{network::*, scheduling::*, simulation::*};

pub fn recovery(count: u32, old_fee: u64, new_fee: u64) -> Scenario {
    let mut c = Scenario {
        network: Network {
            name: "Disruption comparison".into(),
            institutions: ["A", "B", "C"]
                .into_iter()
                .map(|id| Institution {
                    id: id.into(),
                    name: id.into(),
                    opening_balance_cents: 0,
                })
                .collect(),
            rails: [("A", new_fee, false), ("B", old_fee, true)]
                .into_iter()
                .map(|(id, fee, available)| Rail {
                    id: id.into(),
                    name: id.into(),
                    participants: vec!["A".into(), "B".into()],
                    fee_cents: fee,
                    settlement_minutes: 0,
                    available,
                    max_amount_cents: None,
                    batch_capacity_cents: None,
                })
                .collect(),
            payments: vec![],
        },
        arrivals: ArrivalProcess {
            attempts_per_minute: count,
            probability_per_million: 1_000_000,
            flows: vec![PaymentFlow {
                sender: "A".into(),
                receiver: "B".into(),
            }],
            min_amount_cents: 1,
            max_amount_cents: 1,
            min_sla_minutes: 3,
            max_sla_minutes: 3,
        },
        services: ["A", "B"]
            .into_iter()
            .map(|id| RailService {
                rail_id: id.into(),
                period_minutes: 10,
                offset_minutes: 3,
                open_minutes: 1,
                capacity_per_minute_cents: None,
            })
            .collect(),
        strategy: RoutingStrategy::Reserved {
            limits: Default::default(),
        },
        max_active_payments: 256,
        retained_events: 32,
        disruptions: vec![],
    };
    c.disruptions.push(Disruption {
        minute: 1,
        update: RailUpdate {
            rail_id: "A".into(),
            available: Some(true),
            capacity_per_minute_cents: None,
        },
    });
    c
}
pub fn scarcity() -> Scenario {
    let mut c = recovery(2, 1, 2);
    c.arrivals.max_amount_cents = 2;
    c.services[1].capacity_per_minute_cents = Some(2);
    c.services[0].capacity_per_minute_cents = Some(1);
    c.network.rails[0].max_amount_cents = Some(1);
    c.network.rails.push(Rail {
        id: "X".into(),
        name: "X".into(),
        participants: vec!["C".into(), "A".into()],
        fee_cents: 1,
        settlement_minutes: 0,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: None,
    });
    c.services.push(RailService {
        rail_id: "X".into(),
        period_minutes: 1,
        offset_minutes: 0,
        open_minutes: 1,
        capacity_per_minute_cents: None,
    });
    c.arrivals.flows.push(PaymentFlow {
        sender: "C".into(),
        receiver: "B".into(),
    });
    c
}
pub fn hop_recovery() -> Scenario {
    let mut c = recovery(1, 1, 1);
    c.network.rails[1].participants = vec!["A".into(), "C".into()];
    let mut rail = c.network.rails[1].clone();
    rail.id = "Y".into();
    rail.name = "Y".into();
    rail.participants = vec!["C".into(), "B".into()];
    rail.fee_cents = 0;
    c.network.rails.push(rail);
    let mut service = c.services[1].clone();
    service.rail_id = "Y".into();
    c.services.push(service);
    c
}
pub fn cases() -> Vec<(&'static str, Scenario, u64)> {
    let mut delay = recovery(8, 5, 5);
    delay.arrivals.min_sla_minutes = 9;
    delay.arrivals.max_sla_minutes = 9;
    delay.services[0].offset_minutes = 2;
    delay.services[1].offset_minutes = 8;
    let mut capacity = recovery(2, 5, 9);
    capacity.services[1].capacity_per_minute_cents = Some(2);
    capacity.disruptions[0].update = RailUpdate {
        rail_id: "B".into(),
        available: None,
        capacity_per_minute_cents: Some(Some(1)),
    };
    vec![
        ("equal-recovery-24", recovery(24, 5, 5), 42),
        ("expensive-recovery-24", recovery(24, 100, 1), 42),
        ("earlier-recovery-8", delay, 42),
        ("scarcity-sla", scarcity(), 57),
        ("fewer-hops", hop_recovery(), 42),
        ("capacity-loss", capacity, 42),
        ("equal-exact-4", recovery(4, 5, 5), 42),
        ("expensive-exact-4", recovery(4, 100, 1), 42),
    ]
}

/// Exact projection of the *entire existing cohort*, no omitted carry-in demand.
/// These fixtures have no departed prefix at minute 1. The finite calendar is
/// enumerated independently of the bounded recurring router. Keep cohorts small.
pub fn exact_at_one(
    effective: &Scenario,
    cohort: &[ActivePayment],
) -> Result<Option<ScheduledBatchPlan>, ValidationError> {
    assert!(cohort.len() <= 4);
    let deadline = cohort.iter().map(|p| p.deadline).max().unwrap_or(1);
    assert!(deadline <= 9);
    assert!(
        cohort
            .iter()
            .all(|p| p.next_hop == 0 && p.in_flight_until.is_none())
    );
    let payments: Vec<_> = cohort
        .iter()
        .map(|p| TimedPayment {
            payment: p.payment.clone(),
            earliest_execution_minute: 1,
            deadline_minute: Some(p.deadline as u64),
        })
        .collect();
    let mut departures = vec![];
    for s in &effective.services {
        for minute in 1..=deadline as u64 {
            let phase = minute % s.period_minutes;
            if phase >= s.offset_minutes && phase - s.offset_minutes < s.open_minutes {
                departures.push(RailDeparture {
                    rail_id: s.rail_id.clone(),
                    departure_minute: minute,
                    capacity_cents: s.capacity_per_minute_cents,
                    fee_cents: None,
                    settlement_minutes: None,
                });
            }
        }
    }
    optimize_schedule(&effective.network, &payments, &departures)
}
