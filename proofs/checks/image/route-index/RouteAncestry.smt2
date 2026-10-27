(set-logic QF_LIA)
(declare-const first Int)
(declare-const split Int)
(declare-const boundary Int)
(declare-const start Int)
(declare-const stop Int)
(declare-const innerFirst Int)
(declare-const innerStop Int)
(assert (and (<= 0 first) (< first split) (< split boundary) (<= boundary 65535)
             (<= 0 start) (< start stop) (<= stop 65535)))
(define-fun left () Bool (and (<= first start) (<= stop split)))
(define-fun right () Bool (and (<= split start) (<= stop boundary)))
(define-fun arm () Int (ite left 0 (ite right 1 2)))
; A nonempty descendant cannot belong to both arms.
(push)
(assert (and left right))
; EXPECT: unsat
(check-sat)
(pop)
; Descendants of a left-arm parent remain in the left arm.
(push)
(assert (and (<= first innerFirst) (<= innerFirst start) (<= stop innerStop)
             (<= innerStop split) (not (= arm 0))))
; EXPECT: unsat
(check-sat)
(pop)
; The same holds for a right-arm parent.
(push)
(assert (and (<= split innerFirst) (<= innerFirst start) (<= stop innerStop)
             (<= innerStop boundary) (not (= arm 1))))
; EXPECT: unsat
(check-sat)
(pop)
; Equal route bounds do not make a strict ancestor.
(push)
(assert (and (= start first) (= stop boundary) (not (= arm 2))))
; EXPECT: unsat
(check-sat)
(pop)
; Adjacent or disjoint routes cannot be mistaken for descendants.
(push)
(assert (and (or (<= stop first) (<= boundary start)) (not (= arm 2))))
; EXPECT: unsat
(check-sat)
(pop)
; Checking only a child's start admits a route crossing the arm boundary.
(push)
(assert (and (<= first start) (< start split) (< split stop) (<= stop boundary)
             (= arm 2)))
; EXPECT: sat
(check-sat)
(pop)
; A child beginning exactly at the split belongs to the right arm.
(push)
(assert (and (= start split) (<= stop boundary) (= arm 1)))
; EXPECT: sat
(check-sat)
(pop)
