; The two-role regression's projected event IDs are Prefix=0,
; Left=1/Reply=2, Right=3/Reply=4 and Suffix=5. Quantify over every
; completion mask and both previous choices, including unrelated high bits.
(set-logic QF_BV)
(declare-const done (_ BitVec 8))
(declare-const previous_right Bool)
(declare-const next_right Bool)
(declare-const inner_declared Bool)
(define-fun arm_mask ((right Bool)) (_ BitVec 8) (ite right #x18 #x06))
(define-fun complete ((mask (_ BitVec 8))) Bool (= (bvand done mask) mask))
(define-fun parent_complete () Bool (complete (bvor #x21 (arm_mask previous_right))))
(define-fun child_complete () Bool (complete (arm_mask previous_right)))
(define-fun child_admitted () Bool
  (and inner_declared child_complete (complete #x01) (not (complete #x20))))
(define-fun child_reset () (_ BitVec 8) (bvand done (bvnot #x1e)))
(define-fun candidate_unused () Bool
  (not (complete (ite next_right #x08 #x02))))
(define-fun current_visit_admitted () Bool
  (and candidate_unused (= previous_right next_right)))

; An inherited route mark cannot authorize a child iteration.
(push)
(assert (not inner_declared))
(assert child_admitted)
; EXPECT: unsat
(check-sat)
(pop)

; A clear completion bit in the other arm is not an unperformed operation
; of this visit. With no completed roll, committed choice must still agree.
(push)
(assert (not parent_complete))
(assert (not child_admitted))
(assert (not (= previous_right next_right)))
(assert current_visit_admitted)
; EXPECT: unsat
(check-sat)
(pop)

; The rejected shortcut treated the unused right-arm row as immediately
; available after the left reply, before the enclosing suffix.
(push)
(assert (= done #x07))
(assert (not inner_declared))
(assert (not previous_right))
(assert next_right)
(assert candidate_unused)
(assert (not parent_complete))
(assert (not child_admitted))
(assert (not current_visit_admitted))
; EXPECT: sat
(check-sat)
(pop)

; Native reproduction: prefix and left arm finished, suffix unfinished.
; Treating child completion as authority would authorize an implicit loop.
(push)
(assert (= done #x07))
(assert (not previous_right))
(assert (not inner_declared))
(assert (and child_complete (not parent_complete) (not child_admitted)))
; EXPECT: sat
(check-sat)
(pop)

; Explicit nested repetition does not require its parent's suffix.
(push)
(assert (and inner_declared child_complete (complete #x01) (not (complete #x20))))
(assert (not child_admitted))
; EXPECT: unsat
(check-sat)
(pop)

; Completing the old choice admits a different fresh choice. Completion
; must not be recomputed from the new preview's unfinished arm.
(push)
(assert (and inner_declared child_complete (not (= previous_right next_right))))
(assert (and (complete #x01) (not (complete #x20))))
(assert (not (complete (arm_mask next_right))))
(assert child_admitted)
; EXPECT: sat
(check-sat)
(pop)

; Once the parent suffix has committed, a fresh visit must restart the
; parent at Prefix; old child completion cannot skip that prefix.
(push)
(assert (complete #x20))
(assert child_admitted)
; EXPECT: unsat
(check-sat)
(pop)

; Reset only the explicit child's events, preserving both parent boundaries
; and every unrelated event for all eight-bit completion histories.
(push)
(assert (not (= (bvand child_reset #xe1) (bvand done #xe1))))
; EXPECT: unsat
(check-sat)
(pop)

; No event of the child's previous visit survives its reset.
(push)
(assert (not (= (bvand child_reset #x1e) #x00)))
; EXPECT: unsat
(check-sat)
(pop)
