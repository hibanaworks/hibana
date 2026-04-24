# Policy Boundary

`hibana` core keeps the daily policy boundary to resolver inputs only:

- `PolicySignalsProvider`
- `ResolverContext`
- `RouteResolution`
- `LoopResolution`

Slot identity is policy boundary metadata, exposed only as
`hibana::substrate::policy::PolicySlot`.

Core does not own:

- EPF bytecode verification
- EPF VM execution
- management request/reply payloads
- management choreography prefixes

Those live outside `hibana` core, in external integration crates or binaries.

The public authority source remains `Ack | Resolver | Poll`.

Wire auto-mint for `TopologyBegin` also consumes the
`hibana::substrate::policy::PolicySlot::Route` input payload with a
fixed op-specific layout. Capability delegation stays on the lower-layer
endpoint-token path rather than the public descriptor-control surface. The
policy boundary stays single-owner: topology operands do not come from a second
helper or out-of-band registration surface.
