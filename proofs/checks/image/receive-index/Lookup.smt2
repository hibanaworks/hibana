; Integer keys are (logical receiver << 8) | logical label, or 65536
; for a non-receive row. The upper-bound comparator equals key < query+1.
(set-logic QF_AUFLIA)
(declare-const key Int)
(declare-const query Int)
(assert (and (<= 0 key) (<= key 65536) (<= 0 query) (<= query 65535)))
(push)
(assert (not (= (or (< key query) (= key query)) (< key (+ query 1)))))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (= key 65536))
(assert (and (not (< key query)) (< key (+ query 1))))
; EXPECT: unsat
(check-sat)
(pop)

; Bounded constructor check, including arbitrary duplicate keys. A strictly
; sorted (key, ordinal) certificate with three in-range ordinals cannot omit
; a row or repeat an ordinal. Keys are read from the same immutable row map.
(declare-const keys (Array Int Int))
(declare-const a Int)
(declare-const b Int)
(declare-const c Int)
(declare-const missing Int)
(define-fun before ((left Int) (right Int)) Bool
  (or (< (select keys left) (select keys right))
      (and (= (select keys left) (select keys right)) (< left right))))
(assert (and (<= 0 a) (< a 3) (<= 0 b) (< b 3) (<= 0 c) (< c 3)))
(assert (and (before a b) (before b c)))
(push)
(assert (and (<= 0 missing) (< missing 3)
             (not (= missing a)) (not (= missing b)) (not (= missing c))))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (or (= a b) (= a c) (= b c)))
; EXPECT: unsat
(check-sat)
(pop)

; Equal message keys remain legal: distinct ordinals retain both receives.
(push)
(assert (= (select keys 0) (select keys 1)))
; EXPECT: sat
(check-sat)
(pop)

; Removing a completed row preserves a pure live-admission predicate when
; reentry is impossible and the full guard already rejects that row.
(declare-const repeatable Bool)
(declare-const completed Bool)
(declare-const admitted Bool)
(declare-const otherViewPending Bool)
(assert (=> (and (not repeatable) completed)
  (and (not admitted) (not otherViewPending))))
(push)
(assert (or
  (not (= admitted (and (or repeatable (not completed)) admitted)))
  (not (= otherViewPending (and (or repeatable (not completed)) otherViewPending)))))
; EXPECT: unsat
(check-sat)
(pop)

; Negative control: applying the completion filter to a fresh roll loses work.
(push)
(assert (and repeatable completed admitted))
(assert (not (= admitted (and (not completed) admitted))))
; EXPECT: sat
(check-sat)
(pop)

; The 16-bit receive key determines both u8 fields exactly.
(declare-const role Int)
(declare-const label Int)
(declare-const wantedRole Int)
(declare-const wantedLabel Int)
(push)
(assert (and (<= 0 role) (< role 256) (<= 0 wantedRole) (< wantedRole 256)
             (<= 0 label) (< label 256) (<= 0 wantedLabel) (< wantedLabel 256)))
(assert (= (+ (* role 256) label) (+ (* wantedRole 256) wantedLabel)))
(assert (or (not (= role wantedRole)) (not (= label wantedLabel))))
; EXPECT: unsat
(check-sat)
(pop)
