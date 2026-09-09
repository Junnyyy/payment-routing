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

## CLI and artifacts

```sh
cargo run --locked --release -- --evaluate
cargo run --locked --release -- --evaluate --scenario pressure --seed 42
cargo run --locked --release -- --evaluate --scenario all --seeds 0,1,42 \
  --strategies static,reserved,preserve,recompute,tight --ticks 60 --drain 60 --format csv
cargo run --locked --release -- --evaluate --scenario reservation-trap \
  --seeds 0,1,42 --ticks 1 --drain 8 --format payments
```

Defaults: all nine worlds, seeds 0/1/42, static and reserved, 60 arrival minutes,
60 drain minutes, text output. `--scenario` also accepts a comma-separated list.
`--ticks` is the arrival window, not the total processed time. Every CLI evaluation
verifies replay. Text, summary CSV and payment CSV work without terminal setup.
Censoring or runtime errors emit their reports and then exit nonzero. Nothing is
written to disk unless stdout is redirected by the caller.

`static` is the existing current-state exact single-payment route policy.
`reserved` uses default bounded search limits and adaptive disruption handling;
`preserve` and `recompute` use the same search with their named disruption policy.
`tight` uses 1 label/node, 8 labels, 4 expansions, 8 candidates and no pair repairs.
It deliberately probes truncation; it is not a recommended production setting.
Limits/policies are printed in text and recorded in CSV. Library callers can
supply named `Strategy` values with their own limits/allowances. World admission
limits and all other exogenous inputs stay common.

CSV is one quoted rectangular table with `case`, `aggregate` and
`world-aggregate` records. Case records include the complete scenario and strategy
Debug manifests, seed, horizon, version and every simulator metric. Manifests are
human-readable Rust input descriptions, not an import format. The named CLI worlds
and format version are implemented in `src/evaluation/scenarios.rs`; retain the
source revision with exported results. Payment output includes every instruction,
terminal outcome/time and actual fee/hops, keyed by world/seed/strategy/sequence.
Summary records retain exact integers; only text percentages use rounded floats.
No host timings, random fingerprints or timestamps enter deterministic output.

## Aggregation and tail visibility

Overall and per-world summaries use the **intersection of cases complete for all
requested strategies**. Censored/error cases remain in per-seed output and in each
strategy's exclusion counts; no strategy gets a favorable mean by losing hard
runs. The common denominator and total requested cases are always reported. An
empty intersection yields no win or rate claim. Invalid input is rejected rather
than silently omitted. Rates with zero offered demand are undefined, not 100%.

Pooled on-time rates weight each offered payment equally; each case gets one vote
for score wins/ties. These are different summaries. Per-world results expose how
topology/traffic mix changes a pooled result. A tie means equal full score, and
multiple configurations of the same routing policy can tie for best. There is no
claim that this synthetic mix represents a real workload distribution.

Nearest-rank p95 case loss and the worst loss retain the world/seed. A rare failure
can fall outside p95, so worst case is shown separately. The largest **service
shortfall** is `(best tested on-time count - this count) / offered count` within a
case, with its world/seed. It separates avoidable relative loss from a disconnected
world where every tested strategy fails. This is a comparison to observed peers,
not an optimum or a future-aware policy.

Every complete pair also reports exclusive on-time payment IDs and fees on their
common on-time cohort. Text caps displayed IDs at five per side and failure
examples at three (largest actual fees first); payment CSV retains all outcomes.
Different served cohorts are flagged, including when counts/volumes tie. Neither
total fees on different cohorts nor common-cohort fees establish an optimality gap.

Throughput is completed count or principal divided by the common total observation
minutes (arrival + drain); text prints the exact count/time ratio. The separate
arrival-cutoff completion count allows throughput during load to be distinguished
from subsequent drainage. Extending idle drainage changes this throughput
measurement, so compare configurations with the same horizon. Completed latency
statistics describe survivors and must be read beside failures and pending work.

## Scenario coverage and investigation

The five operations presets preserve their established synthetic meanings. Four
additional worlds exercise:

- `missed-connection`: cheap multihop routing misses a recurring connection;
  a more expensive immediate route meets the deadline.
- `reservation-trap`: a cheap future window attracts reservations before an
  unannounced closure; immediate execution can outperform waiting. The one-minute
  cohort isolates this failure, while the normal 60-minute run includes recovery.
- `disconnected`: zero cost with no delivery, plus explicit admission overload.
- `capacity-cliff`: changing shared capacity, heterogeneous principal/SLA and a
  small active limit expose admission and replanning tradeoffs.

These are adversarial fixtures, not oracle certificates. The existing exact
routing/scheduling oracles remain separate. A dominant average should trigger
inspection of payment identities, failures, truncation, admission pressure,
windows and surprises before interpreting fee savings. The first suite run found
that reserved routing's higher pooled on-time count coexisted with fewer case
wins and worse pressure outcomes; this motivated per-world and service-shortfall
summaries. The accompanying measured report records concrete evidence.

## Verification and resource limits

Run `cargo test --locked --offline --all-targets`,
`cargo test --locked --offline --doc`, and
`cargo clippy --locked --offline --all-targets -- -D warnings`.
After `cargo build --locked --offline --release`, run
`python3 scripts/test_evaluation.py` for an independent payment-CSV accountant,
all 135 suite runs, aggregate sums and byte-identical separate CLI invocations.
Python 3.9.6 was detected; Context7's Python 3.9 docs cover
[subprocess.run](https://docs.python.org/3.9/library/subprocess.html#subprocess.run)
and [CSV DictReader](https://docs.python.org/3.9/library/csv.html#csv.DictReader).

Tests cover zero demand, rejection/expiry distinction, failed-work costs, late
settlement, horizon censoring, strategy/world/seed order, checked aggregate
arithmetic, exact wide fraction ordering, future-surprise noninterference,
common-case exclusions, rare worst cases outside p95 and opposing adversaries.
The real world seed stays only in the driver; the injected-input simulator uses
an inert seed and cannot reconstruct future demand from its own RNG state.

Unlike the continuous bounded-history simulator, a finite evaluation deliberately
retains all payment outcomes for paired analysis. Memory grows with generated
payments times strategies and cases; replay also maintains a twin of each active
run. No runtime/CPU ranking or watchdog is provided by this mode. The static
router remains exponential on arbitrary networks; use the existing supervised
benchmark harness for runtime censoring experiments. A slow or externally killed
process has no completed evaluation report and must never be labeled infeasible.
