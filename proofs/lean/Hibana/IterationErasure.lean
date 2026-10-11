import Hibana.StaticProjectability

namespace Hibana

abbrev CausalRoleTimes := Nat → Nat

def CausalRoleTimes.update (times : CausalRoleTimes) (role time : Nat) : CausalRoleTimes :=
  fun query => if query = role then time else times query

/-- A concrete schedule for the query's projected prefix. A relay consumes
its sender's prior local action, sends, then receives. A parallel join waits
for both arms; route execution chooses only one arm. These are interpretation
premises, not claims that Lean has verified arbitrary Rust or carriers. -/
inductive TimedCausalFlowRun (origin : Nat) :
    CausalFlowExpr → CausalRoleTimes → CausalRoleTimes → Prop where
  | empty (before) : TimedCausalFlowRun origin .empty before before
  | seed {before role} (ordered : before role ≤ origin) :
      TimedCausalFlowRun origin (.seed role) before (before.update role origin)
  | relay {before sender receiver sent received}
      (localOrder : before sender < sent) (delivery : sent < received)
      (receiverOrder : before receiver ≤ received) :
      TimedCausalFlowRun origin (.relay sender receiver) before (before.update receiver received)
  | seq {left right before middle after}
      (first : TimedCausalFlowRun origin left before middle)
      (second : TimedCausalFlowRun origin right middle after) :
      TimedCausalFlowRun origin (.seq left right) before after
  | left {left right before after} (run : TimedCausalFlowRun origin left before after) :
      TimedCausalFlowRun origin (.choice left right) before after
  | right {left right before after} (run : TimedCausalFlowRun origin right before after) :
      TimedCausalFlowRun origin (.choice left right) before after
  | parallel {left right before leftAfter rightAfter}
      (first : TimedCausalFlowRun origin left before leftAfter)
      (second : TimedCausalFlowRun origin right before rightAfter) :
      TimedCausalFlowRun origin (.parallel left right) before
        (fun role => max (leftAfter role) (rightAfter role))

def CausalRoles.SoundAt (facts : CausalRoles) (origin : Nat) (times : CausalRoleTimes) : Prop :=
  ∀ role, facts role = true → origin ≤ times role

theorem causal_flow_preserves_schedule_evidence
    {origin : Nat} {flow : CausalFlowExpr} {before after : CausalRoleTimes}
    (run : TimedCausalFlowRun origin flow before after)
    {facts : CausalRoles} (sound : facts.SoundAt origin before) :
    (flow.eval facts).SoundAt origin after := by
  induction run generalizing facts with
  | empty => exact sound
  | @seed before role ordered =>
      intro query found
      by_cases same : query = role
      · simp [CausalRoleTimes.update, same]
      · simp only [CausalFlowExpr.eval, CausalRoles.insert, Bool.or_eq_true] at found
        have prior : facts query = true := by simpa [same] using found
        simpa [CausalRoleTimes.update, same] using sound query prior
  | @relay before sender receiver sent received localOrder delivery receiverOrder =>
      intro query found
      by_cases same : query = receiver
      · subst query
        by_cases reached : facts sender = true
        · have causal := Nat.le_trans (sound sender reached)
            (Nat.le_of_lt (Nat.lt_trans localOrder delivery))
          simpa [CausalRoleTimes.update] using causal
        · have prior : facts receiver = true := by simpa [CausalFlowExpr.eval, reached] using found
          simpa [CausalRoleTimes.update] using Nat.le_trans (sound receiver prior) receiverOrder
      · have prior : facts query = true := by
          by_cases reached : facts sender = true
          · simpa [CausalFlowExpr.eval, reached, CausalRoles.insert, same] using found
          · simpa [CausalFlowExpr.eval, reached] using found
        simpa [CausalRoleTimes.update, same] using sound query prior
  | seq _ _ firstIH secondIH => exact secondIH (firstIH sound)
  | left _ ih =>
      intro role found
      simp only [CausalFlowExpr.eval, Bool.and_eq_true] at found
      exact ih sound role found.1
  | right _ ih =>
      intro role found
      simp only [CausalFlowExpr.eval, Bool.and_eq_true] at found
      exact ih sound role found.2
  | parallel _ _ leftIH rightIH =>
      intro role found
      simp only [CausalFlowExpr.eval, Bool.or_eq_true] at found
      rcases found with first | second
      · exact Nat.le_trans (leftIH sound role first) (Nat.le_max_left _ _)
      · exact Nat.le_trans (rightIH sound role second) (Nat.le_max_right _ _)

