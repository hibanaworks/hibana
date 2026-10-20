; Qualification-only completion write observations. The production runtime
; has no such counter. Checked u64 increments reject exhaustion before a bit
; write; both mark and clear are observed even if the bit value is unchanged.
; This models the observation boundary, not full Rust/history extraction.
(set-logic QF_BV)
(declare-const before_word (_ BitVec 32))
(declare-const event (_ BitVec 32))
(declare-const before_count (_ BitVec 64))
(assert (bvult event (_ bv32 32)))
(define-fun bit () (_ BitVec 32) (bvshl (_ bv1 32) event))
(define-fun cleared () (_ BitVec 32) (bvand before_word (bvnot bit)))
(define-fun recommitted () (_ BitVec 32) (bvor cleared bit))
(define-fun clear_count () (_ BitVec 64) (bvadd before_count (_ bv1 64)))
(define-fun commit_count () (_ BitVec 64) (bvadd clear_count (_ bv1 64)))
(define-fun clear_allowed () Bool (bvult before_count #xffffffffffffffff))
(define-fun commit_allowed () Bool (bvult clear_count #xffffffffffffffff))

; A real reset/recommit can return to exactly the same ordinary progress.
(push)
(assert clear_allowed)
(assert commit_allowed)
(assert (= recommitted before_word))
(assert (not (= commit_count before_count)))
; EXPECT: sat
(check-sat)
(pop)

; A fabricated zero-write clear is rejected by progress equality, even when
; its supplied counters agree. This consistency check does not prove that
; observations are authentic or replace actual write-boundary measurements.
(push)
(assert (= (bvor before_word bit) before_word))
(assert (= before_word cleared))
; EXPECT: unsat
(check-sat)
(pop)

; Equal ordinary progress cannot erase these two actual writes from a gap.
(push)
(assert clear_allowed)
(assert commit_allowed)
(assert (= recommitted before_word))
(assert (= commit_count before_count))
; EXPECT: unsat
(check-sat)
(pop)

; A repeated mark is still an observed source operation, not a zero-write gap.
(push)
(assert clear_allowed)
(assert (= (bvor before_word bit) before_word))
(assert (= clear_count before_count))
; EXPECT: unsat
(check-sat)
(pop)

; Exhaustion cannot wrap into an apparently fresh observation interval.
(push)
(assert (= before_count #xffffffffffffffff))
(assert clear_allowed)
; EXPECT: unsat
(check-sat)
(pop)

; This qualification gate also preserves real first commits.
(push)
(assert (= before_count (_ bv0 64)))
(assert (= before_word (_ bv0 32)))
(assert clear_allowed)
(assert (= clear_count (_ bv1 64)))
(assert (not (= (bvor before_word bit) before_word)))
; EXPECT: sat
(check-sat)
(pop)
