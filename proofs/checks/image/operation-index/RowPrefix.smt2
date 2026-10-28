(set-logic QF_BV)
(declare-const first (_ BitVec 16))
(declare-const stop (_ BitVec 16))
(define-fun count () (_ BitVec 16) (bvsub stop first))
(define-fun packedStart () (_ BitVec 16)
  (ite (= count #x0000) #x0000 first))

; A monotone prefix cannot underflow or produce a count beyond its final row.
(push)
(assert (bvule first stop))
(assert (bvugt count stop))
; EXPECT: unsat
(check-sat)
(pop)

; Nonempty ranges preserve their exact compact end boundary.
(push)
(assert (and (bvule first stop) (not (= count #x0000))))
(assert (not (= (bvadd packedStart count) stop)))
; EXPECT: unsat
(check-sat)
(pop)

; Empty source intervals use the existing canonical empty packed range.
(push)
(assert (= count #x0000))
(assert (not (= packedStart #x0000)))
; EXPECT: unsat
(check-sat)
(pop)

; Mutant: a nonmonotone prefix can wrap a compact subtraction.
(push)
(assert (not (bvule first stop)))
(assert (bvugt count stop))
; EXPECT: sat
(check-sat)
(pop)
