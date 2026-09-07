//! Opt-in, bounded evidence from actual routing searches. No terminal types.
use std::collections::BTreeMap;

use crate::routing::Route;

pub const EVIDENCE_PER_PAYMENT: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEvidence {
    /// Describes a search candidate or rejected prefix, not a global proof.
    pub reason: String,
    pub route: Option<Route>,
    pub departures: Vec<u128>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaymentEvidence {
    pub entries: Vec<SearchEvidence>,
    pub omitted: u128,
}

/// One tick only. Callers choose their own bounded retention policy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DecisionEvidence {
    pub payments: BTreeMap<String, PaymentEvidence>,
}

pub(crate) fn record(
    evidence: &mut Option<&mut DecisionEvidence>,
    id: &str,
    item: impl FnOnce() -> SearchEvidence,
) {
    if let Some(evidence) = evidence {
        let payment = evidence.payments.entry(id.into()).or_default();
        if payment.entries.len() >= EVIDENCE_PER_PAYMENT {
            payment.omitted = payment.omitted.saturating_add(1);
            return;
        }
        let item = item();
        if !payment.entries.contains(&item) {
            payment.entries.push(item);
        }
    }
}