/-- The earlier receive precedes the target sender's prefix in *every*
projected execution of every unfixed route choice. -/
def MustCausalHandoff (flow : CausalFlowExpr) (sender : Nat) : Prop :=
  ∀ origin before after, TimedCausalFlowRun origin flow before after → origin ≤ after sender

theorem accepted_causal_flow_has_must_handoff
    {flow : CausalFlowExpr} {sender : Nat}
    (accepted : flow.eval (fun _ => false) sender = true) : MustCausalHandoff flow sender := by
  intro origin before after run
  have initial : CausalRoles.SoundAt (fun _ => false) origin before := by
    intro role found
    contradiction
  exact causal_flow_preserves_schedule_evidence run initial sender accepted

theorem must_handoff_orders_receive_before_send
    {flow : CausalFlowExpr} {sender origin sent : Nat} {before after : CausalRoleTimes}
    (must : MustCausalHandoff flow sender)
    (run : TimedCausalFlowRun origin flow before after)
    (targetLocalOrder : after sender < sent) : origin < sent :=
  Nat.lt_of_le_of_lt (must origin before after run) targetLocalOrder

theorem receive_precedes_later_send_has_must_handoff
    {flow : CausalFlowExpr} {roleCount : Nat} {earlier later : StaticGlobalOccurrence}
    (accepted : receivePrecedesLaterSend flow roleCount earlier later = true) :
    MustCausalHandoff flow later.event.sender := by
  unfold receivePrecedesLaterSend at accepted
  split at accepted
  next _ => exact accepted_causal_flow_has_must_handoff accepted
  next _ => simp at accepted

/-- The FIFO/causal alternative is preserved across reentry. The two visits
use distinct route choices; a one-sided current-iteration handoff cannot be
borrowed from a different next-iteration route arm. -/
theorem roll_reentry_has_fifo_or_causal_order
    {body : Choreo} {roleCount : Nat}
    {left right : StaticGlobalOccurrence}
    (safe : body.RollBodyReceiveLaneCausalSafety roleCount)
    (leftMember : left ∈ body.staticGlobalOccurrences)
    (rightMember : right ∈ body.staticGlobalOccurrences)
    (leftNonlocal : left.event.sender ≠ left.event.receiver)
    (rightNonlocal : right.event.sender ≠ right.event.receiver)
    (sameReceiver : left.event.receiver = right.event.receiver)
    (sameLane : left.event.lane = right.event.lane)
    (_rightReceiverBound : right.event.receiver < roleCount) :
    left.event.sender = right.event.sender ∨
      MustCausalHandoff
        (body.rollCausalFlow left.globalId (body.globalEvents.length + right.globalId) roleCount)
        right.event.sender := by
  by_cases sameSender : left.event.sender = right.event.sender
  · exact Or.inl sameSender
  · right
    exact receive_precedes_later_send_has_must_handoff
      (roll_reentry_sender_change_requires_causal_handoff safe leftMember rightMember
        leftNonlocal rightNonlocal sameReceiver sameLane sameSender)

private def joinedRoute : Choreo :=
  .seq (.send 0 1 1 1)
    (.seq (.route .intrinsic (.send 1 2 2 1) (.send 1 2 3 1)) (.send 2 1 4 1))

theorem route_join_common_reply_accepted :
    checkStaticProjectability 3 joinedRoute = true := by decide

theorem route_join_common_reply_has_must_handoff :
    MustCausalHandoff (joinedRoute.causalFlow 0 3 3) 2 :=
  accepted_causal_flow_has_must_handoff (by decide)

theorem route_join_one_sided_handoff_rejected :
    (Choreo.seq (.send 0 1 1 1)
      (.seq (.route (.dynamic 7) (.send 1 2 2 1) (.send 3 4 3 1))
        (.send 2 1 4 1))).checkReceiveLaneCausality 5 = false := by decide

theorem parallel_arms_cannot_relay_facts :
    (Choreo.seq (.send 0 1 1 1)
      (.seq (.par (.send 1 2 2 1) (.send 2 3 3 1))
        (.send 3 1 4 1))).checkReceiveLaneCausality 4 = false := by decide

theorem rolled_route_join_closed_cycle_accepted :
    (Choreo.roll (.seq joinedRoute (.send 1 0 5 1))).checkRollReceiveLaneCausality 3 = true := by decide

theorem rolled_route_one_sided_cycle_rejected :
    (Choreo.roll (.seq (.send 0 1 1 1)
      (.seq (.route (.dynamic 7) (.send 1 2 2 1) (.send 3 4 3 1))
        (.seq (.send 2 1 4 1) (.send 1 0 5 1))))).checkRollReceiveLaneCausality 5 = false := by decide

end Hibana
