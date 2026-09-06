# Payment-routing benchmark report

Measured 2026-09-06 on an Apple M5 Pro (18 logical CPUs), macOS 26.6.2 arm64,
Rust 1.97.1 / Cargo 1.97.1. All workloads are deterministic, synthetic USD.

**Ambiguity, candidate storage, and contention determine the measured frontier.**
The batch solver handled 2,048 payments with distinct route costs in 1.40 ms,
but 27 payments with two tied routes exceeded two seconds. A single dense batch
payment crossed the memory guard at eleven institutions. The online simulator
sustained its configured capacity under load, yet could miss half its deadlines
when a pinned path encountered a future closed service.

The optimizer's decisions, candidate enumeration, pruning and tie breaks are
unchanged. Production edits only add counters behind the disabled-by-default
`search-stats` feature. No heuristic cutoff or substitute solver was introduced.

## Evidence and interpretation

There are **223 case configurations across three investigation rounds and 728
fresh worker observations**, including repeated configurations between rounds:

| Round | Configurations | Ordinary timing repeats | Counter runs per case | Worker time limit |
|---|---:|---:|---:|---:|
| [Pilot](../benchmarks/results/pilot/summary.csv) | 82 | 1 | 1 | 1 s |
| [Frontier](../benchmarks/results/frontier/summary.csv) | 115 | 3 | 1 | 2 s |
| [Diagnosis](../benchmarks/results/diagnosis/summary.csv) | 26 | 3 | 1 | 2 s |

Unless labeled pilot, times below are medians of three ordinary release runs.
Raw data retain min/max timing, CPU user/system time, peak worker RSS, faults,
context switches, input and result fingerprints, statuses and commands. Counter
runs use a separate binary. Instrumentation can itself exceed the time budget;
missing counters do not invalidate completed ordinary runs. For example, eight
payments with ten route choices completed in 1.56 s while its counter run timed out.

`timeout` records a killed worker, **not infeasibility**. Returned witnesses are
independently checked; small cases use the existing exhaustive oracles. Constructed
cases have asserted minimum fees or known infeasibility. Killed searches expose no
incumbent or partial counters through the current API. No optimality gap can be
assigned to such searches without a separate certificate.

The 512 MiB RSS stop target is sampled every 100 ms. It is **not a hard memory
limit**: fast allocation overshot it, with the largest killed worker reaching
1,156 MiB. RSS and CPU cover the complete worker, including fixture construction,
witness auditing and fingerprint serialization; they are not solver-only
allocation measurements. Solve time excludes those operations, except the
simulation's bounded per-tick metric sampling. Processes ran sequentially without
CPU pinning or machine-wide isolation. Tiny timings are illustrative; larger
boundary cases and repeat ranges carry more weight.

Four observations in the frontier log are an input-check error: the intended
100,000-event history case exceeded the harness's generic scale bound. They are
preserved and excluded from performance conclusions. The diagnosis rerun fixes
only that argument restriction and completes all four observations. Across the
remaining observations there were no worker errors, witness failures, or replay
mismatches. There were 91 timeouts and 12 sampled-RSS stops, counting both builds.

See the [measurement contract and investigation log](benchmark-method.md) for
precise metric semantics, supervision and window projections. Each results folder
contains `metadata.json`, `raw.jsonl`, `summary.json`, and `summary.csv`; metadata
records the source, lockfile and binary SHA-256, Git revision/state, platform and
complete manifest.

## Observed frontiers under the two-second worker budget

“Next tested failure” is a bracket on these fixture families, not a universal
maximum. Each failure below occurred in all three ordinary repetitions. Memory
stops use the sampled 512 MiB target described above.

