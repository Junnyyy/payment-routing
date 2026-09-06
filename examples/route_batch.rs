//! Synthetic, hand-enumerable batch counterexamples. No terminal or solver.
use payment_routing::{
    batch::optimize_batch,
    network::{Institution, Network, Payment, Rail},
    routing::route_payment,
};

fn rail(id: &str, from: &str, to: &str, fee: u64, minutes: u32, capacity: Option<u64>) -> Rail {
    Rail {
        id: id.into(),
        name: id.into(),
        participants: vec![from.into(), to.into()],
        fee_cents: fee,
        settlement_minutes: minutes,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: capacity,
    }
}

fn payment(id: &str, from: &str, to: &str, amount: u64) -> Payment {
    Payment {
        id: id.into(),
        sender: from.into(),
        receiver: to.into(),
        amount_cents: amount,
        max_delivery_minutes: None,
    }
}

fn network(ids: &[&str], rails: Vec<Rail>) -> Network {
    Network {
        name: "Synthetic static USD batch".into(),
        institutions: ids
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut backup = rail("backup", "A", "B", 3, 0, None);
    backup.max_amount_cents = Some(1);
    let mut net = network(
        &["A", "B"],
        vec![
            rail("cheap", "A", "B", 1, 0, Some(2)),
            backup,
            rail("fallback", "A", "B", 10, 0, None),
        ],
    );
    let payments = vec![payment("P1", "A", "B", 1), payment("P2", "A", "B", 2)];
    println!("Synthetic USD cents. Static budgets; no funds are moved.\n");
    println!("Two-node case: P1=1 cent, P2=2 cents. Cheap capacity=2 cents.");
    println!("Backup ceiling=1 cent; fallback is unlimited. All six assignments:");
    // With two institutions, every simple path is a single rail. Independently
    // enumerate that exact Cartesian product, checking per-hop eligibility first.
    let mut minimum = None::<u128>;
    let mut count = 0;
    for first in &net.rails {
        for second in &net.rails {
            if first
                .max_amount_cents
                .is_some_and(|c| payments[0].amount_cents > c)
                || second
                    .max_amount_cents
                    .is_some_and(|c| payments[1].amount_cents > c)
            {
                continue;
            }
            count += 1;
            let fee = u128::from(first.fee_cents) + u128::from(second.fee_cents);
            let fits = net.rails.iter().all(|r| {
                let used = if r.id == first.id { 1u128 } else { 0 }
                    + if r.id == second.id { 2u128 } else { 0 };
                r.batch_capacity_cents.is_none_or(|c| used <= u128::from(c))
            });
            println!(
                "  {:8} + {:8}: {fee:2} cents | {}",
                first.id,
                second.id,
                if fits {
                    "feasible"
                } else {
                    "capacity exceeded"
                }
            );
            if fits {
                minimum = Some(minimum.map_or(fee, |best| best.min(fee)));
            }
        }
    }
    assert_eq!(count, 6);
    assert_eq!(minimum, Some(4));
    let independent: u128 = payments
        .iter()
        .map(|p| route_payment(&net, p).unwrap().unwrap().total_fee_cents)
        .sum();
    assert_eq!(independent, 2);
    println!("Independent minima: 2 cents, but cheap usage is 3 > 2.");
    println!("Greedy P1-first: cheap + fallback = 11 cents.");
    show(&net, &payments, Some(4))?;
    net.rails.pop();
    println!("\nRemove fallback: greedy P1-first gets stuck; exact routing still costs 4.");
    show(&net, &payments, Some(4))?;
    net.rails[0].batch_capacity_cents = Some(0);
    println!("\nSet cheap capacity to zero: no complete assignment exists.");
    show(&net, &payments, None)?;

    let net = network(
        &["A", "B", "C", "D"],
        vec![
            rail("ab", "A", "B", 0, 1, None),
            rail("cheap-bc", "B", "C", 1, 0, Some(1)),
            rail("cd", "C", "D", 0, 1, None),
            rail("backup-ad", "A", "D", 3, 1, None),
            rail("fallback-bc", "B", "C", 10, 0, None),
        ],
    );
    let mut payments = vec![payment("P1", "A", "D", 1), payment("P2", "B", "C", 1)];
    payments[1].max_delivery_minutes = Some(0);
    println!("\nMultihop case: P1 A->D, urgent P2 B->C; each carries 1 cent.");
    println!("P1 paths: A-B-C-D on cheap-bc (1), A-D (3), A-B-C-D on fallback-bc (10).");
    println!("P2 paths: cheap-bc (1), fallback-bc (10); B-A-D-C misses its zero-minute deadline.");
    println!("Greedy spends cheap-bc on P1: total 11. Exact reserves it for P2: total 4.");
    show(&net, &payments, Some(4))?;
    Ok(())
}

fn show(
    net: &Network,
    payments: &[Payment],
    expected: Option<u128>,
) -> Result<(), Box<dyn std::error::Error>> {
    let before = net.clone();
    let result = optimize_batch(net, payments)?;
    assert_eq!(result.as_ref().map(|p| p.total_fee_cents), expected);
    assert_eq!(net, &before);
    if let Some(plan) = result {
        println!("Exact optimum: {} cents", plan.total_fee_cents);
        for a in plan.assignments {
            let hops: Vec<_> = a
                .route
                .hops
                .iter()
                .map(|h| format!("{} -{}-> {}", h.sender, h.rail_id, h.receiver))
                .collect();
            println!("  {}: {}", a.payment_id, hops.join(", "));
        }
        for r in plan.rail_usage.iter().filter(|r| r.principal_cents > 0) {
            println!(
                "  {} usage: {} principal cents",
                r.rail_id, r.principal_cents
            );
        }
    } else {
        println!("Exact result: batch infeasible");
    }
    Ok(())
}
