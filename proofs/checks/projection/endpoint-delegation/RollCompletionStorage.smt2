; The cursor stores event and retained-roll bits in the same caller slab.
; Column lengths are u16. Source validation imposes stricter scope limits;
; this arithmetic covers the entire representable column domain.
(set-logic QF_LIA)
(declare-const events Int)
(declare-const rolls Int)
(declare-const event Int)
(declare-const slot Int)
(assert (and (<= 0 events) (<= events 65535)))
(assert (and (<= 0 rolls) (<= rolls 65535)))
(define-fun words () Int (div (+ events rolls 31) 32))
(define-fun retained () Int (+ events slot))

(push)
(assert (and (<= 0 slot) (< slot rolls)))
(assert (or (< (div retained 32) 0) (>= (div retained 32) words)))
; EXPECT: unsat
(check-sat)
(pop)

; Reentry epochs and completed-visit credits each use two u32 words per
; borrowed roll, after the aligned event/retained-bit region.
(declare-const borrowed Int)
(declare-const epochSlot Int)
(declare-const creditSlot Int)
(declare-const epochHalf Int)
(declare-const creditHalf Int)
(assert (and (<= 0 borrowed) (<= borrowed rolls)))
(define-fun epochStart () Int words)
(define-fun creditStart () Int (+ words (* 2 borrowed)))
(define-fun totalWords () Int (+ words (* 4 borrowed)))
(define-fun epochWord () Int (+ epochStart (* 2 epochSlot) epochHalf))
(define-fun creditWord () Int (+ creditStart (* 2 creditSlot) creditHalf))
(define-fun validEpoch () Bool
  (and (<= 0 epochSlot) (< epochSlot borrowed) (<= 0 epochHalf) (< epochHalf 2)))
(define-fun validCredit () Bool
  (and (<= 0 creditSlot) (< creditSlot borrowed) (<= 0 creditHalf) (< creditHalf 2)))

(push)
(assert (and validEpoch validCredit))
(assert (or (< epochWord 0) (>= epochWord totalWords)
            (< creditWord 0) (>= creditWord totalWords)))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (and validEpoch validCredit))
(assert (or (< epochWord words) (< creditWord words) (= epochWord creditWord)))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (or (< totalWords 0) (> (* 32 totalWords) 4294967295)))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (and validEpoch validCredit))
(assert (or (distinct epochSlot creditSlot) (distinct epochHalf creditHalf)))
(assert (= (+ creditStart (* 2 epochSlot) epochHalf) creditWord))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (and (= events 4) (= rolls 2) (= borrowed 2)
             (= epochSlot 1) (= creditSlot 1) (= epochHalf 1) (= creditHalf 1)))
(assert (and (= totalWords 9) (= epochWord 4) (= creditWord 8)))
; EXPECT: sat
(check-sat)
(pop)

(push)
(assert (and (<= 0 event) (< event events) (<= 0 slot) (< slot rolls)))
(assert (and (= (div event 32) (div retained 32))
             (= (mod event 32) (mod retained 32))))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert (and (= events 32) (= rolls 1) (= slot 0)))
(assert (and (= words 2) (= (div retained 32) 1) (= (mod retained 32) 0)))
; EXPECT: sat
(check-sat)
(pop)

(push)
(assert (and (= events 32) (= rolls 0) (= words 1)))
; EXPECT: sat
(check-sat)
(pop)
