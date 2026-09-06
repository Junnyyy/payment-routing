use crate::network::{Network, Payment, ValidationError, unique_ids};

/// Uniformly sampled directed flow. Repeated entries give a flow more weight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentFlow {
    pub sender: String,
    pub receiver: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivalProcess {
    pub attempts_per_minute: u32,
    /// Bernoulli probability in [0, 1_000_000], sampled once per attempt.
    pub probability_per_million: u32,
    pub flows: Vec<PaymentFlow>,
    pub min_amount_cents: u64,
    pub max_amount_cents: u64,
    /// Finite inclusive relative deadlines, measured from generated arrival.
    pub min_sla_minutes: u64,
    pub max_sla_minutes: u64,
}

/// A repeating window, wholly contained in its period, and a fresh budget each
/// open minute. This is separate from a static optimization-batch budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailService {
    pub rail_id: String,
    pub period_minutes: u64,
    pub offset_minutes: u64,
    pub open_minutes: u64,
    pub capacity_per_minute_cents: Option<u64>,
}

impl RailService {
    pub(super) fn is_open(&self, minute: u128) -> bool {
        let phase = minute % u128::from(self.period_minutes);
        phase >= u128::from(self.offset_minutes)
            && phase - u128::from(self.offset_minutes) < u128::from(self.open_minutes)
    }
}

/// The initial policy delegates path selection to the existing static router.
/// It considers current availability/capacity, then pins each accepted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    CheapestStatic,
    /// Bounded window-aware routing with complete per-departure reservations.
    Reserved {
        limits: crate::scalable::SearchLimits,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    pub network: Network,
    pub arrivals: ArrivalProcess,
    pub services: Vec<RailService>,
    pub strategy: RoutingStrategy,
    pub max_active_payments: usize,
    /// Zero disables retained history; each step still returns its events.
    pub retained_events: usize,
}

impl Scenario {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.network.validate()?;
        if let RoutingStrategy::Reserved { limits } = self.strategy {
            limits.validate()?;
        }
        let arrivals = &self.arrivals;
        if arrivals.probability_per_million > 1_000_000
            || arrivals.min_amount_cents == 0
            || arrivals.min_amount_cents > arrivals.max_amount_cents
            || arrivals.min_sla_minutes > arrivals.max_sla_minutes
            || arrivals.flows.is_empty()
        {
            return Err(ValidationError(
                "invalid arrival probability, ranges or flows".into(),
            ));
        }
        if self.max_active_payments == 0 {
            return Err(ValidationError(
                "active payment limit must be positive".into(),
            ));
        }
        for flow in &arrivals.flows {
            Payment {
                id: "flow-validation".into(),
                sender: flow.sender.clone(),
                receiver: flow.receiver.clone(),
                amount_cents: arrivals.min_amount_cents,
                max_delivery_minutes: None,
            }
            .validate(&self.network)?;
        }
        unique_ids(
            "service rail",
            self.services.iter().map(|s| s.rail_id.as_str()),
        )?;
        if self.services.len() != self.network.rails.len() {
            return Err(ValidationError(
                "supply exactly one service per rail".into(),
            ));
        }
        for service in &self.services {
            if !self.network.rails.iter().any(|r| r.id == service.rail_id) {
                return Err(ValidationError(format!(
                    "unknown service rail {}",
                    service.rail_id
                )));
            }
            if service.period_minutes == 0
                || service.offset_minutes >= service.period_minutes
                || service.open_minutes > service.period_minutes - service.offset_minutes
            {
                return Err(ValidationError(format!(
                    "invalid recurring window for {}",
                    service.rail_id
                )));
            }
        }
        if self
            .network
            .rails
            .iter()
            .any(|r| r.batch_capacity_cents.is_some())
        {
            return Err(ValidationError(
                "simulation requires unlimited static batch budgets; configure per-minute service capacity instead".into(),
            ));
        }
        Ok(())
    }
}
