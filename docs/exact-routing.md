# Exact routing

Single-payment and finite-batch APIs, with counterexamples and independent verification.
The [terminal demo](operations-console.md) uses continuous simulation.

## Single-payment routing

Each rail permits a transfer between any two distinct members, in either direction. A route carries the full USD principal on every hop, charges the fixed rail fee separately on every hop, and sums settlement minutes. Fees and latency totals use `u128`. Intermediaries can forward the principal; this is a static path model, not a claim about real bank operating rules or funding. Opening balances are descriptive and are neither used as routing capacity nor changed.

Three fields express optional static restrictions without changing existing demo values, ordering or totals:

| Field | Meaning |
| --- | --- |
| `Rail.available` | Whether this rail can be used for this snapshot; the search cannot wait for it to open. |
| `Rail.max_amount_cents` | Inclusive positive ceiling on each hop's principal, excluding fees; `None` means no ceiling. |
| `Payment.max_delivery_minutes` | Inclusive budget for the entire route's latency; `None` means no deadline and `Some(0)` requires zero latency. |

Currency compatibility is the existing USD-only invariant. There is no FX conversion, mixed-currency input, fee deduction from principal, liquidity reservation or payment splitting. Single-payment routing ignores the separate batch capacity field described below. A valid payment may be supplied separately from `network.payments`; its ID is metadata, not a lookup key. Routing first validates the entire network and then that instruction. Structural validation does not require route feasibility.

The exact search enumerates simple institution paths, excluding unavailable or over-limit rails and prefixes exceeding the delivery deadline. It minimizes total fees, then breaks ties by total latency, hop count, and the lexical sequence of `(rail_id, sender, receiver)`. Input collection order does not decide the result. All fees and latencies are nonnegative, so removing a cycle never worsens either and improves hop count; an optimum is therefore simple. Pruning only strictly more expensive prefixes preserves ties. No cheapest-prefix-per-node shortcut is used because a more expensive, faster prefix can be necessary to meet a deadline. Worst-case work is exponential and recursion depth is bounded by institution count; this is intended for small synthetic networks, not large production graphs.

Run the deterministic counterexamples without entering terminal mode:

```sh
cargo run --locked --example route_one
```

The example routes a 100-cent A-to-D payment. The A-B alternatives cost 1 cent / 9 minutes and 5 cents / 3 minutes; B-D costs 1 cent / 2 minutes. A-C-D costs 7 cents / 2 minutes, and A-D costs 9 cents / 1 minute. Those are all simple institution paths; the independently calculated optima are:

| Cumulative scenario changes | Optimal rails | Fee (cents) | Latency (minutes) |
| --- | --- | ---: | ---: |
| No deadline | cheap-ab, bd | 2 | 11 |
| Add a 10-minute deadline | fast-ab, bd | 6 | 5 |
| Limit B-D to 99 cents | ac, cd | 7 | 2 |
| Also mark C-D unavailable | direct | 9 | 1 |
| Tighten the deadline to zero | Unreachable | — | — |

The example asserts each expected answer. More scenarios, including invalid instructions and disconnected endpoints, live in `tests/routing.rs`. `cargo run --locked -- --demo` launches the continuous operations console described above.

## Exact batch routing

`batch::optimize_batch(&network, &payments)` chooses one unsplit route for **every supplied payment**, minimizing the sum of all per-hop fees. It returns `Ok(Some(BatchPlan))`, `Ok(None)` if no complete assignment exists, or a validation error. It validates the entire network and every supplied instruction, including duplicate IDs within the batch. Only the supplied payments consume capacity; stored `network.payments` are not implicitly added. An empty valid batch returns a zero-cost plan.

The sole new domain field is `Rail.batch_capacity_cents: Option<u64>`:

| Value / rule | Meaning |
| --- | --- |
| `None` | Unlimited capacity for the current batch; this is the demo default. |
| `Some(0)` | Valid budget permitting no batch use of that rail. |
| `Some(c)` | Inclusive budget of `c` USD principal cents across the complete batch. |
| Every hop | Consumes its payment's full principal from that rail's budget, excluding fees. |
| Shared service | All member pairs and both directions consume the same budget; nothing nets or replenishes. |

A multihop route consumes capacity on every rail it traverses. This budget is distinct from `max_amount_cents`, which limits each individual hop. Availability, per-hop ceilings and each payment's total delivery deadline still apply. Opening balances remain descriptive, and repeated optimization starts from the same input budgets without mutation, reservation or settlement. Capacity is static across the batch; no execution ordering, waiting, time windows or replenishment is inferred.

```rust
use payment_routing::{batch::optimize_batch, demo::demo_network};

let network = demo_network();
let plan = optimize_batch(&network, &network.payments).unwrap().unwrap();
assert_eq!(plan.assignments.len(), 12);
assert_eq!(plan.total_fee_cents, 60);
```

