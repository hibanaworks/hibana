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
