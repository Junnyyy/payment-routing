#[path = "support/batch_oracle.rs"]
mod fixtures;
use fixtures::{network, payment, rail};
use payment_routing::scheduling::{RailDeparture, TimedPayment, validate_schedule};

fn timed(id: &str, release: u64, deadline: Option<u64>) -> TimedPayment {
    TimedPayment {
        payment: payment(id, "A", "B", 1),
        earliest_execution_minute: release,
        deadline_minute: deadline,
    }
}

fn slot(id: &str, minute: u64) -> RailDeparture {
    RailDeparture {
        rail_id: id.into(),
        departure_minute: minute,
        fee_cents: None,
        settlement_minutes: None,
        capacity_cents: None,
    }
}

#[test]
fn validates_the_whole_input_before_feasibility_or_empty_batch_shortcuts() {
    let mut net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 0, None)]);
    assert!(validate_schedule(&net, &[timed("P", 3, Some(2))], &[]).is_ok());
    assert!(validate_schedule(&net, &[], &[slot("missing", 0)]).is_err());
    assert!(validate_schedule(&net, &[], &[slot("r", 0), slot("r", 0)]).is_err());
    assert!(validate_schedule(&net, &[], &[slot("r", 0), slot("r", 1)]).is_ok());
    assert!(validate_schedule(&net, &[timed("P", 0, None), timed("P", 1, None)], &[]).is_err());
    let mut bad = timed("P2", 0, None);
    bad.payment.receiver = "missing".into();
    assert!(validate_schedule(&net, &[timed("P1", 0, None), bad], &[]).is_err());
    net.payments.push(payment("bad", "missing", "B", 1));
    assert!(validate_schedule(&net, &[], &[]).is_err());
    net.payments.clear();
    net.rails[0].max_amount_cents = Some(0);
    assert!(validate_schedule(&net, &[], &[]).is_err());
}

#[test]
fn timestamps_are_checked_without_wrapping_even_on_closed_or_unused_slots() {
    let mut net = network(&["A", "B"], vec![rail("r", &["A", "B"], 0, 1, None)]);
    net.rails[0].available = false;
    assert!(validate_schedule(&net, &[], &[slot("r", u64::MAX)]).is_err());
    let mut last = slot("r", u64::MAX);
    last.settlement_minutes = Some(0);
    last.capacity_cents = Some(0);
    assert!(validate_schedule(&net, &[], &[last]).is_ok());
    assert!(validate_schedule(&net, &[], &[slot("r", u64::MAX - 1)]).is_ok());
}
