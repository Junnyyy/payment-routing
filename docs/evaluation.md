# Reproducible strategy evaluation

`evaluation::evaluate` runs each named strategy in an independent simulator, from
identical world inputs and seeds. `World` owns the scenario; only the routing
strategy and reoptimization policy differ. Search limits are part of each
`Strategy`. All inputs and payment outcomes remain in the returned `Evaluation`.
This is a finite synthetic USD cohort experiment, not real payment execution.

## Information available at decisions

The evaluation driver alone owns SplitMix64 state and the surprise-event list.
At each integer minute it samples demand once and passes the same revealed
instructions to every run, including instructions rejected by admission limits.
Only due rail updates enter simulators. They contain no future disruptions; the
ordinary simulator also removes future disruptions from its effective planner
view. Planners retain the known recurring service calendar and current conditions.
They do not receive the evaluation horizon, subsequent instructions, future
surprise updates, another strategy's state, or evaluation results. Existing
strategies are compiled enum variants, not arbitrary external callbacks.

The shared sampler preserves the existing four-draw sequence (probability, flow,
amount, SLA), including failed probability trials. Demand is independent of
routing, overload, strategy order and fees. Endogenous events such as departures
and rejections are expected to differ; exogenous instructions and due changes
are identical. Ordinary simulation sampling remains transactional with tick state.

## Horizon, failures and replay

Generate at minutes `[0, arrival_minutes)`. Then process exactly `drain_minutes`
more ticks without new instructions; due surprises still apply. The world does
not announce that admission stopped. Every strategy has the same observation
window, including runs that finish early. A positive arrival horizon is required.
Zero demand is valid. Choose drainage long enough for the cohort's deadlines and
outstanding in-flight hops; longer drainage does not erase an SLA miss.

Incomplete work at the end is **censored**, with pending count/volume and already
known SLA failures retained. It is neither completed nor silently called failed.
Censored runs have no score. Runtime errors retain their committed metrics,
processed minute and error; they have no score either. Input validation covers
all worlds and strategies before execution. Invalid input or a broken evaluation
accounting invariant fails the evaluation rather than returning plausible scores.

Replay verification defaults on: every tick is repeated in an independent twin,
comparing the complete ordered event batch and full simulator state. Repeated
errors must also match. A mismatch is an explicit error, never a successful
verification. This is deterministic model verification; host timing is excluded.

## Objective definitions

Every denominator includes all generated instructions, including overload rejects.
On-time count/volume is completed minus completed-late count/volume. SLA failures
count rejection or a missed inclusive deadline once, even while a final hop drains.
Expired, rejected, late-completed and pending outcomes remain separate.
`never_routed_expired` means an observed expiry without any accepted route; bounded
search failure does not certify that no feasible route existed.

Fees are all actual departed-hop fees, including sunk costs of failed work.
Reservations cost nothing. Completed elapsed time includes waiting and late
completions. Completed latency percentiles use nearest rank, include late work,
and are undefined for an empty completed cohort. Departed hops/principal measure
work, not end-to-end throughput. Completed principal counts each payment once.
Opening balances remain descriptive; no liquidity objective is invented.

For complete runs, minimize this explicit lexicographic score:

1. Generated minus on-time count.
2. Generated volume minus on-time volume.
3. Generated minus completed count (including late completions).
4. Generated volume minus completed volume.
5. Actual fees in cents.
6. Sum of elapsed minutes for completed payments.
7. Actual departed hops.
8. Changed assignments after disruptions.

This prioritizes service before cost and charges failed work. It is an evaluation
preference, not the simulator's route-local ordering or a monetary penalty model.
Payment identity can differ even at equal count/volume. Such comparisons are policy
tradeoffs, not matched-cohort fee optimality gaps. Raw component metrics remain
available so a different business preference can be applied explicitly.

Queue counts are sampled after each tick, excluding in-flight work. Queue
payment-minutes sum those samples; peak queue can be zero during in-flight SLA
failures. Churn is reported with its assignment-comparison denominator, plus route
changes, retimings and withdrawals. Search diagnostics count attempts, including
retries/discarded candidates, not unique unroutable payments.

## Dependencies

Detected Rust/Cargo 1.97.1, edition 2024, Ratatui 0.30.0 and Crossterm 0.29.0 from
the installed compiler and locked dependency graph. Evaluation adds no dependency
and does not use terminal APIs. Context7 exposes the stable standard-library
index, not a 1.97.1 snapshot; APIs are verified by compilation on that toolchain.
References: [BTreeMap ordered iteration](https://doc.rust-lang.org/stable/std/collections/btree_map/struct.BTreeMap.html#method.iter),
[checked addition](https://doc.rust-lang.org/stable/std/primitive.u128.html#method.checked_add),
and [write! / writeln!](https://doc.rust-lang.org/stable/std/fmt/index.html#write--writeln).
