# Hibana Kani proofs

This host-only manifest asks Kani and CBMC to verify the same production
`src/lib.rs`; it does not copy Hibana logic, add a Cargo dependency, enter the
published crate, or affect Pico artifacts.

The pinned gate exhausts the production arithmetic for:

- exact binary decoding of every compact route-arm byte and every valid
  two-bit ready mask;
- exact ready-evidence record, conflict, poll/materialization, consume, and
  clear transitions, preserving a canonical single-arm mask and the invariant
  that poll evidence is a subset of ready evidence;
- exact public-operation prepare classification over the complete endpoint
  operation-state product, including fail-closed rejection and poison reuse;
- exact framed inbound identity over source, lane, and frame label; exact
  headerless identity over lane, logical label, and schema; plus one shared
  zero/one/ambiguous candidate kernel whose ambiguity is absorbing;
- exact preferred-first offer-lane enumeration over the complete 256-lane
  bitset domain, using the same fixed 32-bit lane words on the Kani host and
  Pico targets, with a monotonic remaining cursor that cannot repeat the
  preferred lane;
- exact offer-progress edge classification over all ready-arm and ingress
  evidence transitions: the first observation and every changed observation
  are new evidence, while only an identical repeat may remain pending;
- exact active-offer aggregation: the first concrete owning lane remains the
  representative, all matching entry facts are combined across lane-local
  scope/root identities, and a foreign entry is rejected without partial
  update;
- exact frontier owner-pool mutation under symbolic lane ordering, entry
  removal, and root-row compaction, preserving every surviving `(entry, lane)`
  witness across the production raw-pointer table;
- exact acceptance domains for packed event-conflict rows, optional descriptor
  route-arm bytes, present state indices, and optional event-fact row indices;
- exact packed lane-range round trips with formal rejection of the reserved
  sentinel;
- exact derivation of active-lane count, first lane, and logical span from the
  canonical resident bitmap, including lane 255 and trailing-zero rejection;
- exact contiguous route-commit partition bounds against the lowering-derived
  endpoint builder capacity;
- exact resident decoding for route/roll scope rows, local event headers,
  logical-lane rows, route-arm lane-step rows, binary route-arm indexing, and
  role-local send/receive/local direction over the complete `u8` role domain;
- exact full-domain route-arm lane accumulation: duplicate events preserve one
  relation, lane bits remain exact, and the final local step is retained;
- exact receive-causality witness indexing across the complete `u8` role
  domain: the first authority transfer is never overwritten and distinct roles
  cannot alias, and every symbolic three-event scope-free program gives the
  same result under the single-scan and pairwise checkers;
- atomic normalized scope publication: route, parallel, and roll markers become
  visible only as complete closed scopes; primary route bounds preserve every
  valid compact offset, and proof-only entry metadata erases to the existing
  descriptor marker tag;
- exact route-commit decision identity across scope, arm, and reentry metadata,
  plus exact passive-child binding to a distinct child scope and its recorded
  parent route arm;
- lane-exact route-commit finalization: canonical empty remains empty, a
  matching lane preserves all prepared rows, and a mismatched lane is rejected
  instead of becoming an empty commit;
- exact local-dependency decoding, including event-image range bounds;
- exact compiled atom and route-resolver row decoding, including both per-arm
  canonical participant lists, plus complete dynamic resolver identity over route scope
  and resolver id;
- exact structural program-image identity across facts, canonical atom/resolver/
  scope-marker counts, and all blob bytes, including arbitrary scope-marker
  contents and equal images stored at distinct addresses;
- exact packed program/role column construction across every `u16` offset/len
  and the complete resident stride domain, with fail-closed rejection of
  `usize` stride multiplication overflow;
- canonical contiguous program-image layout across exact count domains, total
  byte overflow rejection, fail-closed rejection of undersized selected bucket
  storage, and injective packing of scope event/reentry tags;
- exact preservation of every 32-bit payload-schema value through packed atom
  decoding, including the concrete little-endian blob layout;
