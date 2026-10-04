# Certified first-receive dispatch subtree

The previous helper enumerated every route, then walked each candidate's parent
chain to decide whether it belonged to the queried root. The new fast path
follows only that root's passive-child edges, using constant-space parent unwind.

## Preconditions and observable behavior

The existing passive-parent certificate checks complete row bounds and decoding,
forward-only child slots, and the inverse parent/arm relationship. With that
certificate, children have a unique parent and cycles are excluded. The root
lookup and optional local event-range checks remain. Uncertified descriptors
continue using the original ordered scan, including its error behavior.

The only two consumers are an OR arm mask and UniqueMatch accumulation. They
depend on candidate membership, not visitation order. Different candidates
remain ambiguous; identical repeated candidates retain their existing behavior.
This optimization changes metadata visitation, not communication or commit order.

## Checks and limitations

Dispatch.lean proves forward-descendant/parent-ancestry equivalence, monotonicity,
absence of distinct cycles under strict child ordering, and uniqueness-result
preservation under equal candidate membership.

check_walk.py compares the exact visitor multisets of the original scanner and
the concrete stackless traversal for 42,275 ascending forests and 290,580 root
queries (sizes 1 through 7). It also checks two symbolic rank obligations with
Z3 and exhibits a self-cycle when the certificate premise is removed.

Rust tests compare the actual old/new candidate multisets and panic outcomes,
including every decodable scope in the fixture and malformed certificate
fallback cases. The existing full internal and public regression suites remain
required. These tests and mathematical models are not an automatic proof of
every Rust execution or of arbitrary native IO behavior.
