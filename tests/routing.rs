use payment_routing::{
    demo::demo_network,
    network::{Institution, Network, Payment, Rail},
    routing::{Route, route_payment},
};

fn network(members: &[&str], rails: Vec<Rail>) -> Network {
    Network {
        name: "Synthetic routing test".into(),
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

fn rail(id: &str, members: &[&str], fee: u64, minutes: u32) -> Rail {
    Rail {
        id: id.into(),
        name: id.into(),
        participants: members.iter().map(|id| (*id).into()).collect(),
        fee_cents: fee,
        settlement_minutes: minutes,
        available: true,
        max_amount_cents: None,
    }
}

fn payment(sender: &str, receiver: &str) -> Payment {
    Payment {
        id: "P".into(),
        sender: sender.into(),
        receiver: receiver.into(),
        amount_cents: 100,
        max_delivery_minutes: None,
    }
}

fn assert_route(route: &Route, expected: &[(&str, &str, &str)], fee: u128, minutes: u128) {
    let hops: Vec<_> = route
        .hops
        .iter()
        .map(|hop| {
            (
                hop.rail_id.as_str(),
                hop.sender.as_str(),
                hop.receiver.as_str(),
            )
        })
        .collect();
    assert_eq!(hops, expected);
    assert_eq!(route.total_fee_cents, fee);
    assert_eq!(route.total_settlement_minutes, minutes);
}

#[test]
fn cheapest_direct_rail_is_not_the_first_or_fastest() {
    let net = network(
        &["A", "D"],
        vec![
            rail("fast", &["A", "D"], 9, 0),
            rail("cheap", &["A", "D"], 2, 8),
        ],
    );
    let route = route_payment(&net, &payment("A", "D")).unwrap().unwrap();
    assert_route(&route, &[("cheap", "A", "D")], 2, 8);
}

#[test]
fn multiple_competing_paths_have_a_known_multihop_optimum() {
    // All A-D simple paths: direct 10, via B 2+3=5, via C 1+7=8.
    let net = network(
        &["A", "B", "C", "D"],
        vec![
            rail("direct", &["A", "D"], 10, 1),
            rail("ab", &["A", "B"], 2, 3),
            rail("bd", &["B", "D"], 3, 4),
            rail("ac", &["A", "C"], 1, 0),
            rail("cd", &["C", "D"], 7, 0),
        ],
    );
    let route = route_payment(&net, &payment("A", "D")).unwrap().unwrap();
    assert_route(&route, &[("ab", "A", "B"), ("bd", "B", "D")], 5, 7);
}

#[test]
fn shared_rails_connect_every_pair_in_both_directions() {
    let net = network(
        &["A", "B", "C"],
        vec![rail("shared", &["A", "B", "C"], 3, 4)],
    );
    for (sender, receiver) in [("A", "C"), ("C", "A")] {
        let route = route_payment(&net, &payment(sender, receiver))
            .unwrap()
            .unwrap();
        assert_route(&route, &[("shared", sender, receiver)], 3, 4);
    }
}

#[test]
fn disconnected_and_empty_rail_networks_are_unreachable() {
    let mut net = network(
        &["A", "B", "C", "D"],
        vec![rail("ab", &["A", "B"], 1, 0), rail("cd", &["C", "D"], 1, 0)],
    );
    assert_eq!(route_payment(&net, &payment("A", "D")), Ok(None));
    net.rails.clear();
    assert_eq!(route_payment(&net, &payment("A", "D")), Ok(None));
}

#[test]
fn ties_prefer_latency_then_hops_then_lexical_hop_ids() {
    let mut net = network(
        &["A", "B", "D"],
        vec![
            rail("slow", &["A", "D"], 4, 9),
            rail("z-fast", &["A", "D"], 4, 2),
            rail("a-fast", &["A", "D"], 4, 2),
            rail("ab", &["A", "B"], 2, 1),
            rail("bd", &["B", "D"], 2, 1),
        ],
    );
    let expected = route_payment(&net, &payment("A", "D")).unwrap().unwrap();
    assert_route(&expected, &[("a-fast", "A", "D")], 4, 2);
    net.rails.reverse();
    net.institutions.reverse();
    for rail in &mut net.rails {
        rail.participants.reverse();
    }
    assert_eq!(route_payment(&net, &payment("A", "D")), Ok(Some(expected)));
}

#[test]
fn malformed_networks_and_external_instructions_return_errors() {
    let mut net = network(&["A", "D"], vec![rail("ad", &["A", "D"], 1, 0)]);
    let mut p = payment("A", "D");
    p.sender = "missing".into();
    assert!(route_payment(&net, &p).is_err());
    p = payment("A", "missing");
    assert!(route_payment(&net, &p).is_err());
    p = payment("A", "A");
    assert!(route_payment(&net, &p).is_err());
    p = payment("A", "D");
    p.amount_cents = 0;
    assert!(route_payment(&net, &p).is_err());
    p.amount_cents = 100;
    for id in ["", " ", " P"] {
        p.id = id.into();
        assert!(route_payment(&net, &p).is_err());
    }
    net.rails[0].participants.push("missing".into());
    assert!(route_payment(&net, &payment("A", "D")).is_err());
}

#[test]
fn routing_preserves_the_stage_zero_fixture_and_awaiting_instructions() {
    let net = demo_network();
    let before = net.clone();
    for p in &net.payments {
        let route = route_payment(&net, p).unwrap().unwrap();
        assert_route(&route, &[("ACH", &p.sender, &p.receiver)], 5, 1_440);
        assert_eq!(route_payment(&net, p), Ok(Some(route)));
    }
    assert_eq!(net, before);
    assert_eq!(net.statistics(), before.statistics());
}

#[test]
fn unavailable_cheapest_rail_cannot_be_used_or_waited_for() {
    let mut net = network(
        &["A", "B", "D"],
        vec![
            rail("ab", &["A", "B"], 0, 0),
            rail("closed", &["B", "D"], 0, 0),
            rail("open", &["A", "D"], 7, 5),
        ],
    );
    net.rails[1].available = false;
    let p = payment("A", "D");
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("open", "A", "D")],
        7,
        5,
    );
    net.rails[2].available = false;
    assert_eq!(net.validate(), Ok(()));
    assert_eq!(route_payment(&net, &p), Ok(None));
}

