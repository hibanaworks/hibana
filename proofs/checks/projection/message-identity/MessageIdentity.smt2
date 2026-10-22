; A framed message carries its static global event position. Dynamic
; occurrences still require FIFO, nonduplication and run isolation.
(set-logic QF_BV)
(declare-const session-a (_ BitVec 32))
(declare-const session-b (_ BitVec 32))
(declare-const lane-a (_ BitVec 8))
(declare-const lane-b (_ BitVec 8))
(declare-const source-a (_ BitVec 8))
(declare-const source-b (_ BitVec 8))
(declare-const target-a (_ BitVec 8))
(declare-const target-b (_ BitVec 8))
(declare-const site-a (_ BitVec 16))
(declare-const site-b (_ BitVec 16))
(define-fun header-a () (_ BitVec 72)
  (concat session-a lane-a source-a target-a site-a))
(define-fun header-b () (_ BitVec 72)
  (concat session-b lane-b source-b target-b site-b))

; The last two bytes preserve every bit of the source position.
(push)
(assert (distinct ((_ extract 15 0) header-a) site-a))
; EXPECT: unsat
(check-sat)
(pop)

; A matching framed header identifies all five wire fields exactly.
(push)
(assert (= header-a header-b))
(assert (or (distinct session-a session-b) (distinct lane-a lane-b)
  (distinct source-a source-b) (distinct target-a target-b)
  (distinct site-a site-b)))
; EXPECT: unsat
(check-sat)
(pop)

; A continuation cannot match a different branch site, even on the same lane.
(push)
(assert (distinct site-a site-b))
(assert (= ((_ extract 15 0) header-a) site-b))
; EXPECT: unsat
(check-sat)
(pop)

; Negative control: matching a shared color does not establish source identity.
(push)
(declare-const color-a (_ BitVec 8))
(declare-const color-b (_ BitVec 8))
(assert (= color-a color-b))
(assert (distinct site-a site-b))
; EXPECT: sat
(check-sat)
(pop)

; Same-site repetitions may have identical bytes. Static identity alone
; cannot reject a duplicate from a previous iteration.
(push)
(assert (= header-a header-b))
; EXPECT: sat
(check-sat)
(pop)
