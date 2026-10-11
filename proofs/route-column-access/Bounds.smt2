; Full compact-domain arithmetic; search semantics use SortedRouteIndex.lean.
(set-logic QF_LIA)
(declare-const offset Int)
(declare-const count Int)
(declare-const probe Int)
(declare-const blob Int)
(assert (and (>= offset 0) (<= offset 65535) (>= count 0) (<= count 65535)
             (>= probe 0) (< probe count) (>= blob 0)))
(push)
(assert (<= (+ offset (* 2 count)) blob))
(push)
(assert (not (< (+ offset (* 2 probe)) blob)))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert (not (< (+ offset (* 2 probe) 1) blob)))
; EXPECT: unsat
(check-sat)
(pop)
(assert (not (< (+ offset (* 2 count)) 4294967296)))
; EXPECT: unsat
(check-sat)
(pop)
; Removing the blob-length bound permits an out-of-bounds probe.
(assert (not (< (+ offset (* 2 probe) 1) blob)))
; EXPECT: sat
(check-sat)
