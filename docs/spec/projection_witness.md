# Projection Witness

The canonical projection witness is:

- `hibana::g::advanced::RoleProgram<ROLE>`

The witness is opaque.

- protocol code composes a local `let program = g::seq(...)` witness
- `project(&program)` is the only public constructor
- `RoleProgram<ROLE>` is durable and does not borrow the local choreography term
- item-level inferred `Program<_>` values are not the canonical stable-Rust path
- `GlobalSteps` is not part of the public surface
- projected local typelists stay internal
- `SessionKit::enter(...)` and `set_resolver(...)` consume the same witness

Compile-time guarantees remain on the constructor path. They are not degraded
into runtime validation.
