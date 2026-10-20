; Prepared send-loop completion, separately from source admission and carrier
; correctness. The Rust kernel loop and its publication call are exercised by
; the unit/Miri/Kani boundary harness, and its copied table by Lean.
(set-logic QF_LIA)
(declare-const boundary Int)
(assert (and (<= 0 boundary) (<= boundary 2)))
; 0 pending, 1 success (a checked commit plan), 2 failure.
(define-fun returned () Int boundary)
(define-fun publications () Int (ite (= boundary 1) 1 0))
(define-fun done () Bool (not (= boundary 0)))
(define-fun retained () Bool (= boundary 0))

(push)
(assert (= boundary 1))
(assert (or (= returned 0) (not done) retained (not (= publications 1))))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (= boundary 2))
(assert (or (= returned 0) (not done) retained (not (= publications 0))))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (= boundary 0))
(assert (or done (not retained) (not (= publications 0))))
; EXPECT: unsat
(check-sat)
(pop)

; Local publication need not precede remote receipt and return. The old
; sender header/witness can coexist with a single returned frame, without
; becoming another usable or preparing operation right.
(declare-const usable Int)
(declare-const preparing Int)
(declare-const frames Int)
(declare-const accepted_witnesses Int)
(declare-const old_sender_header Bool)
(define-fun authority () Int (+ usable preparing frames))
(push)
(assert old_sender_header)
(assert (= accepted_witnesses 1))
(assert (= usable 0))
(assert (= preparing 0))
(assert (= frames 1))
(assert (= authority 1))
; EXPECT: sat
(check-sat)
(pop)
