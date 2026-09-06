//! Witness checks independent of search/ranking/pruning helpers.
use payment_routing::{batch::BatchPlan, network::*, routing::Route, scheduling::*};
use std::collections::{BTreeMap, BTreeSet};

pub fn route(net: &Network, p: &Payment, r: &Route) -> (u128, u128, usize) {
    let mut at = p.sender.as_str();
    let mut seen = BTreeSet::from([at]);
    let mut fee = 0;
    let mut latency = 0;
    for h in &r.hops {
        assert_eq!(h.sender, at);
        assert!(seen.insert(&h.receiver));
        let rail = net.rails.iter().find(|r| r.id == h.rail_id).unwrap();
        assert!(
            rail.available
                && rail.participants.contains(&h.sender)
                && rail.participants.contains(&h.receiver)
        );
        assert!(rail.max_amount_cents.is_none_or(|c| p.amount_cents <= c));
        fee += u128::from(rail.fee_cents);
        latency += u128::from(rail.settlement_minutes);
        at = &h.receiver;
    }
    assert_eq!(at, p.receiver);
    assert!(
        p.max_delivery_minutes
            .is_none_or(|d| latency <= u128::from(d))
    );
    assert_eq!(
        (fee, latency),
        (r.total_fee_cents, r.total_settlement_minutes)
    );
    (fee, latency, r.hops.len())
}

pub fn batch(net: &Network, payments: &[Payment], plan: &BatchPlan) {
    assert_eq!(plan.assignments.len(), payments.len());
    let mut ids = BTreeSet::new();
    let mut used = BTreeMap::<&str, u128>::new();
    let mut fee = 0;
    let mut minutes = 0;
    for a in &plan.assignments {
        assert!(ids.insert(&a.payment_id));
        let p = payments.iter().find(|p| p.id == a.payment_id).unwrap();
        let (f, t, _) = route(net, p, &a.route);
        fee += f;
        minutes += t;
        for h in &a.route.hops {
            *used.entry(&h.rail_id).or_default() += u128::from(p.amount_cents);
        }
    }
    assert_eq!(
        (fee, minutes),
        (plan.total_fee_cents, plan.total_settlement_minutes)
    );
    assert_eq!(plan.rail_usage.len(), net.rails.len());
    for r in &net.rails {
        let u = used.get(r.id.as_str()).copied().unwrap_or(0);
        assert!(r.batch_capacity_cents.is_none_or(|c| u <= u128::from(c)));
        assert_eq!(
            plan.rail_usage
                .iter()
                .find(|v| v.rail_id == r.id)
                .unwrap()
                .principal_cents,
            u
        );
    }
}

pub fn schedule(
    net: &Network,
    payments: &[TimedPayment],
    slots: &[RailDeparture],
    plan: &ScheduledBatchPlan,
) {
    assert_eq!(plan.assignments.len(), payments.len());
    let mut ids = BTreeSet::new();
    let mut used = BTreeMap::<(&str, u64), u128>::new();
    let mut fee = 0;
    let mut elapsed = 0;
    for a in &plan.assignments {
        assert!(ids.insert(&a.payment_id));
        let p = payments
            .iter()
            .find(|p| p.payment.id == a.payment_id)
            .unwrap();
        let mut at = p.payment.sender.as_str();
        let mut ready = p.earliest_execution_minute;
        let mut seen = BTreeSet::from([at]);
        let mut route_fee = 0;
        for h in &a.route.hops {
            let t = &h.transfer;
            assert_eq!(at, t.sender);
            assert!(seen.insert(&t.receiver));
            let r = net.rails.iter().find(|r| r.id == t.rail_id).unwrap();
            let s = slots
                .iter()
                .find(|s| s.rail_id == r.id && s.departure_minute == h.departure_minute)
                .unwrap();
            assert!(
                r.available
                    && r.participants.contains(&t.sender)
                    && r.participants.contains(&t.receiver)
            );
            assert!(
                r.max_amount_cents
                    .is_none_or(|c| p.payment.amount_cents <= c)
            );
            assert!(h.departure_minute >= ready);
            assert_eq!(
                h.arrival_minute,
                h.departure_minute
                    + u64::from(s.settlement_minutes.unwrap_or(r.settlement_minutes))
            );
            assert_eq!(h.fee_cents, s.fee_cents.unwrap_or(r.fee_cents));
            *used.entry((&r.id, h.departure_minute)).or_default() +=
                u128::from(p.payment.amount_cents);
            route_fee += u128::from(h.fee_cents);
            ready = h.arrival_minute;
            at = &t.receiver;
        }
        assert_eq!(at, p.payment.receiver);
        assert!(p.deadline_minute.is_none_or(|d| ready <= d));
        let duration = ready - p.earliest_execution_minute;
        assert!(p.payment.max_delivery_minutes.is_none_or(|d| duration <= d));
        assert_eq!(
            (route_fee, duration),
            (a.route.total_fee_cents, a.route.elapsed_minutes)
        );
        fee += route_fee;
        elapsed += u128::from(duration);
    }
    assert_eq!(
        (fee, elapsed),
        (plan.total_fee_cents, plan.total_elapsed_minutes)
    );
    assert_eq!(plan.departure_usage.len(), slots.len());
    assert_eq!(plan.rail_usage.len(), net.rails.len());
    for s in slots {
        let u = used
            .get(&(s.rail_id.as_str(), s.departure_minute))
            .copied()
            .unwrap_or(0);
        assert!(s.capacity_cents.is_none_or(|c| u <= u128::from(c)));
        assert_eq!(
            plan.departure_usage
                .iter()
                .find(|v| v.rail_id == s.rail_id && v.departure_minute == s.departure_minute)
                .unwrap()
                .principal_cents,
            u
        );
    }
    for r in &net.rails {
        let u: u128 = used
            .iter()
            .filter(|((id, _), _)| *id == r.id)
            .map(|(_, v)| v)
            .sum();
        assert!(r.batch_capacity_cents.is_none_or(|c| u <= u128::from(c)));
        assert_eq!(
            plan.rail_usage
                .iter()
                .find(|v| v.rail_id == r.id)
                .unwrap()
                .principal_cents,
            u
        );
    }
}

