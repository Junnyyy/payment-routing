//! Test-only exhaustive oracle. Enumerate bounded walks by length, then form the
//! full Cartesian product and validate capacity only on complete assignments.
//! No production route generation, ranking or pruning helpers are used.
#![allow(dead_code)]

use payment_routing::network::{Institution, Network, Payment, Rail};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Walk {
    pub hops: Vec<(String, String, String)>,
    pub fee: u128,
    pub minutes: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Answer {
    pub fee: u128,
    pub minutes: u128,
    pub hops: usize,
    pub paths: Vec<Vec<(String, String, String)>>,
}

pub fn network(ids: &[&str], rails: Vec<Rail>) -> Network {
    Network {
        name: "Synthetic batch oracle".into(),
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

pub fn rail(id: &str, ids: &[&str], fee: u64, minutes: u32, capacity: Option<u64>) -> Rail {
    Rail {
        id: id.into(),
        name: id.into(),
        participants: ids.iter().map(|id| (*id).into()).collect(),
        fee_cents: fee,
        settlement_minutes: minutes,
        available: true,
        max_amount_cents: None,
        batch_capacity_cents: capacity,
    }
}

pub fn payment(id: &str, from: &str, to: &str, amount: u64) -> Payment {
    Payment {
        id: id.into(),
        sender: from.into(),
        receiver: to.into(),
        amount_cents: amount,
        max_delivery_minutes: None,
    }
}

pub fn walks(net: &Network, p: &Payment) -> Vec<Walk> {
    let mut frontier = vec![Walk {
        hops: vec![],
        fee: 0,
        minutes: 0,
    }];
    let mut answers = vec![];
    // Walks may revisit institutions and may leave/re-enter the receiver. With
    // nonnegative fees, time and resource consumption, cycle removal ensures an
    // optimal witness exists within n-1 hops. No visited-set rule is shared.
    for _ in 1..net.institutions.len() {
        let mut next = vec![];
        for path in frontier {
            let at = path.hops.last().map(|h| h.2.as_str()).unwrap_or(&p.sender);
            for r in &net.rails {
                if !r.available
                    || r.max_amount_cents.is_some_and(|c| p.amount_cents > c)
                    || !r.participants.iter().any(|id| id == at)
                {
                    continue;
                }
                for to in &r.participants {
                    if to == at {
                        continue;
                    }
                    let mut walk = path.clone();
                    walk.hops.push((r.id.clone(), at.into(), to.clone()));
                    walk.fee += u128::from(r.fee_cents);
                    walk.minutes += u128::from(r.settlement_minutes);
                    if to == &p.receiver
                        && p.max_delivery_minutes
                            .is_none_or(|d| walk.minutes <= u128::from(d))
                    {
                        answers.push(walk.clone());
                    }
                    next.push(walk);
                }
            }
        }
        frontier = next;
    }
    answers
}

pub fn capacity_fits(net: &Network, payments: &[&Payment], paths: &[Walk]) -> bool {
    // Rescan the complete assignment per rail, with wide arithmetic. In
    // particular neither direction nor member pair partitions a rail budget.
    net.rails.iter().all(|r| {
        let used: u128 = payments
            .iter()
            .zip(paths)
            .map(|(p, path)| {
                path.hops.iter().filter(|h| h.0 == r.id).count() as u128
                    * u128::from(p.amount_cents)
            })
            .sum();
        r.batch_capacity_cents.is_none_or(|c| used <= u128::from(c))
    })
}

pub fn answer(paths: &[Walk]) -> Answer {
    Answer {
        fee: paths.iter().map(|w| w.fee).sum(),
        minutes: paths.iter().map(|w| w.minutes).sum(),
        hops: paths.iter().map(|w| w.hops.len()).sum(),
        paths: paths.iter().map(|w| w.hops.clone()).collect(),
    }
}

pub fn exhaustive(net: &Network, payments: &[Payment]) -> Option<Answer> {
    let mut ordered: Vec<_> = payments.iter().collect();
    ordered.sort_by_key(|p| &p.id);
    let mut product = vec![vec![]];
    for p in &ordered {
        let choices = walks(net, p);
        let mut next = vec![];
        for prefix in product {
            for choice in &choices {
                let mut assignment = prefix.clone();
                assignment.push(choice.clone());
                next.push(assignment);
            }
        }
        product = next;
    }
    product
        .iter()
        .filter(|paths| capacity_fits(net, &ordered, paths))
        .map(|paths| answer(paths))
        .min()
}

pub fn greedy(net: &Network, payments: &[Payment]) -> Option<Answer> {
    let mut picked = vec![];
    let mut processed = vec![];
    for p in payments {
        processed.push(p);
        let mut choices = walks(net, p);
        choices.sort_by(|a, b| {
            (a.fee, a.minutes, a.hops.len(), &a.hops).cmp(&(
                b.fee,
                b.minutes,
                b.hops.len(),
                &b.hops,
            ))
        });
        let choice = choices.into_iter().find(|candidate| {
            let mut trial = picked.clone();
            trial.push(candidate.clone());
            capacity_fits(net, &processed, &trial)
        })?;
        picked.push(choice);
    }
    Some(answer(&picked))
}

pub fn greedy_trap() -> (Network, Vec<Payment>) {
    // P1=1 cent, P2=2 cents, same endpoints. Shared cheap rail holds 2 cents.
    // Backup charges 3 cents but carries only 1 cent per hop; fallback costs 10.
    let mut backup = rail("backup", &["A", "B"], 3, 0, None);
    backup.max_amount_cents = Some(1);
    (
        network(
            &["A", "B"],
            vec![
                rail("cheap", &["A", "B"], 1, 0, Some(2)),
                backup,
                rail("fallback", &["A", "B"], 10, 0, None),
            ],
        ),
        vec![payment("P1", "A", "B", 1), payment("P2", "A", "B", 2)],
    )
}
