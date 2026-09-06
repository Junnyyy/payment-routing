# Scalable routing results

The implemented strategy is `RoutingStrategy::Reserved { limits }` in the
continuous simulator and `scalable::plan_schedule` for finite timetables. It uses
bounded label search, deterministic allocation orders, limited local repair and
complete capacity reservations. The original exact static, batch and scheduled
optimizers remain independent and unchanged.

The median fee-gap target is met: **0% across all 199 certified-feasible schedule
cases**, with full allocation in every case. **The worst gap is 70.73%**, a
four-payment coordination trap retained in the final fresh-seed results. All 64
exact same-cohort online comparisons have zero gap. These measured results are
not a universal approximation guarantee.

## Feasibility and objective contract

A returned finite plan serves every supplied payment and obeys membership,
availability, transaction ceilings, release times, inclusive delivery deadlines,
per-departure capacity and static batch capacity. An unresolved heuristic result
is **not infeasibility**. No price improvement can authorize a constraint violation.
The simulator uses recurring budgets instead of static batch budgets and includes
earlier commitments in every decision. Accepted paths have reserved departure
timestamps and must complete by their deadlines; unplanned arrivals may queue,
expire or be rejected at the configured active limit. Opening balances stay
purely descriptive. Fees are charged on actual departure, not on reservation.

Fee gap means `100 * (heuristic fee - optimum fee) / optimum fee` on the identical,
fully allocated workload. A zero-fee plan over a zero optimum has zero gap; positive
fee over a zero optimum has infinite gap. Missing full allocations are counted as
infinite when computing a coverage-inclusive median, and never assigned a finite
gap. Exact-infeasible cases are separately reported. Elapsed time, hops and full
witness agreement are recorded independently; zero fee gap does not assert the
exact optimizer's full secondary/lexical optimum.

## Measurements

The measured source, exact toolchain, platform, lockfile, binary hashes and complete
manifest are in each result folder's `metadata.json`. Ordinary release timings
use three sequential fresh workers; a separate `search-stats` build checks
instrumentation-independent results. All monetary inputs are synthetic USD.
Final workers have a ten-second timeout and a sampled 512-MiB RSS guard. The
watchdog limit covers the entire worker and can overshoot; it is not a solver
memory cap. Earlier experiments use two-second limits, and retain every censored
run. CPU/RSS include fixture construction, auditing and serialization. Reported
solve time excludes those phases for schedules. Online solve time includes
step/report/queue sampling; tiny `sim-quality` cases additionally capture events
and run the independent event accountant. Their exact oracle runs afterward and
has a separate `oracle_ns` field. Simulation p95 ticks aggregate the median of
three per-worker p95 samples; raw logs also retain each maximum.

The final run is recorded in [raw observations](../benchmarks/results/strategy-final/raw.jsonl),
[full summary](../benchmarks/results/strategy-final/summary.json) and
[every schedule gap](../benchmarks/results/strategy-final/gaps.json).
No expensive validation jobs ran concurrently with final timing workers.

## Final results (source 6d6a28f)

Measured on Apple M5 Pro / macOS with Rust 1.97.1 and locked dependencies. There
are **620 configurations and 2,480 fresh worker observations**, with no errors,
timeouts, sampled-RSS stops, feasibility audit failures or replay mismatches in
this final ten-second-guard run. All 33 exact-infeasible schedules remain
`unresolved` under the heuristic, rather than being mislabeled feasible.

Of 199 known feasible schedule cases, 189 have exact-optimizer results and ten
have independent analytical fee certificates. 195 have zero fee gap; four do not.
Every zero-gap case with an exact reference also matches its complete witness,
including elapsed/hop/lexical ties. This agreement is measured, not promised by
the heuristic API. All nonzero cases are five-payment `schedule-mixed` fixtures:

| Seed | Heuristic fee | Exact fee | Fee gap | Heuristic time | Exact time |
|---|---:|---:|---:|---:|---:|
| 65 | 116 cents | 112 cents | 3.5714% | 0.334 ms | 0.308 ms |
| 123 | 215 cents | 213 cents | 0.9390% | 0.201 ms | 0.439 ms |
| 148 | 16 cents | 15 cents | 6.6667% | 0.319 ms | 0.485 ms |
| 155 | 210 cents | 123 cents | **70.7317%** | 0.232 ms | 0.254 ms |

