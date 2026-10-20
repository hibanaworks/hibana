; An arbitrary induction step of the finite source-causal permission graph.
; Lean supplies well-foundedness for arbitrary Source.before; Kani checks the
; actual Rust closure on the small seq/par shapes. This is not a transport model.
(set-logic QF_LIA)
(declare-const owner Int)
(declare-const sender Int)
(declare-const earlier_receiver Int)
(declare-const event Int)
(declare-const earlier Int)
(declare-const needed_send Bool)
(declare-const needed_receive Bool)
(declare-const earlier_send_needed Bool)
(declare-const earlier_authorized Bool)
(assert (and (<= 0 event) (<= 0 earlier)))
(define-fun relay () Bool
  (and needed_receive (< earlier event) (= earlier_receiver sender)))
(define-fun authorized () Bool
  (or (= sender owner) (and relay earlier_authorized)))

; Every needed origin is the owner, or follows this visit's incoming message.
; Receive closure includes that message's sender; all earlier needed sends
; have owner authority by induction. No unauthorized origin can remain.
(push)
(assert needed_send)
(assert (=> needed_send (or (= sender owner) relay)))
(assert (=> needed_receive earlier_send_needed))
(assert (=> (and earlier_send_needed (< earlier event)) earlier_authorized))
(assert (not authorized))
; EXPECT: unsat
(check-sat)
(pop)

; A temporary return is not enough: reentry can move another resource needed
; for that return. A causally enclosing local resource supplies a safe guard.
; These are occurrences at one causally closed cut, not runtime fields.
(declare-const guard_departed Bool)
(declare-const guard_returned Bool)
(declare-const resource_departed Bool)
(declare-const resource_returned Bool)
(define-fun resource_away () Bool
  (and resource_departed (not resource_returned)))
(define-fun guard_away () Bool
  (and guard_departed (not guard_returned)))

(push)
(assert resource_away)
(assert (=> resource_departed guard_departed))
(assert (=> guard_returned resource_returned))
(assert (not guard_away))
; EXPECT: unsat
(check-sat)
(pop)

; Without the return-acceptance edge, the local guard can be restored while
; the remote return is not even accepted. An accepted return may await receipt.
(push)
(assert resource_away)
(assert (=> resource_departed guard_departed))
(assert guard_returned)
; EXPECT: sat
(check-sat)
(pop)

; Source preorder across par is not the departure-to-departure causal edge.
(push)
(assert resource_away)
(assert (=> guard_returned resource_returned))
(assert (not guard_departed))
; EXPECT: sat
(check-sat)
(pop)

; Without the fresh-visit origin obligation, an outside sender remains free
; to send even when the holder has permanently handed the resource away.
(push)
(assert (and needed_send (not (= sender owner)) (not needed_receive)))
(assert (not authorized))
; EXPECT: sat
(check-sat)
(pop)

; A genuinely earlier permit authorizes the remote sender.
(push)
(assert (and needed_receive (< earlier event) (= earlier_receiver sender)))
(assert earlier_authorized)
(assert (not authorized))
; EXPECT: unsat
(check-sat)
(pop)

; Par contributes no before edge. Merely having a permit in a sibling is not
; enough; the construction obligation must reject this origin.
(push)
(assert (and needed_send (not (= sender owner)) (not relay)))
(assert (=> needed_send (or (= sender owner) relay)))
; EXPECT: unsat
(check-sat)
(pop)

; Unrelated par work is outside this resource's causal closure and needs no
; permit from its owner.
(push)
(assert (and (not needed_send) (not (= sender owner)) (not needed_receive)))
(assert (=> needed_send (or (= sender owner) relay)))
; EXPECT: sat
(check-sat)
(pop)
