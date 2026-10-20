# Endpoint ownership and projection

The Rust harness includes the production source tree and ownership validator
from [`src/g/source`](../../../../src/g/source). It does not keep another Rust
implementation of the source language. The independent Lean model is
[`EndpointOwnership.lean`](../../../lean/Hibana/EndpointOwnership.lean).

The checks cover initial-owner inference, unique issuance, transfers through
other logical roles, branch knowledge, parallel independence, repeated
ownership cycles and causal order. Logical role identity is preserved when
the physical holder changes.

`compiler/` builds the production validator with read-only proof observations.
Its output is checked by `Knowledge.lean`, `Causality.lean` and the generated
Lean claims. `kani/` checks production ownership-equation operations. `check.sh`
runs every `.smt2` file in this directory against its declared `EXPECT` results.

The source validator is one part of Endpoint delegation. Initial attachment
uses the transport's unique `EndpointBinding` and requires the source-assigned
initial actor to be attached in the same runtime and session. Value transfer,
continuation, operation admission, cancellation and transport delivery are
checked by the core tests and the common Lean models. The public runnable
example is [`endpoint_delegation.rs`](../../../../examples/endpoint_delegation.rs).

From the repository root:

```sh
TOOLCHAIN=stable bash .github/scripts/lib/resource_guard.sh \
  bash proofs/checks/projection/endpoint-delegation/check.sh
```

Successful source ownership checking does not establish codec correctness,
physical resource identity, authentication or transport delivery. The
integration supplies physical identity at `EndpointBinding`'s unsafe boundary.
The carrier must provide FIFO, nonduplication, run isolation and receive
continuity across holder changes. An I/O error never reissues that binding.
