# Downstream Readiness

Phase 7 treats `hibana`, `hibana-epf`, and `hibana-mgmt` as separate repository
units.

Repository intent:

- `hibana`: generic affine MPST core
- `hibana-epf`: optional EPF appliance
- `hibana-mgmt`: ordinary management prefixes and payload owners

Clean split requirements:

1. crate manifests use immutable GitHub repo revisions rather than path dependencies or floating branches
2. repository metadata points at the final per-repo GitHub home
3. `hibana` core tests and scripts do not assume sibling checkouts
4. cross-repo smoke lives outside the core crate surface

The staging location for cross-repo smoke in this checkout is
`integration/cross-repo/`. It is intentionally self-contained so it can be
promoted into a dedicated integration repository without changing crate APIs.