| Approach / constructed structure | Largest tested completion near a boundary | Next tested failure | Cause supported by counters / controls |
|---|---|---|---|
| Single route, zero-fee clique | 11 institutions: 248 ms | 12: timeout | Zero fees defeat strict fee pruning; simple paths proliferate |
| Single route, isolated receiver beyond a clique | 10 connected + 1 isolated institutions: 202 ms | 11 connected + 1 isolated: timeout | No incumbent exists to support fee pruning |
| Single route, positive-fee clique, receiver listed last | 96 institutions: 1,878 ms | 128: timeout | Late discovery of the direct incumbent; 19.4 million path states at 96 |
| Batch, two tied direct routes | 26 payments: 1,420 ms | 27: timeout | Every one of 2^26 assignments survives to a tie comparison |
| Batch, cheap capacity covers half the demand | 28 payments: 1,587 ms | 29: timeout | Many equal-fee ways to allocate the scarce rail |
| Batch, total capacity one unit short | 29 payments: proves infeasible in 1,170 ms | 30: timeout | Large joint search even though aggregate arithmetic proves the fixture impossible |
| Batch, eight payments / increasing tied routes | 10 choices each: 1,560 ms | 11 choices: timeout | Cartesian assignment growth; nine choices already gives 43,046,721 complete assignments |
| Batch, one payment / one all-member rail | 10 institutions: 143 ms, 125 MiB | 11: memory stop | Materializes 109,601 candidate paths at ten institutions before selecting one |
| Scheduled batch, two tied minute-zero services | 26 payments: 1,755 ms | 27: timeout | Same binary assignment ambiguity |
| Scheduled batch, one-unit slots / unrestricted deadlines | 10 payments and 10 slots: 428 ms | 11: timeout | All 10! assignments have the same aggregate objective |
| Scheduled batch, deadlines processed from latest to earliest | 14 payments: 995 ms | 15: timeout | Many dead prefixes despite only one feasible complete assignment |
| Scheduled, one payment / three-hop chain / zero latency | 64 slots per rail: 63.4 ms, 207 MiB | 128 slots per rail: memory stop | 45,760 timed candidates at 64; every candidate also stores a vector over all budgets |
| Online simulation, zero-fee clique | 9 institutions: 231 ms for 100 ticks | 10: timeout for 100 ticks | Even roughly 75 generated payments inherit expensive static route search |

Sources: [frontier results](../benchmarks/results/frontier/summary.json) and
[diagnosis results](../benchmarks/results/diagnosis/summary.json). A two-second
whole-worker timeout is a censored observation, not a measured two-second solve.

The favorable controls show how conditional these limits are: 2,048 payments on
two distinct-cost direct rails took 1.40 ms; a 100-institution chain took 0.11 ms
(pilot). A two-minute deadline on a 32-institution zero-fee clique took 1.47 ms.
A 128-slot one-hop schedule took 0.095 ms (pilot), but expanding the same slot
choice over three hops eventually hit memory limits. At fixed nine institutions,
increasing pairwise connectivity from span two to full connectivity increased
single-route time from 0.057 to 6.47 ms. See `single-density` for that fixed-size
control; `directed_rail_choices` records the actual available pair/rail choices.

## Why the searches become expensive

**Equal objectives survive strict pruning.** With two tied direct routes and P
payments, the batch counter reports 2^(P+1)-1 assignment states and 2^P complete
assignments. At P=22 that is 8,388,607 states and 4,194,304 leaves, with zero bound
prunes. Removing a tie by changing the second fee lets the optimistic remaining
fee bound reject it. Consequently payment count alone is a poor predictor.

**The batch bound ignores secondary objectives.** The paired `*-latency-ties`
fixtures give two equal-fee routes different latency. At 22 payments, batch still
visits 8,388,607 states and takes 49.0 ms; scheduling visits 45 states and takes
0.035 ms. Scheduling's remaining bound includes elapsed time and hops. At 128
payments that scheduled control still takes 0.092 ms. These controls explain a
real difference between the implementations without modifying either one.

**Capacity-insensitive bounds leave many equivalent assignments.** Ten identical
payments and ten unit slots produce 3,628,800 complete assignments, 9,864,101
visited states and 52,488,910 capacity rejections. Every full permutation costs
ten cents and has the same summed elapsed time. The optimistic remaining score
assumes each unassigned payment can use its own earliest slot, so it understates
waiting caused by competition. Tighter capacity can also make search easier:
the one-unit-short batch rejects prefixes before reaching any complete assignment.
It nevertheless reaches a two-second limit at thirty payments.

**Deadline order matters independently of the set of deadlines.** With twelve
payments, deadlines 0..11 and one unit of service at each minute, processing the
most urgent IDs first visits thirteen assignment states and takes 0.042 ms.
Assigning the same deadline multiset to IDs in reverse order visits 1,151,915
states and takes 30.7 ms. There is only one feasible complete assignment in both
cases. The implementation orders payments by ID rather than deadline, so this is
an input-order sensitivity within its documented deterministic behavior.

**An incumbent discovered late can be costly even with positive fees.** The
participant-order control changes only the position of the receiver in a shared
rail's member list. At 96 institutions the same direct route takes 1,878 ms
(range 1,863–1,899) when the receiver is last and 0.553 ms (0.468–0.556) when it is
first. Path states fall from 19,387,032 to 8,932. At 128 institutions the original
order times out while receiver-first finishes in 0.869 ms. This is a measured
search-order effect; no new search ordering was shipped.

**Candidate storage can dominate before joint search.** Ten institutions on a
shared rail have sum(k=0..8) 8!/(8-k)! = 109,601 simple source-to-destination
paths. Batch retains them even with one payment and a uniquely cheapest direct
route. Similarly, a three-hop zero-latency chain with T departure minutes has
C(T+2,3) nondecreasing departure combinations. At T=64, 45,760 candidates each
carry a 195-element `u128` resource vector (three rails plus 192 slots), before
counting route strings or sorting overhead. This explains the memory growth;
path selection quality remains exact whenever search completes.

