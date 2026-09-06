# payment-routing

A small Rust application for exploring a deterministic, synthetic payment network in a Ratatui terminal interface. Browse institutions, shared payment rails, payment instructions and aggregate statistics.

Run from this directory with Rust and Cargo installed (verified with Rust/Cargo 1.97.1):

```sh
cargo run --locked -- --demo
```

The first build downloads dependencies from crates.io. Later builds can use `--offline` after `--locked`. Ratatui is pinned to 0.30.0, and `Cargo.lock` fixes the dependency graph. No credentials, server, database, scenario download or solver is needed.

Use an interactive terminal with at least **80 columns × 18 rows**; 80 × 24 shows every demo payment at once. Smaller terminals show a resize message and still accept quit keys. Resizing redraws the active view. The app restores the terminal on normal exit and ordinary errors; Ratatui supplies the panic restoration hook.

| Key | Action |
| --- | --- |
| Tab / Shift-Tab | Next / previous view, wrapping at the ends |
| Left / Right or h / l | Previous / next view |
| 1–4 | Overview, institutions, rails, payments |
| Up / Down or k / j | Previous / next row, stopping at the ends |
| Home / End | First / last row |
| q / Esc / Ctrl-C | Exit |

Each table remembers its selected row and scrolls to keep it visible. Run `cargo run --locked -- --help` for usage. No arguments show help; unknown arguments return an error. The demo rejects redirected input or output before changing terminal modes.

## Demo scenario

Institutions are fictional. The demo uses recognizable U.S. payment-rail names: **RTP, FedNow, ACH and Fedwire**. All rail membership, topology, fees and settlement times are synthetic scenario inputs, not verified real-world network data or operating rules. Capacity limits, availability schedules and other network rules are not modeled. All money is USD, stored as integer cents. Loading the fixture always produces the same records in the same order.

| Statistic | Expected value |
| --- | ---: |
| Institutions | 6 |
| Payment rails | 4 |
| Payments awaiting routing | 12 |
| Opening liquidity | USD 1,000,000.00 |
| Payment volume | USD 225,001.50 |
| Largest payment | USD 75,000.00 |

The institutions view shows identifiers, names and opening balances. Rails show their members, fixed fee inputs and settlement minutes (0 means immediate only in this synthetic scenario). Payments show sender and receiver institution IDs, amounts and their awaiting-routing state. The overview computes its totals from the loaded data.

Payments are unassigned instructions. The application does not choose routes, assess feasibility, incur fees, move funds or settle payments. There is no routing optimization or external optimization solver. Scenario-file import, multiple currencies and execution are outside this foundation.

## Code layout

- `src/network.rs`: terminal-independent records, reference validation and exact aggregate calculations. Empty collections are supported. Totals use `u128` to safely sum `u64` amounts.
- `src/demo.rs`: the built-in deterministic fixture.
- `src/lib.rs`: exports the domain and fixture for reuse without UI types.
- `src/app.rs`: selected view, per-table state and keyboard handling.
- `src/ui.rs`: Ratatui widgets and money formatting.
- `src/main.rs`: command-line selection, validation, terminal lifecycle and blocking event loop.

There is one application crate, no async runtime, and no background refresh loop. Domain validation checks identifiers, rail membership and payment endpoints; it does not require a route or sufficient funding.

## Verification

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run --locked -- --demo
```

Tests cover deterministic fixture totals, invalid references and amounts, wide aggregate sums, keyboard navigation and quit handling, CLI process behavior, and actual Ratatui `TestBackend` rendering at 80 × 18 and 80 × 24. They also check scrolling, empty views and small-terminal rendering.

For the interactive check, compare the overview with the table above, visit all four views, use End in Payments to select P012, then Home to return to P001. Quit and confirm the normal shell returns. Repeat with Esc and Ctrl-C to verify each exit path. A real pseudo-terminal launch and terminal-mode comparison are required when changing lifecycle code; buffer tests alone cannot verify cleanup.

Foundation verification on macOS with Rust/Cargo 1.97.1: all 20 tests passed, formatting passed, and Clippy passed with warnings denied. The documented demo command launched at 80 × 24 and 80 × 18; navigation and payment scrolling worked. Separate q, Esc and Ctrl-C runs each exited with status 0, emitted alternate-screen cleanup, and left `stty -g` identical to its value before launch.

Version-specific API references retrieved through Context7: [Terminal initialization and restoration (`src/init.rs`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/src/init.rs), [Table construction and layout-cache changes (`BREAKING-CHANGES.md`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/BREAKING-CHANGES.md), and [Crossterm version re-export (`ratatui-crossterm/README.md`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/ratatui-crossterm/README.md).
