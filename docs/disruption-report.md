# Disruption and stability comparisons

All fixtures are synthetic USD. The paired candidates start at minute 1 from the
same entire existing cohort, after the change and before new arrivals. They keep
executed/in-flight prefixes fixed. Later arrival traffic still runs; reported
actual outcomes/fees count only those original payment IDs. No cohorts are cherry
picked by completion. [Raw rows](../benchmarks/disruption-results.csv) include
seed, explicit policy, candidate coverage, remaining fees/time, churn denominator,
withdrawals, actual original-cohort outcomes and exact-reference status.

```sh
cargo run --locked --example disruptions
cargo test --locked --test disruptions
```

`benchmarks/disruptions.rs` defines the fixtures. `examples/disruptions.rs` asserts
exact comparisons and prints the CSV. These measure outcomes, not throughput or
solver runtimes. No external payments, solvers or provider calls are involved.

## Matched-cohort results

Fees below are cents. Churn includes route changes, departure retimings and plan
withdrawals, divided by previously planned eligible payments. An initial plan for
queued work is not churn. Elapsed minutes sum remaining time to completion across
planned payments. See the [complete metric/policy definition](disruptions.md).

| Disruption | Preserve | Full recomputation | Adaptive, zero allowances |
| --- | --- | --- | --- |
| Equal-cost rail returns; 24 waiting payments | 120 cents, 48 minutes, **0/24 churn** | 120 cents, 48 minutes, **24/24 churn** | Preserve; all 24 complete |
| Cheap rail returns; 24 expensive assignments | **2,400 cents**, 0/24 churn | **24 cents**, 24/24 churn | Recompute; all 24 complete |
| Direct service returns; 1 two-hop assignment | 1 cent, **2 hops**, 0/1 churn | 1 cent, **1 hop**, 1/1 churn | Recompute; explicit one-hop allowance preserves |
| Earlier service returns; 8 waiting payments | 40 cents, **56 minutes**, 0/8 churn | 40 cents, **8 minutes**, 8/8 churn | Recompute; all 8 complete |
| Flexible reservation blocks constrained queued payment | 1/2 planned, 0/1 churn; **one expires** | 2/2 planned, 1/1 churn; **both complete** | Recompute for coverage |
| Capacity drops from two units to one | 1/2 planned, 1/2 churn (one withdrawal) | 1/2 planned, 1/2 churn (a different withdrawal) | Preserve FIFO survivor; flag different served IDs |

In the cheap-rail return, the measured cost of zero churn is **2,376 cents**
($23.76) for that complete 24-payment cohort. Setting the explicit absolute fee
allowance to 2,376 cents chooses preservation; zero chooses recomputation. A fee
allowance alone does not permit extra elapsed time: the earlier-service case
still recomputes with the 2,376-cent allowance and a zero-minute allowance.
The direct-service case separately measures a one-hop stability cost at identical
fees/time: zero extra hops selects recomputation; an explicit one-hop allowance
keeps the two-hop assignment. Hop allowance, time allowance and fee allowance are
independent, with no implicit conversion between them.
Whether avoiding 24 assignment changes is worth $23.76 is an unresolved business
policy choice. Both outcomes are recorded; the library does not price churn or
embed a dollar weight for SLA failure.

The scarcity fixture uses seed 57: the existing flexible A→B payment has amount 1,
and queued C→B has amount 2. Old shared A→B capacity is 2 at minute 3. C→B also
needs a one-cent C→A prefix. Before the recovery, the bounded planner serves the
cheaper flexible payment; both cannot fit. A newly available A→B rail permits only
amount 1. Retaining the flexible assignment blocks C→B. Recompute moves A→B to the
new rail and reserves the old rail for C→B, serving both for four cents. Comparing
that four-cent full cohort to preservation's one-cent partial cohort as a fee gap
would be misleading; coverage and actual expiry are the relevant comparison.

The capacity-loss case exposes an identity/fairness tie: both candidates have the
same counts, fees and churn, but full recomputation withdraws the other payment.
Adaptive retains the preserve candidate's FIFO survivor and reports
`same_planned_cohort = false`. This rule is documented; it is not a hidden score.

