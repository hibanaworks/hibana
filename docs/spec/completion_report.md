# Completion Report

Baseline for this report is `hibana` commit `e105824ab7b931a1ae062327609bdf1407d85033`
before the completion-branch deletions landed.

## Public Shape

- canonical inbound payload contract is `WirePayload`
- `WireDecode` / `WireDecodeOwned` are deleted
- localside hot path is named-future based for `send / recv / offer / decode`
- route preview ownership lives in `RouteBranch` itself; `Endpoint` no longer
  keeps a backing preview slot for offer/decode
- cross-crate smoke moved out of `hibana` into an external integration harness

## Localside Size Deltas

Measured with repo-local tests against the same public API shape.

| metric | baseline | current | delta |
| --- | ---: | ---: | ---: |
| `Endpoint` handle bytes | 8 | 8 | 0 |
| send future bytes | 304 | 304 | 0 |
| recv future bytes | 88 | 88 | 0 |
| offer future bytes | 288 | 288 | 0 |
| decode future bytes | 200 | 136 | -64 |
| `RouteBranch` handle bytes | 16 | 64 | +48 |

Notes:

- `RouteBranch` now owns the materialized kernel preview directly.
- `Endpoint` no longer stores a hidden branch-preview backing slot; drop/decode
  restore from the branch owner itself.

## Resident / Pico Deltas

Measured from `check_pico_size_matrix.sh`.

### Endpoint Header Bytes

| shape | baseline | current | delta |
| --- | ---: | ---: | ---: |
| route-heavy | 240 | 200 | -40 |
| linear-heavy | 240 | 200 | -40 |
| fanout-heavy | 240 | 200 | -40 |

### Compiled Image Persistent Bytes

| shape | program baseline | program current | role baseline | role current |
| --- | ---: | ---: | ---: | ---: |
| route-heavy | 112 | 112 | 2324 | 2324 |
| linear-heavy | 16 | 16 | 1376 | 1376 |
| fanout-heavy | 208 | 208 | 3088 | 3088 |

### Flash Bytes

| shape | baseline | current | delta |
| --- | ---: | ---: | ---: |
| route-heavy | 418724 | 415224 | -3500 |
| linear-heavy | 485060 | 479204 | -5856 |
| fanout-heavy | 565364 | 562104 | -3260 |

### Measured Live Endpoint Bytes

| shape | baseline | current | delta |
| --- | ---: | ---: | ---: |
| route-heavy | 2112 | 2016 | -96 |
| linear-heavy | 1952 | 1856 | -96 |
| fanout-heavy | 2208 | 2112 | -96 |

### Measured Peak Stack Bytes

| shape | baseline | current | delta |
| --- | ---: | ---: | ---: |
| route-heavy | 12320 | 12240 | -80 |
| linear-heavy | 7168 | 7120 | -48 |
| fanout-heavy | 12320 | 12240 | -80 |

All current shapes remain inside the 24 KiB pico kernel stack budget.

### Measured Peak SRAM Bytes

| shape | baseline | current | delta |
| --- | ---: | ---: | ---: |
| route-heavy | 41095 | 40919 | -176 |
| linear-heavy | 30903 | 30759 | -144 |
| fanout-heavy | 42615 | 42439 | -176 |

All current shapes remain inside the 96 KiB peak SRAM budget.

## Summary

- concept count is lower: one canonical inbound payload contract, no
  `WireDecode` / `WireDecodeOwned`, no endpoint-stashed branch preview, no
  in-tree cross-repo harness
- hot-path future ownership is smaller where wrapper residue existed, while the
  public `RouteBranch` grows because it is now the sole owner of the materialized
  preview rather than a lease over endpoint-side storage
- packed image closure removes mixed-responsibility owners from frozen image
  files without regrowing persistent bytes
- repo split is now reflected by an external integration harness rather than a
  staging harness inside `hibana`
