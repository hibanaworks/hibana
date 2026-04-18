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
