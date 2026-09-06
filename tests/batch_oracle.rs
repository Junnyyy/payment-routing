#[path = "support/batch_oracle.rs"]
mod oracle;
use oracle::*;
use payment_routing::{
    batch::optimize_batch,
    network::{Network, Payment},
};

fn verify(net: &Network, payments: &[Payment], label: usize) {
    let before = net.clone();
    let before_payments = payments.to_vec();
    let actual = optimize_batch(net, payments).unwrap();
    let expected = exhaustive(net, payments);
    let actual_rank = actual.as_ref().map(|plan| Answer {
        fee: plan.total_fee_cents,
        minutes: plan.total_settlement_minutes,
        hops: plan.assignments.iter().map(|a| a.route.hops.len()).sum(),
        paths: plan
            .assignments
            .iter()
            .map(|a| {
                a.route
                    .hops
                    .iter()
                    .map(|h| (h.rail_id.clone(), h.sender.clone(), h.receiver.clone()))
                    .collect()
            })
            .collect(),
    });
    assert_eq!(
        actual_rank, expected,
        "case={label}, payments={payments:?}, rails={:?}",
        net.rails
    );
    assert_eq!(net, &before);
    assert_eq!(payments, before_payments);
    if let Some(plan) = actual {
        let mut ordered: Vec<_> = payments.iter().collect();
        ordered.sort_by_key(|p| &p.id);
        assert_eq!(plan.assignments.len(), payments.len());
        let mut fees = 0;
        let mut minutes = 0;
        let mut usage = vec![0u128; net.rails.len()];
        for (p, assignment) in ordered.iter().zip(&plan.assignments) {
            assert_eq!(assignment.payment_id, p.id);
            let mut at = p.sender.as_str();
            let mut visited = vec![at];
            let (mut fee, mut elapsed) = (0, 0);
            for hop in &assignment.route.hops {
                assert_eq!(hop.sender, at);
                let (index, rail) = net
                    .rails
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.id == hop.rail_id)
                    .unwrap();
                assert!(rail.available);
                assert!(rail.participants.contains(&hop.sender));
                assert!(rail.participants.contains(&hop.receiver));
                assert!(rail.max_amount_cents.is_none_or(|c| p.amount_cents <= c));
                assert!(!visited.contains(&hop.receiver.as_str()));
                at = &hop.receiver;
                visited.push(at);
                usage[index] += u128::from(p.amount_cents);
                fee += u128::from(rail.fee_cents);
                elapsed += u128::from(rail.settlement_minutes);
            }
            assert_eq!(at, p.receiver);
            assert!(
                p.max_delivery_minutes
                    .is_none_or(|d| elapsed <= u128::from(d))
            );
            assert_eq!(
                (fee, elapsed),
                (
                    assignment.route.total_fee_cents,
                    assignment.route.total_settlement_minutes
                )
            );
            fees += fee;
            minutes += elapsed;
        }
        assert_eq!(
            (fees, minutes),
            (plan.total_fee_cents, plan.total_settlement_minutes)
        );
        let mut reported: Vec<_> = net
            .rails
            .iter()
            .zip(usage)
            .map(|(rail, amount)| {
                assert!(
                    rail.batch_capacity_cents
                        .is_none_or(|c| amount <= u128::from(c))
                );
                (rail.id.as_str(), amount)
            })
            .collect();
        reported.sort();
        assert_eq!(
            plan.rail_usage
                .iter()
                .map(|r| (r.rail_id.as_str(), r.principal_cents))
                .collect::<Vec<_>>(),
            reported
        );
    }
}

#[test]
fn all_triangle_capacity_states_match_complete_assignment_enumeration() {
    // 6^3 rail configurations x 3 batches x 4 deadlines = 2592 comparisons.
    for code in 0..6usize.pow(3) {
        let mut digits = code;
        let mut rails = vec![];
        for (index, (from, to)) in [("A", "B"), ("A", "C"), ("B", "C")].iter().enumerate() {
            let state = digits % 6;
            digits /= 6;
            let mut r = rail(&format!("R{index}"), &[from, to], 0, 0, Some(1));
            match state {
                0 => r.available = false,
                1 => r.batch_capacity_cents = Some(0),
                2 => {}
                3 => {
                    r.batch_capacity_cents = Some(2);
                    r.fee_cents = 1;
                    r.settlement_minutes = 2;
                }
                4 => {
                    r.batch_capacity_cents = None;
                    r.fee_cents = 3;
                }
                _ => {
                    r.batch_capacity_cents = Some(2);
                    r.max_amount_cents = Some(1);
                }
            }
            rails.push(r);
        }
        let net = network(&["A", "B", "C"], rails);
        for mut payments in [
            vec![payment("P1", "A", "C", 1), payment("P2", "C", "A", 1)],
            vec![payment("P1", "A", "C", 1), payment("P2", "A", "C", 2)],
            vec![
                payment("P1", "A", "B", 1),
                payment("P2", "B", "C", 1),
                payment("P3", "C", "A", 1),
            ],
        ] {
            for deadline in [None, Some(0), Some(2), Some(3)] {
                for p in &mut payments {
                    p.max_delivery_minutes = deadline;
                }
                verify(&net, &payments, code);
            }
        }
    }
}

#[test]
fn overlapping_shared_rails_and_bounded_cycles_match_exhaustive_oracle() {
    // 5^4 overlapping service configurations x 2 amounts x 3 deadlines = 3750.
    // Four institutions permit bounded oracle walks with cycles; production
    // excludes those. Parallel rails and distinct member pairs share capacity.
    for code in 0..5usize.pow(4) {
        let mut digits = code;
        let mut rails = vec![];
        for (index, ids) in [
            &["A", "B", "C"][..],
            &["B", "C", "D"],
            &["A", "B"],
            &["A", "D"],
        ]
        .iter()
        .enumerate()
        {
            let state = digits % 5;
            digits /= 5;
            let mut r = rail(&format!("R{index}"), ids, 0, 0, Some(1));
            match state {
                0 => r.batch_capacity_cents = Some(0),
                1 => {}
                2 => {
                    r.batch_capacity_cents = Some(2);
                    r.fee_cents = 1;
                    r.settlement_minutes = 2;
                }
                3 => {
                    r.batch_capacity_cents = None;
                    r.fee_cents = 3;
                }
                _ => {
                    r.batch_capacity_cents = Some(2);
                    r.max_amount_cents = Some(1);
                }
            }
            rails.push(r);
        }
        let net = network(&["A", "B", "C", "D"], rails);
        for amount in [1, 2] {
            for deadline in [None, Some(0), Some(3)] {
                let mut payments =
                    vec![payment("P2", "D", "A", amount), payment("P1", "A", "C", 1)];
                payments[0].max_delivery_minutes = deadline;
                verify(&net, &payments, code);
            }
        }
    }
}
