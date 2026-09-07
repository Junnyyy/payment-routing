# Bounded routing experiment

The objective is feasible, inexpensive repeated decisions with bounded search
work. The exact static, batch and scheduled optimizers remain independent oracles.
The primary quality metric is fee gap `100 * (fee - optimum) / optimum` on an
identical, fully served workload. Zero optimum and zero fee have zero gap;
positive fee over zero optimum has infinite gap. Unserved work has no finite
gap. Report coverage, every failure, secondary elapsed/hop scores and worst gaps
alongside the median; never compare costs of differently completed cohorts.

## Initial experiment

Compile institution/rail membership once. Search labels contain a simple path,
arrival, fee and sparse resource use. A priority queue explores inexpensive
prefixes, with conservative dominance only when an earlier prefix also uses no
more resources and visits a subset of institutions. Explicit per-node label,
expansion and candidate-examination limits bound difficult searches. Losing a
label or exhausting work is recorded; no failed heuristic declares infeasibility.

Finite timetable planning reserves both departure and batch budgets. Continuous
execution uses the same search with sparse recurring-window arithmetic and
per-minute reservations, without expanding every minute in the SLA. A complete
route is reserved before acceptance, and execution honors its departure times.
Existing reservations include carry-in demand. Admission failure stays queued
until expiry; accepted work must meet its deadline. Fees accrue only at departure.

Measure a FIFO baseline first. Choose the next ordering/repair experiment from
its worst measured class. Preserve all rounds and censored exact runs. Require
independent witness/event accounting, replay, capacity and deadline checks before
calling any measured solution feasible. Online quality against a retrospective
oracle must use the identical generated cohort and calendar, explicitly separate
from isolated windows and full-information planning.

## Environment and API references

