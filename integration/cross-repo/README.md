# Cross-Repo Smoke

This harness is the phase7 staging location for cross-repo integration checks.

Goals:

- consume `hibana`, `hibana-epf`, and `hibana-mgmt` through their public GitHub
  repositories at immutable revisions
- avoid local path or registry-patch assumptions in the harness itself
- keep cross-repo composition tests out of `hibana` core's public-surface guard
  suite

The layout is intentionally self-contained so it can be promoted into a
dedicated integration repository later.
