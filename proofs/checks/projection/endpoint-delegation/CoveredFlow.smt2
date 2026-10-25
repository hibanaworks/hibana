; Causal facts for all 256 actors, using the runtime-independent compiler domain.
(set-logic QF_BV)
(declare-const facts (_ BitVec 256))
(declare-const sender (_ BitVec 8))
(declare-const recipient (_ BitVec 8))
(define-fun sender_bit () (_ BitVec 256)
  (bvshl (_ bv1 256) ((_ zero_extend 248) sender)))
(define-fun receiver_bit () (_ BitVec 256)
  (bvshl (_ bv1 256) ((_ zero_extend 248) recipient)))

; A covered relay cannot change any actor fact.
(push)
(assert (= (bvand facts sender_bit) sender_bit))
(assert (= (bvand facts receiver_bit) receiver_bit))
(assert (distinct (bvor facts receiver_bit) facts))
; EXPECT: unsat
(check-sat)
(pop)

; Checking only the sender would skip a real relay to an unknown receiver.
(push)
(assert (= (bvand facts sender_bit) sender_bit))
(assert (distinct (bvand facts receiver_bit) receiver_bit))
(assert (distinct (bvor facts receiver_bit) facts))
; EXPECT: sat
(check-sat)
(pop)
