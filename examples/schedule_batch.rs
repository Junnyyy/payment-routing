//! Hand-enumerable synthetic time/routing counterexamples. No terminal or clock.
use payment_routing::{
    network::{Institution, Network, Payment, Rail, ValidationError},
    scheduling::{RailDeparture, TimedPayment, optimize_schedule},
};

fn rail(id: &str, members: &[&str], fee: u64, latency: u32) -> Rail {
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

fn network(members: &[&str], rails: Vec<Rail>) -> Network {
    Network {
        name: "Synthetic scheduled USD batch".into(),
        institutions: members
            .iter()
            .map(|id| Institution {
                id: (*id).into(),
                name: (*id).into(),
                opening_balance_cents: 0,
            })
            .collect(),
        rails,
        payments: vec![],
    }
}

fn payment(id: &str, from: &str, to: &str, release: u64, deadline: u64) -> TimedPayment {
    TimedPayment {
        payment: Payment {
            id: id.into(),
            sender: from.into(),
            receiver: to.into(),
            amount_cents: 1,
            max_delivery_minutes: None,
        },
        earliest_execution_minute: release,
        deadline_minute: Some(deadline),
    }
}

fn slot(rail: &str, at: u64, fee: Option<u64>, capacity: Option<u64>) -> RailDeparture {
    RailDeparture {
        rail_id: rail.into(),
        departure_minute: at,
        fee_cents: fee,
        settlement_minutes: None,
        capacity_cents: capacity,
    }
}

fn main() -> Result<(), ValidationError> {
    println!("Synthetic integer minutes and USD cents. Planning only; no funds move.");
    let mut net = network(
        &["A", "B"],
        vec![
            rail("cheap", &["A", "B"], 1, 0),
            rail("fallback", &["A", "B"], 10, 0),
        ],
    );
    let p = vec![payment("P1", "A", "B", 0, 1), payment("P2", "B", "A", 0, 0)];
    let mut slots = vec![
        slot("cheap", 0, None, Some(1)),
        slot("cheap", 1, None, Some(1)),
        slot("fallback", 0, None, None),
        slot("fallback", 1, None, None),
    ];
    println!("\nDelay helps: P1 deadline=1, P2 deadline=0; both arrive at 0.");
    println!("Cheap has one principal cent at each of minutes 0 and 1.");
    enumerate_direct(&net, &p, &slots, 8, 2);
    println!("Greedy P1-first takes cheap@0 and forces fallback@0 for P2: 11 cents.");
    show(&net, &p, &slots, Some(2))?;
    net.rails.pop();
    slots.retain(|s| s.rail_id == "cheap");
    println!("\nRemove fallback: greedy gets stuck, but delaying P1 still works.");
    show(&net, &p, &slots, Some(2))?;
    net.rails[0].batch_capacity_cents = Some(1);
    println!("\nWhole-batch capacity=1: separate departure budgets do not replenish it.");
    show(&net, &p, &slots, None)?;

    let net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 0)]);
    let p = vec![payment("P1", "A", "B", 0, 1), payment("P2", "A", "B", 1, 2)];
    let slots = vec![
        slot("r", 0, Some(3), Some(1)),
        slot("r", 1, Some(1), Some(1)),
        slot("r", 2, Some(10), Some(1)),
    ];
    println!("\nLocally cheapest scheduling loses: P2 arrives at minute 1.");
    enumerate_direct(&net, &p, &slots, 4, 4);
    println!("Greedy waits until minute 1 for P1, forcing P2 to minute 2: 11 cents.");
    show(&net, &p, &slots, Some(4))?;

    let net = network(
        &["A", "B", "C"],
        vec![rail("ab", &["A", "B"], 4, 1), rail("bc", &["B", "C"], 1, 1)],
    );
    let mut p = vec![payment("P", "A", "C", 0, 4)];
    let mut slots = vec![
        slot("ab", 0, None, None),
        slot("ab", 2, Some(1), None),
        slot("bc", 1, None, None),
    ];
    println!("\nWaiting misses a connection: AB@0 arrives 1; cheap AB@2 arrives 3.");
    println!("BC only departs at 1 and arrives 2. Exactly the early prefix works.");
    show(&net, &p, &slots, Some(5))?;
    p[0].earliest_execution_minute = 2;
    println!("Release at minute 2: the whole route becomes infeasible.");
    show(&net, &p, &slots, None)?;
    slots[2].departure_minute = 3;
    println!("Move BC to minute 3: delayed cheap AB now connects, arriving at deadline 4.");
    show(&net, &p, &slots, Some(2))?;
    Ok(())
}

