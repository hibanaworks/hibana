; Boundary obligations for the production pending-send admission rule.
; These are abstract policy checks, not a proof of every Transport implementation.
; The Rust operation classifier is checked by Kani, actual suspension/acceptance
; by Endpoint regressions and Miri, and all operation transitions by Lean.
(set-logic QF_UFBV)
(declare-const resource (_ BitVec 8))
(declare-const sent (_ BitVec 8))
(declare-const owned_send Bool)
(declare-const borrowed_payload Bool)
(declare-const source_enabled Bool)
(declare-const entry_available Bool)
(declare-const session_live Bool)
(declare-const transport_ready Bool)
(declare-const old_admission Bool)
(define-fun payload_borrow_allowed () Bool
  (or (not borrowed_payload) (and owned_send (= sent resource))))
(define-fun live_admission () Bool
  (and source_enabled entry_available session_live payload_borrow_allowed))
(define-fun accepts () Bool (and transport_ready live_admission))

; No other view may restart resource-dependent work while an owned-send
; future holds the required resource. A previous proof cannot authorize it.
(push)
(assert borrowed_payload)
(assert (or (not owned_send) (not (= sent resource))))
(assert accepts)
; EXPECT: unsat
(check-sat)
(pop)

; Reusing admission from a previous poll admits the concrete stale-resource
; counterexample even though the transport itself is ready.
(push)
(assert (and old_admission transport_ready (not entry_available)))
; EXPECT: sat
(check-sat)
(pop)

; Revalidating before transport acceptance excludes that counterexample.
(push)
(assert (and accepts (not entry_available)))
; EXPECT: unsat
(check-sat)
(pop)

; The payload's own transfer remains possible, including a rolled loan head.
(push)
(assert (and borrowed_payload owned_send (= sent resource) accepts))
; EXPECT: sat
(check-sat)
(pop)

; An unrelated unborrowed view gains no dependency on that payload operation.
(push)
(assert (and (not borrowed_payload) (not payload_borrow_allowed)))
; EXPECT: unsat
(check-sat)
(pop)
