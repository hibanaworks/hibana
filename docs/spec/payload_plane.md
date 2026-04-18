# Payload Plane

The canonical payload plane is borrowed-first.

- `WirePayload` is the transport-facing owner
- `MessageSpec::Decoded<'a>` is the app-facing decoded view
- `recv()` and `decode()` return borrowed decoded views when the codec supports it
- fixed-width owned payloads stay on the same contract through `type Decoded<'a> = Self`

The surface does not add parallel owned/borrowed entrypoints. Owned payloads are
expressed through the same `recv()` and `decode()` path.

Drop semantics remain affine:

- a borrowed decoded view keeps endpoint progress borrowed
- preview restash on decode failure is preserved
