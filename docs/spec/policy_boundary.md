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

Those live in:

- `hibana-epf`
- `hibana-mgmt`

The public authority source remains `Ack | Resolver | Poll`.
