(set-logic QF_LIA)
(declare-const first Int)
(declare-const finish Int)
(declare-const predecessor Int)
(declare-const target Int)
(declare-const next Int)
(declare-const limit Int)
(declare-const retained Bool)
(declare-const receiving Bool)

(define-fun contains ((index Int)) Bool
  (and (<= first index) (< index finish)))
(define-fun qualifies ((index Int)) Bool
  (and receiving retained (contains index) (not (contains target))))
(define-fun clipped-end () Int (ite (< finish limit) finish limit))

; A returned witness justifies every skipped predecessor, even if the query
; clips the source interval before its end. The arbitrary done predicate in
; the Lean law is irrelevant on this fully justified prefix.
(push)
(assert (qualifies predecessor))
(assert (<= predecessor next))
(assert (< next clipped-end))
(assert (not (qualifies next)))
; EXPECT: unsat
(check-sat)
(pop)

; The old query is exactly the skipped prefix plus its remaining suffix.
(push)
(assert (qualifies predecessor))
(assert (< predecessor limit))
(assert (not (= (and (<= predecessor next) (< next limit))
                (or (and (<= predecessor next) (< next clipped-end))
                    (and (<= clipped-end next) (< next limit))))))
; EXPECT: unsat
(check-sat)
(pop)

; The target's current visit cannot use its own old retained completion.
(push)
(assert (contains target))
(assert (qualifies predecessor))
; EXPECT: unsat
(check-sat)
(pop)

; Outgoing acceptance cannot use retained incoming continuation credit.
(push)
(assert (not receiving))
(assert (qualifies predecessor))
; EXPECT: unsat
(check-sat)
(pop)

; The half-open interval does not authorize its endpoint.
(push)
(assert (= next finish))
(assert (qualifies next))
; EXPECT: unsat
(check-sat)
(pop)