None of these four hits a path-search limit. They expose allocation-neighborhood
limits, not relaxed feasibility. In seed 155, the heuristic assigns two one-cent
principals to the rail with a two-cent whole-batch capacity. A two-cent payment
then uses the 100-cent fallback. The optimum reroutes those two smaller payments
and moves a third payment to another departure, freeing both scarce principal
units for the large payment. Four assignments change together. A helpful
intermediate move worsens fee or a secondary objective, so monotonically improving
single/pair trials cannot perform that exchange. This case is retained rather
than adding an exact fallback or tuning more orders to the final holdout.
The next quality experiment would be a budgeted larger neighborhood or temporary
resource prices; it must be measured against the repeated-decision cost before
replacing this implementation.

The earlier exact scheduled frontier failed at 27 tied payments, eleven one-unit
slots, fifteen reverse-ordered deadlines, and 128 departures on a three-hop
schedule. Corresponding larger final heuristic cases:

| Feasible workload | Fee gap | Median solve | Peak whole-worker RSS |
|---|---:|---:|---:|
| 2,048 payments, two tied routes | 0% (certificate) | 2.13 ms | 6.00 MiB |
| 512 payments competing for 512 unit slots | 0% (certificate) | 24.78 ms | 3.91 MiB |
| 512 reverse-ordered deadlines | 0% (certificate) | 7.54 ms | 3.83 MiB |
| One payment, three hops, 1,024 slots per rail | 0% (certificate) | 7.59 ms | 3.72 MiB |

These certificates establish minimum fee and feasibility, not secondary rank.
Timings are workload/machine measurements, not general solver capacity promises.

The unchanged exact static policy needs 2,147 ms for 1,000 ticks on the
nine-institution zero-fee clique; the reserved policy takes 1.92 ms for the same
759 completed arrivals and zero fee. It also processes a 512-institution clique
for 10,000 ticks in **511–517 ms** across seeds 0/42/99: about **19,300–19,600 ticks
per second**, 0.074–0.076 ms p95 ticks, 4.17 MiB maximum worker RSS, no queue and no
SLA failures. Zero nonnegative fees on fully completed cohorts give a 0% fee gap
for these dense cases as well.

The multihop mesh is the remaining runtime bottleneck. These longer runs use
1,000 ticks and seeds 0/42/99; p95 is the median per-worker p95, then the range
across seeds. Global online optimality gaps are **unknown** for this table.

| Institutions | Time for 1,000 ticks | p95 tick | Mean queue | p95 queue | SLA failures |
|---|---:|---:|---:|---:|---:|
| 16 | 1.446–1.458 s | 2.04–2.10 ms | 11.01–11.27 | 36 | 0 |
| 32 | 3.631–3.761 s | 7.51–8.05 ms | 10.38–10.49 | 44–45 | 0 |
| 64 | 1.205–1.238 s | 1.89–1.95 ms | 0 | 0 | 0 |
| 128 | 2.026–2.095 s | 3.20–3.41 ms | 0 | 0 | 0 |

The worst observed individual tick is 13.14 ms; maximum worker RSS is 4.47 MiB.
The 32-institution class achieves 266–275 modeled ticks/second. Larger meshes
need the expensive direct fallback to satisfy their eight-minute SLA, explaining
the non-monotonic runtime and zero waiting queues. At seed 42, the 32-institution
run generates 6,002 payments, completes 5,962, and leaves 40 active with reserved
feasible continuations. Cutoffs remain visible: 50 search attempts in that run,
and 2,269 in the 128-institution run. These counts include unsuccessful order/
repair trials, not that many dropped or invalid payments.

The pinned-window trap improves from 376 on-time completions and 383 expirations
to all 759 completed on time. Actual fees rise from 2,263 to 2,667 cents because
lost payments are now served; this is not a gap between equal completed workloads.
In sustained overload (16 attempts/minute, budget ten, 10,000 ticks), the reserved
policy completes 99,989–99,990 payments in 324–327 ms, with 19,779–19,995 explicit
SLA failures and a mean queue of 53.75–53.79. Those failures reflect offered work
above the configured service/admission capacity, never illegal departures.

