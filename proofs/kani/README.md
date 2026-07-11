# Hibana Kani proofs

This host-only manifest asks Kani and CBMC to verify the same production
`src/lib.rs`; it does not copy Hibana logic, add a Cargo dependency, enter the
published crate, or affect Pico artifacts.

The pinned gate exhausts the production arithmetic for:

- exact binary decoding of every compact route-arm byte and every valid
  two-bit ready mask;
- exact acceptance domains for packed event-conflict rows and optional
  descriptor route-arm bytes;
- exact packed lane-range round trips with formal rejection of the reserved
  sentinel;
- exact resident decoding for route/roll scope rows, local event headers,
  logical-lane rows, route-arm lane-step rows, and binary route-arm indexing;
- exact route-commit decision identity across scope, arm, and reentry metadata;
- exact local-dependency decoding, including event-image range bounds;
- exact compiled atom and route-resolver row decoding, plus complete dynamic
  resolver identity over both route scope and resolver id;
- exact structural program-image identity across facts, column layout, and all
  blob bytes, including arbitrary resident atom-row contents and equal images
  stored at distinct addresses;
- exact packed program/role column construction across every `u16` offset/len
  and the complete resident stride domain, with fail-closed rejection of
  `usize` stride multiplication overflow;
- exact callback-registration identity over resident program image and resolver
  id, including separation of distinct program images at the same route site
  and rejection of the intrinsic resolver sentinel;
- resolver bucket first allocation from uninitialized storage, proving complete
  slot initialization before insertion and typed callback dispatch;
- resolver bucket replacement compaction over a real hole-bearing entry array,
  preserving both registration keys and typed callback dispatch;
- fail-closed endpoint lease generation;
- aligned endpoint placement inside a selected gap;
- exact endpoint-lease table sizing and 32-bit bounds across the full `u16`
  capacity domain;
- exact association-column sizing and 32-bit bounds across the full `u16`
  capacity domain;
- exact route frame/lane-column sizing and 32-bit bounds across the full
  `u16 x u16` capacity domain;
- aligned, monotonic resident-sidecar packing;
- aligned, pairwise-disjoint sequential sidecar packing;
- proof that compacted sidecar destinations precede both source ranges;
- exact symmetric sidecar-range collision detection;
- exact resolver-sidecar sizing and 32-bit bounds across the full `u16`
  capacity domain.

The harnesses cover both successful and rejected symbolic inputs. They use no
arbitrary role, lane, descriptor, or slab capacity bound. Lean proves the
general allocation transaction model and checks production-exported owner
snapshots. Miri retains responsibility for strict-provenance execution,
relocation, `UnsafeCell`, and callback re-entry; Kani does not replace it.

Run the pinned fail-closed gate:

```sh
bash .github/scripts/check_kani.sh
```