`BatchPlan` reports payment-ID-sorted assignments, rail-ID-sorted principal usage (including unused rails), total fee, and summed route latency. Sums use `u128`; summed latency is not a batch completion time. Equal-fee assignments prefer lower summed latency, then fewer total hops, then the lexical sequences of `(rail_id, sender, receiver)` in ascending payment-ID order. Collection ordering does not choose the result. The existing `route_payment` deliberately ignores batch capacity, so a one-payment batch can differ when that budget is restrictive. The terminal demo uses the separate continuous simulator, not this finite batch API.

### Why the search is exact

1. Enumerate all simple institution paths for each payment, applying availability, per-hop ceiling, deadline and that route's own rail usage. Keep more expensive alternatives: they may conserve a contested rail or meet a deadline.
2. Backtrack over the Cartesian product of those route choices, accumulating shared per-rail usage and rejecting over-budget prefixes.
3. Once a complete assignment exists, prune a prefix only when its fee plus the sum of each remaining payment's cheapest route fee is **strictly greater** than the best complete fee. That sum ignores competition, so it is a lower bound, not a feasible plan. Equal-fee branches remain eligible for tie-breaking.

Cycle removal never increases nonnegative fees, latency or rail usage, and improves total hop count. Therefore at least one optimum uses only simple paths. All such paths are enumerated; capacity pruning cannot remove a feasible completion; the optimistic fee bound cannot remove a better or tied completion. Comparing all remaining complete assignments with the documented rank yields an exact global optimum. There is no cheapest-prefix shortcut, heuristic cutoff or external routing solver. Candidate storage and runtime can grow exponentially, with recursion bounded by institution count during path generation and batch size during assignment search. This is a small synthetic-network reference.

### Hand-enumerated counterexamples

Run `cargo run --locked --example route_batch` without terminal initialization. All examples are synthetic, assert their expected results, and show chosen routes and principal usage.

The first case has two institutions A/B and two A-to-B payments: P1 carries 1 cent, P2 carries 2. Three rails connect A/B: cheap costs 1 cent with a 2-cent batch budget; backup costs 3 with a 1-cent per-hop ceiling; fallback costs 10 with no limits. Latency is zero. These are all six eligible assignments:

| P1 rail | P2 rail | Total fee (cents) | Feasibility |
| --- | --- | ---: | --- |
| cheap | cheap | 2 | Infeasible: principal usage 3 exceeds capacity 2 |
| cheap | fallback | 11 | Feasible; greedy P1-first result |
| backup | cheap | **4** | **Global optimum** |
| backup | fallback | 13 | Feasible |
| fallback | cheap | 11 | Feasible |
| fallback | fallback | 20 | Feasible |

Removing fallback makes greedy P1-first routing get stuck, while the global optimum remains 4 cents. Setting cheap capacity to zero then makes the whole batch infeasible. Independent minimum-cost routes do not give a higher *feasible* cost: their 2-cent result violates the shared budget. With unlimited capacity and additive fees, independent minima already achieve the global minimum.

A second case uses P1 A-to-D and urgent P2 B-to-C, both carrying 1 cent. A-B and C-D each cost 0 cents / 1 minute; cheap B-C costs 1 / 0 with capacity 1; fallback B-C costs 10 / 0; direct A-D costs 3 / 1. P2's deadline is zero, excluding its B-A-D-C bypass. Greedy spends cheap B-C on P1's multihop route and pays 11 total. The optimum uses direct A-D for P1 and cheap B-C for P2, costing 4. A separate focused test checks the principal consumed on every rail of a selected multihop route.

### Independent verification and iterations

The batch oracle in `tests/support/batch_oracle.rs` enumerates bounded walks by length (including cycles), materializes the complete Cartesian product, and checks capacity only on complete assignments. It uses neither production path generation, ranking nor pruning helpers. Tests independently recompute route continuity, membership, availability, ceilings, deadlines, principal usage, fees and latency from returned witnesses, and compare the entire canonical rank. Bounded walks up to `n-1` hops contain an optimum by cycle removal even though the oracle also explores cyclic witnesses.

Two deterministic sweeps make **6,342 comparisons**: 2,592 triangle configurations/batches/deadlines, plus 3,750 overlapping shared-service configurations with parallel rails, opposing directions, unequal principals, zero budgets, ceilings, deadlines and zero-cost cycles. Focused tests cover the greedy counterexamples, empty/invalid batches, external instructions, input-order ties, unchanged inputs and demo data, and totals beyond `u64` principal/fees and `u32` latency.

The initial independent/greedy baselines fail the hand-enumerated capacity scenarios; those counterexamples require retaining alternatives for joint selection. The exact search passed both oracle sweeps before adding the remaining-fee bound. Prefix-only fee pruning took over a minute on the unchanged 12-payment demo and was stopped. After adding the bound, the full focused batch suite including that demo completed in roughly 0.04 seconds on this machine, and the same oracle sweeps still passed. This timing is a local observation, not a scalability guarantee. No optimizer correctness counterexample was found by these sweeps.

Final batch verification on 2026-09-06: all **56 tests**, including both documentation examples, passed with `--locked --offline`. Formatting and Clippy with warnings denied passed, and both executable routing examples asserted their expected results. The unchanged CLI and TestBackend rendering tests passed in the same run. No TUI or terminal lifecycle code changed, so no new PTY lifecycle run was needed.
