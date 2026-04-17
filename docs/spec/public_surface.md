# Public Surface

`hibana` exposes exactly two public faces:

- app surface: `hibana::g` plus the attached `Endpoint` localside API
- substrate surface: `hibana::g::advanced` plus `hibana::substrate`

The root crate must not regrow:

- in-crate management buckets
- in-crate EPF buckets
- typestate or endpoint-kernel internals
- protocol-specific vocabulary

The external companion crates are:

- `hibana-epf` for the optional EPF appliance
- `hibana-mgmt` for the ordinary management prefixes and payload owners

Those crates are not part of `hibana`'s root public surface.
