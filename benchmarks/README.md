# Reproducible benchmarks

Read the [measured report](../docs/benchmark-report.md) and
[measurement contract](../docs/benchmark-method.md). These fixtures diagnose the
existing algorithms; they are not production payment-network models.

```sh
python3 scripts/benchmark.py --manifest benchmarks/suites/smoke.json \
  --output /tmp/routing-smoke --repeats 3 --timeout 2 --rss-mib 512
```

The driver uses Python 3.9+ standard libraries on macOS or Linux, and builds the
Rust worker with the repository's locked dependencies. It needs permission to
read child RSS through `ps`. No network calls, paid providers, TUI, external
optimizer, benchmark framework, or additional Cargo dependency is required.
Results directories must be new. A nonzero exit can indicate a worker error;
inspect the retained raw records. Timeout and RSS stops are expected censored
observations in the adversarial sweeps.

## Fixture catalog

`scale` has the following explicit meaning. All fixtures are deterministic;
static cases ignore seed. Except demo payments and the two-cent ceiling control,
principal is one cent. Constructed institution and rail names are synthetic.

| Family | Meaning of scale / controlled difficulty |
|---|---|
| `single-positive`, `single-zero`, `single-deadline` | Number of members on one shared rail; fee 1 or 0, latency 1; deadline variant limits transit to 2 minutes |
| `single-positive-first` | Same positive-fee problem; move the receiver from last to first in the participant list |
| `single-disconnected` | Size of the zero-fee clique; one additional isolated institution is the receiver |
| `single-chain` | Institutions in a chain of pairwise rails, fee/latency 1 per hop |
| `single-density` | Maximum index distance connected by a pairwise rail in a fixed nine-institution graph (2..8); zero fee/latency |
| `batch-density` | Number of institutions on one shared positive-fee rail; one payment, full candidate retention |
| `batch-volume` | Payment count; two direct rails cost 1 and 5, unlimited capacity |
| `batch-ties`, `schedule-ties` | Payment count; two equal-fee/latency direct routes; scheduled version has both slots at minute zero |
| `batch-latency-ties`, `schedule-latency-ties` | Equal fees, different latency; isolates the remaining-score bound |
| `batch-choices` | Number of equal direct rail choices for a fixed eight payments |
| `batch-scarce` | Payment count P; cheap rail capacity floor(P/2), unlimited expensive rail |
| `batch-infeasible` | Payment count P; rail capacities sum to P-1; no full plan |
| `batch-ceiling` | Count of two-cent payments excluded by the cheap rail's one-cent transaction ceiling |
| `batch-trap`, `batch-trap-infeasible-greedy` | Fixed two-payment counterexample: flexible payment must yield cheap capacity to urgent payment; scale is unused |
| `batch-demo`, `schedule-demo` | Unchanged twelve-payment demo; scheduled version supplies one explicit minute-zero departure per rail; scale is unused |
| `schedule-contention` | P payments and P slots, one unit of capacity each, unrestricted deadlines |
| `schedule-deadline`, `schedule-reverse-deadline` | Same slots/count; deadline multiset 0..P-1 assigned in increasing or decreasing payment-ID order |
| `schedule-slots` | Departure choices for one payment on one rail; unit slot capacity |
| `schedule-multihop-slots` | Slots per rail on a three-hop, zero-latency chain; one payment |
| `schedule-nonfifo` | Fixed later-faster-departure connection counterexample; scale is unused |
| `sim-load` | Attempts per minute, 75% probability; capacity 10, latency 1, SLA 8, active limit 64 |
| `sim-deadline` | SLA minutes; 16 attempts/minute, other load inputs unchanged |
| `sim-outage` | Period length with only one open minute; eight attempts/minute |
| `sim-disconnected` | Active limit with service always closed, SLA 8 |
| `sim-backlog` | Active limit with service always closed, SLA 10,000; isolates growing queue work |
| `sim-history` | Retained event limit; eight attempts/minute; accepts up to 100,000 events |
| `sim-density` | Clique institutions, zero fee and latency, one attempt/minute, SLA zero; isolates routing cost per tick |
| `sim-pinned`, `sim-pinned-direct` | Downstream service period in the three-node pinned-path trap and its direct-only control; one attempt/minute, SLA 1 |
| `window-static`, `window-batch`, `window-schedule` | Capture width in minutes, after eight-minute warmup; four attempts/minute, service capacity 10 |
| `window-contended-static`, `window-contended-batch`, `window-contended-schedule` | Same capture with service capacity 2; seeds vary releases; static/batch are relaxed projections |

`seed` and `ticks` are explicit manifest fields, defaulting to 42 and 1,000.
Simulation scale must be at least two; the default worker safety range is up to
10,000 except retained history. Tick count is bounded to 100,000. The manifests
choose much smaller sizes for combinatorial searches and use fresh-process guards.

## Files and verification

- `fixtures.rs`: deterministic constructors and seeded window extraction.
- `audit.rs`: returned-witness checks and a specialized independent window certificate.
- `../examples/benchmark.rs`: one worker, JSON lines, isolated API timing and counters.
- `../scripts/benchmark.py`: release builds, process supervision, resource metrics,
  repeat/build agreement and machine-readable summaries.
- `suites/`: smoke, initial pilot, evidence-driven frontier, and follow-up diagnosis.
- `results/`: immutable measured artifacts; preserve unsuccessful runs alongside successes.

```sh
cargo test --locked
cargo test --locked --features search-stats
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --check
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Reproduce one case with the exact worker command in `raw.jsonl`. Append `--dump`
to include the full fixture, witness and any independent window certificate.
The `--skip-build` driver option is only for knowingly reusing an existing binary;
after source changes, use the normal build path. Historical Git/source and binary
hashes are recorded in metadata; results should never be relabeled as measurements
of a different build.
