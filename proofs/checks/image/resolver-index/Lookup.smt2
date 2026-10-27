; Direct resolver lookup. Counts and source row order are unconstrained except
; for the descriptor bounds. Forward entries choose the earliest matching row.
(set-logic AUFLIA)
(declare-const routes (Array Int Int))
(declare-const index (Array Int Int))
(declare-const count Int)
(declare-const extent Int)
(declare-const query Int)
(declare-const row Int)
(assert (and (<= 0 count) (< count 65535) (<= 0 extent) (<= extent 8192)))
(assert (forall ((r Int)) (=> (and (<= 0 r) (< r count))
  (and (<= 0 (select routes r)) (< (select routes r) extent)
       (<= 0 (select index (select routes r)))
       (<= (select index (select routes r)) r)
       (= (select routes (select index (select routes r))) (select routes r))))))
(assert (forall ((q Int)) (=> (and (<= 0 q) (< q extent)
  (not (= (select index q) 65535)))
  (and (<= 0 (select index q)) (< (select index q) count)
       (= (select routes (select index q)) q)))))
(define-fun lookup ((q Int)) Int
  (ite (and (<= 0 q) (< q extent)) (select index q) 65535))

; A source occurrence cannot disappear from the index.
(push)
(assert (and (<= 0 row) (< row count) (= query (select routes row))))
(assert (= (lookup query) 65535))
; EXPECT: unsat
(check-sat)
(pop)

; A present result always selects an existing row of exactly this scope.
(push)
(assert (not (= (lookup query) 65535)))
(assert (or (< (lookup query) 0) (>= (lookup query) count)
            (not (= (select routes (lookup query)) query))))
; EXPECT: unsat
(check-sat)
(pop)

; Unsorted and duplicate rows still select the first occurrence.
(push)
(assert (and (<= 0 row) (< row count) (= query (select routes row))))
(assert (< row (lookup query)))
; EXPECT: unsat
(check-sat)
(pop)

; The route-key domain rejects parallel, repetition, and absent scope tags.
(push)
(assert (>= query 8192))
(assert (not (= (lookup query) 65535)))
; EXPECT: unsat
(check-sat)
(pop)

; Missing keys must remain absent, including holes inside the extent.
(push)
(assert (forall ((r Int)) (=> (and (<= 0 r) (< r count))
                            (not (= (select routes r) query)))))
(assert (not (= (lookup query) 65535)))
; EXPECT: unsat
(check-sat)
(pop)

; A later duplicate cannot be substituted while passing the certificate.
(push)
(assert (and (= count 2) (= extent 8) (= (select routes 0) 7)
             (= (select routes 1) 7) (= (select index 7) 1)))
; EXPECT: unsat
(check-sat)
(pop)

(reset)
(set-logic QF_AUFLIA)
; Sensitivity: unconditional overwrite destroys the first occurrence.
(declare-const first Int)
(declare-const second Int)
(assert (and (<= 0 first) (< first 8192) (= first second)))
(define-fun empty () (Array Int Int) ((as const (Array Int Int)) 65535))
(define-fun overwritten () (Array Int Int) (store (store empty first 0) second 1))
(assert (not (= (select overwritten first) 0)))
; EXPECT: sat
(check-sat)

(reset)
(set-logic QF_LIA)
(declare-const slot Int)
(assert (and (<= 0 slot) (<= slot 65535)))
(define-fun low () Int (mod slot 256))
(define-fun high () Int (div slot 256))
(assert (not (= (+ low (* high 256)) slot)))
; EXPECT: unsat
(check-sat)

(reset)
(set-logic QF_LIA)
(declare-const packed Int)
(declare-const resolver Int)
(assert (and (<= 0 packed) (<= packed 65535) (< (mod packed 32768) 8192)
             (<= 0 resolver) (<= resolver 65535)
             (or (>= packed 32768) (= resolver 0))))
(push)
(assert (>= (mod packed 32768) 8192))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (< packed 32768) (not (= resolver 0))))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (and (= packed 32768) (= resolver 65535)))
; EXPECT: sat
(check-sat)
(pop)
