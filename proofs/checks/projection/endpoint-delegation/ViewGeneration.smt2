; Numeric view identity only. Actor/run binding, transport delivery and affine
; Rust future ownership remain separate refinement obligations.
(set-logic QF_BV)
(define-fun maximum () (_ BitVec 32) #xffffffff)
(define-fun odd ((nonce (_ BitVec 32))) Bool
  (= (bvand nonce #x00000001) #x00000001))
(declare-const current (_ BitVec 32))
(declare-const candidate (_ BitVec 32))
(declare-const issued (_ BitVec 32))
(define-fun next () (_ BitVec 32) (bvadd current #x00000002))
(define-fun rustFresh () Bool
  (and (odd current) (bvule current #xfffffffd) (not (= next maximum))))

; The Nat model's bound agrees with all representable Rust input values.
(push)
(assert (distinct
  (and (odd candidate) (not (= candidate maximum)))
  (and (odd candidate)
    (bvult ((_ zero_extend 1) candidate) ((_ zero_extend 1) maximum)))))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (distinct rustFresh
  (and (odd current)
    (bvult (bvadd ((_ zero_extend 1) current) (_ bv2 33))
      ((_ zero_extend 1) maximum)))))
; EXPECT: unsat
(check-sat)
(pop)

; A permitted live nonce has an exact, non-wrapping retirement successor.
(push)
(assert (and (odd current) (not (= current maximum))))
(assert (or (= (bvadd current #x00000001) #x00000000)
  (= (bvadd current #x00000001) current)))
; EXPECT: unsat
(check-sat)
(pop)

; No previously issued nonce can match the fresh record after one step.
; Lean extends this fact to arbitrary nonempty finite issuance histories.
(push)
(assert rustFresh)
(assert (bvule issued current))
(assert (= issued next))
; EXPECT: unsat
(check-sat)
(pop)

; Checked addition alone would admit the reserved odd maximum and let its
; retirement wrap to zero. This is a rejected-rule counterexample.
(push)
(assert (= current #xfffffffd))
(assert (and (odd current) (bvule current #xfffffffd)))
(assert (= (bvadd next #x00000001) #x00000000))
; EXPECT: sat
(check-sat)
(pop)

; Positive witness for the last permitted fresh issuance and retirement.
(push)
(assert (= current #xfffffffb))
(assert rustFresh)
(assert (= next #xfffffffd))
(assert (= (bvadd next #x00000001) #xfffffffe))
; EXPECT: sat
(check-sat)
(pop)
