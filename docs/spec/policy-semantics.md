# Policy Semantics (AMST Emergency Plane)

## Scope

This document defines the minimal semantics for EPF policy evaluation under the
`PolicySignals` boundary.

- Input boundary: `PolicySignalsProvider::signals(slot) -> PolicySignals`
- Verdict domain: `Proceed | RouteArm(u8) | Reject(u16)`
- Fail-closed reason: `ENGINE_FAIL_CLOSED`

## Judgement Forms

1. Evaluation:

`Gamma |- eval(slot, event, signals) => verdict`

2. Runtime step:

`Gamma |- step(state, verdict) => state'`

`Gamma` contains:

- slot contract (`slot -> allows_get_input / allows_attr`)
- active policy image digest
- capability mask / transport snapshot

## Core Invariants

1. Preservation

Applying `verdict` preserves AMST typing invariants and control capability
integrity.

2. Progress

For a well-typed runtime state and verified image, evaluation yields one of:

- `Proceed`
- `RouteArm(arm <= 1)`
- `Reject(reason)`

3. Non-Interference

Policy evaluation does not directly mutate cap/effect/lease safety kernels.
Policy may only decide routing/abort outcomes.

## Slot Non-Use Contract (SNC)

- `Route`, `EndpointTx`, `EndpointRx`: `GET_INPUT` allowed.
- `Forward`, `Rendezvous`: `GET_INPUT` forbidden and verifier-rejected.

Verifier enforcement belongs to the policy image owner outside `hibana` core;
runtime remains fail-closed at the signal boundary.

## Implementation Mapping

`hibana` core owns the policy input boundary and fail-closed reduction only.
Bytecode verification, VM execution, and management load semantics are outside
this crate boundary.

- Boundary types and signal collection:
  - `src/transport/context.rs`
  - `src/policy_runtime.rs`
- Endpoint-side verdict reduction:
  - `src/endpoint/kernel/core.rs`
- Observation and replay normalization:
  - `src/observe/check.rs`
  - `src/observe/normalise.rs`

## Test Mapping

- Boundary and public surface guards:
  - `tests/public_surface_guards.rs`
  - `tests/docs_surface.rs`
- Replay determinism and audit digest behavior:
  - `tests/policy_replay.rs`
