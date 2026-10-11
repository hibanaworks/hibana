# Local verification, 2026-10-04

## Qualification-source correspondence followup

Full CI for `5ba54b75` (run `37193216883`) passed Rust and the main Lean
gate, then rejected the stale elastic-roll implementation snapshot for
`src/global/const_dsl.rs`. The new diagnostic reexport also changes the
qualified `lowering/seal.rs` source identity. This is a proof-harness
integration failure; runtime code is unchanged by this followup.

Original manifests and qualification sources remain intact. The shared
`source_identity.py` checker permits only the two exact diagnostic-only
additions, checks original/current hashes, and rejects four altered or
missing-addition fixtures. Both existing proof runners use the same identity
bridge. Previous live-source and distributable manifests are archived before
recording the current 367-file source tree.

Fresh local checks completed successfully:

- Full portable elastic-roll replay, including compiler participant masks,
  route-path refinement, passive-child windows and projection-conflict reuse;
  Lean 4.30.0 and Z3 4.16.0, with fresh temporary compiled modules.
- Rolled-route ownership: seven Lean files; Z3 12 UNSAT obligations and
  19 SAT premises/historical witnesses.
- Projection diagnostics: all three Lean theorems and 32 bounded Z3 checks.
- Original qualification/history correspondence, live source-tree identity,
  and the two-file/four-mutation diagnostic correspondence check.
- Text integrity, source-file/maintainability budgets, underscore-discard
  hygiene and `git diff --check`.

The identity bridge proves exact preservation of the qualified gate bodies;
it is not a universal Rust refinement theorem for new diagnostic code.
Remote followup run `37195668447` passed Kani, Rust, the main Lean gate,
the full portable proof replay, and target footprint checks. It then rejected
the stale README rlib measurement: diagnostics changed the complete
`thumbv6m-none-eabi` archive from 88,013 to 93,899 bytes. The linked matrix,
5,290-byte modeled runtime SRAM, and release ceilings were unchanged. The
README now records the exact CI observation; the measurement gate remains
intact. Full remote CI for this documentation correction is still required.
No MCU/runtime API,
production Rust, acceptance gate, memory reservation or rejection was changed.

## Original diagnostic implementation verification

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

The first remote final-form run reached the UI snapshots and rejected 17
expectations because local Rust 1.95 lacked `rust-src`, while CI included it.
The new diagnostic text was identical; the standard-library panic expansion
backtrace differed. Installed the matching official `rust-src`, regenerated
only the affected Rust 1.95 stderr expectations, and reran all 90 UI cases
without overwrite successfully. Production code and accept/reject results are
unchanged by this follow-up.
