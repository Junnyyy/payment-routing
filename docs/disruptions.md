# Disruptions and adaptive replanning

Implementation contract: synthetic USD service changes take effect at the start
of an integer-minute tick, before settlement, planning or departure. A change
persists until replaced. Rail availability and recurring per-minute principal
capacity are independent; reopening a rail still respects its recurring window.
Zero capacity closes its use, and unlimited capacity is explicit. Scheduled
changes are surprises to the planner until their effective tick. Controls are
staged for the next tick, consume no RNG, and override same-tick scenario changes.
Restart restores the original scenario, including its scheduled changes.

Previously departed hops keep their original arrival and fee. Only the unexecuted
suffix may change, from the current institution or the receiver/arrival of an
in-flight hop. A replacement cannot revisit an institution in that fixed prefix.
Invalidated future reservations are released transactionally; principal is never
dropped. Unresolved suffixes queue and retry until their inclusive deadline.
Actual departures always enforce current availability, ceilings and capacity.

On each effective change, compare two candidates from the identical existing
active cohort (before new arrivals). Preserve keeps valid complete suffixes in
FIFO order against the new shared budgets, then routes the remaining requests.
Recompute discards all unexecuted suffixes and replans that whole cohort using the
existing strategy and search limits. This is full recomputation, not a claim of
global optimality. Bounded search failure is unresolved, not infeasibility.

Assignment churn is the number of previously planned eligible payments whose
ordered remaining `(rail, sender, receiver, absolute departure)` assignments
change. Static routing has no departure reservations, so its comparison omits
timestamps. Withdrawal counts as churn; first assignment does not. Report the
denominator, route changes, retimings and withdrawals separately. Fixed prefixes
and final hops already in flight are excluded. Churn is per decision; repeat
changes of the same payment count again in cumulative totals.

Policies are explicit: Preserve, Recompute, or Adaptive with nonnegative absolute
fee, elapsed-minute and hop-count allowances. Adaptive first prefers more planned payments;
for equal coverage of the *same payment IDs*, it retains the preserve candidate
only within all three allowances of recompute. Otherwise it uses recompute. Equal
counts with different served IDs are an identity/fairness tradeoff: choose the
candidate retaining more prior plans, then the preserve candidate on a tie, and
flag the incomparable cohorts. Neither partial-cohort fee difference is an
optimality gap. Stability takes precedence over lexical tie-breaking. Default allowances are zero; callers can explicitly buy stability.
Report both alternatives even when Preserve or Recompute is selected.

Candidate fees cover future hops only; sunk fees are excluded. Reserved elapsed
time runs from the decision minute to final arrival, including any remaining
in-flight time. Static estimates omit future waiting and are not SLA certificates.
Selected plans and actual events remain distinct. There is no monetary weighting
of churn or SLA failures, and no assertion that the preserve candidate minimizes
churn globally. All input, control, arithmetic and invariant errors roll back the
tick, effective conditions, RNG, evidence, reservations and pending controls.

API documentation check: Cargo pins Ratatui 0.30.0 (Crossterm 0.29.0 in the
lockfile); the installed Rust compiler is 1.97.1, edition 2024. Context7 exposes
stable rather than a 1.97.1-specific standard-library index. Ordered maps use its
documented [BTreeMap iteration](https://doc.rust-lang.org/stable/std/collections/struct.BTreeMap.html#method.iter),
[entry](https://doc.rust-lang.org/stable/std/collections/struct.BTreeMap.html#method.entry)
and [retain](https://doc.rust-lang.org/stable/std/collections/struct.BTreeMap.html#method.retain)
APIs already used in this repository; no dependency changes are required.


## Simulation interfaces

`Scenario.disruptions` contains `Disruption { minute, update }` records. Each
`RailUpdate` names an existing `rail_id`, an optional `available` value, and an
optional `capacity_per_minute_cents` replacement. `None` leaves a field unchanged;
`Some(Some(0))` sets zero capacity and `Some(None)` removes the limit. Reject empty
updates, unknown rails, and duplicate scheduled `(minute, rail)` pairs before
execution. Input event order is canonicalized; distinct rail updates at one
minute form one reoptimization decision. Same-tick controls merge over scheduled
fields, with the last control write per field winning. An effective no-op emits
no disruption or churn and does not reoptimize.

```rust
use payment_routing::simulation::{RailUpdate, ReoptimizationPolicy};
// The control applies at the simulator's next minute.
let mut sim = payment_routing::simulation::Simulator::new(
    payment_routing::operations::Preset::Balanced.scenario(
        payment_routing::simulation::RoutingStrategy::Reserved { limits: Default::default() }
    ), 42).unwrap();
sim.queue_rail_update(RailUpdate {
    rail_id: "ACH".into(),
    available: Some(false),
    capacity_per_minute_cents: None,
})?;
sim.set_reoptimization_policy(ReoptimizationPolicy::Adaptive {
    max_extra_fee_cents: 0,
    max_extra_elapsed_minutes: 0,
    max_extra_hops: 0,
});
let report = sim.step()?;
Ok::<(), payment_routing::simulation::SimulationError>(())
```

`scenario()` remains the original validated input; `effective_scenario()` exposes
current conditions. `pending_rail_updates()` is bounded by rail count.
`adaptation_metrics()` exposes cumulative comparisons/churn/withdrawals, and
`last_reoptimization()` includes its decision minute, policy, both assessments,
selected candidate, cohort comparability and both sets of search diagnostics.

`TickReport.events` includes `DisruptionApplied` with before/after conditions,
`Reoptimized` with the comparison, and `PlanRevised` with the selected composed
route and timestamps. An absent route or a route ending short of the instruction's
receiver means the future suffix is unresolved. `has_complete_plan()` distinguishes
that case. Completed/expired/rejected payments still leave active storage; normal
history bounds apply to new events too. Observation captures selected timestamps
separately from bounded traces of discarded candidate searches.

For ordinary reserved admission, existing commitments stay fixed. Only an effective
disruption triggers reconsideration of the existing cohort. Subsequent arrivals
and retries may create additional plans after the reported comparison, so the
candidate assessments describe that decision, not every later event in the tick.
Recompute and Preserve choose their named candidate even if the other has higher
coverage. Adaptive is the default; preservation is not a hard promise to keep
invalid capacity or closed-rail assignments.
