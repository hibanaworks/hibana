# Local verification, 2026-10-04

Base: `9fbb84cdc932cbd0a81ee995a8689393f322763e` on
`development/rolled-route-ownership`.

- Internal core tests: 470 passed, 8 already-ignored tests remain ignored.
- Root integration/unit targets and UI harness: all 35 targets pass; the UI
  harness separately checks 90 accepted/rejected compilation fixtures.
- New public diagnostic tests: 7 passed, including the original passive
  collector rejection and its explicit-notification repair.
- Repository surface tests: 180 passed across 8 targets.
- Differential witness test: 32,805 structured four-event graphs, no acceptance
  difference and no invented sender/receiver/lane witness.
- Strict Clippy for the library and new public diagnostic target: passed.
- `thumbv6m-none-eabi`, no default features: passed.
- Rustdoc, source-file budget, maintainability budget and explicit public API
  allowlist checks: passed.
- Lean 4.30.0: three scoped witness/acceptance theorems checked.
- Z3: 32 bounded symbolic checks of report-preservation and first-failure.

Expected Rust 1.95 compiler snapshots were refreshed for the additional
explanation. No invalid fixture was made valid or removed. The existing
projection acceptance functions and runtime descriptor construction remain
unchanged; failure reporting calls the same gate before deriving witnesses.

The formerly opaque QUIC fragments now identify receive collector role 28,
scope 2, and delivery collector role 29, scope 1. Explicit branch/terminal
notifications repair source and receive fragments under the same projection
rules. Publication integration and full QUIC stream reuse remain separate work.

These results do not claim a complete Hibana Rust proof, all embedded hardware
qualification, all QUIC interop, or completion of remote Kani/final-form CI.
