# External proof binding with qualified compiler optimizations

This integration combines local `a1598cd9` and external `d906c2c4`. The external
head already contains the same elastic wire allocator and all 14 security
regressions. It adds source binding and CI replay; it does not introduce a new
coloring algorithm or relax runtime admission.

All production Rust bytes match `a1598cd9`. The qualified passive child-marker
window and packed conflict reuse remain unchanged. Rust fixture changes use the
external oracle location and boundary test names, with the exact immutable token
mapping retained. Both branches' original evidence, source snapshots, and
correspondence records remain independently verifiable.

Fresh validation of this integrated source:

- 469 internal tests passed; 8 existing host-only proof exports remain ignored
- SAME14 security regressions: 14 passed, including queued Open rejection as old
  Inspect, correct current offer, true old elastic reentry, and preview handling
- Program capacity: 1 passed; rolled resolver reentry: 13 passed; visible route
  reentry: 37 passed
- Internal allocator tests retain all 256 colors, reject a 257th distinct elastic
  domain, preserve ordinary sequential reuse beyond 256 events, and check nested
  ownership and source/receiver/lane/original-color partitioning
- Fresh Lean 4.30.0 dependency compilation and full proof replay passed with local
  Z3 5.1.0, including the original 31 elastic queries and retained compiler,
  passive-window, and conflict-reuse proofs
- Selected source hygiene gates and `git diff --check` passed

Rust ran in a new, isolated target directory with one build job, ordinary test
stacks, and no compiler-limit overrides. `rust-tests.json` records the exact
command, source hash, source stability, resource observations, and result. The
elapsed run is not a controlled performance benchmark.

The proof remains conditional on Covers/SameClassUnique and the recorded source
models. These checks do not establish universal Rust selector correctness or
maximally economical coloring. The external branch uses exactly the same owner
partition, so it neither improves nor further restricts that allocation policy.

CI additions only install the named Python/Z3 proof prerequisites and replay the
checked-in proof runner with Lean 4.30.0. No secrets, publication steps, or token
permission changes were added. The existing workflow inherits repository token
defaults; their effective value was not queried. Remote CI status is unverified.
The external README footprint update is deliberately absent: this integration
has not produced a fresh merged-source Pico measurement.
