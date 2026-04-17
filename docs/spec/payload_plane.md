# Payload Plane

The canonical payload plane is borrowed-first.

- `WirePayload` is the transport-facing owner
- `MessageSpec::Decoded<'a>` is the app-facing decoded view
- `recv()` and `decode()` return borrowed decoded views when the codec supports it

The surface does not add parallel owned/borrowed entrypoints. Owned payloads are
expressed through the same `recv()` and `decode()` path.

Drop semantics remain affine:

- a borrowed decoded view keeps endpoint progress borrowed
- preview restash on decode failure is preserved
