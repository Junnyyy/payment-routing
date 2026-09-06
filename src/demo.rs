use crate::network::{Institution, Network, Payment, Rail};

/// A synthetic USD scenario with stable identifiers, amounts and ordering.
/// RTP, FedNow, ACH and Fedwire are real rail names; all membership, fees and
/// settlement times here are synthetic inputs, not those networks' operating rules.
/// No clock, randomness, files, network calls, settlement or routing is involved.
pub fn demo_network() -> Network {
    let institutions = [
        ("ALP", "Alpine Bank", 25_000_000),
        ("BRK", "Brook Bank", 18_000_000),
        ("CDR", "Cedar Credit", 12_500_000),
        ("DLT", "Delta Bank", 30_000_000),
        ("ELM", "Elm Treasury", 9_500_000),
        ("FLD", "Field Credit", 5_000_000),
    ]
    .into_iter()
    .map(|(id, name, opening_balance_cents)| Institution {
        id: id.into(),
        name: name.into(),
        opening_balance_cents,
    })
    .collect();
    // FedNow reuses the synthetic instant inputs and member set used for RTP.
    // This is a fixture choice, not a claim that the real networks are equivalent.
    let rails = [
        ("RTP", "RTP", vec!["ALP", "BRK", "CDR", "DLT"], 25, 0),
        ("FEDNOW", "FedNow", vec!["ALP", "BRK", "CDR", "DLT"], 25, 0),
        (
            "ACH",
            "ACH",
            vec!["ALP", "BRK", "CDR", "DLT", "ELM", "FLD"],
            5,
            1_440,
        ),
        ("FEDWIRE", "Fedwire", vec!["ALP", "DLT", "ELM"], 1_500, 30),
    ]
    .into_iter()
    .map(
        |(id, name, participants, fee_cents, settlement_minutes)| Rail {
            id: id.into(),
            name: name.into(),
            participants: participants.into_iter().map(String::from).collect(),
            fee_cents,
            settlement_minutes,
        },
    )
    .collect();
    let payments = [
        ("P001", "ALP", "BRK", 1_250_000),
        ("P002", "BRK", "CDR", 875_050),
        ("P003", "CDR", "DLT", 2_000_000),
        ("P004", "DLT", "ELM", 5_000_000),
        ("P005", "ELM", "FLD", 325_000),
        ("P006", "FLD", "ALP", 150_075),
        ("P007", "ALP", "ELM", 7_500_000),
        ("P008", "BRK", "DLT", 1_000_000),
        ("P009", "CDR", "FLD", 420_000),
        ("P010", "DLT", "ALP", 3_250_000),
        ("P011", "ELM", "BRK", 640_000),
        ("P012", "FLD", "CDR", 90_025),
    ]
    .into_iter()
    .map(|(id, sender, receiver, amount_cents)| Payment {
        id: id.into(),
        sender: sender.into(),
        receiver: receiver.into(),
        amount_cents,
    })
    .collect();
    Network {
        name: "Demo / six institutions".into(),
        institutions,
        rails,
        payments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Statistics;

    #[test]
    fn fixture_is_deterministic_valid_and_has_known_totals() {
        let network = demo_network();
        assert_eq!(network, demo_network());
        assert_eq!(network.validate(), Ok(()));
        assert_eq!(
            network.statistics(),
            Statistics {
                institution_count: 6,
                rail_count: 4,
                payment_count: 12,
                opening_balance_cents: 100_000_000,
                payment_volume_cents: 22_500_150,
                largest_payment_cents: 7_500_000,
            }
        );
    }
}
