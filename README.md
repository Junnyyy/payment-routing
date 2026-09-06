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

Institutions are fictional. The demo uses recognizable U.S. payment-rail names: **RTP, FedNow, ACH and Fedwire**. All rail membership, topology, fees and settlement times are synthetic scenario inputs, not verified real-world network data or operating rules. The demo sets all rails available, with no transaction ceilings or delivery deadlines. Aggregate capacity, availability schedules and other network rules are not modeled. All money is USD, stored as integer cents. Loading the fixture always produces the same records in the same order.

| Statistic | Expected value |
| --- | ---: |
| Institutions | 6 |
| Payment rails | 4 |
| Payments awaiting routing | 12 |
| Opening liquidity | USD 1,000,000.00 |
| Payment volume | USD 225,001.50 |
| Largest payment | USD 75,000.00 |

The institutions view shows identifiers, names and opening balances. Rails show their members, fixed fee inputs and settlement minutes (0 means immediate only in this synthetic scenario). Payments show sender and receiver institution IDs, amounts and their awaiting-routing state. The overview computes its totals from the loaded data.

The rails appear in this stable order. **Every membership, fee and timing value below is synthetic.**

| ID | Display name | Synthetic members | Synthetic fee (USD) | Synthetic settlement minutes |
| --- | --- | --- | ---: | ---: |
| RTP | RTP | ALP, BRK, CDR, DLT | 0.25 | 0 |
| FEDNOW | FedNow | ALP, BRK, CDR, DLT | 0.25 | 0 |
| ACH | ACH | ALP, BRK, CDR, DLT, ELM, FLD | 0.05 | 1440 |
| FEDWIRE | Fedwire | ALP, DLT, ELM | 15.00 | 30 |

RTP, ACH and Fedwire retain the existing instant, batch and wire fixture inputs respectively. FedNow is a fourth rail that reuses the synthetic instant membership, fee and timing inputs. The shared values are a demo choice and do not imply that RTP and FedNow operate identically. Institutions, balances and payment instructions are unchanged.

Payments remain unassigned instructions in the Stage 0 viewer. The library also exposes `routing::route_payment(&network, &payment)` for read-only, minimum-fee routing through shared rails. It returns `Ok(Some(route))`, `Ok(None)` when no route exists, or a validation error for malformed input. It never incurs fees, moves funds or settles payments. Scenario-file import, multiple currencies and execution remain outside this foundation; no external optimization solver is used.

## Single-payment routing

Each rail permits a transfer between any two distinct members, in either direction. A route carries the full USD principal on every hop, charges the fixed rail fee separately on every hop, and sums settlement minutes. Fees and latency totals use `u128`. Intermediaries can forward the principal; this is a static path model, not a claim about real bank operating rules or funding. Opening balances are descriptive and are neither used as routing capacity nor changed.

Three fields express optional static restrictions without changing existing demo values, ordering or totals:

| Field | Meaning |
| --- | --- |
| `Rail.available` | Whether this rail can be used for this snapshot; the search cannot wait for it to open. |
| `Rail.max_amount_cents` | Inclusive positive ceiling on each hop's principal, excluding fees; `None` means no ceiling. |
| `Payment.max_delivery_minutes` | Inclusive budget for the entire route's latency; `None` means no deadline and `Some(0)` requires zero latency. |

Currency compatibility is the existing USD-only invariant. There is no FX conversion, mixed-currency input, fee deduction from principal, liquidity reservation, payment splitting, or batch optimization. A valid payment may be supplied separately from `network.payments`; its ID is metadata, not a lookup key. Routing first validates the entire network and then that instruction. Structural validation does not require route feasibility.

The exact search enumerates simple institution paths, excluding unavailable or over-limit rails and prefixes exceeding the delivery deadline. It minimizes total fees, then breaks ties by total latency, hop count, and the lexical sequence of `(rail_id, sender, receiver)`. Input collection order does not decide the result. All fees and latencies are nonnegative, so removing a cycle never worsens either and improves hop count; an optimum is therefore simple. Pruning only strictly more expensive prefixes preserves ties. No cheapest-prefix-per-node shortcut is used because a more expensive, faster prefix can be necessary to meet a deadline. Worst-case work is exponential and recursion depth is bounded by institution count; this is intended for small synthetic networks, not large production graphs.

Run the deterministic counterexamples without entering terminal mode:

```sh
cargo run --locked --example route_one
```

The example routes a 100-cent A-to-D payment. The A-B alternatives cost 1 cent / 9 minutes and 5 cents / 3 minutes; B-D costs 1 cent / 2 minutes. A-C-D costs 7 cents / 2 minutes, and A-D costs 9 cents / 1 minute. Those are all simple institution paths; the independently calculated optima are:

