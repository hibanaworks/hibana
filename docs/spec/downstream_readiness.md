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

Dedicated integration repo:

- `hibana-cross-repo`: `https://github.com/hibanaworks/hibana-cross-repo`

Required smoke lanes in that repo:

1. `hibana` + `hibana-mgmt`
2. `hibana` + `hibana-epf`
3. `hibana` + both siblings together
4. current local branch verification through `hibana-cross-repo/run_workspace_smoke.sh`
   so the dedicated harness exercises the in-flight sibling worktrees without
   weakening the immutable manifest contract

`hibana` core must not keep an in-tree cross-repo harness after the split is
closed. The dedicated integration repo is the only owner of those composition
tests.
