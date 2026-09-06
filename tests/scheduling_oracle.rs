#[path = "support/batch_oracle.rs"]
mod fixtures;
#[path = "support/scheduling_oracle.rs"]
mod oracle;
use fixtures::{network, payment, rail};
use oracle::*;

#[test]
fn all_triangle_slot_states_match_independent_complete_assignments() {
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
fn overlapping_rails_parallel_paths_and_cyclic_walks_match_full_rank() {
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
