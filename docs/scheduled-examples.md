# Scheduled batch optimization

`scheduling::optimize_schedule(&network, &timed_payments, &departures)` returns a
globally minimum-cost full routing and execution **plan** for a finite synthetic
timetable. It returns `Ok(None)` for infeasibility and an error for malformed
inputs. It never executes a transfer or changes balances, instructions or budgets.
The static routing APIs retain their existing behavior.

The [time model](time-model.md) defines the full contract and exactness
argument. Time uses integer minutes (`u64`) and a finite list of departure slots;
there is no clock loop, calendar, implicit schedule or chosen search horizon.

| Input | Meaning |
| --- | --- |
| `TimedPayment.earliest_execution_minute` | Arrival/release: no hop can depart before this minute. |
| `TimedPayment.deadline_minute` | Optional inclusive absolute final-arrival deadline. The wrapped payment's relative delivery budget also applies, including all waiting from release. |
| `RailDeparture.departure_minute` | One exact departure on its named rail. Unlisted minutes are closed. Duplicate rail/minute entries are invalid. |
| Slot `fee_cents`, `settlement_minutes` | Optional overrides of base fee/latency, fixed at departure. Later departures may arrive earlier. |
| Slot `capacity_cents` | Shared principal budget for that departure across all member pairs/directions; `None` is unlimited, zero is closed. |

Waiting is free at the sender and intermediaries. Same-minute forwarding is
allowed after zero-latency arrival. Slot budgets count departures, not in-flight
occupancy. Every hop must also satisfy static rail availability, amount ceilings
and whole-batch capacity; the latter never replenishes at a later time. Fees are
separate from principal. Missing timetables cannot silently use static routes.

Results report each hop's rail, endpoints, departure, arrival and effective fee,
plus all rail and slot usage, including zero usage. Payment assignments sort by
ID. Ties minimize total elapsed time from release, total hops, then lexical timed
hop sequences. Aggregates use `u128`; invalid overflowing timestamps are rejected.

Run the asserted counterexamples without terminal initialization:

```sh
cargo run --locked --example schedule_batch
```

### Counterexamples driving the implementation

**Delaying helps the batch.** Two 1-cent payments arrive at minute 0. P1 can arrive
by minute 1; urgent P2 must arrive by minute 0. Cheap costs 1 cent and has capacity
1 at both minutes 0 and 1. Fallback costs 10 with unlimited capacity at both times.
All latencies are zero. These are the eight deadline-eligible joint choices:

| P1 | P2 | Fee (cents) | Result |
| --- | --- | ---: | --- |
| cheap@0 | cheap@0 | 2 | Infeasible: slot usage 2 exceeds 1 |
| cheap@0 | fallback@0 | 11 | Greedy P1-first result |
| cheap@1 | cheap@0 | **2** | **Global optimum: delay P1** |
| cheap@1 | fallback@0 | 11 | Feasible |
| fallback@0 | cheap@0 | 11 | Feasible |
| fallback@0 | fallback@0 | 20 | Feasible |
| fallback@1 | cheap@0 | 11 | Feasible |
| fallback@1 | fallback@0 | 20 | Feasible |

Forbidding waiting raises the optimum to 11. Removing fallback makes greedy get
stuck, while the exact 2-cent solution survives. Adding a 1-cent whole-batch budget
to cheap makes the batch infeasible despite its two separately funded slots.

**Locally cheapest waiting can also hurt.** A rail has one-cent principal capacity
at minutes 0/1/2, with fees 3/1/10. P1 arrives at 0 and is due at 1. P2 arrives at 1
and is due at 2. Greedy delays P1 until the cheapest minute 1, forcing P2 to minute
2 for total cost 11. The optimum sends P1 at 0 and P2 at 1 for cost 4. The example
independently enumerates all four eligible assignments.

**Waiting can destroy a route.** AB departs at 0 for fee 4, or at 2 for fee 1; both
take one minute. BC departs only at 1, costs 1 and takes one minute. Waiting for
cheap AB misses BC. The exact route costs 5 and arrives at 2; a payment released at
2 is infeasible. Moving BC to minute 3 makes the delayed 2-cent route feasible at
deadline 4.

Further tests require a later but faster AB departure followed by an intermediary
wait, and an expensive early AB prefix that frees a later BC slot for a newly
arriving payment. These expose unsafe earliest-departure, cheapest-prefix and
independent-scheduling reductions.

### Exact verification and limits

Production enumerates feasible simple paths with every feasible departure choice,
then backtracks over joint assignments. Free waiting can replace a cycle while
retaining every downstream departure, never increasing cost or resource usage,
and reducing hops. This justifies simple paths even with nonconstant latency.
The sum of remaining unconstrained minimum `(fee, elapsed, hops)` scores gives an
optimistic bound; only strictly worse scores are pruned so lexical ties survive.

The separate oracle in `tests/support/scheduling_oracle.rs` expands bounded walks
by length, including cycles, and materializes the complete Cartesian product.
It checks deadlines only on completed walks and capacities only on complete
assignments. It shares no production search, pruning or ranking helpers. Returned
witnesses are independently checked for continuity, membership, timestamps,
release/deadline compliance, both budgets, fees and reported usage.

Two sweeps make **3,940 full-rank comparisons**: 2,916 triangle timetables and
arrival/deadline patterns, plus 1,024 overlapping/parallel service configurations
with cyclic walks and additional constraints. Focused tests cover the hand cases,
both deadline boundaries, zero-latency chains, shared capacities, wide totals,
input permutations, malformed inputs, empty batches and immutability. A sparse
slot at `u64::MAX` verifies that time gaps do not trigger tick-by-tick expansion.

Final scheduled verification on 2026-09-06: all **74 tests**, including three
compiled documentation examples and all 56 pre-existing tests, passed with
`--locked --offline`. Formatting, Clippy with warnings denied, and all three
executable routing/scheduling examples passed. Existing static oracle files,
domain algorithms, fixture data, dependencies, CLI and UI implementation are
unchanged. No terminal lifecycle code changed, so no new PTY run was needed.

The unchanged 12-payment demo is also tested with an explicit slot at minute zero
for each rail. Its exact plan sends every payment directly on ACH, costs 60 cents
and has summed elapsed time 17,280 minutes. A 5-cent lower bound per payment proves
optimality for this timetable; it does not establish performance for arbitrary
timetables. The focused scheduled suite, including this fixture, took roughly
0.02 seconds locally; the 3,940 oracle comparisons took roughly 0.21–0.25 seconds.

Search remains exponential in institution count, departure alternatives and batch
assignments. No fixture needed approximation or a reduced timetable, and no
cutoff, sampling or external solver is present. Infeasibility is always relative
to the supplied finite timetable. There is no modeled liquidity, netting, payment
splitting or real settlement.
