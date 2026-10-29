; Independent per-event direction specification for the image constructor.
(set-logic QF_BV)
(declare-const source (_ BitVec 8))
(declare-const target (_ BitVec 8))
(declare-const sender Bool)
(declare-const receiver Bool)
(define-fun mask () (_ BitVec 8)
  (ite (= source target)
    (ite sender #x01 #x00)
    (bvor (ite sender #x02 #x00) (ite receiver #x04 #x00))))

(push)
(assert (distinct (not (= (bvand mask #x01) #x00)) (and (= source target) sender)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (distinct (not (= (bvand mask #x02) #x00)) (and (distinct source target) sender)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (distinct (not (= (bvand mask #x04) #x00)) (and (distinct source target) receiver)))
; EXPECT: unsat
(check-sat)
(pop)

; Both directions must survive co-location. Collapsing them to Local is wrong.
(push)
(assert (distinct source target))
(assert sender)
(assert receiver)
(assert (distinct mask #x01))
; EXPECT: sat
(check-sat)
(assert (distinct mask #x06))
; EXPECT: unsat
(check-sat)
(pop)

; The two required bits are visited once in Send, Recv order.
(push)
(assert (= mask #x06))
(define-fun remaining () (_ BitVec 8) (bvand mask (bvsub mask #x01)))
(assert (or (distinct remaining #x04)
  (distinct (bvand remaining (bvsub remaining #x01)) #x00)))
; EXPECT: unsat
(check-sat)
(pop)
