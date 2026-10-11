The projected publication roll accepts CRYPTO, ACK, PTO, then a finite boundary.
Rust 1.95.0 release builds reproduced `offer / PhaseInvariant` when that boundary
arrived after a previous descendant visit. The standalone reproduction uses
Hibana's existing framed test transport and contains no QUIC or TLS dependency.
QUIC retains an independent capacity-one regression using its actual carrier.

`preview_selected_arm_for_scope_from_parts` returned a resident selection even
when `reentrant_route_arm_event_row_done` proved it complete. Descendant
materialization then tried to commit that previous selection against the new
poll's different arm. The complete row is now excluded from the preview, using
the same live-selection predicate as `CursorEndpoint`'s existing preview. A live
conflict continues to violate the contract. No queue, stack, resource ceiling,
public operation check, receive identity check, or preflight assertion changed.

The Lean and Z3 models describe this two-arm selection contract. `selected`
maps to the resident scope selection, `completed` to the existing event-row
completion predicate, and `ready` to the current single-arm poll mask. Waiting
and rejection remain distinct. The proofs establish actual authority for every
chosen arm, completed-visit rejection as authority, and preservation of live
conflict rejection. The historical wrong-arm example remains a SAT witness.
These source-linked models do not prove Rust memory safety or interop success.

The first CI run rejected the regression fixture's atomic wake flag under the
repository's existing test/runtime hygiene rule. The fixture now records actual
wake calls through a test-only mutex counter. Every pending poll still requires
at least one actual wake; the poll bound and all publication assertions remain
unchanged. Production source, descriptors, resource budgets and proof models are
unchanged. Debug, release/LTO and strict Clippy checks exercise the actual fixture.

Run `bash proofs/live-descendant-preview/check.sh [evidence-directory]` with
Lean 4.30.0 and Z3, then both debug and release `rolled_publication_exit` tests.
The checker audits eight theorem axiom
closures, and four UNSAT results plus one historical SAT result. The Rust test
also covers starting with CRYPTO, ACK, PTO, or the boundary and parking the
receiver before a changed arm. Physical UDP, performance and Docker are separate
validation requirements.

Fresh local validation on Rust 1.95.0 (2026-10-04 UTC): `cargo test --locked
--workspace -- --test-threads=1` passed 874 tests with 11 ignored; Clippy with
`--workspace --all-targets -- -D warnings` passed. The new test passes both debug
and release, including `CARGO_PROFILE_RELEASE_LTO=true` and
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`. The release rolled-resolver and nested
resolver suites also pass (22 tests total including this regression). Repository
tests use `RUSTFLAGS='--cfg hibana_repo_tests'`. Existing controller-offer proofs
still pass 13 Lean theorems, 9 UNSAT obligations and 2 historical SAT witnesses.
`evidence/` retains the new source/Lean/Z3 output, historical release failure,
and actual release/LTO execution log. The historical standalone harness resolved
Futures 0.3.34; the registered regression uses the repository's locked 0.3.31.

Current bounded measurements: thumb no-default release library 88,013 bytes
(previous 67cbf9f0: 87,856; increase 157), maximum measured GNU host runtime stack
2,519 bytes, modeled SRAM 5,218 bytes. Their unchanged budgets are 169,965,
3,663 and 8,954 respectively. The isolated route-arm projection pressure check
also passed its unchanged limits. These are local resource measurements, not
an interop throughput comparison or a claim of improved performance.
