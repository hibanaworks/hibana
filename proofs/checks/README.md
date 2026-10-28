# Implementation checks

These checks are grouped by the part of Hibana they model. Paths in the tables
refer to the current implementation. The Lean and SMT files prove mathematical
properties of those algorithms. Production tests and generated-input checks
exercise their connection to Rust. A model theorem alone is not a proof that
every Rust execution follows it.

## Projection

| Checks | Implementation and property |
| --- | --- |
| [endpoint-delegation](projection/endpoint-delegation/README.md) | [`g/source`](../../src/g/source): initial ownership, transfers, branch knowledge and actor order |
| [compiler-participant-mask](projection/compiler-participant-mask) | [`seal.rs`](../../src/global/compiled/lowering/seal.rs): exact set of participating roles |
| [passive-child-window](projection/passive-child-window) | [`seal/passive_child.rs`](../../src/global/compiled/lowering/seal/passive_child.rs): lower bounds with duplicate offsets |
| [projection-conflict-reuse](projection/projection-conflict-reuse) | [`projection.rs`](../../src/global/role_program/image_impl/projection.rs): reuse of an immutable conflict calculation preserves results and errors |
| [projection-diagnostics](projection/projection-diagnostics) | [`g/diagnostic.rs`](../../src/g/diagnostic.rs): rejected protocol classifications |
| [lane-allocation](projection/lane-allocation) | [`allocation.rs`](../../src/global/const_dsl/allocation.rs): choice-arm lane remapping |
| [message-identity](projection/message-identity) | [`event_rows.rs`](../../src/global/role_program/image_impl/event_rows.rs), [`FrameHeader`](../../src/transport.rs) and [`DescriptorImage`](../lean/Hibana/DescriptorImage.lean): static source positions distinguish message sites |
| [explicit-resource-join](projection/explicit-resource-join) | [`g/source/ownership.rs`](../../src/g/source/ownership.rs): independent resource work and its join |

`.github/scripts/check_projection_algorithms.sh` runs the participant,
passive-child, conflict-reuse, lane and framed-message identity checks.
The common Lean model and generated public projection tests separately compare
wire identities with source positions. Repeated visits to one site share an
identity and require FIFO delivery, nonduplication and run isolation.

## Protocol images

| Checks | Implementation and property |
| --- | --- |
| [arm-row-index](image/arm-row-index) | [`lane_image.rs`](../../src/global/role_program/image_impl/lane_image.rs): packed arm fields and bounds |
| [passive-parent-index](image/passive-parent-index) | [`metadata/passive_parent.rs`](../../src/global/role_program/image_impl/metadata/passive_parent.rs): parent lookup preserves checked scan results |
| [route-index](image/route-index) | [`metadata/route_index.rs`](../../src/global/role_program/image_impl/metadata/route_index.rs): route lookup, invalid keys and duplicates. [`program_ref/actors.rs`](../../src/global/compiled/images/image/program_ref/actors.rs): ancestor-arm queries from checked source intervals. [`blob_image/route_topology.rs`](../../src/global/role_program/image_impl/blob_image/route_topology.rs) and [`ref_access/route_topology.rs`](../../src/global/role_program/image_impl/ref_access/route_topology.rs): source-grammar interpolation and conditional compressed-entry models, with a bounded independent Rust source-shape checker; Rust serialization and source-marker correspondence remain separate checks. [`decision_state.rs`](../../src/endpoint/kernel/decision_state.rs) and [`commit_delta.rs`](../../src/endpoint/kernel/core/commit_delta.rs): complete ancestor chains, direct lookup by path length, unique scopes and the suffix owned by a new enclosing visit |
| [operation-index](image/operation-index) | [`ref_access/operation_index.rs`](../../src/global/role_program/image_impl/ref_access/operation_index.rs) and [`cursor/navigation.rs`](../../src/global/typestate/cursor/navigation.rs): exact message keys, candidate ranges and completion filtering; bounded ordinal-certificate checks |
| [route-column-access](image/route-column-access) | [`lane_image.rs`](../../src/global/role_program/image_impl/lane_image.rs): reads stay within their columns |
| [resolver-index](image/resolver-index) | [`resolver_index.rs`](../../src/global/compiled/images/image/resolver_index.rs), [`route_resolvers.rs`](../../src/global/compiled/images/image/route_resolvers.rs): compiled direct lookup and first-match preservation for sparse, unsorted and duplicate scope IDs |
| [dispatch-subtree](image/dispatch-subtree) | [`first_recv_dispatch.rs`](../../src/global/typestate/cursor/first_recv_dispatch.rs): traversal of the reachable dispatch subtree |

