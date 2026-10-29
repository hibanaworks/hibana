; Arithmetic core of compressed-entry equivalence. The general Lean theorem
; names the greatest-predecessor precondition explicitly. This file does not
; assume or establish that Rust produced a correct predecessor link.
(set-logic QF_LIA)
(declare-const start Int)
(declare-const stop Int)
(declare-const step Int)
(declare-const greatest Int)
(declare-const witness Int)
(assert (and (<= 0 start) (<= start step) (< step stop)))
; A predecessor in the arm cannot exceed the greatest predecessor while the
; compressed test reports that greatest predecessor before the arm.
(push)
(assert (and (<= start witness) (< witness step) (<= witness greatest)
             (< greatest start)))
; EXPECT: unsat
(check-sat)
(pop)
; A separating binary sibling excludes the whole interval, not only its
; latest row. The recursive source-grammar theorem is in RouteTopology.lean.
(declare-const cut Int)
(declare-const split Int)
(declare-const after Int)
(push)
(assert (and (<= 0 cut) (<= cut step) (< step split) (<= split after)
             (<= cut witness) (<= witness step) (not (< witness split))))
; EXPECT: unsat
(check-sat)
(pop)
; Sensitivity: a query before the sibling entry cannot be discarded with it.
(push)
(assert (and (< witness cut) (<= 0 witness) (<= cut step) (< step split)
             (<= split after)))
; EXPECT: sat
(check-sat)
(pop)
; A greatest predecessor at/after the arm's start is itself a witness inside
; its upper boundary, because it strictly precedes the selected entry.
(push)
(assert (and (<= start greatest) (< greatest step) (not (< greatest stop))))
; EXPECT: unsat
(check-sat)
(pop)
; Mutation witness: using only the immediately previous same-view descriptor
; row can reject a legal entry when that row is an independent sibling.
(push)
(assert (and (<= 0 greatest) (< greatest start) (<= start witness)
             (< witness step) (not (<= witness greatest))))
; EXPECT: sat
(check-sat)
(pop)
