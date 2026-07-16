# Hibana Kani proofs

This host-only manifest asks Kani and CBMC to verify the same production
`src/lib.rs`; it does not copy Hibana logic, add a Cargo dependency, enter the
published crate, or affect Pico artifacts.

The pinned gate exhausts the production arithmetic for:

- exact binary decoding of every compact route-arm byte and every valid
  two-bit ready mask;
- exact public-operation prepare classification over the complete endpoint
  operation-state product, including fail-closed rejection and poison reuse;
- exact acceptance domains for packed event-conflict rows and optional
  descriptor route-arm bytes;
- exact packed lane-range round trips with formal rejection of the reserved
  sentinel;
- exact resident decoding for route/roll scope rows, local event headers,
  logical-lane rows, route-arm lane-step rows, and binary route-arm indexing;
- exact route-commit decision identity across scope, arm, and reentry metadata;
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
  every built-in scalar and borrowed-byte codec, plus zero-, one-, and
  four-byte representatives of the const-generic fixed-array codec;
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
- exact endpoint-lease table sizing and 32-bit bounds across the full `u16`
  capacity domain;
- exact association-column sizing and 32-bit bounds across the full `u16`
  capacity domain;
- aligned, monotonic resident-sidecar packing;
- aligned, pairwise-disjoint sequential sidecar packing;
- proof that compacted destinations for all three resident sidecar owners precede
  their own and every later live source range;
- exact symmetric sidecar-range collision detection;
- exact resolver-sidecar sizing and 32-bit bounds across the full `u16`
  capacity domain.

Kani itself discovers and verifies every production `#[kani::proof]`; the gate
does not pass a harness filter or maintain a second execution inventory. It
rejects a missing, partial, failed, or empty complete-harness summary. Transport
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
