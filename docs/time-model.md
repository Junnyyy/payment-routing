# Discrete execution planning

The scheduled API adds a finite timetable to the existing synthetic USD network.
Static `route_payment` and `optimize_batch`, their inputs, and the viewer retain
their existing behavior. There is no real clock, timezone, calendar, recurring
schedule, settlement simulation, external solver or implicit time horizon.

- Time is an absolute nonnegative integer minute (`u64`). Each `TimedPayment`
  wraps an existing instruction with its earliest execution (arrival/release)
  minute and an optional inclusive absolute arrival deadline.
- Each `RailDeparture` opens one rail at one exact minute. Unlisted minutes are
  closed. Every member pair and both directions share this departure opportunity.
  An empty timetable permits no transfers, including on otherwise available rails.
- A slot optionally overrides the base fee and latency. These values are sampled
  at departure and remain fixed for the hop even if the rail has no later slots.
  Omitted fee/latency inherit the rail inputs. Latency is still `u32` minutes;
  overflowing absolute arrivals are rejected before search.
- A slot's optional capacity is a principal budget shared by every hop at that
  departure, excluding fees. `None` is unlimited and zero permits no use. It is
  a departure budget, not in-flight occupancy. Different slots have separate
  budgets; they do not borrow, net or roll unused capacity forward.
- Static rail availability is still a hard gate; a timetable cannot reopen a
  disabled rail. Per-hop amount ceilings and whole-batch capacity remain hard
  constraints. Every hop consumes its entire unchanged principal from both the
  slot and rail batch budgets. Batch capacity never replenishes with time.
- Waiting is free and unrestricted at the sender and at intermediaries. The next
  hop can depart at or after arrival; zero-latency same-minute chains are allowed.
  The underlying `Payment.max_delivery_minutes` limits final arrival minus
  release, including all waiting. The optional absolute deadline also applies.
  A deadline before release is valid input but infeasible, not a validation error.
- Plans contain one unsplit, continuous timed route per supplied instruction,
  plus exact fees and principal usage for every rail and departure, even unused
  ones. Inputs, opening balances and budgets are never changed. Stored network
  payments are validated but only the supplied batch is optimized.

The objective is minimum summed fee, then summed elapsed time from release,
total hop count, and lexicographic hop sequences in payment-ID order. Hop order
compares `(rail_id, sender, receiver, departure_minute, arrival_minute, fee_cents)`;
the last two fields are fixed by the slot. All aggregate money and elapsed-time
sums use `u128`. This produces a deterministic answer independent of input order.

Exact search must retain different departure choices and different prefixes,
including more expensive and later-arriving ones. A later departure may arrive
earlier when latency varies; earlier arrival may allow a connection but cost a
contested slot. Neither earliest-departure routing nor individually cheapest
scheduling is a valid global reduction.

Enumerating simple institution paths is sufficient *because waiting is allowed*.
Replace any return to an earlier institution with waiting there until the same
downstream departure. This preserves the rest of the timed route and its arrival,
never increases nonnegative cost or either capacity usage, and reduces hop count.
This argument would need revisiting if holding limits, waiting costs, negative
fees or institution liquidity were introduced.

The search is finite in supplied slots, independent of gaps between timestamps,
but exponential in network size, departure alternatives and batch assignments.
It has no cutoff, approximation, sampling or hidden smaller horizon. Infeasibility
means no full plan exists within the supplied timetable, not for all possible
future times. Verification must report impractical exact fixtures explicitly.

Detected versions: Rust/Cargo 1.97.1, edition 2024; Ratatui 0.30.0 and Crossterm
0.29.0 remain locked and unused by the scheduled module. Context7 exposes stable
Rust documentation rather than a 1.97.1-specific snapshot. API references:
[checked integer addition (`u64::checked_add`)](https://doc.rust-lang.org/stable/std/primitive.u64.html#method.checked_add),
[Vec as a stack (`Vec::push`)](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.push),
[pop](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.pop), and
[derived ordering (`Ord`)](https://doc.rust-lang.org/stable/std/cmp/derive.Ord.html).
Compilation and checks use the detected installed toolchain.