## Seeded simulation windows: burst structure changes the limit

The window fixture replays the real seeded simulator, warms up eight minutes,
and captures all generated payments during the next W minutes. It preserves
releases and relative/absolute deadlines. The contended variant has one rail,
one-minute latency, unit payments, an eight-minute SLA and two units of capacity
per departure. It builds a finite timetable through the last deadline.

Offline planning treats the captured cohort in isolation with full capacity:
carry-in and background demand are excluded. Static batch sums slot budgets and
removes waiting; independent static routing also ignores capacity. Those
projections are relaxations, and their very low solve times do not demonstrate
that the online workload is easy to execute.

| Window / seed | Payments | Exact scheduled time | Independently certified feasible fee | Certificate elapsed sum (upper bound) |
|---|---:|---:|---:|---:|
| 4 minutes / 42 | 11 | 3.70 ms | 11 cents | 20 minutes |
| 4 minutes / 99 | 11 | 0.871 ms | 11 cents | 18 minutes |
| 5 minutes / 42 | 14 | 193 ms | 14 cents | 28 minutes |
| 5 minutes / 99 | 14 | 45.1 ms | 14 cents | 26 minutes |
| 6 minutes / 0 | 12 | 0.106 ms | 12 cents | 15 minutes |
| 6 minutes / 42 | 17 | Timeout | 17 cents | 38 minutes |
| 6 minutes / 99 | 18 | Timeout | 18 cents | 40 minutes |
| 8 minutes / 0 | 18 | 397 ms | 18 cents | 30 minutes |
| 8 minutes / 42 | 24 | Timeout | 24 cents | 67 minutes |
| 8 minutes / 99 | 25 | Timeout | 25 cents | 74 minutes |

Source: [diagnosis observations](../benchmarks/results/diagnosis/summary.json).
All timeouts here occurred in every ordinary repeat. The FIFO certificates use
only the fixture's single rail and are independently audited against the complete
timetable. Every payment must pay that rail's one-cent fee, so the full witness
also establishes minimum fee. It does not establish the exact solver's complete
elapsed/hop/lexical optimum. No timeout is called infeasible.

Eighteen payments spread across eight release minutes (seed 0) finish, while
eighteen over six release minutes (seed 99) time out. Even equal payment counts
and equal total fees can hide different waiting and assignment ambiguity. The
four-minute cases both contain eleven payments and 88 candidates, yet visit
176,782 versus 42,729 assignment states. These differences motivated the separate
feasibility certificates and the five-minute boundary cases.

## Continuous execution: capacity, queues, and policy quality

The load sweep uses 3,000 ticks, seeds 0/42/99, one-unit payments, one-minute
settlement, a ten-unit per-minute budget, eight-minute SLA and 64 active records.
Each configured attempt generates with probability 0.75. Values below span the
three seeds; fees accrue only on actual departures.

| Attempts per minute | Expected offered payments/minute | Observed completions/minute | SLA failure fraction | Mean end-of-tick queue | p95 queue |
|---:|---:|---:|---:|---:|---:|
| 13 | 9.75 | 9.711–9.760 | 0% | 3.53–3.98 | 12–13 |
| 14 | 10.5 | 9.995–9.996 | 4.26–4.66% | 50.14–51.67 | 54 |
| 16 | 12 | 9.996–9.997 | 16.32–16.56% | 53.56–53.65 | 54 |
| 32 | 24 | 9.997 | 58.20–58.24% | 53.97–53.98 | 54 |

Source: [frontier simulation results](../benchmarks/results/frontier/summary.json).
The apparent throughput plateau is the configured service budget, while rejection
absorbs excess offered work. Ten in-flight payments plus 54 waiting records can
fill the active limit. At sixteen attempts/seed 42, 3,000 ticks take 133 ms
(about 22,554 modeled ticks per wall second). That host processing rate does not
measure a real payment rail's throughput. Active records at the observation end
are unfinished, not counted as completed or failed preemptively.

Shorter SLAs can reduce queue residence and host work while preserving the same
capacity plateau. In the pilot at sixteen attempts/minute, a two-minute SLA
causes 1,981 expirations and no overload rejections; an eight-minute SLA causes
1,940 overload rejections and no expirations. Both complete 9,990 payments in
1,000 ticks. Periodic closures also reduce completed throughput: opening for only
one of every 2/8/32 minutes produces 4,996/1,246/316 completions and roughly
16%/78%/94% SLA failure in the pilot. These are represented recurring service
windows, not real-world calendar behavior.