/// A cheap feasible witness for the suite's one-rail, unit-payment windows.
/// Failure returns no certificate; it never declares general infeasibility.
/// Each successful payment must pay the only rail's fee, so a full witness also
/// certifies the minimum fee. This does not certify elapsed or lexical optimality.
pub fn window_reference(
    net: &Network,
    payments: &[TimedPayment],
    slots: &[RailDeparture],
) -> Option<ScheduledBatchPlan> {
    assert_eq!(net.institutions.len(), 2);
    assert_eq!(net.rails.len(), 1);
    assert!(payments.iter().all(|p| p.payment.amount_cents == 1));
    let r = &net.rails[0];
    let mut ordered: Vec<_> = payments.iter().collect();
    ordered.sort_by_key(|p| {
        (
            p.earliest_execution_minute,
            p.deadline_minute,
            &p.payment.id,
        )
    });
    let mut used = vec![0_u128; slots.len()];
    let mut assignments = vec![];
    for p in ordered {
        let (i, s) = slots
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                s.departure_minute >= p.earliest_execution_minute
                    && s.capacity_cents.is_none_or(|c| used[*i] < u128::from(c))
            })
            .min_by_key(|(_, s)| s.departure_minute)?;
        let arrival =
            s.departure_minute + u64::from(s.settlement_minutes.unwrap_or(r.settlement_minutes));
        if p.deadline_minute.is_some_and(|d| arrival > d)
            || p.payment
                .max_delivery_minutes
                .is_some_and(|d| arrival - p.earliest_execution_minute > d)
        {
            return None;
        }
        used[i] += 1;
        let fee = s.fee_cents.unwrap_or(r.fee_cents);
        assignments.push(ScheduledPaymentRoute {
            payment_id: p.payment.id.clone(),
            route: ScheduledRoute {
                hops: vec![ScheduledHop {
                    transfer: payment_routing::routing::RouteHop {
                        rail_id: r.id.clone(),
                        sender: p.payment.sender.clone(),
                        receiver: p.payment.receiver.clone(),
                    },
                    departure_minute: s.departure_minute,
                    arrival_minute: arrival,
                    fee_cents: fee,
                }],
                total_fee_cents: u128::from(fee),
                elapsed_minutes: arrival - p.earliest_execution_minute,
            },
        });
    }
    assignments.sort_by(|a, b| a.payment_id.cmp(&b.payment_id));
    let total = used.iter().sum();
    if r.batch_capacity_cents
        .is_some_and(|c| total > u128::from(c))
    {
        return None;
    }
    let plan = ScheduledBatchPlan {
        total_fee_cents: assignments.iter().map(|a| a.route.total_fee_cents).sum(),
        total_elapsed_minutes: assignments
            .iter()
            .map(|a| u128::from(a.route.elapsed_minutes))
            .sum(),
        assignments,
        rail_usage: vec![payment_routing::batch::RailUsage {
            rail_id: r.id.clone(),
            principal_cents: total,
        }],
        departure_usage: slots
            .iter()
            .zip(used)
            .map(|(s, principal_cents)| DepartureUsage {
                rail_id: s.rail_id.clone(),
                departure_minute: s.departure_minute,
                principal_cents,
            })
            .collect(),
    };
    schedule(net, payments, slots, &plan);
    Some(plan)
}
