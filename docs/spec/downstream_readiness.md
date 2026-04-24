# Downstream Readiness

`hibana` core must be consumable as a standalone repository unit.

Repository intent:

- `hibana`: generic affine MPST core
- external integrations: policy appliances, management protocols, and
  transport/appkit compositions built around the core

Clean split requirements:

1. downstream sibling harnesses default to immutable `git` + `rev` dependencies; local worktree overlays stay smoke-only
2. repository metadata describes `hibana` as the protocol-neutral core rather than as an umbrella workspace
3. `hibana` core tests and scripts do not assume sibling checkouts
4. cross-crate smoke lives outside the core crate surface and outside this repo

Composition testing boundary:

- the core repo is not the owner of cross-crate smoke
- those composition tests belong to an external integration harness

Required smoke coverage in that harness:

1. `hibana` plus an external management integration
2. `hibana` plus an external policy integration
3. `hibana` plus both integrations together
4. branch-local verification through the current sibling worktrees only

`hibana` core must not keep an in-tree cross-crate harness. Composition tests
must stay outside the core repository.
