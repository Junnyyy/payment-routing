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