The [untimed six-institution example comparison](../benchmarks/results/demo-comparison/README.md)
also exposes an online completion tradeoff. From the same 19,520 generated
payments, `Reserved` completes **12,409**, expires 7,105 and leaves six active;
`CheapestStatic` completes **12,849**, expires 6,665 and leaves six active. Both
have zero late completions. Reserved actual fees are 3,242,640 cents versus
4,694,565 cents, completed elapsed time is 58,388 versus 43,238 minutes, and peak
active count is 18 versus 15. Those lower fees do not establish a better complete
workload or a finite gap. Reserving cheaper future departures can make capacity
unavailable to later arrivals; the policy does not universally improve online
completion throughput. These example outputs pass replay checks but have no exact
global oracle and are not included in the 620 timed configurations.

## Investigation and failed approaches

The complete iteration log is in [the strategy contract](scalable-routing.md).
FIFO routing reached a 0% median while leaving three known-feasible cases
unresolved and charging up to 381.82% above optimum. Multiple allocation orders
fixed those cases. A broader mixed-cost suite then exposed a 67.72% case (213
versus 127 cents), corrected by pair repair. Unfinished-hop lower bounds reduced
dense-network search, and a positive-latency, deadline-indexed fee bound addressed
sparse multihop exploration. A subsequent untouched seed exposed stale route
choices after a later repair; spending the remaining repair budget on single-route
repricing corrected its 4.42% gap without increasing the work budget.

All earlier results remain under `benchmarks/results/strategy-*`; corrected cases
were not deleted or relabeled. Candidate limits, closed queues and exact-infeasible
fixtures remain visible. No exact solution has been substituted into the heuristic.

## Remaining limits

This is a bounded heuristic, not an approximation-ratio guarantee. Per-node label
limits can remove a prefix needed for an optimal or feasible solution. Total label,
expansion and candidate limits can end search early, with explicit diagnostics.
Dominance is conservative about visited nodes and constrained resource use;
limits are still deliberate approximations. Rejected search attempts and terminal
SLA failures are distinct counters. Finite calendar validation still scans input
pairs for uniqueness and can dominate very large fixtures.

Local repair has a fixed budget and only handles groups of at most 16 pending
requests. It does not enumerate three-way exchanges or accept temporarily worse
allocations to escape a local minimum. Large batches use at most four order trials.
Online decisions reserve future capacity irrevocably relative to later ticks;
future arrivals are unknown. The mesh and long-run load cohorts have no known
global online optimum, so their actual fees and completions are reported without
an invented gap. A lower-cost run that leaves more work active is not automatically
a better completed workload. End-of-run active work is censored, never silently
counted as completed or failed.

The dynamic fee bound only applies to recurring constant-fee, strictly positive
latency services with at most 64 minutes remaining. Zero-latency services, long
SLAs and finite fee/latency overrides use the general bounded search. No universal
throughput promise is made for arbitrary graph size, active limits or deadlines.

## Reproduce

```sh
cargo run --locked --example simulate -- 10000 42 reserved
python3 scripts/benchmark.py --manifest benchmarks/suites/strategy-final.json --output /tmp/payment-routing-final --repeats 3 --timeout 10 --rss-mib 512
python3 scripts/strategy_gaps.py /tmp/payment-routing-final
```

Output directories must be new. On macOS, the benchmark supervisor requires host
access for `ps` and worker CPU/RSS accounting. Each raw observation includes the
exact replay command; append `--dump` to include its fixture and returned witness.
The example asserts deterministic manual/paced/restart replay and prints actual
fees, SLA outcomes and bounded search diagnostics. Omitting `reserved` selects the
original static policy.


## Final verification

`cargo test --locked` passes 115 tests including doctests; the `search-stats`
variant passes 116. Clippy across all targets/features with warnings denied,
formatting and eight Python supervisor/gap tests pass. The 3,940 independent
batch scenarios audit each returned feasible witness; their 7,880 singleton
comparisons also match the independent minimum fee. Reserved event accounting
covers 128 scenarios / 10,240 event batches with paired and grouped replay.
The separate 100,000-tick overload case retains bounded active payments,
reservations and history, with zero late completions. Numeric failures roll back
RNG, reservations, metrics and events together, including at the u64 time boundary.
The unchanged exact source modules have no diff from the starting revision.
No terminal lifecycle code, dependency, external service or live payment was added.
