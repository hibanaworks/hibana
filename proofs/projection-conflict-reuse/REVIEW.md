# Independent source and proof review

A separate read-only reviewer found no blocker in the optimization and checked:

* Original evaluation order: scope lookup, first conflict scan, determinant/row
  construction, dependency evaluation/writes, conflict selection/serialization.
* Exact packed-u16 reuse; no decode/repack of the saved value. The no-route
  branch still selects `PackedEventConflict::none()`.
* Immutable EffList/ScopeMarkerView inputs and cursor-owned mutable dependency
  fields. The omitted successful second scan has no distinct source-level
  error because its inputs and all pure helper calls are identical.
* Recorded pre-edit proof/log/source/patch hashes, and exact original emitter
  and helper bodies after documented name/import/visibility adaptations.

The reviewer qualified the result: Lean/Z3 prove models with a manually audited
Rust bridge. Error equivalence concerns payload and ordering, not source line
locations/backtraces or evaluator resource use. Rust differential and unit runs
are separate required validation. The initial differential corpus concentrated
on layout/count/index failures; an additional 24-case crossing-route marker
corpus was added to exercise early scope-selection failures before scan reuse.

Review source references (candidate lines at review time):
`projection.rs:143–179,300–317`, `blob_image.rs:200–221`,
`projection/dependency.rs:66–77,225–254`,
`const_dsl/eff_list.rs:302–307`, `const_dsl/source_arena.rs:23–26`,
`typestate/facts.rs:35–36,72–90,139–147`,
`const_dsl/scope_ranges.rs:101–117`, `scope_ranges/nesting.rs:39–59`.
