# Immutable descriptor facts within event admission

`EventCursor::event_enabled` reads one checked `LocalEventRow` for an admission. The row owns the immutable action identity, successor, dependency and conflict facts. The live completion, route selection, reentry and lane-head checks still run for every admission; there is no persistent metadata or eligibility cache.

The Lean observation-equivalence theorem quantifies over immutable row and lane-step readers and an arbitrary live-check continuation with observer state. It shows that sharing the static row preserves the result, certificate and live observations. Missing rows and identity mismatches still reject before live checks. A later admission invokes its current continuation even when the static descriptor is unchanged.

Z3 covers the six typed commit identity fields and arbitrary live-check functions, including their short-circuit observation state. The optional route arm has an explicit presence discriminator; absent payload bits have no semantic identity, while `None` and `Some` always differ. It checks equivalent result/certificate/observations, rejection of identity mismatch (including a presence-only mismatch), missing row, invalid lane and blocked dependency. SAT witnesses establish an accepted operation, acceptance despite different unused `None` payload bits, a change from blocked to eligible after live progress, and unsoundness if the immutable-reader premise is removed.

The premise is supplied by Rust's immutable static descriptor references and checked descriptor construction. The model does not prove arbitrary Rust effects or the implementation of every live predicate; those remain covered by the production descriptor/kernel Lean artifacts, cursor/reference corpus, malformed-descriptor tests, and atomic commit tests. The focused Rust regressions exercise all commit identity mismatches without mutation and a real predecessor commit followed by a fresh admission.

Run `bash proofs/event-admission/check.sh`. This executes the Lean kernel and Z3 and compares their actual results. It performs no source-name or source-path inspection.
