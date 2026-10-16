`fresh_route_start_for_rows` is the common boundary used by resolver preview
and commit preflight. An existing route starts another visit only when its
selected arm is complete and the candidate is that route's reentry head.
A fresh containing visit also owns its descriptor-proven descendants.

The previous resolver preview used completion alone. A child entering another
iteration could therefore re-evaluate its containing choice. Commit preflight
correctly retained that choice, and the inconsistent fresh ranges caused a
post-send invariant panic. The two-role reproduction fails on remote main
f1c4dd4bab211b005687cfec15bb53a1f77500ed and repair candidate 0a296df.

`tests/nested_resolver_reentry.rs` checks three containing visits, two child
revisions per visit, and changing the containing resolver's input after its
choice committed. That input must not affect the child's new visit. Entering
the containing route again still evaluates its own resolver.

The Lean and Z3 models check the freshness and retained-choice rules. They
assume the completion, own-entry and containing-visit facts supplied by the
descriptor and runtime. They do not verify their Rust calculation, memory
safety, arbitrary transports, or QUIC. The production-endpoint regression,
Miri, existing full proof gates, and consumer checks cover separate boundaries.
