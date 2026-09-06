//! Exact routing and execution planning over a finite synthetic timetable.
//! See `docs/time-model.md` for the time, capacity and optimality contract.

use crate::{
    batch::RailUsage,
    network::{Network, Payment, ValidationError, unique_ids},
    routing::RouteHop,
};

/// Arrival/release time is the first minute the payment can execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedPayment {
    pub payment: Payment,
    pub earliest_execution_minute: u64,
    /// Inclusive absolute completion deadline; None adds no absolute deadline.
    /// The underlying max_delivery_minutes also limits elapsed time from release,
    /// including waiting at the sender and intermediaries.
    pub deadline_minute: Option<u64>,
}

/// One rail-wide departure opportunity. Missing minutes are closed: there is no
/// implicit timetable or horizon expansion. All member pairs share this slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailDeparture {
    pub rail_id: String,
    pub departure_minute: u64,
    /// None inherits the rail's fee. The effective fee is charged per hop.
    pub fee_cents: Option<u64>,
    /// None inherits the rail's latency. Later departures may arrive earlier.
    pub settlement_minutes: Option<u32>,
    /// Principal budget for this departure, shared across all pairs/directions.
    /// None is unlimited; Some(0) closes the slot. The static batch budget applies
    /// additionally, across all slots, and never replenishes.
    pub capacity_cents: Option<u64>,
}

/// Effective inputs and exact timestamps make the returned plan replayable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScheduledHop {
    pub transfer: RouteHop,
    pub departure_minute: u64,
    pub arrival_minute: u64,
    pub fee_cents: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRoute {
    pub hops: Vec<ScheduledHop>,
    pub total_fee_cents: u128,
    /// Final arrival minus release, including every wait. Not summed hop latency.
    pub elapsed_minutes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledPaymentRoute {
    pub payment_id: String,
    pub route: ScheduledRoute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartureUsage {
    pub rail_id: String,
    pub departure_minute: u64,
    pub principal_cents: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledBatchPlan {
    /// Exactly the supplied payments, sorted by payment ID.
    pub assignments: Vec<ScheduledPaymentRoute>,
    /// Every network rail, including unused rails, sorted by rail ID.
    pub rail_usage: Vec<RailUsage>,
    /// Every supplied slot, including unused slots, sorted by rail ID then time.
    pub departure_usage: Vec<DepartureUsage>,
    pub total_fee_cents: u128,
    pub total_elapsed_minutes: u128,
}

/// Validate all inputs even for empty or infeasible batches. A deadline before
/// release is valid but infeasible. Timetables cannot reopen unavailable rails.
/// Unknown rails, duplicate (rail, minute) slots and overflowing arrivals are
/// malformed. Only the supplied timed payments will consume planning capacity.
pub fn validate_schedule(
    network: &Network,
    payments: &[TimedPayment],
    departures: &[RailDeparture],
) -> Result<(), ValidationError> {
    network.validate()?;
    unique_ids(
        "timed payment",
        payments.iter().map(|p| p.payment.id.as_str()),
    )?;
    for payment in payments {
        payment.payment.validate(network)?;
    }
    let mut seen = vec![];
    for slot in departures {
        let Some(rail) = network.rails.iter().find(|r| r.id == slot.rail_id) else {
            return Err(ValidationError(format!(
                "departure references unknown rail {}",
                slot.rail_id
            )));
        };
        let key = (&slot.rail_id, slot.departure_minute);
        if seen.contains(&key) {
            return Err(ValidationError(format!(
                "duplicate departure for rail {} at minute {}",
                slot.rail_id, slot.departure_minute
            )));
        }
        seen.push(key);
        if slot
            .departure_minute
            .checked_add(u64::from(
                slot.settlement_minutes.unwrap_or(rail.settlement_minutes),
            ))
            .is_none()
        {
            return Err(ValidationError(format!(
                "arrival overflows u64 for rail {} at minute {}",
                slot.rail_id, slot.departure_minute
            )));
        }
    }
    Ok(())
}
