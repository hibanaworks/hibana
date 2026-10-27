; Packed route-arm fields and cumulative lane spans.
(set-logic ALL)
(declare-const stop (_ BitVec 16))
(declare-const child (_ BitVec 16))
(define-fun packed () (_ BitVec 32) (concat stop child))
(push)
(assert (or (distinct ((_ extract 31 16) packed) stop)
            (distinct ((_ extract 15 0) packed) child)))
; EXPECT: unsat
(check-sat)
(pop)

(declare-const a Int)
(declare-const b Int)
(declare-const c Int)
(declare-const d Int)
(declare-const total Int)
(define-fun valid-span ((start Int) (end Int)) Bool
  (and (<= 0 start) (<= start end) (<= end total) (<= (- end start) 256)))
(assert (and (<= 0 total) (<= total 65535)))
(push)
(assert (and (valid-span 0 a) (valid-span a b) (valid-span b c) (valid-span c d)))
(assert (or (distinct (+ a (- b a)) b)
            (distinct (+ a (- b a) (- c b)) c)
            (distinct (+ a (- b a) (- c b) (- d c)) d)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (valid-span a b) (< b a)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (valid-span a b) (< total b)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (valid-span a b) (> (- b a) 256)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (valid-span a b) (= a b) (distinct (- b a) 0)))
; EXPECT: unsat
(check-sat)
(pop)
; The full 16-bit end can use the former padding byte.
(push)
(assert (and (= stop #xffff) (= child #xfffe)))
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert (and (valid-span a b) (= a 65279) (= b 65535) (= total 65535)))
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert (and (valid-span a b) (= a 65535) (= b 65535) (= total 65535)))
; EXPECT: sat
(check-sat)
(pop)
(push)
(assert (and (valid-span 0 a) (valid-span a b) (> a 0)))
; EXPECT: sat
(check-sat)
(pop)
