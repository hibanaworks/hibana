# Participating-role mask: preimplementation proof gate

## Scope and proposed production change

Baseline is the unchanged vendored Hibana a9371bea437bbc1f4303ceeb3fc833f605efe730. `vendor/check_hibana.py` verified 833 snapshot files, no local patches. No Rust source was changed to produce this proof.

Only proposal: in `src/global/compiled/lowering/seal.rs::validate_route_projection_guarantees`, scan the initialized source events once into a 32-byte bit mask containing BOTH sender and receiver roles. Pass the mask by reference to `validate_route_scope`; preserve its existing ascending `usize` loop `0 .. role_count`, calling the existing observer check only when that bit is set. Keep every other validation stage, route ordering, controller/selector check and active-role check unchanged. Do not change `max_role`, `compiled_program_role_count`, `contains_role`, role-projection admission, source lowering, runtime state, descriptors or authority.

Do not implement until the parent releases that step. The separate rolled-route runtime repair remains out of scope.

## Source correspondence

- `src/global/compiled/lowering/seal.rs:99-121`: one route-marker-order traversal; this is where the mask is proposed to be built once. The mask is compile-time scratch, not persisted runtime data.
- `seal.rs:123-163`: controller check, selector-conflict check, then ascending role observer loop. Lean `routeOriginal`, `routeOptimized` and `ordered` model exactly this logical order; the theorem preserves the first failing role as well as its error, stronger than merely preserving success.
- `seal.rs:167-172`: controller role or successful observer merge supplies branch knowledge. This condition is unchanged; the candidate does not separately short-circuit controller work.
- `seal.rs:183-205`: receive-lane, parallel, reentry, passive-child checks precede route validation. `pipeline_exact` preserves arbitrary errors in these stages and arbitrary per-route earlier errors. It is not a proof of those validators themselves.
- `src/global/compiled/lowering/driver/impls/image.rs:122-123`: role count is `max_role as usize + 1`, so role255 requires loop bound256. No u8 increment or role-count narrowing is permitted.
- `src/global/const_dsl/endpoint_selectors.rs:58-76`: `observer_path_decision(None,None)` is Accept; matching inbound IDs continue; distinct inbound IDs accept; other pairs reject. Lean `observerMerge` is this selector-stream computation.
- `endpoint_selectors.rs:339-381`: `next_local_endpoint_selector` searches only initialized events in its supplied range, prioritizing sender over receiver. `nextSelector`, `selector`, and `selectors` model that priority and index. Absence proves both searches return None, independently of labels, schema, route structure, or starting indexes.
- `src/global/const_dsl/eff_list.rs:49-58,310-315`: initialized event reads and exact source partitions. The premise is a well-formed lowered source; arbitrary corruption of private arena rows is not modeled. Structurally invalid user graphs still have well-formed lowered rows, and their earlier errors are preserved.

## Lean claims

`ParticipantMask.lean` proves, for arbitrary source and role lists:

1. Folded mask membership equals the union of every event's sender and receiver (`collect_exact`, `participants_exact`)
2. An absent role occurs in no source event; every appearing sender OR receiver is marked
3. Duplicate insertions are idempotent; self-sends remain marked
4. For any two arms whose events come from the source, absence forces BOTH next-selector searches to return None and the observer merge to accept (`absent_arms`)
5. Removing only successful roles preserves the exact first failing role/error in any list (`ordered_skip_successful`); filtering is a stable sublist
6. Every route result and the first error of the whole unchanged ordered pipeline are identical (`route_exact`, `pipeline_exact`)
7. Role count stays1..256, every participant lies below it, each role maps into byte0..31/bit0..7, and255+1 does not wrap a32-bit-or-larger usize

The model uses `Fin256` roles and unbounded natural event indexes. Source generation guarantees compact event indexes below65535. The absence theorem does not depend on inbound identity validity; it never selects an inbound event for an absent role. Source-array capacity invariants are not changed by this proposal.

The theorem is a logical refinement of this narrow control-flow transformation, not a complete verified translation of Rust or proof of the entire Hibana protocol.

## Z3 claims and non-vacuity

`check_participant_mask.py` independently uses a32-entry array of8-bit words and symbolic8-bit role IDs. It proves exact insertion/query behavior for arbitrary masks and roles, duplicate idempotence, and byte/bit bounds across the entire role domain. It checks concrete folded event masks and bounded selector searches through16 symbolic events, and exact first-failing-role/error preservation through all256 possible roles.

SAT witnesses include nonempty duplicate/self-send graphs at role255, genuine late-role rejection, every earlier-stage error, and an accepting sparse route with distinct inbound evidence. A sender-only mask has a concrete SAT counterexample: both branches have controller0, one sends to255 and the other to1; the real validation rejects at role1, while sender-only collection falsely accepts. An arbitrary omitted rejecting role also has a SAT counterexample.

Every solver query has a20-second timeout, rejects unknown, and asserts its expected SAT/UNSAT result. Lean has no `sorry`, custom axioms, or `admit`; reported dependencies are Lean's standard `propext`, `Quot.sound`, and where needed `Classical.choice`.

## Expected compile effect and limitation

Source-extracted operation model: TLS333 sends/140 routes/5rolls yields3640 observer calls and51992 event reads; skipping24 absent roles removes3360 calls and51432 event reads, adding one333-event mask pass. TLS+key par354 sends/148 routes/6rolls/1par removes3256 of3848 observer calls and48290 of53272 event reads, adding one354-event pass. Exact source counts are in the accompanying cost-model JSON.

This candidate affects validation AFTER `ProgramProjection::SOURCE` construction. It does not remove the earlier source-lowering roll-coloring hot spot and is not claimed to fix the measured source-constant memory failure. Total rustc CPU/RSS improvement remains unmeasured.

## Later proposals, not implemented or qualified by this gate

- Add a1024 source bucket between512 and2048: TLS903 rows and par961 rows fit; capacity/padding invariance is a different proof obligation
- Exact ternary route-path partition refinement for roll coloring: source model detects2.23M/2.30M marker-loop visits; two u16[E] scratch arrays could replace these with compact exact class comparisons. A separate arbitrary-refinement proof, concrete packing/bounds bridge, byte-for-byte coloring tests and compiler measurements are required
