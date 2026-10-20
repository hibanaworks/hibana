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
| [route-path-refinement](projection/route-path-refinement) | [`event_relations.rs`](../../src/global/const_dsl/event_relations.rs): route-path classes preserve membership equality |
| [projection-conflict-reuse](projection/projection-conflict-reuse) | [`projection.rs`](../../src/global/role_program/image_impl/projection.rs): reuse of an immutable conflict calculation preserves results and errors |
| [projection-diagnostics](projection/projection-diagnostics) | [`g/diagnostic.rs`](../../src/g/diagnostic.rs): rejected protocol classifications |
| [elastic-roll-colors](projection/elastic-roll-colors) | [`frame_labels`](../../src/global/const_dsl/allocation/frame_labels): distinct frame labels for competing repeated events |
| [wire-frame-refinement](projection/wire-frame-refinement) | [`frame_labels`](../../src/global/const_dsl/allocation/frame_labels) and [`DescriptorImage`](../lean/Hibana/DescriptorImage.lean): allocation, scope ownership and exact generated images |
| [explicit-resource-join](projection/explicit-resource-join) | [`g/source/ownership.rs`](../../src/g/source/ownership.rs): independent resource work and its join |

`projection/elastic-roll-colors/check_all.sh` runs the participant, route-path,
passive-child and conflict-reuse checks as well as frame-allocation checks.
`projection/wire-frame-refinement/check_all.sh` checks exported production
descriptors and finite allocation fixtures. Runtime frame matching still
requires the model's coverage and same-class uniqueness assumptions.

## Protocol images

| Checks | Implementation and property |
| --- | --- |
| [arm-row-index](image/arm-row-index) | [`lane_image.rs`](../../src/global/role_program/image_impl/lane_image.rs): packed arm fields and bounds |
| [passive-parent-index](image/passive-parent-index) | [`metadata/passive_parent.rs`](../../src/global/role_program/image_impl/metadata/passive_parent.rs): parent lookup preserves checked scan results |
| [route-index](image/route-index) | [`metadata/route_index.rs`](../../src/global/role_program/image_impl/metadata/route_index.rs): ordered route lookup, invalid keys and duplicates |
| [receive-index](image/receive-index) | [`ref_access/receive_index.rs`](../../src/global/role_program/image_impl/ref_access/receive_index.rs) and [`cursor/navigation.rs`](../../src/global/typestate/cursor/navigation.rs): exact message keys, candidate ranges and completion filtering; bounded ordinal-certificate checks |
| [route-column-access](image/route-column-access) | [`lane_image.rs`](../../src/global/role_program/image_impl/lane_image.rs): reads stay within their columns |
| [resolver-index](image/resolver-index) | [`route_resolvers.rs`](../../src/global/compiled/images/image/route_resolvers.rs): immutable resolver-site lookup |
| [dispatch-subtree](image/dispatch-subtree) | [`first_recv_dispatch.rs`](../../src/global/typestate/cursor/first_recv_dispatch.rs): traversal of the reachable dispatch subtree |

## Local execution

| Checks | Implementation and property |
| --- | --- |
| [event-admission](runtime/event-admission) | [`event_progress.rs`](../../src/global/typestate/cursor/scope_route/event_progress.rs): dependency and completion guards |
| [pending-events](runtime/pending-events) | [`pending_events.rs`](../../src/global/typestate/cursor/pending_events.rs): completion-bit traversal |
| [recv-lane-census](runtime/recv-lane-census) | [`first_recv_dispatch.rs`](../../src/global/typestate/cursor/first_recv_dispatch.rs): enabled receive lanes |
| [controller-offer](runtime/controller-offer) | [`kernel/offer`](../../src/endpoint/kernel/offer): controller selection and conflict-chain order |
| [live-descendant-preview](runtime/live-descendant-preview) | [`first_recv_dispatch.rs`](../../src/global/typestate/cursor/first_recv_dispatch.rs): preview follows a live descendant |
| [ordinary-offer-ingress](runtime/ordinary-offer-ingress) | [`kernel/offer`](../../src/endpoint/kernel/offer): ordinary messages preceding a choice |
| [offer-prefix-dependency](runtime/offer-prefix-dependency) | [`kernel/offer`](../../src/endpoint/kernel/offer): preceding communication remains a dependency |
| [parallel-offer-ingress](runtime/parallel-offer-ingress) | [`kernel/offer`](../../src/endpoint/kernel/offer) and [`parallel.rs`](../../src/endpoint/parallel.rs): branch preview and independent fragments |
| [parallel-resolver-progress](runtime/parallel-resolver-progress) | [`decision_resolver`](../../src/endpoint/kernel/core/decision_resolver): independent application-resolved choices |
| [nested-resolver-reentry](runtime/nested-resolver-reentry) | [`decision_resolver`](../../src/endpoint/kernel/core/decision_resolver) and [`scope_route/roll`](../../src/global/typestate/cursor/scope_route/roll): nested choices on reentry |
| [roll-reentry-selectors](runtime/roll-reentry-selectors) | [`source validation`](../../src/g/source) and [`send_preview.rs`](../../src/global/typestate/cursor/scope_route/send_preview.rs): distinguish repeated sends from a continuation |
| [roll-continuation-dependency](runtime/roll-continuation-dependency) | [`event_progress.rs`](../../src/global/typestate/cursor/scope_route/event_progress.rs): event membership and prerequisite completion are distinct |
| [rolled-route-ownership](runtime/rolled-route-ownership) | [`scope_route`](../../src/global/typestate/cursor/scope_route): retained choices, visit admission, reset bounds and continuation order |

Run a directory's `check.sh` from the repository root through the resource guard.
Checks without a shell entry point are invoked directly by the
[workflow](../../.github/workflows/quality-gates.yml) or by the two projection
runners above. The common Lean models and generated Rust observations are
checked by `.github/scripts/check_lean_proofs.sh`.
