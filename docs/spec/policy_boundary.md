# Policy Boundary

`hibana` core keeps only the generic policy boundary:

- `PolicySignalsProvider`
- `PolicySlot`
- `ResolverContext`
- `DynamicResolution`

Core does not own:

- EPF bytecode verification
- EPF VM execution
- management request/reply payloads
- management choreography prefixes

Those live outside `hibana` core, in external integration crates or binaries.

The public authority source remains `Ack | Resolver | Poll`.

Wire auto-mint for `TopologyBegin` also consumes the `PolicySlot::Route` input
payload with a fixed op-specific layout. Capability delegation stays on the
lower-layer endpoint-token path rather than the public descriptor-control
surface. The policy boundary stays single-owner: topology operands do not come
from a second helper or out-of-band registration surface.
