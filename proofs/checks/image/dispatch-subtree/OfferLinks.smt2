; Guarded links are a strictly increasing, complete list of eligible targets.
(set-logic QF_LIA)
(declare-const row Int)
(declare-const a Int)
(declare-const b Int)
(declare-const c Int)
(declare-const known_a Bool)
(declare-const known_b Bool)
(declare-const known_c Bool)
(define-fun min_present ((x Int) (y Int)) Int
  (ite (= x (- 1)) y (ite (= y (- 1)) x (ite (<= x y) x y))))
(define-fun linked () Int
  (ite known_a (ite known_b (ite known_c (- 1) c) b) a))
(define-fun scanned () Int
  (min_present (ite known_a (- 1) a)
    (min_present (ite known_b (- 1) b) (ite known_c (- 1) c))))
(define-fun ranked () Bool
  (and (<= 0 row) (< row a) (< a b) (< b c) (< c 65535)))

; The first unknown link equals the independently accumulated minimum.
(push)
(assert ranked)
(assert (not (= linked scanned)))
; EXPECT: unsat
(check-sat)
(pop)

; No target can occur twice in a ranked chain.
(push)
(assert ranked)
(assert (or (= a b) (= a c) (= b c)))
; EXPECT: unsat
(check-sat)
(pop)

; Missing output is equivalent to every eligible scope being known.
(push)
(assert ranked)
(assert (not (= (= linked (- 1)) (and known_a known_b known_c))))
; EXPECT: unsat
(check-sat)
(pop)

; A selected record advances strictly past the querying row.
(push)
(assert ranked)
(assert (and (not (= linked (- 1))) (<= linked row)))
; EXPECT: unsat
(check-sat)
(pop)

; Omitted-fork control: dropping a live inner record changes the result.
(push)
(assert ranked)
(assert (and (not known_a) (not known_b)))
(assert (not (= (ite known_b (ite known_c (- 1) c) b) scanned)))
; EXPECT: sat
(check-sat)
(pop)

; Wrong-known-skip control: skipping unknown records loses a real target.
(push)
(assert ranked)
(assert (and (not known_a) (not known_b) (not known_c)))
(assert (not (= (ite known_a a (ite known_b b (ite known_c c (- 1)))) scanned)))
; EXPECT: sat
(check-sat)
(pop)

; Nonmonotonic control: complete membership alone does not justify first.
(push)
(assert (and (= row 0) (= a 4) (= b 2) (= c 6)))
(assert (and (not known_a) (not known_b) known_c))
(assert (not (= linked scanned)))
; EXPECT: sat
(check-sat)
(pop)
