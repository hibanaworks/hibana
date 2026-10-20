(set-logic QF_LIA)
; -1 means no evidence; 0/1 are the two arms; -2 means rejection.
(declare-const selected Int)
(declare-const completed Bool)
(declare-const ready Int)
(assert (and (<= (- 1) selected) (<= selected 1)))
(assert (and (<= (- 1) ready) (<= ready 1)))
(define-fun live () Int (ite completed (- 1) selected))
(define-fun preview () Int
  (ite (= live (- 1)) ready
    (ite (and (not (= ready (- 1))) (not (= live ready))) (- 2) live)))
(define-fun old () Int (ite (= selected (- 1)) ready selected))

; Every chosen arm has live committed authority or current poll authority.
(push)
(assert (>= preview 0))
(assert (not (or (and (not completed) (= selected preview)) (= ready preview))))
(check-sat)
(pop)
; Completion cannot authorize an old arm after the poll changes.
(push)
(assert completed)
(assert (>= ready 0))
(assert (not (= preview ready)))
(check-sat)
(pop)
; Completion without a new poll must wait.
(push)
(assert (and completed (= ready (- 1)) (not (= preview (- 1)))))
(check-sat)
(pop)
; A live disagreement must still reject.
(push)
(assert (and (not completed) (>= selected 0) (>= ready 0)
             (not (= selected ready)) (not (= preview (- 2)))))
(check-sat)
(pop)
; The old helper's completed Left / polled Right defect remains reproducible.
(push)
(assert (and completed (= selected 0) (= ready 1) (= old 0) (= preview 1)))
(check-sat)
(pop)
