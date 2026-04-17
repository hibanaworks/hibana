# Projection Witness

The canonical projection witness is:

- `hibana::g::advanced::RoleProgram<'prog, ROLE, Mint = DefaultMint>`

The witness is opaque.

- `project(&PROGRAM)` is the only public constructor
- `GlobalSteps` is not part of the public surface
- projected local typelists stay internal
- `SessionKit::enter(...)` and `set_resolver(...)` consume the same witness

Compile-time guarantees remain on the constructor path. They are not degraded
into runtime validation.
