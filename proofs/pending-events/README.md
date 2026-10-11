# Pending event traversal

The cursor's existing completion bitset is the sole completion authority.
Lane-prefix admission and dependency/route-row completion traverse the exact
uncompleted subset before applying their existing lane and conflict predicates.
No bitmap, cache, cursor state, public API, or capacity is added. Roll reentry
continues to clear the same completion bits before they are examined.

`Scan.lean` proves mask membership, preservation of other selected bits, selection
of a real least bit, backing-word bounds, the admission predicate equivalence,
and compact-index arithmetic. The proofs are checked by Lean 4.30.0; their printed
axiom inventories contain only standard kernel axioms. No native-decision axiom
is used. `Scan.smt2` independently checks the exact 32-bit Rust mask and subtract-
one clearing expressions, strict decrease, and reentry clearing for every word.

Run `bash proofs/pending-events/check.sh`. Rust differential tests cover every
completion subset and selected route arm in a projected `par`/`route`/`roll`
fixture, all range boundaries across four words, and the last partial word at
65535 steps. These establish the traversal's scope; they do not prove physical
servo convergence or claim a performance improvement without native measurement.
