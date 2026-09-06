//! Run with `cargo run --locked --example route_one` (no terminal required).
//! All inputs and expected answers below are synthetic and hand-calculated.

use payment_routing::{
    network::{Institution, Network, Payment, Rail},
    routing::route_payment,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut network = Network {
        name: "Single-payment routing counterexamples / synthetic USD".into(),
        institutions: ["A", "B", "C", "D"]
            .into_iter()
            .map(|id| Institution {
                id: id.into(),
                name: format!("Institution {id}"),
                opening_balance_cents: 0,
            })
            .collect(),
        rails: [
            ("cheap-ab", "A", "B", 1, 9),
            ("fast-ab", "A", "B", 5, 3),
            ("bd", "B", "D", 1, 2),
            ("ac", "A", "C", 3, 1),
            ("cd", "C", "D", 4, 1),
            ("direct", "A", "D", 9, 1),
        ]
        .into_iter()
        .map(
            |(id, sender, receiver, fee_cents, settlement_minutes)| Rail {
                id: id.into(),
                name: id.into(),
                participants: vec![sender.into(), receiver.into()],
                fee_cents,
                settlement_minutes,
                available: true,
                max_amount_cents: None,
            },
        )
        .collect(),
        payments: vec![],
    };
    let mut payment = Payment {
        id: "P".into(),
        sender: "A".into(),
        receiver: "D".into(),
        amount_cents: 100,
        max_delivery_minutes: None,
    };
    println!("Synthetic USD 1.00 payment, A -> D. No funds are moved.\n");
    show(
        "No deadline",
        &network,
        &payment,
        Some((2, 11, &["cheap-ab", "bd"])),
    )?;
    payment.max_delivery_minutes = Some(10);
    show(
        "10-minute deadline",
        &network,
        &payment,
        Some((6, 5, &["fast-ab", "bd"])),
    )?;
    network.rails[2].max_amount_cents = Some(99);
    show(
        "B-D ceiling is 99 cents",
        &network,
        &payment,
        Some((7, 2, &["ac", "cd"])),
    )?;
    network.rails[4].available = false;
    show(
        "C-D is also unavailable",
        &network,
        &payment,
        Some((9, 1, &["direct"])),
    )?;
    payment.max_delivery_minutes = Some(0);
    show("Zero-minute deadline", &network, &payment, None)?;
    Ok(())
}

fn show(
    label: &str,
    network: &Network,
    payment: &Payment,
    expected: Option<(u128, u128, &[&str])>,
) -> Result<(), Box<dyn std::error::Error>> {
    let route = route_payment(network, payment)?;
    match (route, expected) {
        (Some(route), Some((fee, minutes, rails))) => {
            assert_eq!(route.total_fee_cents, fee);
            assert_eq!(route.total_settlement_minutes, minutes);
            let actual: Vec<_> = route.hops.iter().map(|hop| hop.rail_id.as_str()).collect();
            assert_eq!(actual, rails);
            println!(
                "{label}: {} | {fee} cents | {minutes} minutes",
                actual.join(" -> ")
            );
        }
        (None, None) => println!("{label}: unreachable"),
        (actual, expected) => panic!("{label}: expected {expected:?}, got {actual:?}"),
    }
    Ok(())
}
