# Public Surface

`hibana` exposes exactly two public faces:

- app surface: `hibana::g` plus the attached `Endpoint` localside API
- substrate surface: `hibana::substrate::program` plus `hibana::substrate`

The root crate must not regrow:

- in-crate management buckets
- in-crate EPF buckets
- typestate or endpoint-kernel internals
- protocol-specific vocabulary

External integrations may provide:

- an optional policy appliance or VM
- ordinary management prefixes and payload owners

Those integrations are not part of `hibana`'s root public surface.
