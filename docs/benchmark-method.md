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
