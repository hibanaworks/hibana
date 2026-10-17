; One arbitrary ancestor/parent pair witnesses any failure of dependency-cut
; closure. Indices range over all naturals, not a fixed event-count bound.
(set-logic QF_LIA)
(declare-const first Int)
(declare-const stop Int)
(declare-const candidate Int)
(declare-const ancestor Int)
(declare-const parent Int)
(declare-const ancestor_done Bool)
(declare-const parent_done Bool)
(assert (and (<= 0 first) (<= first stop) (<= 0 candidate)
  (<= 0 ancestor) (<= 0 parent)))
(define-fun inside ((event Int)) Bool (and (<= first event) (< event stop)))
(define-fun reset_ancestor () Bool (and (not (inside ancestor)) ancestor_done))
(define-fun reset_parent () Bool (and (not (inside parent)) parent_done))

; Source ancestors precede the candidate; their parents precede them. The
; pre-body portion of this same dependency cut is closed before preparation.
(push)
(assert (and (< ancestor candidate) (< candidate stop) (< parent ancestor)))
(assert (=> (and (< ancestor first) ancestor_done) parent_done))
(assert (and reset_ancestor (not reset_parent)))
; EXPECT: unsat
(check-sat)
(pop)

; A numerical prefix includes unrelated parallel history. An old continuation
; can stay complete while its own rolled body is fresh. The independent
; candidate must not acquire that continuation as a new source dependency.
(push)
(assert (and (= first 0) (= stop 4) (= ancestor 4) (= parent 0) (= candidate 5)))
(assert (and ancestor_done parent_done reset_ancestor (not reset_parent)))
; EXPECT: sat
(check-sat)
(pop)

; The pre-body prefix premise is necessary. Resetting a body cannot repair
; an already-invalid ancestor/parent pair entirely outside that body.
(push)
(assert (and (= first 3) (= stop 6) (= candidate 5) (= ancestor 1) (= parent 0)))
(assert (and ancestor_done (not parent_done) reset_ancestor (not reset_parent)))
; EXPECT: sat
(check-sat)
(pop)
