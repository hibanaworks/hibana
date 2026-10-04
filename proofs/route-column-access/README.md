# Checked role route-column probes

The role descriptor constructor already certifies every immutable route ID and
strict ordering. The hot lookup previously decoded and bounds-checked each
binary-search probe again. It now checks the complete two-byte column once and
compares the same full raw IDs directly. The search algorithm, wrong-kind
handling, gaps/nonzero ordinals and checked path for uncertified descriptors are
unchanged. No mutable lookup cache, extra index storage or public API is added.

The source bridge is `RoleImageRef::route_scope_slot` and its constructor's
`route_scopes_are_sorted` certificate. The descriptor's static bytes and sealed
columns are an existing premise; arbitrary mutation of a forged internal
certificate is not admitted. The byte reader is the existing private BlobPtr
reader; no new unsafe operation is introduced.

`Bounds.lean` proves that a probe below the count stays inside the checked blob
and that compact u16 arithmetic fits a 32-bit or wider usize. `check_bounds.py`
checks the full compact arithmetic domain with three UNSAT obligations and a
SAT witness when the column bound is removed. The existing
`proofs/route-index/SortedRouteIndex.lean` proves the binary-search semantics.
Rust tests compare against the checked scan for every decodable u16 query and
constructed sorted, gapped, permuted, duplicate and malformed columns.
These models are scoped obligations, not a proof of arbitrary Rust programs.

The initial isolated 32 MiB release-client measurement improved from 2.2052 s
to 2.0022 s (CPU user time 2.0455 s to 1.8614 s), without increasing total linked
text/data/BSS footprint. Three measured runs followed warmup. This approximately
9% change is much smaller than the remaining gap with Neqo. A separate confirmation series measured 1.9052 s (user CPU 1.7565 s).
The internal suite (474 tests), source-surface suite (124 tests), strict library
Clippy and thumbv6m library check passed. Consumer tests and actual native peer
checks also ran against this lookup. These remain workload-specific results;
do not claim parity.
