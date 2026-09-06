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
