# Hibana Kani proofs

This host-only manifest asks Kani and CBMC to verify the same production
`src/lib.rs`; it does not copy Hibana logic, add a Cargo dependency, enter the
published crate, or affect Pico artifacts.

The pinned gate exhausts the production arithmetic for:

- fail-closed endpoint lease generation;
- aligned endpoint placement inside a selected gap;
- aligned, monotonic resident-sidecar packing;
- exact symmetric sidecar-range collision detection.

The harnesses cover both successful and rejected symbolic inputs. They use no
arbitrary role, lane, descriptor, or slab capacity bound. Lean proves the
general allocation transaction model and checks production-exported owner
snapshots. Miri retains responsibility for strict-provenance execution,
relocation, `UnsafeCell`, and callback re-entry; Kani does not replace it.

Run the pinned fail-closed gate:

```sh
bash .github/scripts/check_kani.sh
```
