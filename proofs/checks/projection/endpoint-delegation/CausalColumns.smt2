; Bit-column joins and source interval masks used by receive lane assignment.
(set-logic QF_BV)
(declare-const left Bool)
(declare-const right Bool)
(declare-const left_seed Bool)
(declare-const right_seed Bool)
(define-fun joined () Bool
  (or (and left right) (and left left_seed) (and right right_seed)))
(define-fun selected () Bool
  (ite left_seed left (ite right_seed right (and left right))))
(push)
(assert (not (and left_seed right_seed)))
(assert (distinct joined selected))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (not (and left_seed right_seed)))
(assert (distinct (or left right) selected))
; EXPECT: sat
(check-sat)
(pop)

; All u16 source endpoints, all compact-source words and all 64 bit positions.
(declare-const start (_ BitVec 16))
(declare-const end (_ BitVec 16))
(declare-const word (_ BitVec 10))
(declare-const bit (_ BitVec 6))
(define-fun lo () (_ BitVec 32) ((_ zero_extend 16) start))
(define-fun hi () (_ BitVec 32) ((_ zero_extend 16) end))
(define-fun base () (_ BitVec 32) (bvshl ((_ zero_extend 22) word) (_ bv6 32)))
(define-fun stop () (_ BitVec 32) (bvadd base (_ bv64 32)))
(define-fun low () (_ BitVec 32) (ite (bvugt lo base) (bvsub lo base) (_ bv0 32)))
(define-fun high () (_ BitVec 32) (ite (bvult hi stop) (bvsub hi base) (_ bv64 32)))
(define-fun mask () (_ BitVec 64)
  (ite (or (bvule hi base) (bvuge lo stop)) (_ bv0 64)
    (bvand (bvshl (_ bv18446744073709551615 64) ((_ zero_extend 32) low))
      (bvlshr (_ bv18446744073709551615 64)
        ((_ zero_extend 32) (bvsub (_ bv64 32) high))))))
(define-fun point () (_ BitVec 32) (bvadd base ((_ zero_extend 26) bit)))
(define-fun member () Bool
  (distinct (bvand mask (bvshl (_ bv1 64) ((_ zero_extend 58) bit))) (_ bv0 64)))
(push)
(assert (bvule lo hi))
(assert (distinct member (and (bvule lo point) (bvult point hi))))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (bvult lo hi))
(assert (distinct member (and (bvule lo point) (bvule point hi))))
; EXPECT: sat
(check-sat)
(pop)
