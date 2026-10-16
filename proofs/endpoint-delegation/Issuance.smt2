; Initial issuance at one unique physical resource boundary.
; 0 = available, 1 = reserved, 2 = issued.
; The integration supplies that unique physical identity. This model does not
; infer aliasing between arbitrary operating-system or remote-service handles.
(set-logic QF_LIA)
(declare-const state Int)
(declare-const command Int)
(declare-const count Int)
(assert (and (<= 0 state) (<= state 2)))
(assert (and (<= 0 command) (<= command 2)))
(define-fun next () Int
  (ite (and (= command 0) (= state 0)) 1
    (ite (and (= command 1) (= state 1)) 2
      (ite (and (= command 2) (= state 1)) 0 state))))

; Neither reserve, publish nor abandon can replenish an issued authority.
(push)
(assert (= state 2))
(assert (distinct next 2))
; EXPECT: unsat
(check-sat)
(pop)

; The invariant "a previous publication implies issued" is inductive and
; prevents another publication, independently of runtime/session/role identity.
(push)
(assert (and (<= 0 count) (<= count 1)))
(assert (=> (= count 1) (= state 2)))
(define-fun next_count () Int
  (+ count (ite (and (= command 1) (= state 1)) 1 0)))
(assert (or (> next_count 1) (and (= next_count 1) (distinct next 2))))
; EXPECT: unsat
(check-sat)
(pop)

; Failure before publication rolls back the reservation.
(push)
(assert (= state 1))
(assert (= command 2))
(assert (distinct next 0))
; EXPECT: unsat
(check-sat)
(pop)

; Incorrect rule: local endpoint/runtime destruction revives issued rights.
(push)
(assert (= state 2))
(declare-const after_drop Int)
(assert (= after_drop 0))
(assert (= (+ 1 (ite (= after_drop 0) 1 0)) 2))
; EXPECT: sat
(check-sat)
(pop)

; A legal first publication is possible.
(push)
(assert (= state 1))
(assert (= command 1))
(assert (= next 2))
; EXPECT: sat
(check-sat)
(pop)
