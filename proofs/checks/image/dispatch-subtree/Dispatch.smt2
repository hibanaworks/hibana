; Inductive rank facts and explicit missing-premise counterexample.
(set-logic QF_LIA)
(declare-const parent Int)
(declare-const child Int)
(declare-const n Int)
(push)
(assert (and (<= 0 parent) (< parent child) (< child n) (not (< (- n child) (- n parent)))))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (<= 0 parent) (< parent child) (< child n) (not (< parent child))))
; EXPECT: unsat
(check-sat)
(pop)
(assert (and (= parent child) (<= 0 child) (< child n)))
; EXPECT: sat
(check-sat)
