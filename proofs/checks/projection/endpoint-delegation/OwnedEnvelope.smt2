; Three immutable wire bytes: u8 resource followed by a big-endian u16 event.
; This is encoding identity, not run isolation or a mutable ownership model.
(set-logic QF_BV)
(declare-const firstRole (_ BitVec 8))
(declare-const firstEvent (_ BitVec 16))
(declare-const secondRole (_ BitVec 8))
(declare-const secondEvent (_ BitVec 16))
(define-fun envelope ((role (_ BitVec 8)) (event (_ BitVec 16))) (_ BitVec 24)
  (concat role event))

; No different resource/event pair has the same complete envelope.
(push)
(assert (= (envelope firstRole firstEvent) (envelope secondRole secondEvent)))
(assert (or (distinct firstRole secondRole) (distinct firstEvent secondEvent)))
; EXPECT: unsat
(check-sat)
(pop)

; A countermodel for a hypothetical codec that drops the high event byte.
; This is not a claim that the current or previous Rust encoder used it.
(push)
(assert (= firstRole secondRole))
(assert (= ((_ extract 7 0) firstEvent) ((_ extract 7 0) secondEvent)))
(assert (distinct firstEvent secondEvent))
; EXPECT: sat
(check-sat)
(pop)

; The codec retains every bit at the largest resource/event boundary.
; Event admission still has its own source-range and sentinel restrictions.
(push)
(assert (= firstRole #xff))
(assert (= firstEvent #xffff))
(assert (= (envelope firstRole firstEvent) #xffffff))
; EXPECT: sat
(check-sat)
(pop)
