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
- preview ownership stays affine to the endpoint
