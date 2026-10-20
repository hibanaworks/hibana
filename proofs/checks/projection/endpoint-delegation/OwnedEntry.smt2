; Static entry obligations are independently derived from checked source rows.
; Arrays represent immutable source/descriptor facts, never runtime owner state.
; Constant arrays require the solver's extended array theory.
(set-logic ALL)
(declare-const required (Array (_ BitVec 8) Bool))
(declare-const actual (Array (_ BitVec 8) Bool))
(declare-const source-owner (Array (_ BitVec 8) (_ BitVec 16)))
(declare-const actual-owner (Array (_ BitVec 8) (_ BitVec 16)))
(declare-const resource (_ BitVec 8))
(define-fun exact () Bool
  (and (= required actual)
    (forall ((key (_ BitVec 8)))
      (=> (select required key)
        (= (select source-owner key) (select actual-owner key))))))

; A required resource cannot be omitted from an accepted descriptor.
(push)
(assert exact)
(assert (select required resource))
(assert (not (select actual resource)))
; EXPECT: unsat
(check-sat)
(pop)

; Checking declared entries alone accepts a completely missing table.
(push)
(assert (select required resource))
(assert (= actual ((as const (Array (_ BitVec 8) Bool)) false)))
(assert (forall ((key (_ BitVec 8)))
  (=> (select actual key) (select required key))))
; EXPECT: sat
(check-sat)
(pop)

; An in-range but different owner reference cannot replace the source reference.
(push)
(assert exact)
(assert (select required resource))
(assert (not (= (select source-owner resource) (select actual-owner resource))))
; EXPECT: unsat
(check-sat)
(pop)

; Unrelated parallel work has no entry obligation and remains admissible.
(push)
(assert exact)
(assert (= required ((as const (Array (_ BitVec 8) Bool)) false)))
; EXPECT: sat
(check-sat)
(pop)

; A real resource outside this roll cannot substitute for a required resource.
(push)
(assert exact)
(assert (select actual resource))
(assert (not (select required resource)))
; EXPECT: unsat
(check-sat)
(pop)
