; Possible-holder union only prunes absent roles. Actor values include all u8 IDs.
(set-logic QF_BV)
(declare-const controller (_ BitVec 8))
(declare-const a (_ BitVec 8))
(declare-const b (_ BitVec 8))
(declare-const c (_ BitVec 8))
(declare-const d (_ BitVec 8))
(declare-const role (_ BitVec 8))
(declare-const left-choice Bool)
(declare-const right-choice Bool)
(define-fun marked () Bool
  (or (= role controller) (= role a) (= role b) (= role c) (= role d)))
(define-fun full-check () Bool
  (or (= role controller)
    (= (= role (ite left-choice a b)) (= role (ite right-choice c d)))))
(push)
(assert (distinct (or (not marked) full-check) full-check))
; EXPECT: unsat
(check-sat)
(pop)

; Equal unions cannot replace the correlation-sensitive check.
(push)
(assert (= c b))
(assert (= d a))
(assert (= left-choice right-choice))
(assert (distinct a b))
(assert (= role a))
(assert (distinct role controller))
(assert (not full-check))
; EXPECT: sat
(check-sat)
(pop)

; A mask which misses a possible receiver changes rejection to acceptance.
(push)
(assert (= role a))
(assert (distinct role controller))
(assert (not full-check))
(assert (distinct (or (not (= role controller)) full-check) full-check))
; EXPECT: sat
(check-sat)
(pop)
