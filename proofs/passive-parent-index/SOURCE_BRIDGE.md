# Passive-parent reverse lookup: source bridge

Base is core3aef31ba015c75ea824b8b41f5603b03f5dd336b plus the qualified stage1 patch83693843e29ad7d1dad93d692a051affdd44a3ab69d0e590945d85bad324480d. Stage1 remains staged in this isolated worktree; both new Lean and Z3 gates passed before stage2 Rust edits.

## Immutable operation

Current first_recv_dispatch.rs::passive_child_parent_route visits route slots in order and arm0 before arm1, validating each fact, and returns the first fact whose child ScopeId exactly matches the query. It can return before later malformed rows. The proposed fast path reads the queried child's immutable conflict owner(parentScope,arm), locates that exact parent/arm fact, and returns the owner only if the fact's child exactly equals the query. Otherwise it returns None. An owner alone never creates an edge.

## Certificate and source obligations

1. All route IDs are valid and unique. The already-qualified strictly sorted certificate supplies this and maps complete raw IDs to slots. Binary arms and unique scope IDs supply unique(parentScope,arm) keys. No raw scope kinds may be masked into a false match.
2. Every fact the legacy scan could read must be well-formed: route row decoding, binary arm index, packed event range, reserved arm bits, zero-event/lane-step coherence, accumulated lane-step range, and event row bounds. Reuse the existing total arm-metadata decoder; do not introduce a divergent encoding.
3. Every present child slot must be in range and strictly greater than its parent slot, exactly as the old decoder requires. Its child's conflict row must decode and match the actual parent ScopeId and arm using the existing matching predicate. These facts rule out passive-edge cycles; the equivalence theorem itself does not assume a tree.
4. Every conflict row, including unused ones, must decode. The new direct lookup may read one that the old scan never touched. An invalid unused owner must disable this optimization rather than change a previously harmless miss into a failure.
5. The constructor must keep count/column spans paired with its own immutable static bytes. It must certify the same route-count domain as the old scan. Total certificate failures only clear the parent-index bit; they do not reject a descriptor. Sorted and parent-index facts can occupy two bits of the existing one-byte certificate field, without growing the metadata object or blob.
6. If any certificate condition fails, run the exact legacy ordered scan. This preserves early success before later malformed facts, decoder failures before the first match, duplicate-target behavior, missing rows, and all other malformed cases. Do not eagerly raise a new constructor error for failed certification.
7. On the certified path, read the candidate parent/arm through the existing fact accessor and recheck the exact child, preserving its decoder and edge validation. Missing query IDs, missing owner IDs, owner-none and owner-without-edge return None. Unused cyclic owner fields grant no edge; only a confirmed passive edge is followed.
8. Keep first_recv_dispatch_root_arm's route-count-bounded ancestor loop unchanged. No assumptions about arbitrary conflict-owner graphs or global tree structure authorize removing its bound or invariant outcome. Keep all dynamic event/frame/route/roll/liveness authority code unchanged.

## Model boundary

The abstract key is the complete parent Route ID plus binary arm. A child is the unique route slot resolved by stage1; absent or wrong-kind queries cannot match a valid edge. The legacy model has explicit malformed outcomes and first-match early return. A valid certificate must establish totality of all skipped reads, not merely a plausible reverse link. The source/model correspondence and immutable ownership remain explicit reviewed assumptions; this is not a full Rust or QUIC correctness proof.

## Required witnesses

Positive gapped scopes and exact edge, owner without edge, missing query, wrong owner/arm, duplicate target, self/backward edge, passive cycle, unused cyclic owners, malformed row before/after first match, unused invalid owner and empty relation. Mutation checks must show why both the global certificate and the exact-edge recheck are necessary.


## Final implementation checks

The two certificates occupy bits 1 and 2 of route_lookup_index. The parent bit is only set when the scope column is certified sorted, its count equals the footprint scan count, and passive_parent_rows_are_coherent returns true. That total helper reuses the unchanged arm-metadata decoder, checks accumulated step/event bounds, decodes all owner rows through the canonical conflict decoder, and reuses passive_child_parent_matches for each present edge. The optimized lookup keeps the original fact accessor and exact child recheck. The original scan and bounded ancestor walk bodies are text-identical to the qualified base.

Rust tests cover every decodable query, malformed first/late edges, bad unused owners, cycles and 360 primitive byte mutations. The actual six-role 50-route graph certifies and all 366 sampled present/absent/other-kind parent lookups match the old scan. source.sha256 binds the final implementation; QUALIFICATION.md records results and remaining integration limits.
