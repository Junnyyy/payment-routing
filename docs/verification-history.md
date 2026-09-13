# Verification history

Historical implementation checkpoints. For current checks, see [development](development.md).

### Historical routing checkpoints

The following records describe the original library/fixture stages; current
console verification is documented in the operations guide.

Routing adds 15 focused tests for known optima, infeasible cheaper routes, inclusive constraint boundaries, zero-cost cycles, deterministic tie-breaking, malformed inputs, external instructions, unchanged demo data and sums beyond `u64` fees / `u32` latency. Two oracle tests make 37,768 comparisons: 4,096 assignments of absent/free/slow/fast two-member services with four deadlines and both endpoint directions, plus 625 assignments of overlapping shared services with closures, ceilings, parallel connections, two amounts and four deadlines. The independent oracle uses dynamic programming over exact hop count and elapsed time, permits cycles, and applies deadlines only at the final scan; it shares no search or pruning code with the router. Returned route witnesses are separately checked for continuity, membership, constraints and exact totals. A compiled documentation example checks the public API.

Development used red/green iterations: the initial API failed five known-route tests; exact shared-rail search made them pass. Adding the constraint scenarios then produced five failures (closure, amount ceiling, faster-prefix deadline, zero deadline and zero-ceiling validation); enforcing those rules made them pass. The wide-integer, cycle and independent-oracle checks found no further counterexamples. All 20 original Stage 0 tests remain intact; the TUI, terminal lifecycle, CLI and dependency files are unchanged.

Final routing verification on 2026-09-06: all 38 tests (including the documentation example) passed with `--locked --offline`, formatting and Clippy with warnings denied passed, and the executable example asserted all five expected outcomes. The unchanged Stage 0 rendering and CLI tests passed in that same run; terminal lifecycle code was not changed or re-verified in a PTY for this routing addition.

For current interface verification, follow [the operations guide](operations-console.md#verification) and run `python3 scripts/test_console.py` after building. The older stage-specific verification records below and above describe those changes at their original checkpoints; current console coverage replaces the old viewer-specific keyboard/rendering assertions.

Rail vocabulary verification on 2026-09-06 on macOS with Rust/Cargo 1.97.1: all 20 tests passed, formatting passed, and Clippy passed with warnings denied. The documented demo command launched at 80 × 24 and 80 × 18; all four rail identities and synthetic inputs displayed, navigation and payment scrolling worked. Separate q, Esc and Ctrl-C runs each exited with status 0, emitted alternate-screen cleanup, and left `stty -g` identical to its value before launch.

Version-specific API references retrieved through Context7: [Terminal initialization and restoration (`src/init.rs`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/src/init.rs), [Table construction and layout-cache changes (`BREAKING-CHANGES.md`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/BREAKING-CHANGES.md), and [Crossterm version re-export (`ratatui-crossterm/README.md`)](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/ratatui-crossterm/README.md).

Routing verification used the detected Rust/Cargo 1.97.1, edition 2024, with Ratatui still locked to 0.30.0 and no added dependencies. Context7 returned the stable standard-library documentation rather than a 1.97.1-specific snapshot: [Using Vec as a stack (`Vec::push`)](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.push), [`Vec::pop`](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.pop), and [Derive Ord (`std/cmp/derive.Ord.html`)](https://doc.rust-lang.org/stable/std/cmp/derive.Ord.html). The implementation uses stable APIs verified by compilation and tests on the installed toolchain.

Batch implementation uses the installed Rust/Cargo 1.97.1, edition 2024, with Ratatui still pinned/locked to 0.30.0 and no dependency changes. Context7 supplied stable standard-library documentation rather than a 1.97.1-specific snapshot: [Using Vec as a stack (`Vec::push` / `Vec::pop`)](https://doc.rust-lang.org/stable/std/vec/struct.Vec.html#method.push), [Custom comparison sorting (`sort_unstable_by`)](https://doc.rust-lang.org/stable/std/bstr/struct.ByteStr.html#method.sort_unstable_by), and [Derive Ord (`std/cmp/derive.Ord.html`)](https://doc.rust-lang.org/stable/std/cmp/derive.Ord.html). Compilation and verification use the detected installed toolchain.
