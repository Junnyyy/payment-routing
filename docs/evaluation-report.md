# Paired evaluation: measured results

Code revision: `5f397f9` (evaluation format/scenario version 1). Rust/Cargo 1.97.1,
edition 2024, locked Ratatui 0.30.0/Crossterm 0.29.0. Release build, synthetic USD;
no provider calls, external payments, wall-clock objective or optimum claims.

## Reproduce

```sh
cargo build --locked --offline --release
target/release/payment-routing --evaluate --scenario all --seeds 0,1,42 \
  --strategies static,reserved,preserve,recompute,tight --ticks 60 --drain 60 \
  --format csv > benchmarks/evaluation-results.csv
target/release/payment-routing --evaluate --scenario reservation-trap --seeds 0,1,42 \
  --strategies static,reserved,preserve,recompute,tight --ticks 1 --drain 8 \
  --format csv > benchmarks/evaluation-shock-results.csv
python3 scripts/test_evaluation.py
```

The [main CSV](../benchmarks/evaluation-results.csv) contains 135 individual runs
(9 worlds × 3 seeds × 5 configurations), 5 overall aggregates and 45 per-world
aggregates. Each configuration receives the same 4,856 instructions across its
27 world/seed cases. All runs complete and pass tick-by-tick replay verification;
there are no censored/error exclusions. The [shock CSV](../benchmarks/evaluation-shock-results.csv)
is a separate horizon experiment and is not pooled into the main results.

SHA-256:

- Main: `ac926288ba9b978ee4e4c0df56cbce1be6332d7706c967622f5488d493368a4c`
- Shock: `68893c9524e21c86ca288ba76226fb6c818992f2cc77926799196b61a8f664ae`

## Pooled outcomes and case wins disagree

Fees below include every actual departure, including unsuccessful work. Count
wins use the full service-first lexicographic score defined in the
[evaluation contract](evaluation.md#objective-definitions). These are five
configurations of two routing algorithms; preserve/recompute change the reserved
algorithm's disruption policy, and tight lowers its search limits.

| Configuration | On time / 4,856 | SLA failures | Actual fee, cents | Unique case wins | Tied best cases |
| --- | ---: | ---: | ---: | ---: | ---: |
| Static | 2,151 | 2,705 | 548,238 | 17 | 5 |
| Reserved adaptive | 2,177 | 2,679 | 408,514 | 0 | 10 |
| Reserved preserve | 2,177 | 2,679 | 408,456 | 0 | 10 |
| Reserved recompute | 2,179 | 2,677 | 408,545 | 0 | 10 |
| Reserved tight | 2,121 | 2,735 | 340,385 | 0 | 4 |

A pooled fee/delivery comparison favors reserved configurations; static wins more
individual world/seed comparisons. A few large service gains, notably the missed
connection world, outweigh losses across other cases. The lowest-fee configuration
also has the fewest on-time completions. None of these summaries alone establishes
a generally better strategy. The synthetic workload mix and three chosen seeds
are a reproducibility set, not a population estimate or confidence interval.

All configurations fail 100% of the disconnected workload. That expected outcome
makes the absolute worst-case field insufficient to distinguish strategies.
The added relative service-shortfall field reveals:

| Configuration | Largest shortfall versus tested best | World / seed |
| --- | ---: | --- |
| Static | 51 / 99 offered (51.52 percentage points) | missed-connection / 42 |
| Reserved adaptive, preserve, recompute | 31 / 470 (6.60 points) | pressure / 1 |
| Reserved tight | 40 / 470 (8.51 points) | pressure / 1 |

This comparator is the best observed on-time count for that same world/seed,
not a clairvoyant reference or an optimality gap. Per-world aggregates preserve
the underlying differences; every case remains in the raw CSV.

## Investigating the pressure result

At pressure/42, both runs receive 490 identical instructions and reach the same
16-active-payment limit. No search truncation occurs in either default strategy.

| Metric | Static | Reserved adaptive |
| --- | ---: | ---: |
| On time | 151 | 126 |
| Rejected at admission | 253 | 291 |
| Expired after admission | 86 | 73 |
| Actual fee, cents | 63,505 | 38,935 |
| Completed elapsed sum, minutes | 467 | 553 |
| Queue payment-minutes | 461 | 486 |
| On-time IDs exclusive to this strategy | 35 | 10 |

The strategies share 116 on-time completions; fees on those common IDs are
52,530 versus 34,355 cents. The 24,570-cent total fee difference includes a
changed served cohort and must not be called an optimality gap. The queue and
elapsed evidence is consistent with waiting/reservation choices reducing
admission turnover; it does not isolate a single causal contribution.
The explicit rejection counts show why fewer expiries do not imply better SLA
performance. No scoring denominator omits the extra rejected instructions.

Reproduce the full paired IDs and highest-fee failure examples with:

```sh
target/release/payment-routing --evaluate --scenario pressure --seed 42 \
  --strategies static,reserved --ticks 60 --drain 60
target/release/payment-routing --evaluate --scenario pressure --seed 42 \
  --strategies static,reserved --ticks 60 --drain 60 --format payments
```

## Opposing adversarial cases

In missed-connection/42, static completes all 99 payments, but 51 finish late.
Reserved completes all 99 on time. Static fees are 1,215 cents versus reserved's
1,260: a small fee advantage hides a large SLA loss if late completions are counted
as successful service. The score and report keep these outcomes distinct.

The isolated reservation-trap cohort generates two payments at seed 0, none at
seed 1 and one at seed 42. At minute 0, the cheap rail's known recurring window
opens at minute 4, while the expensive immediate rail is open. At minute 1 both
rails unexpectedly close; they reopen at minute 8, after the minute-6 deadlines.
Static delivers all three generated payments for 60 cents. Every reserved variant
waits and subsequently expires all three for zero fees. Its 100% failure rate
on each nonempty seed is visible and loses the score. Seed 1 remains a zero-demand
case with undefined rates; it is not reported as 100% delivery.

The known window is legitimate current information. The future closure is not:
the driver withholds it until minute 1. Tests confirm that changing unrevealed
surprises leaves prior decisions unchanged, and strategies receive neither the
real workload seed nor the future event list. This counterexample demonstrates
that scheduling knowledge does not make the strategy clairvoyant.

## Verification and limits

- All 155 target tests and 3 doctests pass; formatting and Clippy with warnings
  denied pass. The 11 integration evaluation tests also pass with `search-stats`.
- The independent Python CSV accountant checks 135 runs, all 27 shared workloads,
  exact payment-level cost/volume/elapsed/outcome accounting and 50 aggregate rows.
  Two fresh release invocations produce byte-identical summary CSV. The complete
  debug summary is also byte-identical to the saved release artifact.
- Tests cover a deliberately induced replay mismatch, order independence,
  future-surprise noninterference, no-demand rates, censoring, common-case
  exclusions, checked wide totals, and rare failure outside the p95 case tail.
- This evaluates routing outcomes, not CPU or memory efficiency. Evaluation
  retains finite payment ledgers, the exact static router remains exponential,
  and unusually large user-specified cases need separate resource supervision.
  There is no inference about real-network performance, arbitrary topology,
  economic penalty weights or globally optimal online routing.
