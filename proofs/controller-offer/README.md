# Nested controller offers

The mouth lifecycle exposed a core defect on `e75d413b`: a controller arm
starting directly with another resolver-backed route returned that child's
first label without evaluating its resolver. Right/Right selected label 2
instead of label 3. A fresh nested region and a completed enclosing roll could
also prefer an old descendant over the parent decision or align to an interior
arm member instead of the actual entry.

The additional peer regression exposed another materialization defect: a real
send entry received synthetic terminal metadata with wire color zero. The strict
branch-send identity guard correctly rejected it. Materialization now dispatches
on the resident action and retains the real send/receive/local descriptor;
synthetic arm metadata is restricted to actual terminal arms. The identity guard
is preserved. This contract is checked by wire-send, dropped-future and payload
schema regressions rather than a duplicate mathematical payload model.
Branch-send validation also uses the existing live-arm predicate: a completed
visit's decision is history, while a differing decision in a live visit still
rejects the send. Fresh-roll preflight continues to validate the descriptor's
complete selected ancestry before publishing any progress.

Controller descent now transfers the existing affine preview into the immediate
child offer. It commits no intermediate decision. The selected event descriptor
supplies its whole ancestry to the existing preflight/publish transaction. Every
fresh locally controlled resolver row emits its successful audit exactly once
after event commit. The conflict-chain scan prioritizes the outermost pending
ancestor, alignment requires an actual route entry, and a completed enclosing
visit owns its reentry. Public API, persistent endpoint storage, wire format,
capacities and external dependencies are unchanged.

`ControllerOffer.lean` proves selection rules for arbitrary finite decision trees
and conflict chains, including parent rejection, ancestor decision provenance
and parent priority. It also checks the exact-entry and completed-envelope guards
and the reproduced label selection. All 13 theorems are kernel checked with only
`propext` and `Quot.sound` admitted in their axiom closure. The Z3 model checks
nine negated guard obligations as UNSAT and retains two SAT witnesses of the
old priority and arm-membership shortcuts.

These are source-linked mathematical models, not a universal refinement proof
of the Rust executor. Descriptor decoding, affine preview restoration, terminal
rejection and atomic event publication are checked by the permanent runtime
regressions and the existing refinement gates. Physical I/O, scheduling fairness
and audio/display synchronization are outside these supplemental proofs.

Run `bash proofs/controller-offer/check.sh [evidence-directory]`. The runner checks
the exact source inventory and SHA-256 identities, pins Lean 4.30.0, audits every
printed axiom closure and rejects Z3 errors, unknown results or count drift. It
is part of the final-form CI gate. The Rust regressions are registered as
`nested_resolver_self_continuations`; they exercise the real projected kernel
without choreography macros or a substitute state machine.

Local qualification on 2026-10-03 passed 873 workspace tests (eight explicit
exporters/measurement cases ignored by the default run), strict workspace
all-target Clippy and all eight new strict-provenance Miri regressions. The
canonical Lean gate passed 709 static/506 generated theorems, 182 parallel and
36 causal correspondences, runtime atomic-failure cases and the public-operation
kernel. The supplemental checker passed 13 kernel theorems and Z3's 9 UNSAT/2
SAT checks. The thumbv6m no-default release rlib totals 87,856 bytes; the three
host runtime samples peak at 2,639 stack bytes and 5,306 modeled SRAM bytes,
within the unchanged ceilings. These sample figures are not a measurement of
the complete robot's peak memory. The MCU consumer passed 63 tests, host/native
Clippy and its pinned binary-SDK link. No firmware was flashed during core
qualification. Complete Kani and final-form CI remain the publication gates.

Evidence: `/tmp/hibana-controller-final-workspace-evidence.wbAVzp`,
`/tmp/hibana-nested-final-miri-evidence.oyEMNn`,
`/tmp/hibana-controller-lean-evidence.gP8KCm`,
`/tmp/hibana-controller-offer-proofs.ujrPlI`,
`/tmp/hibana-controller-resource-evidence.CRmPoe`. Rust products were removed
after each validation, including failed diagnostic runs.
