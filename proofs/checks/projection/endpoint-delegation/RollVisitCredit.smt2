; Arithmetic of EventCursor's packed u64 completed-visit counters. Scope
; ownership and protocol progress are checked separately.
(set-logic QF_BV)
(declare-const credit (_ BitVec 64))
(define-fun maximum () (_ BitVec 64) #xffffffffffffffff)
(define-fun one () (_ BitVec 64) #x0000000000000001)
(define-fun canIncrement () Bool (distinct credit maximum))
(define-fun incremented () (_ BitVec 64) (bvadd credit one))
(define-fun canConsume () Bool (bvugt credit #x0000000000000000))
(define-fun consumed () (_ BitVec 64) (bvsub credit one))

(push)
(assert (distinct (concat ((_ extract 63 32) credit) ((_ extract 31 0) credit)) credit))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert canIncrement)
(assert (bvule incremented credit))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert canIncrement)
(assert (distinct (bvsub incremented one) credit))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert canConsume)
(assert (bvuge consumed credit))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert canConsume)
(assert (distinct (bvadd consumed one) credit))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (= credit maximum))
; EXPECT: sat
(check-sat)
(assert canIncrement)
; EXPECT: unsat
(check-sat)
(pop)
