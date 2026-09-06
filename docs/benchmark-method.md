# Benchmark contract

This suite measures the existing exact static, batch and scheduled searches and
the online `CheapestStatic` simulation policy. All inputs are synthetic USD.
It does not change optimization, budgets, tie breaks, or correctness oracles.

Timing uses the ordinary release build. An independent `search-stats` build
collects saturating per-thread counters with no search cutoffs. Every timing and
counter result must have identical deterministic output. A timeout is censored
work, never evidence of infeasibility. No incumbent is available from the current
APIs when a worker is killed. Counters are unavailable for killed workers.

The worker measures one solver call including input validation and output
construction, excluding fixture generation, witness checks and serialization.
Simulation timing includes step reports and bounded queue samples; capture of a
finite generated-payment window is a separate operation. Resource measurements
cover the entire fresh worker process, not just the timed call. They include
fixture generation, validation, and output and must not be described as solver
allocation counts. Repeats run sequentially, with no machine-wide isolation claim.

Implementation uses standard libraries only. The initial environment is Rust
1.97.1, Cargo 1.97.1, Python 3.9.6; Cargo.toml/Cargo.lock pin Ratatui 0.30.0 and
Crossterm 0.29.0, neither used by the driver. Context7 was queried before coding:
[Instant::elapsed](https://doc.rust-lang.org/stable/std/time/struct.Instant.html#method.elapsed),
[Duration::as_nanos](https://doc.rust-lang.org/stable/std/time/struct.Duration.html#method.as_nanos),
[LocalKey with Cell](https://doc.rust-lang.org/stable/std/thread/struct.LocalKey.html),
[Python 3.9 wait4](https://docs.python.org/3.9/library/os.html#os.wait4), and
[Popen attributes](https://docs.python.org/3.9/library/subprocess.html#subprocess.Popen.returncode).
Context7 exposes rolling Rust std docs and Python minor-version docs, not these
exact patch snapshots; compile/test against the recorded installed versions.

## Reproduce

Run `python3 scripts/benchmark.py --manifest benchmarks/suites/pilot.json --output
/tmp/payment-routing-pilot --repeats 3 --timeout 3 --rss-mib 512` from the repository
(join the displayed line break). The driver builds both variants sequentially
with `cargo build --locked --release --example benchmark`. Output directories must
be new. `--skip-build` reuses binaries; their SHA-256 is always recorded, and the
caller is responsible for rebuilding after source changes.

Each case runs in a fresh child process three times without instrumentation and
once with counters. The supervisor polls completion every 5 ms and RSS every
100 ms; limits may overshoot by a poll interval plus OS scheduling delay. RSS
limits are sampled guardrails, not an OS-enforced hard allocation cap. Time limits
cover the entire worker, including preparation and witness auditing. `solve_ns`
uses Rust's monotonic clock around the API call only. For tiny cases, consult
repeat min/max; process launch cost is deliberately not part of solve time.

Once the watchdog observes a timeout or RSS limit, the observation remains
censored even if the worker exits before the termination signal arrives. The
supervisor tolerates that `ProcessLookupError`, reaps the child once, and retains
its actual exit code, resource usage and any completed output. Other supervisor
errors still propagate. This race is covered for both limits by deterministic
tests using the Python 3.9
[process exception](https://docs.python.org/3.9/library/exceptions.html#ProcessLookupError)
and [mock side effects](https://docs.python.org/3.9/library/unittest.mock.html#unittest.mock.Mock.side_effect)
APIs retrieved through Context7.

`metadata.json` records the command, manifest, machine/CPU, toolchain, Git state,
source/lock/binary hashes and limits. `raw.jsonl` retains every observation,
including killed workers. `summary.csv` and `summary.json` preserve censored counts
and report median/min/max of *completed* ordinary runs. A mixed completed/censored
row is not a completed benchmark. CPU and peak RSS are fresh-worker `wait4`
measurements; macOS RSS bytes and Linux KiB are normalized to bytes. Process
resources include setup, auditing and serialization. No allocator telemetry or
portable exact solver memory attribution is claimed.

A worker can be replayed with `target/benchmark/plain/release/examples/benchmark
FAMILY SCALE SEED TICKS --dump` (join the line break). This prints the complete
fixture and witness as JSON strings containing Rust debug representations. The
manifest plus source/binary hash is the executable replay format. Stable FNV-1a
fingerprints detect changed fixtures, full witnesses or simulation final states;
they are diagnostic checksums, not cryptographic proofs. SHA-256 identifies code
and binaries. Repeats and counter builds must agree on deterministic results.

## Meaning of metrics

- `path_states`: recursive path visits, including immediately pruned states.
- `candidates`: feasible destination paths; batch/schedule retain these before
  joint assignment. Single routing keeps only its incumbent; its count excludes
  paths already removed by fee/deadline pruning.
- `candidate_hops`: summed witness hops, a storage proxy, not allocated bytes.
- `assignment_states`, `complete_assignments`, `bound_prunes`: visited joint
  prefixes, complete surviving assignments, and score-bound exits. Single-router
  fee-bound exits also increment `bound_prunes`.
- `capacity_rejects`: failed joint candidate capacity checks. It excludes path
  enumeration capacity filtering. Deadline counters count search checks, not a
  comparable number of unique paths across algorithms.
- Simulation counters include repeated single-route attempts. Queue samples are
  taken after each committed tick: queued means active with no in-flight hop.
  Peak active therefore excludes within-tick admission spikes. Mean/p95 queue,
  oldest queued age, terminal counts, cumulative fees, per-rail resource usage,
  actual simulated throughput and host processing throughput remain distinct.
  SLA failures include overload and expiry; end-of-run active work is censored.
- `optimal` means the existing exact solver returned a full solution; returned
  witnesses are independently audited. Constructed cases assert hand-derived
  minimum fees; small versions are checked against existing independent oracles.
  A measured single-router solution ignores batch capacity by contract.

## Seeded windows

`window-*` replays a continuous simulator from seed through an eight-minute
warmup and captures every `Generated` instruction in the next `scale` minutes,
including any rejected instructions. It preserves release times and deadlines.
The timetable explicitly expands each recurring service from the window start
through the cohort's last deadline. It gives the cohort full slot capacity,
without carry-in or background demand: an isolated retrospective counterfactual.
The batch projection sums those departure budgets per rail and drops waiting;
the single projection also drops capacity. These are relaxations, not executions
of the online policy. Fees can be compared as lower bounds only when the relaxed
batch is feasible; no online optimality gap is inferred from these projections.

## Investigation log

1. **Pilot** (`suites/pilot.json`): 82 cases, one timing observation plus one counter
   run each, 1-second/512-MiB guards. Equal-cost assignments and departure contention
   were much more expensive than distinct-cost volume. Dense one-payment batch
   enumeration reached 109,601 candidates and about 125 MiB at ten institutions.
   All observations are retained; these exploratory single samples are not
   repeat-based frontier claims.
2. **Follow-up** (`suites/frontier.json`): narrow those time/memory boundaries with
   three timing repeats; add parallel route choices at fixed payment count,
   intermediate network densities, reversed deadline order, and equal-fee/different-
   latency controls. The pilot's batch/demo versus schedule/demo gap motivates
   isolating the different remaining-score bounds. Add multihop slot expansion to
   test whether the cheap one-hop slot sweep generalizes. Reduce per-minute window
   capacity from ten to two and use seeds 0, 42, 99 because the initial captured
   windows had no contention. Pair pinned-route failure with a direct-only network,
   and lengthen the SLA of a disconnected queue to expose active-state scan cost.

For `window-contended-*`, capture uses two principal cents per minute; ordinary
windows use ten. `sim-pinned-direct` removes the cheap multihop option from the
same seeded workload; it is a fixture control, not a new optimizer policy. In
`sim-backlog`, service is closed and SLA is 10,000 minutes, keeping work active
through the measured 1,000 ticks instead of expiring it after eight minutes.
3. **Diagnosis** (`suites/diagnosis.json`): a positive-fee 64-institution clique
   visited 3,657,320 path states despite its direct route. Rotate the receiver to
   the start of the rail's participant list, preserving the optimization problem,
   and compare full answers. Several contended windows timed out, so construct and
   independently audit a cheap one-rail FIFO schedule outside the timed solver.
   A successful witness establishes feasibility and minimum fee (every payment
   must use the only rail); it does not certify elapsed/lexical optimality. A failed
   certificate does not declare infeasibility. Narrow the remaining route-choice,
   capacity and deadline-order boundaries.

The frontier run exposed one **harness input error**, not a solver failure:
`sim-history 100000` exceeded the worker's generic 10,000 scale assertion. All four
failed observations remain in the raw log and are excluded from performance
claims. The diagnosis run permits the intended 100,000-event history limit for
this family and reruns it. The driver reports worker errors and exits nonzero
while preserving all other cases and summaries.

The pilot's recorded dirty source state is recoverable using
`benchmarks/results/pilot/working-tree.patch` on its recorded Git revision. The
patch was reconstructed from the two tracked harness edits and verified to
reproduce the recorded combined source SHA-256 exactly. Frontier and diagnosis
were measured from clean source commits. Preserve a source patch or commit before
new measurement runs if exact historical source recovery matters.
