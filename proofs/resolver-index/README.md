# Sealed resolver metadata lookup

This is a runtime implementation optimization with no public API or protocol
semantics change. It is separate from the explicit-resource-join validation
bundle, which did not change the runtime.

## Source correspondence

- CompiledProgramRef::compact validates every resolver row and both canonical
  participant lists once, alongside existing atom validation.
- The descriptor borrows immutable static bytes. A private construction-time
  sorted certificate is derived from complete decoded ScopeIds. It is not
  mutable connection state and fits existing descriptor padding in the host test.
- Certified lookup uses a half-open binary search over complete raw IDs, clearing
  only the authority tag previously checked by decoding. Gaps and nonzero first
  ordinals are supported. Resolver IDs retain the full u16 domain.
- Valid unsorted/duplicate internal tables retain first-match linear lookup.
  Invalid participant rows are rejected at construction, rather than being
  rediscovered on a later lookup. This deliberately moves error detection
  earlier; it is not equivalence for dormant malformed internal descriptors.
- Endpoint admission, wire decoding, route decisions, cancellation, and atomic
  commit rules are unchanged.

The existing SortedRouteIndex.lean search proof was rerun before editing Rust.
check_lookup.py separately checks this lookup's first-match fallback (which has
a different malformed-table policy from the older role-index scanner), tag/kind
separation and immutable-validation premise. It reports 22 UNSAT checks and
3 SAT witnesses, including a counterexample when immutability is omitted.

The Rust differential test covers every decodable u16 query on sorted/gapped,
permuted and duplicate tables, and tests early malformed-row rejection.
These are an explicit source bridge and scoped models, not a Rust compiler proof.

## Measurement scope

An isolated release-client experiment used one unchanged release Neqo sender,
AES128, PMTUD disabled, loopback, one stream, and identical 1 MiB files written
to disk. Three measured repetitions after warmup gave median download times
1.688703312 s for this initial optimization and 0.008791115 s for Neqo.
The previous candidate's single measurement was 3.323310267 s.

This is an improvement, but still approximately 192 times slower in that
workload. It does not establish Neqo parity, server performance, full-size
throughput, or official interop qualification. Later optimizations must be
measured separately. Profiling runs are not used as benchmark results.