#[test]
fn inclusive_amount_ceiling_is_checked_on_every_hop() {
    let mut net = network(
        &["A", "B", "D"],
        vec![
            rail("ab", &["A", "B"], 1, 0),
            rail("bd", &["B", "D"], 1, 0),
            rail("direct", &["A", "D"], 9, 0),
        ],
    );
    net.rails[1].max_amount_cents = Some(100);
    let mut p = payment("A", "D");
    for amount in [99, 100] {
        p.amount_cents = amount;
        assert_route(
            &route_payment(&net, &p).unwrap().unwrap(),
            &[("ab", "A", "B"), ("bd", "B", "D")],
            2,
            0,
        );
    }
    p.amount_cents = 101;
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("direct", "A", "D")],
        9,
        0,
    );
    net.rails[2].max_amount_cents = Some(100);
    assert_eq!(route_payment(&net, &p), Ok(None));
    net.rails[1].max_amount_cents = Some(u64::MAX);
    p.amount_cents = u64::MAX;
    assert!(route_payment(&net, &p).unwrap().is_some());
}

#[test]
fn a_more_expensive_faster_prefix_is_needed_to_meet_delivery() {
    // A-B cheap: (fee=1,time=9), fast: (5,3). B-D: (1,2).
    // With a 10-minute deadline, fee 2 takes 11 and fails; fee 6 takes 5
    // and wins over the direct fee 9. Keeping only B's cheapest prefix fails.
    let net = network(
        &["A", "B", "D"],
        vec![
            rail("cheap", &["A", "B"], 1, 9),
            rail("fast", &["A", "B"], 5, 3),
            rail("bd", &["B", "D"], 1, 2),
            rail("direct", &["A", "D"], 9, 1),
        ],
    );
    let mut p = payment("A", "D");
    p.max_delivery_minutes = Some(10);
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("fast", "A", "B"), ("bd", "B", "D")],
        6,
        5,
    );
    p.max_delivery_minutes = Some(11);
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("cheap", "A", "B"), ("bd", "B", "D")],
        2,
        11,
    );
    p.max_delivery_minutes = None;
    assert_eq!(route_payment(&net, &p).unwrap().unwrap().total_fee_cents, 2);
    p.max_delivery_minutes = Some(0);
    assert_eq!(route_payment(&net, &p), Ok(None));
}

