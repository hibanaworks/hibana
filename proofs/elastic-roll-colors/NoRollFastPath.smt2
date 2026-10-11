; Fresh abstraction matching NoRollFastPath.lean and its two recorded outcomes.
; Scratch is total/read-only; source row validation is outside this rewrite.
(set-logic QF_UFLIA)
(declare-const source Int)
(declare-const has_roll Bool)
(declare-fun finish (Int) Int)
(define-fun late () Int (ite has_roll (finish source) source))
(define-fun early () Int (ite has_roll (finish source) source))
(push)
(assert (not (= late early)))
; EXPECT: unsat
(check-sat)
(pop)
; Nonvacuous no-roll source preservation.
(assert (and (not has_roll) (= early source)))
; EXPECT: sat
(check-sat)
