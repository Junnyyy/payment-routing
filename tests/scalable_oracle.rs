#[path = "support/batch_oracle.rs"]
mod fixtures;
#[path = "support/scheduling_oracle.rs"]
mod oracle;
use fixtures::{network, payment, rail};
use oracle::*;
#[allow(dead_code)]
#[path = "../benchmarks/audit.rs"]
mod audit;
fn verify(
    net: &payment_routing::network::Network,
    p: &[payment_routing::scheduling::TimedPayment],
    slots: &[payment_routing::scheduling::RailDeparture],
) {
    let optimum = exhaustive(net, p, slots);
    let answer =
        payment_routing::scalable::plan_schedule(net, p, slots, Default::default()).unwrap();
    if let Some(plan) = answer.plan {
        audit::schedule(net, p, slots, &plan);
        assert!(plan.total_fee_cents >= optimum.unwrap().fee);
    }
    // Single-request search at generous limits must retain the fee optimum in
    // this small corpus. Independent bounded walks include cyclic witnesses.
    for payment in p {
        let singleton = std::slice::from_ref(payment);
        let expected = exhaustive(net, singleton, slots);
        let actual =
            payment_routing::scalable::plan_schedule(net, singleton, slots, Default::default())
                .unwrap();
        assert_eq!(actual.diagnostics.truncated_searches, 0);
        assert_eq!(
            actual.plan.as_ref().map(|p| p.total_fee_cents),
            expected.map(|p| p.fee)
        );
        if let Some(plan) = actual.plan {
            audit::schedule(net, singleton, slots, &plan);
        }
    }
}

#[test]
fn bounded_triangle_witnesses_and_single_request_optima() {
    // 3^6 timetables x 4 arrival/deadline patterns = 2,916 comparisons.
    let mut count = 0;
    for code in 0..3usize.pow(6) {
        let net = network(
            &["A", "B", "C"],
            vec![
                rail("ab", &["A", "B"], 0, 0, Some(3)),
                rail("bc", &["B", "C"], 0, 0, None),
                rail("ac", &["A", "C"], 2, 1, Some(2)),
            ],
        );
        let mut digits = code;
        let mut slots = vec![];
        for r in &net.rails {
            for at in 0..2 {
                let state = digits % 3;
                digits /= 3;
                match state {
                    0 => {}
                    1 => slots.push(slot(&r.id, at, Some(0), Some(0), Some(1))),
                    _ => slots.push(slot(&r.id, at, Some(2), Some(1), Some(2))),
                }
            }
        }
        for timing in 0..4 {
            let mut p = vec![
                timed(payment("P1", "A", "C", 1), 0, None),
                timed(payment("P2", "C", "A", 1), 0, None),
            ];
            match timing {
                0 => {}
                1 => p[1].deadline_minute = Some(0),
                2 => {
                    p[0].earliest_execution_minute = 1;
                    p[1].payment.amount_cents = 2;
                }
                _ => {
                    p[0].deadline_minute = Some(1);
                    p[1].payment.max_delivery_minutes = Some(1);
                }
            }
            verify(&net, &p, &slots);
            count += 1;
        }
    }
    assert_eq!(count, 2916);
}

#[test]
fn bounded_overlapping_witnesses_and_single_request_optima() {
    // 4^4 service states x 4 constraint patterns = 1,024 comparisons. The fourth
    // institution allows oracle cycles, including repeated same-minute slots.
    let mut count = 0;
    for code in 0..4usize.pow(4) {
        let mut net = network(
            &["A", "B", "C", "D"],
            vec![
                rail("shared-abc", &["A", "B", "C"], 0, 0, None),
                rail("shared-bcd", &["B", "C", "D"], 0, 0, Some(3)),
                rail("parallel-ab", &["A", "B"], 1, 0, Some(2)),
                rail("direct-ad", &["A", "D"], 3, 0, None),
            ],
        );
        let mut digits = code;
        let mut slots = vec![];
        for r in &net.rails {
            let state = digits % 4;
            digits /= 4;
            match state {
                0 => slots.push(slot(&r.id, 0, None, None, Some(0))),
                1 => slots.push(slot(&r.id, 0, None, None, Some(2))),
                2 => slots.push(slot(&r.id, 1, None, None, Some(2))),
                _ => {
                    slots.push(slot(&r.id, 0, Some(0), Some(2), Some(1)));
                    slots.push(slot(&r.id, 1, Some(1), Some(0), None));
                }
            }
        }
        for pattern in 0..4 {
            net.rails[0].available = pattern != 3;
            net.rails[1].max_amount_cents = if pattern == 2 { Some(1) } else { None };
            let mut p = vec![
                timed(payment("P2", "D", "A", 1), 0, Some(2)),
                timed(payment("P1", "A", "C", 1), 0, Some(2)),
            ];
            match pattern {
                0 => {}
                1 => p[0].deadline_minute = Some(0),
                2 => {
                    p[0].earliest_execution_minute = 1;
                    p[0].payment.amount_cents = 2;
                }
                _ => p[1].payment.max_delivery_minutes = Some(0),
            }
            verify(&net, &p, &slots);
            count += 1;
        }
    }
    assert_eq!(count, 1024);
}
