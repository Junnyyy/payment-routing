//! Independent test oracle: expand bounded walks by length (including cycles),
//! form the complete Cartesian product, then rescan whole assignments for budgets.
//! No production validation, search, score or pruning helpers are used.
#![allow(dead_code)]

use payment_routing::{
    network::{Network, Payment},
    scheduling::{RailDeparture, ScheduledBatchPlan, TimedPayment, optimize_schedule},
};

pub type Hop = (String, String, String, u64, u64, u64);

#[derive(Clone, Debug)]
pub struct Walk {
    pub hops: Vec<Hop>,
    pub fee: u128,
    pub elapsed: u128,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Answer {
    pub fee: u128,
    pub elapsed: u128,
    pub hops: usize,
    pub paths: Vec<Vec<Hop>>,
}

pub fn timed(payment: Payment, release: u64, deadline: Option<u64>) -> TimedPayment {
    TimedPayment {
        payment,
        earliest_execution_minute: release,
        deadline_minute: deadline,
    }
}

pub fn slot(
    rail: &str,
    at: u64,
    fee: Option<u64>,
    latency: Option<u32>,
    capacity: Option<u64>,
) -> RailDeparture {
    RailDeparture {
        rail_id: rail.into(),
        departure_minute: at,
        fee_cents: fee,
        settlement_minutes: latency,
        capacity_cents: capacity,
    }
}

pub fn walks(net: &Network, p: &TimedPayment, slots: &[RailDeparture]) -> Vec<Walk> {
    let mut frontier = vec![Walk {
        hops: vec![],
        fee: 0,
        elapsed: 0,
    }];
    let mut answers = vec![];
    // Waiting can replace any cycle while retaining downstream slots. n-1
    // contains an optimum, but unlike production these walks can revisit nodes,
    // even leaving and re-entering the receiver. Deadlines are checked only when
    // collecting complete witnesses, and all capacities only on full batches.
    for _ in 1..net.institutions.len() {
        let mut next = vec![];
        for path in frontier {
            let (at, ready) = path
                .hops
                .last()
                .map(|h| (h.2.as_str(), h.4))
                .unwrap_or((&p.payment.sender, p.earliest_execution_minute));
            for slot in slots {
                let r = net.rails.iter().find(|r| r.id == slot.rail_id).unwrap();
                if slot.departure_minute < ready
                    || !r.available
                    || r.max_amount_cents
                        .is_some_and(|c| p.payment.amount_cents > c)
                    || !r.participants.iter().any(|id| id == at)
                {
                    continue;
                }
                let arrive = u64::try_from(
                    u128::from(slot.departure_minute)
                        + u128::from(slot.settlement_minutes.unwrap_or(r.settlement_minutes)),
                )
                .unwrap();
                for to in &r.participants {
                    if to == at {
                        continue;
                    }
                    let mut walk = path.clone();
                    let fee = slot.fee_cents.unwrap_or(r.fee_cents);
                    walk.hops.push((
                        r.id.clone(),
                        at.into(),
                        to.clone(),
                        slot.departure_minute,
                        arrive,
                        fee,
                    ));
                    walk.fee += u128::from(fee);
                    walk.elapsed = u128::from(arrive) - u128::from(p.earliest_execution_minute);
                    if to == &p.payment.receiver
                        && p.deadline_minute.is_none_or(|d| arrive <= d)
                        && p.payment
                            .max_delivery_minutes
                            .is_none_or(|d| walk.elapsed <= u128::from(d))
                    {
                        answers.push(walk.clone());
                    }
                    next.push(walk);
                }
            }
        }
        frontier = next;
    }
    answers
}

pub fn fits(
    net: &Network,
    payments: &[&TimedPayment],
    slots: &[RailDeparture],
    paths: &[Walk],
) -> bool {
    let principal = |rail: &str, at: Option<u64>| -> u128 {
        payments
            .iter()
            .zip(paths)
            .map(|(p, path)| {
                path.hops
                    .iter()
                    .filter(|h| h.0 == rail && at.is_none_or(|t| h.3 == t))
                    .count() as u128
                    * u128::from(p.payment.amount_cents)
            })
            .sum()
    };
    net.rails.iter().all(|r| {
        r.batch_capacity_cents
            .is_none_or(|c| principal(&r.id, None) <= u128::from(c))
    }) && slots.iter().all(|s| {
        s.capacity_cents
            .is_none_or(|c| principal(&s.rail_id, Some(s.departure_minute)) <= u128::from(c))
    })
}

pub fn rank(paths: &[Walk]) -> Answer {
    Answer {
        fee: paths.iter().map(|p| p.fee).sum(),
        elapsed: paths.iter().map(|p| p.elapsed).sum(),
        hops: paths.iter().map(|p| p.hops.len()).sum(),
        paths: paths.iter().map(|p| p.hops.clone()).collect(),
    }
}

pub fn product(
    net: &Network,
    payments: &[&TimedPayment],
    slots: &[RailDeparture],
) -> Vec<Vec<Walk>> {
    let mut product = vec![vec![]];
    for p in payments {
        let mut next = vec![];
        for path in walks(net, p, slots) {
            for prefix in &product {
                let mut assignment = prefix.clone();
                assignment.push(path.clone());
                next.push(assignment);
            }
        }
        product = next;
    }
    product
}

pub fn exhaustive(
    net: &Network,
    payments: &[TimedPayment],
    slots: &[RailDeparture],
) -> Option<Answer> {
    let mut ordered: Vec<_> = payments.iter().collect();
    ordered.sort_by_key(|p| &p.payment.id);
    product(net, &ordered, slots)
        .iter()
        .filter(|p| fits(net, &ordered, slots, p))
        .map(|p| rank(p))
        .min()
}

pub fn greedy(net: &Network, payments: &[TimedPayment], slots: &[RailDeparture]) -> Option<Answer> {
    let mut processed = vec![];
    let mut paths = vec![];
    for payment in payments {
        processed.push(payment);
        let mut choices = walks(net, payment, slots);
        choices.sort_by(|a, b| {
            (a.fee, a.elapsed, a.hops.len(), &a.hops).cmp(&(
                b.fee,
                b.elapsed,
                b.hops.len(),
                &b.hops,
            ))
        });
        let chosen = choices.into_iter().find(|c| {
            let mut trial = paths.clone();
            trial.push(c.clone());
            fits(net, &processed, slots, &trial)
        })?;
        paths.push(chosen);
    }
    Some(rank(&paths))
}

pub fn verify(
    net: &Network,
    payments: &[TimedPayment],
    slots: &[RailDeparture],
) -> Option<ScheduledBatchPlan> {
    let before = (net.clone(), payments.to_vec(), slots.to_vec());
    let plan = optimize_schedule(net, payments, slots).unwrap();
    let expected = exhaustive(net, payments, slots);
    let actual = plan.as_ref().map(|plan| {
        let mut ordered: Vec<_> = payments.iter().collect();
        ordered.sort_by_key(|p| &p.payment.id);
        assert_eq!(plan.assignments.len(), payments.len());
        let mut paths = vec![];
        for (p, a) in ordered.iter().zip(&plan.assignments) {
            assert_eq!(a.payment_id, p.payment.id);
            let mut at = p.payment.sender.as_str();
            let mut ready = p.earliest_execution_minute;
            let mut walk = Walk {
                hops: vec![],
                fee: 0,
                elapsed: 0,
            };
            for hop in &a.route.hops {
                let h = &hop.transfer;
                let r = net.rails.iter().find(|r| r.id == h.rail_id).unwrap();
                let slot = slots
                    .iter()
                    .find(|s| s.rail_id == h.rail_id && s.departure_minute == hop.departure_minute)
                    .unwrap();
                assert_eq!(h.sender, at);
                assert_ne!(h.sender, h.receiver);
                assert!(r.participants.contains(&h.sender) && r.participants.contains(&h.receiver));
                assert!(
                    r.available
                        && r.max_amount_cents
                            .is_none_or(|c| p.payment.amount_cents <= c)
                );
                assert!(hop.departure_minute >= ready);
                let arrival = u128::from(hop.departure_minute)
                    + u128::from(slot.settlement_minutes.unwrap_or(r.settlement_minutes));
                assert_eq!(u128::from(hop.arrival_minute), arrival);
                let fee = slot.fee_cents.unwrap_or(r.fee_cents);
                assert_eq!(hop.fee_cents, fee);
                walk.hops.push((
                    h.rail_id.clone(),
                    h.sender.clone(),
                    h.receiver.clone(),
                    hop.departure_minute,
                    hop.arrival_minute,
                    fee,
                ));
                walk.fee += u128::from(fee);
                ready = hop.arrival_minute;
                at = &h.receiver;
            }
            assert_eq!(at, p.payment.receiver);
            assert!(p.deadline_minute.is_none_or(|d| ready <= d));
            walk.elapsed = u128::from(ready) - u128::from(p.earliest_execution_minute);
            assert!(
                p.payment
                    .max_delivery_minutes
                    .is_none_or(|d| walk.elapsed <= u128::from(d))
            );
            assert_eq!(
                (a.route.total_fee_cents, u128::from(a.route.elapsed_minutes)),
                (walk.fee, walk.elapsed)
            );
            paths.push(walk);
        }
        assert!(fits(net, &ordered, slots, &paths));
        let principal = |rail: &str, at: Option<u64>| -> u128 {
            ordered
                .iter()
                .zip(&paths)
                .map(|(p, path)| {
                    path.hops
                        .iter()
                        .filter(|h| h.0 == rail && at.is_none_or(|t| h.3 == t))
                        .count() as u128
                        * u128::from(p.payment.amount_cents)
                })
                .sum()
        };
        let mut usage: Vec<_> = net
            .rails
            .iter()
            .map(|r| (r.id.clone(), principal(&r.id, None)))
            .collect();
        usage.sort();
        assert_eq!(
            plan.rail_usage
                .iter()
                .map(|u| (u.rail_id.clone(), u.principal_cents))
                .collect::<Vec<_>>(),
            usage
        );
        let mut usage: Vec<_> = slots
            .iter()
            .map(|s| {
                (
                    s.rail_id.clone(),
                    s.departure_minute,
                    principal(&s.rail_id, Some(s.departure_minute)),
                )
            })
            .collect();
        usage.sort();
        assert_eq!(
            plan.departure_usage
                .iter()
                .map(|u| (u.rail_id.clone(), u.departure_minute, u.principal_cents))
                .collect::<Vec<_>>(),
            usage
        );
        let answer = rank(&paths);
        assert_eq!(
            (plan.total_fee_cents, plan.total_elapsed_minutes),
            (answer.fee, answer.elapsed)
        );
        answer
    });
    assert_eq!(
        actual, expected,
        "payments={payments:?}; slots={slots:?}; rails={:?}",
        net.rails
    );
    assert_eq!(
        (&before.0, before.1.as_slice(), before.2.as_slice()),
        (net, payments, slots)
    );
    plan
}
