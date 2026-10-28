(set-logic QF_LIA)
(declare-const low Int)
(declare-const high Int)
(declare-const size Int)
(declare-const cut Int)
(define-fun middle () Int (+ low (div (- high low) 2)))
(assert (and (<= 0 low) (<= low cut) (<= cut high) (<= high size) (< low high)))

; Every binary-search iteration reads an active route inside its interval.
(push)
(assert (not (and (<= low middle) (< middle high))))
; EXPECT: unsat
(check-sat)
(pop)

; A covering midpoint lies before the prefix cut. Advancing past it preserves
; the cut and strictly shrinks the interval, including equal route ends.
(push)
(assert (< middle cut))
(assert (not (and (<= (+ middle 1) cut) (< (- high (+ middle 1)) (- high low)))))
; EXPECT: unsat
(check-sat)
(pop)

; A noncovering midpoint is at or after the prefix cut.
(push)
(assert (<= cut middle))
(assert (not (and (<= cut middle) (< (- middle low) (- high low)))))
; EXPECT: unsat
(check-sat)
(pop)

; Mutant: retaining a covering midpoint instead of advancing can stall.
(push)
(assert (< middle cut))
(assert (= (- high middle) (- high low)))
; EXPECT: sat
(check-sat)
(pop)
