; One selected loan/parallel-work/return visit. Cursor completion and physical
; acceptance are distinct observations. This finite check supplements the
; general canonical-domain and descriptor-family Lean proofs.
(set-logic QF_LIA)
(declare-const loan_received Bool)
(declare-const first_done Bool)
(declare-const second_done Bool)
(declare-const return_done Bool)
(declare-const first_accepted Bool)
(declare-const second_accepted Bool)
(assert (=> first_done loan_received))
(assert (=> second_done loan_received))
(assert (=> return_done (and first_done second_done)))
(assert (=> first_done first_accepted))
(assert (=> second_done second_accepted))
(define-fun first_admitted () Bool (and loan_received (not first_done)))
(define-fun second_admitted () Bool (and loan_received (not second_done)))
(define-fun return_admitted () Bool (and first_done second_done (not return_done)))

(push)
(assert loan_received)
(assert (not first_done))
(assert (not second_done))
(assert (not (and first_admitted second_admitted)))
; EXPECT: unsat
(check-sat)
(pop)

(push)
(assert loan_received)
(assert first_accepted)
(assert (not first_done))
(assert second_done)
(assert (not return_done))
; Publication, not a second send, is pending after the physical acceptance.
; EXPECT: sat
(check-sat)
(pop)

(push)
(assert first_done)
(assert second_done)
(assert (not return_done))
(assert (not return_admitted))
; EXPECT: unsat
(check-sat)
(pop)

; The unique global row lookup rejects missing and duplicate actor coverage.
(declare-const matching_rows Int)
(declare-const claimed_checked_cut Bool)
(assert (>= matching_rows 0))
(assert (=> claimed_checked_cut (= matching_rows 1)))
(push)
(assert claimed_checked_cut)
(assert (= matching_rows 0))
; EXPECT: unsat
(check-sat)
(pop)
(push)
(assert claimed_checked_cut)
(assert (= matching_rows 2))
; EXPECT: unsat
(check-sat)
(pop)

; Coverage is checked against the path normalized from the global itself.
(declare-const canonical_domain Bool)
(assert (=> claimed_checked_cut canonical_domain))
(push)
(assert claimed_checked_cut)
(assert (not canonical_domain))
; EXPECT: unsat
(check-sat)
(pop)