| Cumulative scenario changes | Optimal rails | Fee (cents) | Latency (minutes) |
| --- | --- | ---: | ---: |
| No deadline | cheap-ab, bd | 2 | 11 |
| Add a 10-minute deadline | fast-ab, bd | 6 | 5 |
| Limit B-D to 99 cents | ac, cd | 7 | 2 |
| Also mark C-D unavailable | direct | 9 | 1 |
| Tighten the deadline to zero | Unreachable | — | — |

The example asserts each expected answer. More scenarios, including invalid instructions and disconnected endpoints, live in `tests/routing.rs`. The original `cargo run --locked -- --demo` still launches the Stage 0 viewer with its four views and awaiting-routing instructions.

## Code layout

- `src/network.rs`: terminal-independent records, reference validation and exact aggregate calculations. Empty collections are supported. Totals use `u128` to safely sum `u64` amounts.
- `src/demo.rs`: the built-in deterministic fixture.
- `src/routing.rs`: exact single-payment routing, independent of terminal rendering.
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
cargo run --locked --example route_one
cargo run --locked -- --demo
```

Tests cover deterministic fixture totals, invalid references and amounts, wide aggregate sums, keyboard navigation and quit handling, CLI process behavior, and actual Ratatui `TestBackend` rendering at 80 × 18 and 80 × 24. They also check scrolling, empty views and small-terminal rendering.

Routing adds 15 focused tests for known optima, infeasible cheaper routes, inclusive constraint boundaries, zero-cost cycles, deterministic tie-breaking, malformed inputs, external instructions, unchanged demo data and sums beyond `u64` fees / `u32` latency. Two oracle tests make 37,768 comparisons: 4,096 assignments of absent/free/slow/fast two-member services with four deadlines and both endpoint directions, plus 625 assignments of overlapping shared services with closures, ceilings, parallel connections, two amounts and four deadlines. The independent oracle uses dynamic programming over exact hop count and elapsed time, permits cycles, and applies deadlines only at the final scan; it shares no search or pruning code with the router. Returned route witnesses are separately checked for continuity, membership, constraints and exact totals. A compiled documentation example checks the public API.

Development used red/green iterations: the initial API failed five known-route tests; exact shared-rail search made them pass. Adding the constraint scenarios then produced five failures (closure, amount ceiling, faster-prefix deadline, zero deadline and zero-ceiling validation); enforcing those rules made them pass. The wide-integer, cycle and independent-oracle checks found no further counterexamples. All 20 original Stage 0 tests remain intact; the TUI, terminal lifecycle, CLI and dependency files are unchanged.

Final routing verification on 2026-09-06: all 38 tests (including the documentation example) passed with `--locked --offline`, formatting and Clippy with warnings denied passed, and the executable example asserted all five expected outcomes. The unchanged Stage 0 rendering and CLI tests passed in that same run; terminal lifecycle code was not changed or re-verified in a PTY for this routing addition.

For the interactive check, compare the overview with the statistics table above and visit all four views. Confirm that Rails shows all four names and complete IDs, the synthetic-input label, and the rail values listed above at both 80 × 18 and 80 × 24. Use End in Rails to select Fedwire. Use End in Payments to select P012, then Home to return to P001. Quit and confirm the normal shell returns. Repeat with Esc and Ctrl-C to verify each exit path. A real pseudo-terminal launch and terminal-mode comparison are required when changing lifecycle code; buffer tests alone cannot verify cleanup.

Rail vocabulary verification on 2026-09-06 on macOS with Rust/Cargo 1.97.1: all 20 tests passed, formatting passed, and Clippy passed with warnings denied. The documented demo command launched at 80 × 24 and 80 × 18; all four rail identities and synthetic inputs displayed, navigation and payment scrolling worked. Separate q, Esc and Ctrl-C runs each exited with status 0, emitted alternate-screen cleanup, and left `stty -g` identical to its value before launch.

Version-specific API references retrieved through Context7: [Terminal initialization and restoration (`src/init.rs`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/src/init.rs), [Table construction and layout-cache changes (`BREAKING-CHANGES.md`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/BREAKING-CHANGES.md), and [Crossterm version re-export (`ratatui-crossterm/README.md`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/ratatui-crossterm/README.md).

Routing verification used the detected Rust/Cargo 1.97.1, edition 2024, with Ratatui still locked to 0.30.0 and no added dependencies. Context7 returned the stable standard-library documentation rather than a 1.97.1-specific snapshot: [Using Vec as a stack (`Vec::push`)](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.push), [`Vec::pop`](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.pop), and [Derive Ord (`std/cmp/derive.Ord.html`)](https://doc.rust-lang.org/stable/std/cmp/derive.Ord.html). The implementation uses stable APIs verified by compilation and tests on the installed toolchain.
