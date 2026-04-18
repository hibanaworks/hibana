# Descriptor Kernel

The localside hot path is descriptor-first and manual-future driven.

Public names stay fixed:

- `flow().send()`
- `recv()`
- `offer()`
- `decode()`

Implementation constraints:

- no compiler-generated `async fn` state machine on the hot path
- no whole-lane rebuild in `offer()`
- no whole-scope scan in `offer()`
- pending transport futures are retained and re-polled, not recreated
- `Endpoint` owns send/recv progress; `RouteBranch` owns the sole live
  materialized preview for offer/decode
- preview state is not duplicated between `Endpoint` and `RouteBranch`; the
  endpoint only provides restore/commit authority for the branch-owned preview