Detected Cargo.toml/Cargo.lock: Rust edition 2024, Ratatui 0.30.0, Crossterm
0.29.0; installed Rust 1.97.1 and Python 3.9.6. No new dependencies or terminal
APIs. Context7 provides rolling std documentation rather than a 1.97.1 snapshot;
compile and test on that exact installed toolchain. Retrieved sections:
[BinaryHeap: Min-heap](https://doc.rust-lang.org/stable/std/collections/struct.BinaryHeap.html#min-heap),
and [Entry::or_default](https://github.com/rust-lang/rust/blob/main/library/std/src/collections/hash/map.rs).

## Round 1: bounded FIFO (source 4c4a9d4)

[Raw results](../benchmarks/results/strategy-fifo/raw.jsonl) and
[every gap](../benchmarks/results/strategy-fifo/gaps.json). Three ordinary release
repeats plus a separate counter build, two-second whole-worker/512-MiB sampled
limits. Of 35 cases with a certified minimum fee, 32 returned full plans. The
coverage-inclusive median (unresolved counted as infinite) is 0%, but maximum
finite gap is 381.82% (128-payment knapsack); the 8-payment version is 200% and
the two-payment urgency trap is 175%. Reverse-deadline cases at 12 and 128 plus
the no-fallback urgency trap remain unresolved despite known feasible solutions.
The 48 seeded mixed cases include 33 exact-infeasible cases, retained explicitly.

The reserved policy eliminates the pinned-path SLA loss. At 1,000 ticks it takes
1.65 ms versus 2.18 ms for the static policy. Dense zero-fee online routing takes
5.63 / 67.06 / 1126.48 ms at 9 / 32 / 128 institutions; 256 is censored in every
repeat. The static policy at nine institutions is also censored for 1,000 ticks.
These timings are host processing, not real rail capacity.

Next experiment: independent FIFO, deadline-first, amount-first and reverse
allocation orders, keeping the best complete result. This directly targets both
unserved urgent payments and over-allocation to large low-benefit payments.
Retain the baseline failures. Subsequently target the avoidable exploration of
prefixes already unable to improve a direct incumbent in dense zero-fee graphs.

## Round 2: allocation orders (source 8216a82)

[Every gap](../benchmarks/results/strategy-orders/gaps.json). All 35 certified
feasible cases now have full plans and zero fee gap. All 33 exact-infeasible mixed
cases remain unresolved, with no false feasibility claim. A four-order upper
bound adds cost: 128-slot contention takes 1.79 ms instead of 0.96 ms; a closed
256-record backlog takes 136 ms/1,000 ticks instead of 58 ms. Queue/completion/fee
outcomes for the original online families are unchanged between these rounds.

Dense online search remains the worst runtime class: 128 institutions takes
1,111 ms/1,000 ticks, and 256 is censored. The next change uses `prefix hops + 1`
as a lower bound for an unfinished path. This safely prunes equal-fee/equal-time
prefixes which cannot match an incumbent's hop count. It changes no constraints.
Expand validation to 3,940 independent small batch witnesses plus 7,880 single
payment oracle comparisons, 64 guaranteed-feasible mixed-cost cases and 16 small
online cohorts compared with the exact schedule for those identical arrivals.
The online cohort has zero SLA and an unlimited expensive fallback, so every
payment completes within its release tick; no carry-in or unfinished work is
omitted from that comparison. These are narrow online quality cases, not claims
of clairvoyant performance with future arrivals.

## Round 3: tighter bound and broader quality (source 326abc4)

[Every gap](../benchmarks/results/strategy-refined/gaps.json). All 103 certified
feasible schedules returned full plans. Median gap is 0%; seed 40 / five mixed
payments has 1.80% gap and seed 60 has 67.72% (213 versus 127 cents). The latter
allocates three of four cheap principal units to two payments, forcing a
large payment onto the 100-cent fallback. The optimum moves one small payment
onto a 14-cent multihop route, freeing two units for the large payment. Four
whole-batch orders miss this exchange. This is the next quality experiment:
try at most 16 pairs, each in both orders, against all other fixed reservations.
Only groups of at most 16 enter this repair pass; larger groups retain bounded
order trials. Earlier online commitments are fixed for ordinary admission. Disruptions explicitly
reconsider unexecuted suffixes through the separate [adaptive layer](disruptions.md).

Dense simulation now takes 14.69 / 28.03 / 56.52 ms per 1,000 ticks at 128 / 256 /
512 institutions. At 512 the observed p95 tick is 84.9 microseconds and peak
whole-worker RSS 3.4 MiB. The exact static-policy nine-institution control still
hits the two-second worker guard. Scheduled ties at 2,048 payments take 1.80 ms;
512 unit slots take 26.61 ms; a three-hop 1,024-slot timetable takes 8.66 ms.
All 16 exact same-cohort online comparisons have zero fee gap. Online output
contains the exact fee and gap for both the static and reserved policies.

## Round 4: pair repair (source 2000f6c)

[Every gap](../benchmarks/results/strategy-repair/gaps.json). All 103 certified
schedule cases and all 16 exact online cohorts have zero fee gap. The 67.72%
case falls from 213 to 127 cents. All previous failures remain in earlier logs.
Repair costs more on small heterogeneous batches; it is skipped for large
batches and equal-fee fully allocated groups. Ordinary admission never releases earlier online
commitments.

A new sparse mesh (two local service links per institution plus an expensive
shared fallback, periodic local closures, SLA eight minutes) is a more difficult
runtime class. At 100 ticks, 16 / 32 / 64 / 128 institutions take 363 / 1893 / 852 /
1732 ms. The 32-institution p95 tick is 26.8 ms with 6,561 truncated searches across
order and repair trials. No accepted work misses a deadline; global optimality
is unknown for these cohorts. Non-monotonic timing reflects the eight-minute
SLA: farther destinations make the expensive direct rail increasingly necessary.

Next experiment: for recurring services with strictly positive latencies and
remaining SLA at most 64 minutes, compute minimum fees in a time-indexed relaxed
network. This ignores aggregate competition and window closures, so it is an
admissible fee bound. Shared-rail relaxation uses the cheapest member value,
without materializing a clique. Use the bound for priority and pruning; skip it
for zero latency, long horizons and finite fee/latency overrides. Accepted
witnesses still undergo the original full calendar/resource checks.

## Round 5 and untouched-seed check (sources a14bc2c / c740bfe)

[Bound results](../benchmarks/results/strategy-bounds/gaps.json) keep all 103
certified fees optimal. On the sparse 32-institution mesh the bound cuts 100 ticks
from 1,893 to 410 ms and truncations from 6,561 to 2. Queue mean rises from 6.00 to
10.55 and end-of-window completions fall from 587 to 572, with zero SLA failures:
cheaper longer routes leave more in-flight work at the observation boundary.
Actual fees are 1,439 versus 14,016 cents; these are **not optimality gaps** because
completed cohorts differ and the global online optimum is unknown.

The [untouched-seed check](../benchmarks/results/strategy-holdout/gaps.json) finds
full plans for all 64 new feasible batches: median 0%, gaps 3.57% (seed 65), 4.42%
(seed 66), 0.94% (seed 123), and zero elsewhere. All 32 additional exact online
cohorts have zero gap. Long mesh runs complete at roughly 260–790 ticks/second;
the 32-institution p95 is about 8.4 ms. The 512-institution zero-fee simulation
processes 10,000 ticks in 537–547 ms across seeds 0/42/99. No source tests ran
concurrently with these final timing rounds; exploratory round 4 overlapped a
validation run, so its timing differences should not be attributed entirely to
the algorithm. Every round records raw repeat ranges and host resources.

Inspecting seed 66 shows a late pair repair frees a cheaper direct route for an
earlier assignment. Its stored eight-cent route can now be replaced by a
three-cent route without disturbing anything else. The final experiment spends
only unused repair trials on one single-route repricing sweep. It changes 118 to
113 cents without raising the configured repair budget. Seeds 65 and 123 involve
coordinated allocations rather than this stale local choice. Retain them and add
32 new schedule seeds plus 16 new online seeds for final reporting.

## Final stopping point (functional source 6d6a28f)

The [final report](scalable-routing-report.md) records 620 configurations / 2,480
fresh observations. The median target is achieved: all 199 certified-feasible
schedules have full plans, median fee gap 0%, with four remaining gaps up to
70.73%; all 64 exact online cohorts have 0% gap. The worst held-out case requires
four coordinated assignments and is outside the monotonically improving
single/pair neighborhood. All deadline and resource audits pass. The sparse
32-institution mesh remains the runtime bottleneck at approximately 8 ms p95,
while the 512-institution zero-fee case sustains roughly 19,000 ticks/second.

An untimed comparison of the supported example shows 440 fewer completions under
Reserved (12,409 versus 12,849), despite lower actual fees and no late completions.
The same generated cohort has a different served subset, so no gap is assigned.
Retain this evidence of irrevocable reservation/arrival tradeoffs. Do not infer
universal throughput or objective dominance from the measured median target.
