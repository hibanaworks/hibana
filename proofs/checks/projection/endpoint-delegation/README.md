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

`OrderFilter.lean` and `OrderFilter.smt2` check dependency bounds used by the
word-batched source-order comparison. Required source choices of either row
can settle a guarded dependency; unknown choices retain conservative bounds.
All possible extra direct orders are checked. Positive obligations retain a
conservative set containing every cover of the active source order. A cover
is a required predecessor with no active source operation between the pair.
`ActorProgress.lean` proves that these checks imply full predecessor coverage
and preserve readiness on source-closed progress. Cover existence follows
from finite, backward, transitive source order. The theorem is for one full
choice/holder assignment within one visit, not a claim about roll resets.
The Rust tests compare the optimized validator with a full pairwise traversal
over the same projected rows. For sources of up to 12 events and four choices,
they independently enumerate choices and actor bindings, derive covers using
the typed source order, and check that no cover is omitted. These finite tests
do not prove the Rust candidate traversal correct for every source tree.
Conditional pairs are checked on paths containing both source events; holder
choices not determined by those paths remain quantified. Lane predecessors
are intersected with the source-successor bit set and visited newest first.
Native tests compare each indexed predecessor with a scalar row scan, and Kani
checks the word intersection at every prefix boundary.

The role-image constructor separately checks that those rows are complete.
It walks every global event and the checked actor-binding DAG, independently
of the iterator used to generate the rows. A logical self message requires
one Local row. A message between distinct logical roles requires each actor's
Send and Recv rows, including both when one actor handles both sides. The
check rejects missing, extra, repeated, reordered and wrong-direction rows.
For one actor it takes O(D + E + L) work for D decisions, E global events and
L local rows, using one scratch bit per decision. The actor bindings still come
from the source ownership compiler. This row check does not independently prove
that compiler's assignment of logical roles to actors.
[`ActorRows.lean`](../../image/operation-index/ActorRows.lean) specifies the
direction rule. Native mutation tests, Miri and Kani check the implementation.

Each source subtree indexes the roles whose Endpoints it transfers. Queries
skip a subtree only when that role is absent, and still compare the complete
component and role at each leaf. This construction-only index is not stored in
runtime progress. `TransferIndex.lean` proves that role pruning preserves exact
key membership; `TransferIndex.smt2` checks the 256-role bitmap. Kani checks
production sequence, parallel, choice and repeated sources over symbolic full
endpoint keys and payload kinds.

Ownership queries reuse the initial-owner equations already inferred for the
source. When both arms reduce to the same owner, the join retains that owner
without enumerating another choice. Ownership compilation retains its checked
equations and message rows alongside the actor bindings. Reentry checks borrow
those facts and index source messages directly. `OwnerJoin.lean` and
`OwnerJoin.smt2` state the normalization and join rules. Kani checks normalization
against production union equations over symbolic complete keys, including aliases
and independent unbound keys.

Reentry checks reuse the collected resource keys instead of rediscovering each
key at every ordinary message. Kani checks unique catalog membership over full
component/role keys, including control uses before the first transfer and aliases.
The same catalog answers whether a queried role is delegated.
A body that sends no other endpoint has no irreversible outgoing seed. When checked
ownership ends at its entry owner, return existence follows from the source
transfer trace, without expanding unused causal closures. `OwnerJoin.lean`
proves that property for every finite receiver trace. Native tests compare the
result with explicit departure checks and reject a second resource with the
same role in a different component. This derives a source return, not delivery
after a transport failure.

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

For fixed actor bindings, receive-lane allocation computes causal facts for
source receives in bit columns. Only receives whose logical receiver has
different senders need columns; a single-sender receiver has no receive-order
pair to check under any coloring. Compact seed ordinals preserve the source
intervals used by choices and rolls. Each roll's complete first visit feeds a
fresh visit with independent choices. Conditional actor bindings use the
path-specific queries. These are compiler workspaces; no columns or caches
are added to endpoint progress. `CausalFlow.lean` defines the seed, relay,
choice and parallel rules and proves the conditional column join.
`CausalColumns.smt2` checks that join and all compact-source interval masks,
with incorrect joins and interval boundaries as negative controls. Native
tests compare the batched queries with scalar queries on small structured
sources, including actor aliases and boundaries across 64-bit words. These
comparisons do not establish a refinement proof for every Rust source tree.

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
