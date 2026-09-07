# Operations console

Launch `cargo run --locked -- --demo`. The console starts paused before minute 0,
with seed 42, the balanced scenario and Reserved selected for inspection. Optional
`--seed N`, `--scenario balanced|pressure|outage|limited`, and
`--strategy static|reserved` are validated before terminal initialization.

All execution is synthetic USD with accelerated timing. The static
`demo_network()` fixture and exact routing APIs retain their existing inputs and
contracts. The console generates new instructions; it does not execute the 12
stored static fixture payments. No real transfers or liquidity debits occur.

## Keyboard workflow

| Key | Action |
| --- | --- |
| Space | Start or pause both strategies |
| . | Pause and execute exactly one minute |
| + / = / - | Select 1, 2, 5, 20 or 100 target ticks/second |
| r | Restart current seed, paused, clearing runtime history |
| n | Start the next seed (increment, wrapping at `u64::MAX`), paused |
| c | Cycle scenarios and restart the current seed, paused |
| s | Inspect the other strategy at the same minute without resetting |
| 1–6 | Overview, payments, rails, network, optimizer, comparison |
| Tab / Shift-Tab, h/l, Left/Right | Cycle views |
| j/k, Up/Down, PgUp/PgDn | Move rows or scroll a text page |
| Home / End | First/last row or page; Home enables newest-payment following |
| f | Cycle all, active, queued, SLA/overload and finished payment filters |
| / | Search ID, endpoints, status or accepted route rail; pauses execution |
| Enter | Apply search, or inspect selected payment/rail and pause |
| Esc | Cancel search, close detail/help, otherwise quit |
| ? | Open help and pause |
| q / Ctrl-C | Quit (q is text while editing search) |

The header shows the **last processed minute** and **next minute**, avoiding an
ambiguous off-by-one time display. A tick processes minute 0 first. Speed controls
host pacing only; they consume no randomness. Ticks do not catch up after a slow
calculation, and drawing is limited to about 20 Hz during uninterrupted running.
The target rate is not a throughput promise. `twin ...ms` measures host computation
for the pair of observed ticks, excluding draw time. The paused loop blocks on
input or resize. Terminal initialization, ordinary errors and exit all restore
terminal modes; simulation errors pause and retain the last committed twin state.

## Reading each view

- **Overview:** generated and active principal, unrouted and reserved queues,
  in-flight and draining work, oldest queue age, fees, delivery/SLA metrics and
  conservation. Scroll to 120 retained post-tick samples and recent values. A zero
  queue can coexist with in-flight SLA failures. SLA failure rate divides all
  failures (including overload) by generated demand; active work is not a success.
- **Payments:** newest first by default. Moving selection holds a payment ID as
  rows change; Home resumes following newest. Filters apply to retained records.
  A selected record that ages out of retention falls back to the newest match.
  Enter shows endpoints, deadline, status, the other strategy's retained result,
  actual fees and elapsed completion time, accepted route, future reserved
  departures where retained, search evidence, and a bounded lifecycle.
- **Rails:** last-tick open state, used principal/capacity, utilization, assigned
  waiting payments, cumulative departures and fees. Enter exposes full membership,
  service windows, cumulative departed/settled/in-flight principal and hops, all
  future reserved slots and payments waiting for that rail. Unrouted demand is
  excluded from per-rail queues. Unlimited/zero denominators display `n/a`.
- **Network:** current queued and inbound work at each institution, active work
  originating there, shared-rail membership and descriptive opening balances.
- **Optimizer:** selected policy and objective, reservations, Reserved search
  attempts/expansions/candidates, truncations, unresolved trials, repairs and
  configured limits. Static runtime counters are labeled unavailable. Counters
  include discarded order/repair trials and do not count unique payments.
- **Comparison:** both strategies always use the same generated demand and the
  same simulated minute. Completion counts, overload, expiry, fees and latency
  are visible together at 80×18. Scroll to matched retained payment IDs.
  Completion cohorts differ, so a fee difference is **not an optimality gap**.

A minimum 80×18 terminal is supported. Text details wrap and scroll; tables keep
the selection visible. Wider/taller terminals expose more records at once.
These are keyboard views with sparse color for selection and lifecycle state;
every status also has a textual label.

## Evidence and separation

`Simulator::step_observed()` returns the ordinary tick report plus evidence from
actual static and reserved searches. It does not rerun an optimizer against a
later snapshot. Static evidence includes rejected SLA/cost prefixes, rejected
ranked routes, unavailable/capacity-limited rails and candidates that improved an
incumbent. Reserved evidence includes candidates from order/repair trials,
capacity conflicts, missed-deadline departures and unresolved/truncated results.
A candidate may later be replaced; a prefix is not a complete route; an alternate
trial may use different tentative reservations. No exhaustive rejected-alternative
list or global feasibility certificate is implied. A bounded failure is unresolved,
never proven infeasible. The final accepted route is displayed separately.

