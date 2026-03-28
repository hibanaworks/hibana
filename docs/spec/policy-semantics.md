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

Verifier enforcement is compile-time for bytecode load (`load_commit` path),
runtime remains fail-closed.

## Implementation Mapping

- Boundary types and verdict reduction:
  - `src/epf.rs`
  - `src/transport/context.rs`
- Slot contract:
  - `src/epf/slot_contract.rs`
- Verifier enforcement:
  - `src/epf/verifier.rs`
  - `src/epf/loader.rs`
  - `src/runtime/mgmt.rs` (`load_commit`)
- Fail-closed application:
  - `src/endpoint/kernel/core.rs`
  - `src/transport/forward.rs`

## Test Mapping

- Slot contract / verifier:
  - `src/epf/verifier.rs` tests
  - `src/epf/loader.rs` tests
- Replay determinism (same input => same verdict):
  - `tests/policy_replay.rs`
