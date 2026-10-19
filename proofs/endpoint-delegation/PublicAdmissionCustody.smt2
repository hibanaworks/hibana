; Existing public-call lease, operation phase, publication and fault boundaries.
; These are observations, not new runtime state or a receiver ACK protocol.
(set-logic QF_LIA)
(declare-const disposition Int)
(declare-const lease_held Bool)
(declare-const run_live Bool)
(declare-const old_tag_issued Bool)
(declare-const payload_phase Int)
; PublicOperationPhase: sendPayload = 9; see PublicOperationKernel.lean.
(assert (and (<= 0 payload_phase) (<= payload_phase 9)))
; A retained accepted witness: 0 still polling, 1 published, 2 failed.
(assert (and (<= 0 disposition) (<= disposition 2)))
(assert (=> (= disposition 0) (or lease_held (= payload_phase 9))))
(assert (=> (= disposition 1) (not old_tag_issued)))
(assert (=> (= disposition 2) (not run_live)))

(push)
(assert run_live)
(assert (not lease_held))
(assert (not (= payload_phase 9)))
(assert old_tag_issued)
; EXPECT: unsat
(check-sat)
(pop)

; A lease-only exclusion is false for real asynchronous sends. The carrier
; can expose the frame before its send future observes the acknowledgement.
(push)
(assert run_live)
(assert (not lease_held))
(assert old_tag_issued)
; EXPECT: sat
(check-sat)
(pop)

; The payload remains privately borrowed between polls, while an independent
; parallel fragment in this same actor is free to complete another operation.
(push)
(assert (= disposition 0))
(assert run_live)
(assert (not lease_held))
(assert (= payload_phase 9))
(assert old_tag_issued)
; EXPECT: sat
(check-sat)
(pop)

; Legal remote receipt/return before the sender finishes its poll. A live raw
; header and retained witness do not supply a second operation authority.
(declare-const usable Int)
(declare-const preparing Int)
(declare-const frames Int)
(push)
(assert (= disposition 0))
(assert lease_held)
(assert run_live)
(assert old_tag_issued)
(assert (= usable 0))
(assert (= preparing 0))
(assert (= frames 1))
(assert (= (+ usable preparing frames) 1))
; EXPECT: sat
(check-sat)
(pop)

; Release after uncertain delivery can leave a raw tag but not a live run.
(push)
(assert (= disposition 2))
(assert (not lease_held))
(assert old_tag_issued)
; EXPECT: sat
(check-sat)
(pop)

; No old nonce may be reissued by a later successful issuance in this run.
(declare-const old_nonce Int)
(declare-const publication_counter Int)
(declare-const current_counter Int)
(declare-const fresh_nonce Int)
(push)
(assert (<= old_nonce publication_counter))
(assert (<= publication_counter current_counter))
(assert (= fresh_nonce (+ current_counter 2)))
(assert (= fresh_nonce old_nonce))
; EXPECT: unsat
(check-sat)
(pop)
