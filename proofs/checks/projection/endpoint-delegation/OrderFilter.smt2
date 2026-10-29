(set-logic QF_BV)
(declare-const expected (_ BitVec 64))
(declare-const actual (_ BitVec 64))
(declare-const possible (_ BitVec 64))
(declare-const certain (_ BitVec 64))
(declare-const excluded (_ BitVec 64))
(define-fun pending () (_ BitVec 64)
  (bvand (bvor (bvand expected (bvnot certain))
               (bvand (bvnot expected) possible)) (bvnot excluded)))
(assert (= (bvand certain (bvnot actual)) #x0000000000000000))
(assert (= (bvand actual (bvnot possible)) #x0000000000000000))

; No compatible, differing answer can be omitted from the word comparison.
(push)
(assert (not (= (bvand (bvxor expected actual)
  (bvand (bvnot pending) (bvnot excluded))) #x0000000000000000)))
; EXPECT: unsat
(check-sat)
(pop)

; Sensitivity: failing to inspect a missing expected edge is unsound.
(push)
(define-fun missing () (_ BitVec 64) (bvand (bvand (bvnot expected) possible) (bvnot excluded)))
(assert (not (= (bvand (bvxor expected actual)
  (bvand (bvnot missing) (bvnot excluded))) #x0000000000000000)))
; EXPECT: sat
(check-sat)
(pop)

; Sensitivity: failing to inspect an unwanted actual edge is unsound.
(push)
(define-fun extra () (_ BitVec 64) (bvand (bvand expected (bvnot certain)) (bvnot excluded)))
(assert (not (= (bvand (bvxor expected actual)
  (bvand (bvnot extra) (bvnot excluded))) #x0000000000000000)))
; EXPECT: sat
(check-sat)
(pop)
