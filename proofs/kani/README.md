# Hibana Kani proofs

This host-only manifest asks Kani and CBMC to verify the same production
`src/lib.rs`; it does not copy Hibana logic, add a Cargo dependency, enter the
published crate, or affect Pico artifacts.

The pinned gate exhausts the production arithmetic for:

- exact binary decoding of every compact route-arm byte and every valid
  two-bit ready mask;
- exact acceptance domains for packed event-conflict rows and optional
  descriptor route-arm bytes;
- exact binary indexing for resident descriptor route-arm rows;
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