## Exact comparisons and limits

For the entire four-payment equal-cost cohort, the independent exact scheduled
optimizer returns 20 cents, eight elapsed minutes and four hops. Both policies
match the fee/time/hop optimum, yet recomputation changes all four assignments.
For four expensive assignments, the exact result is four cents/eight minutes/four
hops; preservation costs 400 cents, a **396-cent stability premium**. The scarcity
cohort's exact complete optimum is four cents/four minutes/three hops. In the
capacity-loss case, exact enumeration proves that the complete two-payment cohort
is infeasible. The online bounded solver itself reports unplanned/unresolved.

The projection enumerates every service departure through these payments' actual
deadlines. It includes the complete existing cohort, and these fixtures have no
in-flight prefix or omitted carry-in reservations. Exact fee/elapsed/hop equality
is asserted in tests; lexical equality is not claimed. Larger 24/8-payment cases
compare with full **bounded** recomputation and are labeled `not-run` for exact
search. Their direct unconstrained rails also provide a hand-checkable per-payment
fee lower bound, but there is no general optimality certificate for arbitrary
networks or future online demand.

A separate two-candidate-budget test finds a more subtle failure: a newly opened
lexically earlier dead-end rail consumes the search budget before the old useful
rail is explored. Recompute becomes unresolved despite a valid known plan.
Preservation validates that witness directly and keeps service; adaptive chooses
its higher coverage. Truncation is retained in the report, never called
infeasibility. A zero-fee recovery test explicitly handles a zero optimum without
percentage division.

The inherited simple-witness constraint also applies across the immutable prefix:
replacement routes cannot return through an institution already visited. This
keeps retained paths bounded by institution count, but disruption can make such
backtracking useful. In the in-flight regression, a C→B replacement costs five
cents and arrives at minute 3; relaxing the prefix constraint would allow C→A→B
for two cents, arriving at minute 7 (still within its minute-10 deadline). Both
reoptimization policies use the same constraint. This three-cent/four-minute
tradeoff is a model limitation, **not** a measured cost of stability; no claim of
unrestricted dynamic-network optimality or physical infeasibility is made.

## Verification and iteration

The generated suite exercises 24 seeds × two strategies × three policies over 60
minutes: **144 scenarios and 8,640 independently audited event batches**. Each
batch also matches an observed run, and each final state matches a fresh restart.
Closures, recovery, capacity decreases/increases, unlimited transitions, zero
latency, deadlines, overload and small search limits are combined. State, principal,
fees, per-minute capacities, lifecycle and immutable prefixes are checked from
events independently of the production search. Reservation ledgers are rebuilt
from witnesses before every tick commits.

Across the 1,440 effective-change comparisons in that suite, preservation serves
fewer payments in **6**, and served identities differ in **41**. No comparison with
identical served IDs has higher preservation churn than recomputation in this
sample. These observations prioritize coverage loss and identity ties for review;
they do not prove a global minimum-churn policy or performance bound.

Focused regressions verify a capacity reduction that only retimes one of two
payments, closure during a final hop, repair from an in-flight receiver, prevention
of cycles through a fixed prefix, unrepairable intermediate expiry, recovery after
withdrawal, same-tick witness capture, one-time route acceptance/SLA accounting,
invalid and merged controls, no-op changes, restart, and rollback of effective
conditions, pending controls, metrics and event cursors on arithmetic exhaustion.
The existing exact-router/oracle, simulation replay and 100,000-tick stress cases
remain part of the full suite.

The console's disruption preset additionally runs in the actual PTY harness, with
both candidate rows visible at 80×18. All five presets retain terminal modes and
zero exits on the existing q/Esc/Ctrl-C paths. The library does not depend on
Ratatui; simulation interfaces carry controls, change events, selected witnesses
and assessments to both the console and headless examples.

Final validation: **142 tests passed** with all features, including existing exact
oracles and long-run stress tests. Strict Clippy, formatting and whitespace checks
passed. The CSV regenerates deterministically. Reproduce the complete checks:

```sh
cargo test --locked --offline --all-features
cargo clippy --locked --offline --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo build --locked --offline
python3 scripts/test_console.py
```
