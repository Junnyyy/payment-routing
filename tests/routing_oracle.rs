//! Independent verification: the production search enumerates simple paths;
//! this oracle relaxes all bounded walks by exact hop count and elapsed minute.
//! It uses neither production search helpers nor an external solver.

use payment_routing::{
    network::{Institution, Network, Payment, Rail},
    routing::{Route, route_payment},
};

const IDS: [&str; 4] = ["A", "B", "C", "D"];
const PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

fn network() -> Network {
    Network {
        name: "Independent oracle fixture".into(),
        institutions: IDS
            .iter()
            .map(|id| Institution {
                id: (*id).into(),
                name: (*id).into(),
                opening_balance_cents: 0,
            })
            .collect(),
        rails: vec![],
        payments: vec![],
    }
}

fn payment(amount: u64, deadline: Option<u64>) -> Payment {
    Payment {
        id: "P".into(),
        sender: "A".into(),
        receiver: "D".into(),
        amount_cents: amount,
        max_delivery_minutes: deadline,
    }
}

fn rail(id: usize, participants: Vec<String>, fee: u64, minutes: u32) -> Rail {
    Rail {
        id: format!("R{id}"),
        name: format!("Rail {id}"),
        participants,
        fee_cents: fee,
        settlement_minutes: minutes,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: None,
    }
}

fn oracle(net: &Network, payment: &Payment) -> Option<(u128, u128, usize)> {
    assert_eq!(net.institutions.len(), IDS.len());
    let max_hops = IDS.len() - 1;
    let max_time = max_hops
        * net
            .rails
            .iter()
            .map(|r| r.settlement_minutes as usize)
            .max()
            .unwrap_or(0);
    let mut costs = vec![vec![vec![None::<u128>; IDS.len()]; max_time + 1]; max_hops + 1];
    let start = IDS.iter().position(|id| *id == payment.sender).unwrap();
    let end = IDS.iter().position(|id| *id == payment.receiver).unwrap();
    costs[0][0][start] = Some(0);

    for hops in 0..max_hops {
        for elapsed in 0..=max_time {
            for from in 0..IDS.len() {
                let Some(cost) = costs[hops][elapsed][from] else {
                    continue;
                };
                // Expand services directly; no production adjacency or pruning is reused.
                for rail in &net.rails {
                    if !rail.available
                        || payment.amount_cents > rail.max_amount_cents.unwrap_or(u64::MAX)
                    {
                        continue;
                    }
                    if !rail.participants.contains(&IDS[from].to_owned()) {
                        continue;
                    }
                    let time = elapsed + rail.settlement_minutes as usize;
                    if time > max_time {
                        continue;
                    }
                    for (to, id) in IDS.iter().enumerate() {
                        if from == to || !rail.participants.contains(&(*id).to_owned()) {
                            continue;
                        }
                        let candidate = cost + u128::from(rail.fee_cents);
                        let slot = &mut costs[hops + 1][time][to];
                        *slot = Some(slot.unwrap_or(u128::MAX).min(candidate));
                    }
                }
            }
        }
    }

    // Only the final scan applies the delivery budget. The oracle deliberately
    // retains all intermediate time buckets and permits revisiting institutions.
    let mut answers = vec![];
    for (hops, by_time) in costs.iter().enumerate().skip(1) {
        for (elapsed, by_node) in by_time.iter().enumerate() {
            if elapsed as u128 <= u128::from(payment.max_delivery_minutes.unwrap_or(u64::MAX))
                && let Some(cost) = by_node[end]
            {
                answers.push((cost, elapsed as u128, hops));
            }
        }
    }
    answers.into_iter().min()
}

fn verify(net: &Network, p: &Payment, scenario: usize) {
    let actual = route_payment(net, p).unwrap();
    assert_eq!(
        actual
            .as_ref()
            .map(|r| (r.total_fee_cents, r.total_settlement_minutes, r.hops.len())),
        oracle(net, p),
        "scenario={scenario}, payment={p:?}, rails={:?}",
        net.rails,
    );
    if let Some(route) = actual {
        verify_witness(net, p, &route);
    }
}

fn verify_witness(net: &Network, p: &Payment, route: &Route) {
    let mut at = p.sender.as_str();
    let mut visited = vec![at];
    let (mut fee, mut elapsed) = (0, 0);
    for hop in &route.hops {
        let rail = net.rails.iter().find(|r| r.id == hop.rail_id).unwrap();
        assert_eq!(hop.sender, at);
        assert!(rail.available);
        assert!(rail.participants.contains(&hop.sender));
        assert!(rail.participants.contains(&hop.receiver));
        assert!(p.amount_cents <= rail.max_amount_cents.unwrap_or(u64::MAX));
        assert!(!visited.contains(&hop.receiver.as_str()));
        at = &hop.receiver;
        visited.push(at);
        fee += u128::from(rail.fee_cents);
        elapsed += u128::from(rail.settlement_minutes);
    }
    assert_eq!(at, p.receiver);
    assert_eq!(
        (fee, elapsed),
        (route.total_fee_cents, route.total_settlement_minutes)
    );
    assert!(elapsed <= u128::from(p.max_delivery_minutes.unwrap_or(u64::MAX)));
}

#[test]
fn every_four_institution_topology_matches_the_bounded_walk_oracle() {
    // All 4^6 = 4096 assignments to the six possible two-member services:
    // absent, free/immediate, cheap/slow, expensive/fast. Four deadlines,
    // both endpoint directions: 32768 independent routing comparisons.
    for code in 0..4usize.pow(6) {
        let mut net = network();
        let mut digits = code;
        for (index, (from, to)) in PAIRS.iter().enumerate() {
            let state = digits % 4;
            digits /= 4;
            let (fee, minutes) = match state {
                0 => continue,
                1 => (0, 0),
                2 => (1, 3),
                _ => (4, 1),
            };
            net.rails.push(rail(
                index,
                vec![IDS[*from].into(), IDS[*to].into()],
                fee,
                minutes,
            ));
        }
        for deadline in [None, Some(0), Some(2), Some(4)] {
            let mut p = payment(100, deadline);
            verify(&net, &p, code);
            p.sender = "D".into();
            p.receiver = "A".into();
            verify(&net, &p, code);
        }
    }
}

#[test]
fn shared_parallel_and_restricted_rails_match_the_oracle() {
    // No randomness: exhaust 5^4 states of four overlapping shared services.
    // Includes parallel A-B rails, static closure, ceilings and zero-cost cycles.
    let member_sets = [
        vec!["A", "B", "C"],
        vec!["B", "C", "D"],
        vec!["A", "B"],
        vec!["A", "D"],
    ];
    for code in 0..5usize.pow(4) {
        let mut net = network();
        let mut digits = code;
        for (index, members) in member_sets.iter().enumerate() {
            let state = digits % 5;
            digits /= 5;
            let mut r = rail(index, members.iter().map(|id| (*id).into()).collect(), 0, 0);
            match state {
                0 => r.available = false,
                1 => r.max_amount_cents = Some(100),
                2 => {
                    r.fee_cents = 1;
                    r.settlement_minutes = 3;
                }
                3 => {
                    r.fee_cents = 4;
                    r.settlement_minutes = 1;
                }
                _ => {}
            }
            net.rails.push(r);
        }
        for amount in [100, 101] {
            for deadline in [None, Some(0), Some(2), Some(4)] {
                verify(&net, &payment(amount, deadline), code);
            }
        }
    }
}
