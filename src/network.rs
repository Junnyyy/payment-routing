use std::{collections::HashSet, error::Error, fmt};

/// All monetary values in this foundation are USD cents. No floating-point money.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Institution {
    pub id: String,
    pub name: String,
    pub opening_balance_cents: u64,
}

/// A shared payment service and its member institutions, not a computed route.
/// A recognizable name does not imply verified real-world operating rules.
/// Demo membership, fees and settlement times are explicitly synthetic inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rail {
    pub id: String,
    pub name: String,
    pub participants: Vec<String>,
    pub fee_cents: u64,
    pub settlement_minutes: u32,
}

/// An instruction awaiting routing. Loading or viewing it never moves funds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    pub id: String,
    pub sender: String,
    pub receiver: String,
    pub amount_cents: u64,
}

impl Payment {
    /// Validate an instruction, including one not stored in the network.
    /// Validity does not imply a route or sufficient funding.
    pub fn validate(&self, network: &Network) -> Result<(), ValidationError> {
        unique_ids("payment", std::iter::once(self.id.as_str()))?;
        for endpoint in [&self.sender, &self.receiver] {
            if !network.institutions.iter().any(|i| &i.id == endpoint) {
                return Err(ValidationError(format!(
                    "payment {} references unknown institution {endpoint}",
                    self.id
                )));
            }
        }
        if self.amount_cents == 0 || self.sender == self.receiver {
            return Err(ValidationError(format!(
                "payment {} needs a positive amount and different endpoints",
                self.id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub name: String,
    pub institutions: Vec<Institution>,
    pub rails: Vec<Rail>,
    pub payments: Vec<Payment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statistics {
    pub institution_count: usize,
    pub rail_count: usize,
    pub payment_count: usize,
    // Wide totals preserve exact sums even when several u64 amounts are combined.
    pub opening_balance_cents: u128,
    pub payment_volume_cents: u128,
    pub largest_payment_cents: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ValidationError {}

impl Network {
    /// Validate identifiers, references and positive, non-self payment instructions.
    /// Empty collections are valid; routing feasibility is deliberately not checked.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError("network name must not be empty".into()));
        }
        let institutions = unique_ids(
            "institution",
            self.institutions.iter().map(|i| i.id.as_str()),
        )?;
        unique_ids("rail", self.rails.iter().map(|r| r.id.as_str()))?;
        unique_ids("payment", self.payments.iter().map(|p| p.id.as_str()))?;

        for institution in &self.institutions {
            if institution.name.trim().is_empty() {
                return Err(ValidationError(format!(
                    "institution {} has no name",
                    institution.id
                )));
            }
        }
        for rail in &self.rails {
            if rail.name.trim().is_empty() || rail.participants.len() < 2 {
                return Err(ValidationError(format!(
                    "rail {} needs a name and at least two participants",
                    rail.id
                )));
            }
            unique_ids(
                &format!("participant in rail {}", rail.id),
                rail.participants.iter().map(String::as_str),
            )?;
            for participant in &rail.participants {
                if !institutions.contains(participant.as_str()) {
                    return Err(ValidationError(format!(
                        "rail {} references unknown institution {participant}",
                        rail.id
                    )));
                }
            }
        }
        for payment in &self.payments {
            payment.validate(self)?;
        }
        Ok(())
    }

    pub fn statistics(&self) -> Statistics {
        Statistics {
            institution_count: self.institutions.len(),
            rail_count: self.rails.len(),
            payment_count: self.payments.len(),
            opening_balance_cents: self
                .institutions
                .iter()
                .map(|i| u128::from(i.opening_balance_cents))
                .sum(),
            payment_volume_cents: self
                .payments
                .iter()
                .map(|p| u128::from(p.amount_cents))
                .sum(),
            largest_payment_cents: self
                .payments
                .iter()
                .map(|p| p.amount_cents)
                .max()
                .unwrap_or(0),
        }
    }
}

fn unique_ids<'a>(
    kind: &str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<HashSet<&'a str>, ValidationError> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.trim().is_empty() || id.trim() != id {
            return Err(ValidationError(format!(
                "{kind} identifier must be nonempty with no surrounding whitespace"
            )));
        }
        if !seen.insert(id) {
            return Err(ValidationError(format!("duplicate {kind} identifier {id}")));
        }
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::demo_network;

    #[test]
    fn rejects_duplicate_identifiers_in_each_collection() {
        let mut network = demo_network();
        network.institutions.push(network.institutions[0].clone());
        assert!(
            network
                .validate()
                .unwrap_err()
                .to_string()
                .contains("duplicate institution")
        );
        let mut network = demo_network();
        network.rails.push(network.rails[0].clone());
        assert!(
            network
                .validate()
                .unwrap_err()
                .to_string()
                .contains("duplicate rail")
        );
        let mut network = demo_network();
        network.payments.push(network.payments[0].clone());
        assert!(
            network
                .validate()
                .unwrap_err()
                .to_string()
                .contains("duplicate payment")
        );
    }

    #[test]
    fn rejects_dangling_payment_and_rail_references() {
        let mut network = demo_network();
        network.payments[0].sender = "missing".into();
        assert!(
            network
                .validate()
                .unwrap_err()
                .to_string()
                .contains("unknown institution missing")
        );
        let mut network = demo_network();
        network.payments[0].receiver = "missing".into();
        assert!(network.validate().is_err());
        let mut network = demo_network();
        network.rails[0].participants[0] = "missing".into();
        assert!(network.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_or_insufficient_rail_membership() {
        let mut network = demo_network();
        network.rails[0].participants = vec!["ALP".into(), "ALP".into()];
        assert!(network.validate().is_err());
        network.rails[0].participants.pop();
        assert!(network.validate().is_err());
    }

    #[test]
    fn rejects_zero_and_self_payments() {
        let mut network = demo_network();
        network.payments[0].amount_cents = 0;
        assert!(network.validate().is_err());
        network.payments[0].amount_cents = 1;
        network.payments[0].receiver = network.payments[0].sender.clone();
        assert!(network.validate().is_err());
    }

    #[test]
    fn rejects_empty_names_and_blank_or_padded_ids() {
        let mut network = demo_network();
        network.name = " ".into();
        assert!(network.validate().is_err());
        network.name = "Network".into();
        network.institutions[0].id = " ".into();
        assert!(network.validate().is_err());
        network.institutions[0].id = " ALP".into();
        assert!(network.validate().is_err());
        network.institutions[0].id = "ALP".into();
        network.institutions[0].name.clear();
        assert!(network.validate().is_err());
    }

    #[test]
    fn aggregates_empty_network_and_wide_amounts_exactly() {
        let empty = Network {
            name: "Empty".into(),
            institutions: vec![],
            rails: vec![],
            payments: vec![],
        };
        assert_eq!(empty.validate(), Ok(()));
        assert_eq!(
            empty.statistics(),
            Statistics {
                institution_count: 0,
                rail_count: 0,
                payment_count: 0,
                opening_balance_cents: 0,
                payment_volume_cents: 0,
                largest_payment_cents: 0,
            }
        );
        let mut network = demo_network();
        for payment in &mut network.payments {
            payment.amount_cents = u64::MAX;
        }
        for institution in &mut network.institutions {
            institution.opening_balance_cents = u64::MAX;
        }
        let stats = network.statistics();
        assert_eq!(stats.payment_volume_cents, u128::from(u64::MAX) * 12);
        assert_eq!(stats.opening_balance_cents, u128::from(u64::MAX) * 6);
    }
}
