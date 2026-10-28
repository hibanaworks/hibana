(set-logic QF_LIA)
(declare-const outer-start Int)
(declare-const outer-end Int)
(declare-const child-start Int)
(declare-const child-end Int)
(declare-const split Int)
(declare-const target Int)
(define-fun inside ((first Int) (last Int)) Bool
  (and (<= first target) (< target last)))

; Source containment keeps every ancestor on a matching receive's path.
(push)
(assert (and (<= outer-start child-start) (<= child-end outer-end)))
(assert (inside child-start child-end))
(assert (not (inside outer-start outer-end)))
; EXPECT: unsat
(check-sat)
(pop)

; The known event cannot require both arms of a binary route.
(push)
(assert (inside outer-start split))
(assert (inside split outer-end))
; EXPECT: unsat
(check-sat)
(pop)

; The arm boundary belongs only to the right arm.
(push)
(assert (= target split))
(assert (< split outer-end))
(assert (or (inside outer-start split) (not (inside split outer-end))))
; EXPECT: unsat
(check-sat)
(pop)

; Without checked containment, pruning the parent can lose a child match.
(push)
(assert (inside child-start child-end))
(assert (not (inside outer-start outer-end)))
; EXPECT: sat
(check-sat)
(pop)
