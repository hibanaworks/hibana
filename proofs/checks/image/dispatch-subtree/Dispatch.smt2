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
(push)
(assert (and (= parent child) (<= 0 child) (< child n)))
; EXPECT: sat
(check-sat)
(pop)

; The nodes below denote route arms, not whole scopes. Child arms remain
; within the same selected root arm, including along a nested child path.
(declare-const root_start Int)
(declare-const root_stop Int)
(declare-const parent_start Int)
(declare-const parent_stop Int)
(declare-const child_start Int)
(declare-const child_stop Int)
(declare-const target Int)
(push)
(assert (and (<= root_start parent_start) (<= parent_stop root_stop)
             (<= parent_start child_start) (<= child_stop parent_stop)))
(assert (not (and (<= root_start child_start) (<= child_stop root_stop))))
; EXPECT: unsat
(check-sat)
(pop)

; Every child dispatch target is already a receive in the root arm range.
(push)
(assert (and (<= root_start child_start) (<= child_stop root_stop)
             (<= child_start target) (< target child_stop)))
(assert (not (and (<= root_start target) (< target root_stop))))
; EXPECT: unsat
(check-sat)
(pop)

; An empty root cannot contain any dispatched target, even via a child.
(push)
(assert (= root_start root_stop))
(assert (and (<= root_start child_start) (<= child_stop root_stop)
             (<= child_start target) (< target child_stop)))
; EXPECT: unsat
(check-sat)
(pop)

(declare-const root_recv Bool)
(declare-const child_recv Bool)
(push)
(assert (=> child_recv root_recv))
(assert (not (= (or root_recv child_recv) root_recv)))
; EXPECT: unsat
(check-sat)
(pop)

; The two independently exact arm bits retain their 1/2 encoding.
(declare-const root_right_recv Bool)
(declare-const child_right_recv Bool)
(push)
(assert (and (=> child_recv root_recv) (=> child_right_recv root_right_recv)))
(assert (not (= (+ (ite (or root_recv child_recv) 1 0)
                   (ite (or root_right_recv child_right_recv) 2 0))
                (+ (ite root_recv 1 0) (ite root_right_recv 2 0)))))
; EXPECT: unsat
(check-sat)
(pop)

; Missing containment: both first-receive summaries can be exact, while
; a child outside an empty root introduces a new dispatch bit.
(push)
(assert (and (= root_start 0) (= root_stop 0)
             (= child_start 1) (= child_stop 2) (= target 1)))
(assert (and (<= child_start target) (< target child_stop)))
(assert (and (not root_recv) child_recv))
(assert (not (= (or root_recv child_recv) root_recv)))
; EXPECT: sat
(check-sat)
(pop)

; Missing root inclusion: a child-only traversal can lose a root receive.
(push)
(assert (and root_recv (not child_recv)))
(assert (not (= child_recv root_recv)))
; EXPECT: sat
(check-sat)
(pop)
