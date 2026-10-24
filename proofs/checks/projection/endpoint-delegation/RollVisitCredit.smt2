; EventCursor retains at most one child completion in an existing u64 word.
; Source-dependent parent completion and independent-entry checks are inputs
; here; protocol admission and scope selection are checked separately.
(set-logic QF_BV)
(declare-const credit (_ BitVec 64))
(declare-const completedParallelParent Bool)
(declare-const independentEntry Bool)
(define-fun zero () (_ BitVec 64) #x0000000000000000)
(define-fun one () (_ BitVec 64) #x0000000000000001)
(define-fun reset ((prior (_ BitVec 64)) (parentComplete Bool)) (_ BitVec 64)
  (ite (or (distinct prior zero) parentComplete) one zero))
(define-fun consumed () (_ BitVec 64) zero)
(define-fun preserved () Bool (and (distinct credit zero) independentEntry))

(push)
(assert (distinct (concat ((_ extract 63 32) credit) ((_ extract 31 0) credit)) credit))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (and (distinct (reset credit completedParallelParent) zero)
             (distinct (reset credit completedParallelParent) one)))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (distinct (reset zero false) zero))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (distinct (reset (reset credit completedParallelParent) completedParallelParent)
                  (reset credit completedParallelParent)))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (distinct (reset one false) one))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (distinct (reset consumed false) zero))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (distinct (reset zero true) one))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (not independentEntry))
(assert preserved)
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (= credit zero))
(assert completedParallelParent)
(assert (= (reset credit completedParallelParent) one))
; EXPECT: sat
(check-sat)
(pop)
