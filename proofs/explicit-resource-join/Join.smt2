; Bounded endpoint/IO integration model, not an arbitrary-Rust proof.
(set-logic QF_LIA)
(declare-const current Int)
(declare-const rx_op Int)
(declare-const tx_op Int)
(declare-const rx_settled Bool)
(declare-const tx_settled Bool)
(declare-const rx_received Bool)
(declare-const tx_received Bool)
(declare-const rx_ok Bool)
(declare-const tx_ok Bool)
(define-fun premise () Bool (and (=> rx_received rx_settled) (=> tx_received tx_settled)))
(define-fun joined () Bool (and rx_received tx_received rx_ok tx_ok (= rx_op current) (= tx_op current)))
; normal-path-is-reachable
(push)
(assert premise)
(assert joined)
; EXPECT: sat
(check-sat)
(pop)
; RX-only-is-not-normal-return
(push)
(assert premise)
(assert (and joined (not tx_received)))
; EXPECT: unsat
(check-sat)
(pop)
; acceptance-is-not-completion
(push)
(assert premise)
(assert (and joined (not tx_settled)))
; EXPECT: unsat
(check-sat)
(pop)
; failure-is-not-normal-return
(push)
(assert premise)
(assert (and joined (not tx_ok)))
; EXPECT: unsat
(check-sat)
(pop)
; old-RX-operation-is-not-current
(push)
(assert premise)
(assert (and joined (not (= rx_op current))))
; EXPECT: unsat
(check-sat)
(pop)
; old-TX-operation-is-not-current
(push)
(assert premise)
(assert (and joined (not (= tx_op current))))
; EXPECT: unsat
(check-sat)
(pop)
; weakened-RX-only-rule-is-unsafe
(push)
(assert premise)
(assert (and rx_received rx_ok (= rx_op current) (not tx_settled)))
; EXPECT: sat
(check-sat)
(pop)
; fabricated-completion-breaks-IO-guarantee: omit premise deliberately.
(push)
(assert (and joined (not tx_settled)))
; EXPECT: sat
(check-sat)
(pop)
(declare-const owned0 Int)
; two-successful-extractions-from-one-owner
(push)
(assert premise)
(assert (and (= owned0 1) (> owned0 0) (> (- owned0 1) 0) (>= (- owned0 2) 0)))
; EXPECT: unsat
(check-sat)
(pop)
(declare-const stopped Bool)
; emergency-stop-before-join
(assert premise)
(assert (and stopped (not rx_received) (not tx_received) (not joined)))
; EXPECT: sat
(check-sat)
