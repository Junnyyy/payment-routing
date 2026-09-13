# Demo guide

```sh
cargo run --locked -- --demo
```

Starts paused on Watch with seed 42, balanced demand and Reserved selected.
Optional arguments: `--seed N`, `--scenario NAME`, `--strategy static|reserved`.
Both strategies advance together. Time is the last processed minute; Ready means
minute zero has not run. All execution is synthetic USD with accelerated timing.
The continuous stream is separate from the static fixture's 12 payments.

## Watch

The rail table stays in a fixed display order. Capacity is principal departed in
the last processed minute, divided by that minute's budget. It is not occupancy.
Queued counts only payments assigned to that rail; Unassigned counts waiting work
without a complete route. Moving counts unsettled hops. Zero-latency transfers
appear as events rather than artificial travel animations.

The event line shows a recent disruption or payment event, timestamped and retained
for up to three simulated minutes. Re-enabling a rail does not override its service
window. Larger terminals also show the selected rail's active payments.
Enter pauses and opens the rail; `p` opens its matching payment records.

## Compare and inspect

Compare shows identical revealed demand under Static and Reserved:

| Label | Meaning |
| --- | --- |
| On time | Completed by the deadline |
| Missed | Deadline failures, including late deliveries and late work still in flight |
| Rejected | Admission limit reached |
| Active | All unfinished payments, including late work in flight |
| Fees | Every executed hop, including failed work |

Missed and Active can overlap. They are not additive outcome buckets.
These are ongoing runs, so there is no winner or optimality-gap claim.
`e` opens full totals; very wide values link there instead of clipping.

The payment table lists retained IDs with different status, route or actual fee.
Change identifies the first difference: status, then route, then fee.
Not retained means one strategy's record aged out, not that it rejected the payment.
Enter pauses and shows both statuses, routes, actual fees and recorded events.
A partial route is a fixed prefix awaiting a suffix. A dash in the timeline means
no displayed event, not evidence of waiting. Omitted histories are counted.

`e` opens detailed evidence for the strategy named in the header. If that strategy's
record aged out, inspection selects the surviving record's strategy. Esc returns
to the short inspection, then to the originating view.

## Controls

| Key | Action |
| --- | --- |
| Space / . | Run or pause / step one minute |
| Tab / 1 / 2 | Switch Watch and Compare / Watch / Compare |
| ↑↓ or j/k | Select or scroll |
| Enter | Inspect and pause |
| e | Evidence in inspection; full totals in Compare |
| Esc | Back; quit from Watch or Compare |
| p | Payment list; from a rail, filter to that rail |
| / / f | Search payments / cycle filter |
| Home / End | Follow newest / select oldest; top / bottom in details |
| PgUp / PgDn | Page through records or details |
| c | Choose scenario; Enter restarts, Esc cancels |
| s | Inspect the other strategy |
| + / - | Playback speed |
| r / n | Restart / next seed |
| 3 / 4 / 5 | Rail metrics / network / optimizer diagnostics |
| ? | Help and pause |
| q / Ctrl-C | Quit |

Manual selection holds a payment ID while it remains in the current list.
Returning to Home follows the newest record. Payment filters apply to the payment
list; Compare always considers both retained histories. Search and scenario
selection pause execution. Playback speed changes host pacing only.

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
Accepted reserved departure timestamps are captured before execution in a separate
observation map, independent of the search-entry cap, so zero-latency completion
cannot remove the plan from the retained dossier. Ordinary tick events are unchanged.

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

## Verification

The [development checks](development.md#verification) include all-feature tests,
Clippy and real PTY sessions. The terminal tests cover both main views at 80×18
and 120×32, stable selection, paired timelines, retained-record differences,
wide values, search, scenario cancellation and evidence navigation.
`scripts/test_console.py` checks run/pause, all five presets, resizing and terminal
restoration after q, Esc and Ctrl-C. Snapshots are written under ignored `target/`.

Ratatui is locked to 0.30.0, with its Crossterm 0.29.0 re-export.
Context7 references: [stateful Table / selection](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Table.html#method.row_highlight_style),
[Row / height](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Row.html#method.height),
[Paragraph / scroll](https://docs.rs/ratatui/0.30.0/ratatui/widgets/struct.Paragraph.html#method.scroll)
and [Layout / vertical](https://docs.rs/ratatui/0.30.0/ratatui/layout/struct.Layout.html#method.vertical).

## Dynamic disruptions

`--scenario disruptions` uses the balanced demand fixture, closes ACH at minute 4,
reduces RTP to 10,000 cents/minute at minute 8, reopens ACH at minute 12, and
reduces ACH to 10,000 cents/minute at minute 16, then restores ACH to 200,000
and RTP to 100,000 cents/minute at minute 20. Changes are surprises at those
ticks; the router does not read future scenario events. The static demo fixture
is unchanged. At seed 42 after 80 ticks, static completes 93, expires 66 and spends
41,680 cents; reserved completes 90, expires 67 and spends 35,570 cents. Reserved
changes 3 of 5 previously planned assignments across five repair decisions. Both
generate 163 payments; these different completion cohorts are not quality gaps.

Optimizer diagnostics show aggregate assignment churn and both candidate
assessments at 80×18, the selected policy and explicit allowances, and recent rail
changes. Rail evidence uses effective capacity/availability and retains change evidence;
payment details distinguish an accepted route from a fixed prefix awaiting a new
suffix. `PlanRevised` carries the final composed witness even for same-tick
completion; `Reoptimized` contains both assessments. Operations retains the newest
32 network events independently of its payment dossiers and normal event ring.

`Operations::queue_rail_update` validates/stages a control for both runs or neither.
`set_reoptimization_policy` applies to both; restart preserves the explicit policy
and replays the original scheduled changes, discarding pending controls. Rendering
never triggers optimization. See [disruption semantics](disruptions.md) and the
[matched-cohort report](disruption-report.md). The PTY harness also runs this preset.