// Independent enumeration restricted to the two-node zero-latency fixtures above.
// Every route is one departure; print every deadline-eligible joint assignment.
fn enumerate_direct(
    net: &Network,
    p: &[TimedPayment],
    slots: &[RailDeparture],
    expected_count: usize,
    expected_fee: u128,
) {
    assert_eq!(net.institutions.len(), 2);
    assert_eq!(p.len(), 2);
    assert!(
        net.rails
            .iter()
            .all(|r| r.settlement_minutes == 0 && r.available)
    );
    let eligible = |p: &TimedPayment, s: &RailDeparture| {
        s.departure_minute >= p.earliest_execution_minute
            && p.deadline_minute.is_none_or(|d| s.departure_minute <= d)
    };
    let mut count = 0;
    let mut minimum = None;
    for first in slots.iter().filter(|s| eligible(&p[0], s)) {
        for second in slots.iter().filter(|s| eligible(&p[1], s)) {
            count += 1;
            let fee: u128 = [first, second]
                .iter()
                .map(|s| {
                    u128::from(
                        s.fee_cents.unwrap_or(
                            net.rails
                                .iter()
                                .find(|r| r.id == s.rail_id)
                                .unwrap()
                                .fee_cents,
                        ),
                    )
                })
                .sum();
            let fits = slots.iter().all(|s| {
                let used: u128 = [first, second]
                    .iter()
                    .zip(p)
                    .filter(|(chosen, _)| {
                        chosen.rail_id == s.rail_id && chosen.departure_minute == s.departure_minute
                    })
                    .map(|(_, p)| u128::from(p.payment.amount_cents))
                    .sum();
                s.capacity_cents.is_none_or(|c| used <= u128::from(c))
            });
            println!(
                "  {}@{} + {}@{}: {fee:2} cents | {}",
                first.rail_id,
                first.departure_minute,
                second.rail_id,
                second.departure_minute,
                if fits {
                    "feasible"
                } else {
                    "slot capacity exceeded"
                }
            );
            if fits {
                minimum = Some(minimum.map_or(fee, |m: u128| m.min(fee)));
            }
        }
    }
    assert_eq!(count, expected_count);
    assert_eq!(minimum, Some(expected_fee));
}

fn show(
    net: &Network,
    p: &[TimedPayment],
    slots: &[RailDeparture],
    expected: Option<u128>,
) -> Result<(), ValidationError> {
    let before = (net.clone(), p.to_vec(), slots.to_vec());
    let plan = optimize_schedule(net, p, slots)?;
    assert_eq!(plan.as_ref().map(|p| p.total_fee_cents), expected);
    assert_eq!(
        (&before.0, before.1.as_slice(), before.2.as_slice()),
        (net, p, slots)
    );
    if let Some(plan) = plan {
        println!(
            "Exact optimum: {} cents; summed elapsed {} minutes",
            plan.total_fee_cents, plan.total_elapsed_minutes
        );
        for a in plan.assignments {
            for h in a.route.hops {
                println!(
                    "  {}: {} -{}-> {} | depart {}, arrive {}, fee {}",
                    a.payment_id,
                    h.transfer.sender,
                    h.transfer.rail_id,
                    h.transfer.receiver,
                    h.departure_minute,
                    h.arrival_minute,
                    h.fee_cents
                );
            }
        }
        for u in plan
            .departure_usage
            .iter()
            .filter(|u| u.principal_cents > 0)
        {
            println!(
                "  {}@{} uses {} principal cents",
                u.rail_id, u.departure_minute, u.principal_cents
            );
        }
    } else {
        println!("Exact result: no full plan within this timetable.");
    }
    Ok(())
}
