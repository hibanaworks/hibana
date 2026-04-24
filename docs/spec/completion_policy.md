# Completion Policy

This branch closes `hibana` by deletion, not by scaffolding.

Hard rules:

- no second public surface
- no alias surface
- no dual public receive/decode trait story
- no raw-pointer frozen image owners
- no wrapper-future regressions in localside hot paths
- no repo-boundary ambiguity between `hibana` core and external integration crates

Canonical payload contract:

- outbound payloads implement `WireEncode`
- inbound `recv()` / `decode()` payloads implement `WirePayload`
- owned-by-value payloads stay on the same contract via `type Decoded<'a> = Self`

Completion gates are blockers:

- canonical docs must not reintroduce `owned default path` wording
- canonical docs must not teach `WireDecode` as a peer receive/decode surface
- canonical docs must not describe mgmt/epf ownership as living inside `hibana` core
- frozen-image owners must stay free of pointer-rich ownership
- future-size / resident-size / stack-budget checks must stay green
