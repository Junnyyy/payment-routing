# Continuous synthetic simulation

The `simulation` library owns a deterministic, discrete-event payment execution
model driven in integer-minute ticks. It uses no terminal, wall clock, sleep,
thread, external service or finite prebuilt timetable. All money is synthetic USD
principal; institution opening balances remain descriptive. This is a throughput
and delivery model, not a bank liquidity or real settlement model.

## Scenario and recurring capacity

A scenario supplies a validated network, directed arrival flows, a number of
Bernoulli arrival attempts per minute, integer probability in millionths, inclusive
amount and SLA ranges, and one recurring service per rail. Stored network payments
are validated but are not submitted to the simulation. Flows are selected uniformly;
duplicate flows can express integer weights. Each attempt samples probability,
flow, amount and SLA before any admission decision, so overload does not change
future random draws. The seeded generator is explicitly specified SplitMix64 with
integer rejection sampling; it does not depend on a platform random source.

A service opens when `(minute % period) >= offset` and
`(minute % period) - offset < open_minutes`. Windows must fit within a positive
period. Zero open minutes means permanently closed. Static `Rail.available` is a
hard veto. Fees, latencies, membership and transaction ceilings retain their static
meanings. Per-minute capacity is a new, explicitly replenishing principal budget
shared by all members and directions, excluding fees. Every departing hop consumes
its full principal. Unused capacity is discarded; settlement never refunds it.
There is no in-flight occupancy constraint. `None` is unlimited and zero is closed
to departures. Static batch capacities are rejected in simulation scenarios:
they describe a finite optimization batch and must not silently become recurring
budgets. Existing static and finite-schedule APIs remain unchanged.

## Tick and execution order

`next_minute` starts at zero and identifies the next unprocessed minute. Each tick:

1. Resets rail usage and samples recurring availability in rail-ID order.
2. Settles previously dispatched hops due now, in generated payment sequence order.
3. Generates this minute's arrivals. At `max_active_payments`, records an explicit
   overload rejection instead of growing the queue. Settlement can free admission
   space; departures and deadline expiry later in this tick cannot.
4. Processes active payments in sequence order. Unrouted payments consult the
   existing `route_payment` strategy using currently open rails with enough
   remaining principal capacity, and the remaining SLA budget. An accepted route
   is pinned; downstream hops wait for service and capacity. Capacity is consumed
   and the fee charged only when each hop actually departs. Zero-latency hops
   settle immediately, allowing an entire chain to finish in this minute before
   the next payment is considered. No future capacity is reserved.
5. Marks every still-incomplete payment whose inclusive deadline is this minute
   as an SLA failure, exactly once. Waiting payments expire immediately. An
   already-departed hop drains to settlement: a final hop completes late; an
   intermediate hop settles before the payment expires. No further hops depart
   after an SLA failure. Physical work already in flight is never erased.

This deliberately simple FIFO, current-state strategy is not a joint optimizer
and does not predict future closures or congestion. Routing acceptance does not
promise delivery. Deadline zero still permits a same-minute zero-latency route.
Rejections count as immediate service/SLA failures, separately from deadline
failures. Failed work can incur fees and hop volume without completing end-to-end.

## Controls, replay and storage

The simulator starts paused. `start` and `pause` only gate `tick`; a paused `tick`
does nothing. `step` always processes exactly one minute, leaving the run/pause
flag alone. `advance_ticks` repeats `step` without collecting an unbounded report.
A caller implements speed by choosing when/how often to call these methods; the
model never accepts wall-clock deltas. `restart(seed)` resets time, RNG, metrics,
active payments and history, keeping the scenario and returning to paused.

Each step returns its complete ordered event batch with contiguous event sequence
numbers. Consumers can stream these externally; the simulator retains only a
configurable recent-event ring (zero disables retention). Completed, expired and
rejected instructions are removed. The active set is explicitly bounded, each
accepted path is simple, and recurring schedules store no historical slots.
Persistent storage is bounded by the scenario, active limit, path length and event
limit, independent of elapsed time. Caller-retained reports are caller-owned.

Time, sequence counters and objective totals use checked `u128` arithmetic. There
is no configured end horizon. Literal infinite precision is incompatible with
bounded storage: exhausting a numeric field returns an explicit error and leaves
that entire tick, including RNG and events, unchanged. Previously successful ticks
in `advance_ticks` stay committed. This is an explicit numeric limit, not silent
wrapping or saturation. Only the specified 64-bit RNG wraps intentionally.

## Accounting invariants

- Generated count and volume equal completed + expired + rejected + active.
- SLA failures equal rejected + expired + late completions + overdue active work.
- Per-rail departed principal minus settled principal equals principal in flight;
  the analogous hop-count identity also holds.
- Routing cost equals the sum of actual departed-hop fees across rails.
- Current per-minute usage never exceeds capacity and is zero on closed services.
- Active sequences are strictly increasing; active records are either unexpired
  queued/ready work or a single outstanding hop. Terminal work is not retained.
- Same scenario, seed and processed ticks produce identical state and event
  sequences, regardless of pauses, caller speed, or tick grouping.

## Dependency references

Detected Rust/Cargo 1.97.1, edition 2024; Ratatui 0.30.0 and Crossterm 0.29.0 stay
locked and are unused by this module. No dependency is added. Context7 returned
stable standard-library documentation, not a 1.97.1-specific snapshot. The APIs
are checked by compiling on the installed toolchain:
[VecDeque ring buffer (`push_back` / `pop_front`)](https://doc.rust-lang.org/stable/std/collections/struct.VecDeque.html),
[modular addition (`wrapping_add`)](https://doc.rust-lang.org/stable/std/primitive.u32.html#method.wrapping_add),
[modular multiplication (`wrapping_mul`)](https://doc.rust-lang.org/stable/std/primitive.i64.html#method.wrapping_mul),
and [checked addition (`checked_add`)](https://doc.rust-lang.org/stable/std/primitive.u16.html#method.checked_add).