- exact canonical bytes, widths, schema identities, and rejection domains for
  every built-in scalar and borrowed-byte codec, plus complete-domain
  injectivity and collision-boundary rejection for the const-generic
  fixed-array schema and concrete zero-, one-, and four-byte codecs;
- exact round-trip preservation and injective identity of all eight core frame
  header bytes across the complete input domain;
- independent fail-closed fit-probe and direct-constructor obligations for
  undersized program storage, plus early role-image plan rejection before its
  large scratch image is built and rejection of any plan/resident byte-length
  disagreement after exact construction;
- exact callback-registration identity over resident program image and resolver
  id, including separation of distinct program images at the same route site
  and the complete `u16` resolver-id domain;
- resolver bucket first allocation from uninitialized storage, proving complete
  slot initialization before insertion and typed callback dispatch;
- resolver bucket replacement compaction over a real hole-bearing entry array,
  preserving both registration keys and typed callback dispatch;
- fail-closed endpoint lease generation;
- published-only, idempotent runtime-local membership sealing across every
  endpoint lease state;
- exact endpoint-operation and nested-scratch transitions, including restoration
  to the operation barrier rather than registry-visible availability;
- aligned endpoint placement inside a selected gap;
- exact endpoint-lease slot-count/last-index correspondence, including all
  65,536 identifiers, plus table sizing and 32-bit bounds for the empty table
  and every nonempty table in that domain;
- exact association-column sizing and 32-bit bounds across the full `u16`
  capacity domain;
- one checked layout-arithmetic authority for scratch, endpoint, and resident
  storage, including exact bit-word sizing, complete-`usize` alignment, and
  absolute-offset overflow;
- exact completion-bit presence and word/bit identity over the complete
  `usize` local-step domain;
- exact terminal cursor-position encoding across every `u16` value, including
  `u16::MAX` after the last compact event identity;
- aligned, monotonic resident-sidecar packing;
- aligned, pairwise-disjoint sequential sidecar packing;
- proof that compacted destinations for all three resident sidecar owners precede
  their own and every later live source range;
- exact symmetric sidecar-range collision detection;
- exact resolver-sidecar sizing and 32-bit bounds across the full `u16`
  capacity domain.

Kani itself discovers every production `#[kani::proof]` into its structured
JSON inventory. The gate compares that output with the reviewed inventory and
then verifies every listed harness without passing a filter. The complete CBMC
summary must report exactly the same total, so a deleted, added, skipped,
partial, failed, or empty harness set is rejected without a hand-written second
execution list. Harnesses contain no `kani::assume`: finite subdomains use
total surjective generators, while constrained product domains preserve every
valid symbolic candidate and map only invalid candidates to a canonical valid
representative. Closed enums used as symbolic proof inputs derive
`kani::Arbitrary` from the enum declaration, so enum growth enters CBMC without
a second hand-written list. The gate rejects any future assumption in
production source. The reviewed harness inventory pins names and ownership; it
does not treat an unchanged harness name as proof that its body was not
intentionally changed. Because Kani's `should_panic` attribute establishes only
that one or more panic checks are reachable, Hibana uses it only for fixed
rejection-path witnesses. The gate rejects direct symbolic inputs and direct
panics before the production call in those harnesses. Universal rejection
domains use pure acceptance predicates or checked constructors and ordinary
assertions over symbolic inputs.
Transport
FIFO, exactly-once/no-replay delivery, peer-close observation, and one-shot
receive receipt resolution are modeled and proved in Lean, mirrored by the Rust
conformance tests, and checked under Miri.

The harnesses cover both successful and rejected symbolic inputs. They use no
arbitrary role, lane, descriptor, or slab capacity bound. Lean proves the
general allocation transaction model and checks production-exported owner
snapshots. Miri retains responsibility for strict-provenance execution,
relocation, `UnsafeCell`, and callback re-entry; Kani does not replace it.

Run the pinned fail-closed gate:

```sh
bash .github/scripts/check_kani.sh
```