Evidence is opt-in; `step()` and exact optimizers preserve their existing outputs.
Each observed tick collects at most 24 entries per payment and counts omitted
entries. The latest decision tick replaces earlier decision evidence for a queued
payment. Dossiers retain all active payments and the newest 128 terminal records,
32 lifecycle events per record, and 120 queue samples per strategy. Each tick's
transient evidence is bounded by configured admission/arrival limits; retained
state never grows with elapsed simulation time. Individual executed fees and
completion elapsed time remain available even if earlier lifecycle events roll
off. Render functions never run search or advance the simulation.

`operations::Operations` owns the twin transaction and observations without
Ratatui, Crossterm, terminal or wall-clock types. `App` owns keys, view selection,
filters and host timing. `ui` only renders. `main` owns terminal lifecycle/pacing.
The console deliberately exposes supported continuous strategies; it does not
pretend a finite-batch optimum exists for an unbounded online execution cohort.

## Scenarios and reproducible findings

`balanced` has 3 probabilistic attempts/minute, 64 active slots, USD 100–1,500
payments, inclusive 0–12 minute SLAs, ACH latency 5 and Fedwire latency 2.
ACH opens 3/8 minutes with USD 2,000/minute capacity; Fedwire 3/4 with USD 2,000;
RTP/FedNow open every minute with USD 1,000. `pressure` raises attempts to 12 and
lowers active slots to 16. `outage` disables ACH. `limited` uses the balanced
network with Reserved limited to one expansion and two candidates per search.
Other limits remain at their defaults. These are diagnostic scenarios, not real
rail rules or optimized policy recommendations.

Reproduce the displayed records without a terminal:

```sh
cargo run --locked --example operations -- 80 42
```

This example compares each observed strategy against an ordinary simulator after
every tick. At seed 42 after 80 ticks (last minute 79), this worktree produced:

| Scenario | Policy | Generated | Completed | Expired | Overload | Active | Fees USD | SLA failures |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| balanced | static | 163 | 99 | 60 | 0 | 4 | 416.70 | 60 |
| balanced | reserved | 163 | 95 | 62 | 0 | 6 | 325.05 | 62 |
| pressure | static | 644 | 202 | 108 | 322 | 12 | 838.55 | 430 |
| pressure | reserved | 644 | 162 | 94 | 377 | 11 | 439.50 | 471 |
| outage | static | 163 | 73 | 85 | 0 | 5 | 538.25 | 85 |
| outage | reserved | 163 | 73 | 85 | 0 | 5 | 538.25 | 85 |
| limited | static | 163 | 99 | 60 | 0 | 4 | 416.70 | 60 |
| limited | reserved | 163 | 55 | 102 | 0 | 6 | 2.75 | 102 |

These runs show why fees alone are misleading: Reserved spends less in the
pressure case but completes fewer payments and incurs more SLA failures. The
limited run reports 4,055 truncated searches and 6,684 unresolved trials, not
4,055/6,684 failed payments. No completed payment was late in these four runs;
a separate console fixture shows a zero-queue in-flight SLA miss at minute 3,
then a late completion at minute 4 with its two cents of actual fees.

## Verification and iteration

The first 80×18 review exposed clipped rail utilization, comparison fees below
the fold, an unscrollable help page, and payment selection following an old record
without a visible mode. The revised console adds full rail drill-down, puts fees
alongside outcomes on the first comparison screen, clamps scrollable help/details,
and labels `FOLLOW newest` versus `HOLD ID`. Actual execution costs are separate
from planned route fees. These changes were rerun against real deterministic
scenarios and terminal sessions.

```sh
cargo fmt --check
cargo test --locked --offline --all-features
cargo clippy --locked --offline --all-targets --all-features -- -D warnings
CONSOLE_SNAPSHOT_DIR=target/console-screens cargo test --locked --offline --bin payment-routing
cargo build --locked --offline
python3 scripts/test_console.py
```

Tests compare observation with ordinary simulation state/event-for-event across
four scenarios and both policies, replay twin runs on restart, and bound histories
across 300 ticks per scenario. An additional headless run at seed 7 checks 1,000
ticks per scenario against both ordinary simulators. TestBackend renders all six views at 80×18 and
120×32, with empty/no-result and undersized cases, retained terminal-payment
investigation, keyboard controls, selection identity, and scrolling.

The standard-library PTY harness launches the actual binary. It exercises
start/pause, stepping, speed, search, payment/rail inspection, comparison,
restart/new seed, resize to 120×32 and 40×10, and 80 ticks each of pressure,
outage and limited scenarios. Separate q, Esc and Ctrl-C runs exit zero, emit
alternate-screen restoration, and preserve terminal modes. On macOS the harness
keeps a shell-like parent alive to inspect modes before the controlling terminal
is revoked. Its small VT reader is supplemental snapshot tooling, not a full
terminal emulator. Snapshots live under ignored `target/console-pty`.

Version-specific Context7 references used with detected Ratatui 0.30.0 and its
locked Crossterm 0.29.0 re-export: [Table / row_highlight_style](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Table.html#method.row_highlight_style),
[TableState](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.TableState.html),
[Paragraph / scroll](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html#method.scroll),
[initialization and restoration](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/src/init.rs),
and [Crossterm re-export](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/ratatui-crossterm/README.md).
No dependencies were added or upgraded.
