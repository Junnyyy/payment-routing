# Payment routing

Payment rails are the systems banks use to transfer money to one another,
such as ACH and Fedwire. Each has its own fees, timing and rules.
Routing means choosing how a payment reaches its destination.

A cheap payment route may be too slow. A fast route may run out of capacity.
When many payments share the same rails, routing each one affects the others.

This simulation lets you watch those tradeoffs unfold, compare strategies,
and inspect individual payments as demand and constraints change.

## Watch it

```sh
cargo run --locked -- --demo
```

Requires Rust and an interactive terminal of at least **80 × 18**.
No server, credentials or database needed.

![Watch view: rail capacity, queued payments and in-flight hops](docs/images/watch.png)

**Space** runs or pauses. **Tab** switches Watch / Compare. **Enter** inspects.
Use **c** to choose a scenario and **?** for all controls.

Watch shows rail capacity, queues and payments in flight. Compare shows outcomes
and fees for the same demand. Select a payment difference to inspect both journeys.

## Strategies

**Static** chooses the cheapest route using rails usable now. It keeps that route
and waits when a hop is blocked. It does not book future capacity, so waiting
can cause a deadline miss.

**Reserved** chooses a route and departure times, then books capacity for every
hop. It tries several payment orders, preferring more planned payments, then
lower fees. Its search has limits, so it can miss a workable plan.

A route's fee is the sum of its hop fees. Reserved plans must also satisfy:

```text
Booked amount per rail/minute ≤ capacity
Planned arrival ≤ payment deadline
```

Both strategies see the same demand; neither knows future disruptions, which
can invalidate plans. Compare delivery outcomes before fees. Spending less
can mean delivering fewer payments.

## Model

Fictional institutions share synthetic USD rails with fees, service windows,
capacity and payment deadlines. Scenarios add demand pressure and rail disruptions.
The rail names are familiar; their rules and timings are synthetic.
No real transfers occur, and opening balances are descriptive.

## Details

- [Demo guide](docs/operations-console.md): controls, metrics and inspection.
- [Strategy evaluation](docs/evaluation.md) and [results](docs/evaluation-report.md).
- [Simulation](docs/simulation.md), [reserved routing](docs/scalable-routing.md) and [disruptions](docs/disruptions.md).
- [Exact routing](docs/exact-routing.md), [timetables](docs/time-model.md) and [static fixture](docs/reference-network.md).
- [Benchmarks](benchmarks/README.md) and [development](docs/development.md).