#[test]
fn zero_deadline_accepts_only_zero_total_latency() {
    let mut net = network(
        &["A", "B", "D"],
        vec![rail("ab", &["A", "B"], 0, 0), rail("bd", &["B", "D"], 0, 0)],
    );
    let mut p = payment("A", "D");
    p.max_delivery_minutes = Some(0);
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("ab", "A", "B"), ("bd", "B", "D")],
        0,
        0,
    );
    net.rails[1].settlement_minutes = 1;
    assert_eq!(route_payment(&net, &p), Ok(None));
}

#[test]
fn zero_transaction_ceiling_is_invalid_even_on_an_unavailable_rail() {
    let mut net = network(&["A", "D"], vec![rail("ad", &["A", "D"], 1, 0)]);
    net.rails[0].max_amount_cents = Some(0);
    for available in [true, false] {
        net.rails[0].available = available;
        assert!(net.validate().is_err());
        assert!(route_payment(&net, &payment("A", "D")).is_err());
    }
}

#[test]
fn fees_and_latency_sum_exactly_beyond_their_input_integer_widths() {
    let mut net = network(
        &["A", "B", "D"],
        vec![
            rail("ab", &["A", "B"], u64::MAX, u32::MAX),
            rail("bd", &["B", "D"], u64::MAX, u32::MAX),
        ],
    );
    let mut p = payment("A", "D");
    let minutes = 2 * u64::from(u32::MAX);
    p.max_delivery_minutes = Some(minutes);
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("ab", "A", "B"), ("bd", "B", "D")],
        2 * u128::from(u64::MAX),
        u128::from(minutes),
    );
    p.max_delivery_minutes = Some(minutes - 1);
    assert_eq!(route_payment(&net, &p), Ok(None));
    p.max_delivery_minutes = None;
    // A wrapping or saturating fee sum could wrongly prefer the faster detour.
    net.rails[0].settlement_minutes = 0;
    net.rails[1].settlement_minutes = 0;
    net.rails.push(rail("direct", &["A", "D"], u64::MAX, 1));
    assert_route(
        &route_payment(&net, &p).unwrap().unwrap(),
        &[("direct", "A", "D")],
        u128::from(u64::MAX),
        1,
    );
}

#[test]
fn zero_cost_cycles_terminate_and_equal_prefixes_can_improve_a_tie() {
    let mut net = network(
        &["A", "B", "C", "D"],
        vec![
            rail("ac", &["A", "C"], 0, 0),
            rail("bc", &["B", "C"], 0, 0),
            rail("ab", &["A", "B"], 0, 0),
            rail("bd", &["B", "D"], 0, 0),
        ],
    );
    // Search order encounters A-C-B-D before the better A-B-D at equal fee/time.
    let expected = route_payment(&net, &payment("A", "D")).unwrap().unwrap();
    assert_route(&expected, &[("ab", "A", "B"), ("bd", "B", "D")], 0, 0);
    net.rails.reverse();
    assert_eq!(route_payment(&net, &payment("A", "D")), Ok(Some(expected)));
}

#[test]
fn lexical_tie_break_includes_intermediate_institution_ids() {
    let mut net = network(
        &["A", "B", "C", "D"],
        vec![
            rail("entry", &["A", "C", "B"], 1, 1),
            rail("exit", &["D", "C", "B"], 1, 1),
        ],
    );
    let expected = route_payment(&net, &payment("A", "D")).unwrap().unwrap();
    assert_route(&expected, &[("entry", "A", "B"), ("exit", "B", "D")], 2, 2);
    net.rails.reverse();
    for rail in &mut net.rails {
        rail.participants.reverse();
    }
    assert_eq!(route_payment(&net, &payment("A", "D")), Ok(Some(expected)));
}
