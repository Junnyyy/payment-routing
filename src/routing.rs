//! Exact, read-only routing through shared USD payment rails.

use crate::network::{Network, Payment, ValidationError};

/// A transfer of the full payment principal between two members of a rail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RouteHop {
    pub rail_id: String,
    pub sender: String,
    pub receiver: String,
}

/// Fees are charged separately from principal; nothing is debited or reserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub hops: Vec<RouteHop>,
    pub total_fee_cents: u128,
    pub total_settlement_minutes: u128,
}

impl Route {
    fn rank(&self) -> (u128, u128, usize, &[RouteHop]) {
        (
            self.total_fee_cents,
            self.total_settlement_minutes,
            self.hops.len(),
            &self.hops,
        )
    }
}

/// Return the cheapest route, or `None` for a valid but unreachable payment.
/// The payment may be supplied independently of `network.payments`.
/// Ties prefer lower latency, then fewer hops, then the lexicographically smaller
/// sequence of `(rail_id, sender, receiver)`. Collection ordering is irrelevant.
///
/// A shared rail connects every distinct pair of participants in both directions.
/// Each hop carries the full USD principal and adds one fixed fee and latency.
/// Opening balances are descriptive input, not a modeled funding constraint.
/// Unavailable rails and rails below the principal's transaction amount are
/// excluded. Total latency must not exceed the payment's optional deadline.
/// Availability is fixed for the entire route; waiting and FX are not modeled.
///
/// The search enumerates simple institution paths. Removing a cycle never raises
/// nonnegative fees or latency, and wins the hop-count tie, so an optimum is simple.
/// This is exact but exponential in the worst case: intended for small synthetic
/// networks, without a heuristic cutoff or external solver.
///
/// ```
/// use payment_routing::{demo::demo_network, routing::route_payment};
///
/// let network = demo_network();
/// let mut payment = network.payments[0].clone();
/// let cheapest = route_payment(&network, &payment)?.unwrap();
/// assert_eq!(cheapest.total_fee_cents, 5);
/// assert_eq!(cheapest.hops[0].rail_id, "ACH");
///
/// payment.max_delivery_minutes = Some(0);
/// let immediate = route_payment(&network, &payment)?.unwrap();
/// assert_eq!(immediate.total_fee_cents, 25);
/// assert_eq!(immediate.total_settlement_minutes, 0);
/// # Ok::<(), payment_routing::network::ValidationError>(())
/// ```
pub fn route_payment(
    network: &Network,
    payment: &Payment,
) -> Result<Option<Route>, ValidationError> {
    network.validate()?;
    payment.validate(network)?;
    let mut best = None;
    visit(
        network,
        &payment.sender,
        payment,
        &mut vec![payment.sender.as_str()],
        &mut Route {
            hops: vec![],
            total_fee_cents: 0,
            total_settlement_minutes: 0,
        },
        &mut best,
    );
    Ok(best)
}

fn visit<'a>(
    network: &'a Network,
    current: &'a str,
    payment: &Payment,
    visited: &mut Vec<&'a str>,
    path: &mut Route,
    best: &mut Option<Route>,
) {
    if let Some(deadline) = payment.max_delivery_minutes
        && path.total_settlement_minutes > u128::from(deadline)
    {
        return;
    }
    // Strict cost pruning preserves equal-cost candidates that can win a tie.
    if let Some(route) = best.as_ref()
        && path.total_fee_cents > route.total_fee_cents
    {
        return;
    }
    if current == payment.receiver {
        let improves = match best {
            Some(route) => path.rank() < route.rank(),
            None => true,
        };
        if improves {
            *best = Some(path.clone());
        }
        return;
    }

    for rail in &network.rails {
        if !rail.available || !rail.participants.iter().any(|id| id == current) {
            continue;
        }
        if let Some(limit) = rail.max_amount_cents
            && payment.amount_cents > limit
        {
            continue;
        }
        for next in &rail.participants {
            if visited.contains(&next.as_str()) {
                continue;
            }
            visited.push(next);
            path.hops.push(RouteHop {
                rail_id: rail.id.clone(),
                sender: current.into(),
                receiver: next.clone(),
            });
            // At most |institutions|-1 hops. u128 holds sums of u64 fees and u32
            // minutes for any addressable simple path on supported Rust targets.
            path.total_fee_cents += u128::from(rail.fee_cents);
            path.total_settlement_minutes += u128::from(rail.settlement_minutes);
            visit(network, next, payment, visited, path, best);
            path.total_settlement_minutes -= u128::from(rail.settlement_minutes);
            path.total_fee_cents -= u128::from(rail.fee_cents);
            path.hops.pop();
            visited.pop();
        }
    }
}
