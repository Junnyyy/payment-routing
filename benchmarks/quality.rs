//! Adversarial and seeded quality fixtures, without calls into any optimizer.
use crate::fixtures::*;
use payment_routing::scheduling::*;

pub fn case(family: &str, scale: usize, seed: u64) -> StaticCase {
    if family == "schedule-trap" || family == "schedule-trap-blocked" {
        let mut c = static_case(
            if family.ends_with("blocked") {
                "batch-trap-infeasible-greedy"
            } else {
                "batch-trap"
            },
            2,
        );
        for r in &c.network.rails {
            c.slots.push(slot(r, 0, None));
        }
        return c;
    }
    if family == "schedule-dense" {
        let mut c = static_case("batch-density", scale);
        c.slots = c.network.rails.iter().map(|r| slot(r, 0, None)).collect();
        return c;
    }
    if family == "schedule-knapsack" {
        let mut c = static_case("batch-scarce", scale);
        c.network.rails[0].batch_capacity_cents = Some((scale - 1) as u64);
        c.payments[0].amount_cents = (scale - 1) as u64;
        c.timed[0].payment.amount_cents = (scale - 1) as u64;
        c.expected_fee = Some((scale + 4) as u128);
        c.slots = c.network.rails.iter().map(|r| slot(r, 0, None)).collect();
        return c;
    }
    assert_eq!(family, "schedule-random");
    let mut state = seed;
    let mut draw = || {
        state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    };
    let mut c = StaticCase {
        network: network(4),
        payments: vec![],
        timed: vec![],
        slots: vec![],
        expected_fee: None,
        expected_infeasible: false,
    };
    for i in 0..4 {
        for j in i + 1..4 {
            let mut r = rail(
                c.network.rails.len(),
                vec![format!("N{i:03}"), format!("N{j:03}")],
                draw() % 6,
                (draw() % 2) as u32,
            );
            r.batch_capacity_cents = Some(1 + draw() % 6);
            r.max_amount_cents = Some(1 + draw() % 3);
            for time in 0..3 {
                if !draw().is_multiple_of(4) {
                    let mut s = slot(&r, time, Some(1 + draw() % 4));
                    s.fee_cents = Some(draw() % 8);
                    s.settlement_minutes = Some((draw() % 3) as u32);
                    c.slots.push(s);
                }
            }
            c.network.rails.push(r);
        }
    }
    for i in 0..scale {
        let mut p = payment(i, 3);
        p.sender = format!("N{:03}", draw() % 3);
        p.amount_cents = 1 + draw() % 2;
        p.max_delivery_minutes = Some(1 + draw() % 4);
        c.timed.push(TimedPayment {
            payment: p.clone(),
            earliest_execution_minute: draw() % 2,
            deadline_minute: Some(2 + draw() % 3),
        });
        c.payments.push(p);
    }
    c
}
