# Development

## Code layout

- `src/network.rs`: terminal-independent records, reference validation and exact aggregate calculations. Empty collections are supported. Totals use `u128` to safely sum `u64` amounts.
- `src/demo.rs`: the built-in deterministic fixture.
- `src/routing.rs`: exact single-payment routing, independent of terminal rendering.
- `src/batch.rs`: exact joint routing, static shared-capacity accounting and canonical batch results.
- `src/scheduling.rs`: finite timetables, timed constraints, exact joint routing/scheduling and execution-plan witnesses.
- `src/simulation.rs` and `src/simulation/`: seeded arrivals, recurring services, execution, bounded state, accounting invariants and clock-independent controls.
- `src/lib.rs`: exports the domain and fixture for reuse without UI types.
- `src/observation.rs` and `src/operations.rs`: optional search evidence, twin transactions and bounded payment dossiers, without terminal types.
- `src/app.rs`: simulation controls, selected views, per-table state, search/filter and keyboard handling.
- `src/ui.rs`: Ratatui widgets and money formatting.
- `src/main.rs`: command-line selection, validation, terminal lifecycle and paced event loop.

There is one application crate and no async runtime. The console paces ticks while running and blocks on input while paused. Domain validation checks identifiers, rail membership and payment endpoints; it does not require a route or sufficient funding.

## Verification

```sh
cargo fmt --check
cargo test --locked --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo run --locked --example route_one
cargo run --locked --example route_batch
cargo run --locked --example schedule_batch
cargo run --locked --example simulate -- 10000 42
cargo build --locked
python3 scripts/test_console.py
```

Tests cover deterministic fixture totals, invalid references and amounts, wide aggregate sums, keyboard navigation and quit handling, CLI process behavior, and actual Ratatui `TestBackend` rendering at 80 × 18 and 120 × 32. They also check identity-preserving selection, payment/rail investigation, empty views, in-flight SLA failures and small-terminal rendering. The operations guide records the current PTY and scenario verification.

Historical checkpoints are recorded in [verification history](verification-history.md).

## Terminal renders

`CONSOLE_SNAPSHOT_DIR=target/console-screens cargo test --locked --bin payment-routing`
writes deterministic text renders. The README illustration renders
`watch-demo-80x18.txt` (disruptions, seed 42, minute 12) as a PNG using Pillow 12.3.0 and Menlo 24px, on a 15×32px cell grid.
It is test-rendered terminal output, not a live screen capture.
Rendering reference: [ImageDraw.text](https://github.com/python-pillow/pillow/blob/12.3.0/docs/reference/ImageDraw.rst).

See [scheduled examples](scheduled-examples.md) for enumerated timetable cases.
