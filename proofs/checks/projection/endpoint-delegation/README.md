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

`OrderFilter.lean` and `OrderFilter.smt2` check the word-batched source-order
comparison. A pair can be omitted only when its unconditional answers agree
or its operations belong to incompatible choice arms. The Rust tests compare
the optimized validator with a pairwise traversal over the same projected rows.

The role-image constructor separately checks that those rows are complete.
It walks every global event and the checked actor-binding DAG, independently
of the iterator used to generate the rows. A logical self message requires
one Local row. A message between distinct logical roles requires each actor's
Send and Recv rows, including both when one actor handles both sides. The
check rejects missing, extra, repeated, reordered and wrong-direction rows.
For one actor it takes O(D + E + L) work for D decisions, E global events and
L local rows, using one scratch bit per decision. This check trusts the global
actor bindings; their agreement with source ownership is checked separately.
[`ActorRows.lean`](../../image/operation-index/ActorRows.lean) specifies the
direction rule. Native mutation tests, Miri and Kani check the implementation.

The same entry point exhausts every `u8` actor triple for fixed parallel, route
and repeated-prefix sources: 184,549,376 production queries in total. Independent
scalar expectations check the results. Another 7,873,200 queries compare pruned
and full scans across small structured sources. These run with overflow checks
and debug assertions enabled. They enumerate the full stated domains; they do
not enumerate all possible source trees. Kani separately checks linear causal
handoffs, actor bitsets, receive indexing and scope searches.

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