## Local execution

| Checks | Implementation and property |
| --- | --- |
| [event-admission](runtime/event-admission) | [`event_progress.rs`](../../src/global/typestate/cursor/scope_route/event_progress.rs) and [`select_candidates.rs`](../../src/endpoint/kernel/offer/select_candidates.rs): dependency/completion guards and candidate filtering before admission |
| [pending-events](runtime/pending-events) | [`cursor.rs`](../../src/global/typestate/cursor.rs), [`pending_events.rs`](../../src/global/typestate/cursor/pending_events.rs) and [`roll.rs`](../../src/global/typestate/cursor/scope_route/roll.rs): bitmap storage bounds, traversal and selected scope completion |
| [recv-lane-census](runtime/recv-lane-census) | [`contract.rs`](../../src/endpoint/kernel/recv/contract.rs): enabled receive lanes |
| [controller-offer](runtime/controller-offer) | [`kernel/offer`](../../src/endpoint/kernel/offer): controller selection, admitted resolver-entry identity, direct-send admission and conflict-chain order |
| [live-descendant-preview](runtime/live-descendant-preview) | [`first_recv_dispatch.rs`](../../src/global/typestate/cursor/first_recv_dispatch.rs): preview follows a live descendant |
| [ordinary-offer-ingress](runtime/ordinary-offer-ingress) | [`kernel/offer`](../../src/endpoint/kernel/offer): ordinary messages preceding a choice |
| [offer-prefix-dependency](runtime/offer-prefix-dependency) | [`kernel/offer`](../../src/endpoint/kernel/offer): preceding communication remains a dependency |
| [parallel-offer-ingress](runtime/parallel-offer-ingress) | [`kernel/offer`](../../src/endpoint/kernel/offer) and [`parallel.rs`](../../src/endpoint/parallel.rs): branch preview, independent fragments and one receive poll per direction in a call |
| [parallel-resolver-progress](runtime/parallel-resolver-progress) | [`decision_resolver`](../../src/endpoint/kernel/core/decision_resolver) and [`parallel.rs`](../../src/endpoint/kernel/core/parallel.rs): independent choices and borrowing a parallel continuation after a completed repetition |
| [nested-resolver-reentry](runtime/nested-resolver-reentry) | [`decision_resolver`](../../src/endpoint/kernel/core/decision_resolver) and [`scope_route/roll`](../../src/global/typestate/cursor/scope_route/roll): nested choices on reentry |
| [roll-reentry-selectors](runtime/roll-reentry-selectors) | [`source validation`](../../src/g/source) and [`send_preview.rs`](../../src/global/typestate/cursor/scope_route/send_preview.rs): distinguish repeated sends from a continuation |
| [roll-continuation-dependency](runtime/roll-continuation-dependency) | [`event_progress.rs`](../../src/global/typestate/cursor/scope_route/event_progress.rs): event membership and prerequisite completion are distinct |
| [rolled-route-ownership](runtime/rolled-route-ownership) | [`scope_route`](../../src/global/typestate/cursor/scope_route): retained choices, visit admission, reset bounds and continuation order |

Run a directory's `check.sh` from the repository root through the resource guard.
The rolled-route check also compares all 128 combinations of completion bits,
retained child completions and entry positions for a three-message parallel
fixture. Its Rust cursor results are compared with `completeAt` in
`ResetAlignment.lean`. The same Lean function generates the SMT truth tables;
overly strict and overly permissive rules must produce counterexamples.
This finite comparison supplements the general model lemmas and public
Endpoint tests; it does not establish correspondence for every protocol.

Checks without a shell entry point are invoked directly by the
[workflow](../../.github/workflows/quality-gates.yml) or by the projection runner
above. The common Lean models and generated Rust observations are checked by
`.github/scripts/check_lean_proofs.sh`.