**Zero queue is not sufficient evidence of service quality.** The pinned-path
trap opens its cheap downstream rail every other minute. The policy chooses the
two-hop path while it is open, reaches the intermediate institution after it
closes, and expires at the SLA. The direct rail remains available throughout.

| Same 1,000-tick seed-42 arrivals | Completed on time | Expired | Queue peak after ticks | Total actual fees |
|---|---:|---:|---:|---:|
| Cheap multihop option plus direct fallback | 376 / 759 | 383 | 0 | 2,263 cents |
| Direct-only control | 759 / 759 | 0 | 0 | 3,795 cents |

The lower total fee in the first row accompanies lost completions and wasted
partial-hop fees. Including those fees, cost per completed payment is 6.02 cents
versus 5 cents in the control. This diagnoses the policy's lack of future-window
prediction; it is not an optimality-gap claim between equal completed workloads.

**Bounded state still has a cost.** With service closed and SLA extended to
10,000 minutes, increasing the active limit 8 → 64 → 256 → 1,024 increases the
1,000-tick cost 6.07 → 37.2 → 148 → 532 ms. Queue age reaches 999 minutes and
completions remain zero. Failed routing retries track queue residence: 938,767
path visits at the 1,024 limit. Rejection keeps retained work bounded but does
not make it free to scan, clone transactionally, validate and retry.

With 10,000 ticks and the same successful workload, increasing retained history
from 2 to 4,096 to 100,000 events gives 87.1 / 89.7 / 94.5 ms and approximately
3.0 / 4.8 / 49.3 MiB peak worker RSS. The final number includes full-state
fingerprinting, so it cannot be assigned entirely to live simulator storage.
All three preserve identical aggregate execution metrics. Persistent completed
payment history is not introduced.

## Objective checks and retained correctness

The adversarial batch trap has an exact four-cent optimum versus an eleven-cent
FIFO greedy allocation (2.75 times the fee). Removing the expensive fallback
makes greedy report no full allocation while the exact four-cent plan remains
feasible. The non-FIFO timetable fixture requires a later, faster departure to
make the connection and costs four cents. These are independently enumerated
checks, not favorable random anecdotes. Unchanged demo batches cost sixty cents.

All existing correctness suites remain intact: routing, static batch and scheduled
bounded-walk/product oracles; lexical ties and overflow checks; simulation replay,
event accounting, conservation and long stress runs; CLI and terminal rendering.
New tests cover benchmark witnesses, full oracle ranks on small constructed
cases, stable windows, independent certificates, participant-order answer
invariance, the pinned-path control, and exact thread-local search counters.

Final verification passed: `cargo test --locked` (102 tests including doctests),
`cargo test --locked --features search-stats` (103), Clippy across all targets and
features with warnings denied, formatting and diff checks, and three Python
supervisor tests covering exit status, timeout, RSS stop/reaping and replay checks.
The documented smoke command also completed eight cases × four observations,
with matching results across repeats and builds. Those 32 validation observations
are separate from the 728 retained research observations above. No optimizer-quality
tuning was performed in response to these measurements.

## Reproduce and extend

```sh
python3 scripts/benchmark.py --manifest benchmarks/suites/pilot.json \
  --output /tmp/routing-pilot --repeats 1 --timeout 1 --rss-mib 512
python3 scripts/benchmark.py --manifest benchmarks/suites/frontier.json \
  --output /tmp/routing-frontier --repeats 3 --timeout 2 --rss-mib 512
python3 scripts/benchmark.py --manifest benchmarks/suites/diagnosis.json \
  --output /tmp/routing-diagnosis --repeats 3 --timeout 2 --rss-mib 512
```

Use a new output directory per run. Running the current frontier manifest uses
the corrected history bound; the historical error is reproducible at its recorded
revision. Frontier and diagnosis were measured from clean source commits. The
pilot had two uncommitted harness fixes; its
[working-tree patch](../benchmarks/results/pilot/working-tree.patch) was reconstructed
and checked against the originally recorded combined source SHA-256. Apply it to
the pilot's recorded revision for exact source recovery. Worker commands and
hashes in each raw/metadata file identify historical code. Add a case to a manifest to vary a represented dimension; the
worker's `--dump` option prints full inputs, returned witnesses and available
window certificates. The full fixture catalog is in
[benchmarks/README.md](../benchmarks/README.md).

The next useful measurements would isolate stronger capacity-aware lower-bound
opportunities, candidate-vector storage costs, and workload ordering on additional
fixed topologies. These are diagnosis directions, not changes made in this Goal.
This suite does not model liquidity reservation, real rail calendars, FX, external
provider latency, production traffic distributions or arbitrary-topology guarantees.
