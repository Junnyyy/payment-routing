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
fee and elapsed-minute allowances. Adaptive first prefers more planned payments;
for equal coverage of the *same payment IDs*, it retains the preserve candidate
only within both allowances of recompute. Otherwise it uses recompute. Equal
counts with different served IDs are an identity/fairness tradeoff: choose the
candidate retaining more prior plans, then the preserve candidate on a tie, and
flag the incomparable cohorts. Neither partial-cohort fee difference is an
optimality gap. Default allowances are zero; callers can explicitly buy stability.
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
