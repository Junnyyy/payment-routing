# Project guidance

- Keep `src/network.rs` and `src/demo.rs` independent of Ratatui and Crossterm. The library owns data, validation and aggregate calculations; the binary owns interaction and rendering.
- This foundation is synthetic and USD-only. Amounts are integer cents (`u64`); aggregate sums use `u128`. Opening balances are input data, never reduced by loading or viewing payments.
- Rails describe shared services and explicit membership, not directed graph edges. Payments are unassigned instructions; validation does not promise a feasible route or sufficient liquidity.
- `demo_network()` must remain deterministic, including collection ordering. The fixture has 6 institutions, 3 rails, 12 payments, USD 1,000,000.00 opening liquidity and USD 225,001.50 payment volume.
- Detect dependency versions from `Cargo.toml` and `Cargo.lock`, then query Context7 before changing library API usage. Ratatui is pinned to 0.30.0; use its Crossterm re-export to avoid conflicting event types ([Crossterm Version and Re-export](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/ratatui-crossterm/README.md)). With default features disabled, retain `layout-cache` ([Breaking changes](https://github.com/ratatui/ratatui/blob/ratatui-v0.30.0/BREAKING-CHANGES.md)).
- Run relevant tests between changes, make atomic checkpoint commits, and create a PR only when directed. No routing optimization, settlement engine or external solver belongs in this foundation.
