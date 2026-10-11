import Std

namespace Hibana

/-- The only two route arms admitted by the normalized proof language. -/
inductive RouteArm where
  | left
  | right
  deriving Repr, DecidableEq, BEq

/-- The unique authority attached to one normalized route site. -/
inductive RouteAuthority where
  | intrinsic
  | dynamic (resolver : Nat)
  deriving Repr, DecidableEq, BEq

/-- Whether a route site belongs to a reenterable roll body. -/
inductive ReentryMode where
  | singlePass
  | rolled
  deriving Repr, DecidableEq, BEq

/-- The normalized choreography language owned by the proof boundary. -/
inductive Choreo where
  | send (sender receiver label schema : Nat)
  | seq (left right : Choreo)
  | par (left right : Choreo)
  | route (authority : RouteAuthority) (left right : Choreo)
  | roll (body : Choreo)
  deriving Repr, DecidableEq, BEq

/-- One role-local interpretation of a global send. -/
inductive LocalAction where
  | send (peer label schema : Nat)
  | recv (peer label schema : Nat)
  | local (label schema : Nat)
  deriving Repr, DecidableEq, BEq

def LocalAction.label : LocalAction -> Nat
  | .send _ label _ | .recv _ label _ | .local label _ => label

def LocalAction.schema : LocalAction -> Nat
  | .send _ _ schema | .recv _ _ schema | .local _ schema => schema

def Choreo.localAction?
    (role sender receiver label schema : Nat) : Option LocalAction :=
  match Nat.beq role sender, Nat.beq role receiver with
  | true, true => if schema = 0 then some (.local label schema) else none
  | true, false => some (.send receiver label schema)
  | false, true => some (.recv sender label schema)
  | false, false => none

private theorem nat_beq_false_comm
    {left right : Nat} (different : Nat.beq left right = false) :
    Nat.beq right left = false := by
  cases reverse : Nat.beq right left with
  | false => rfl
  | true =>
      exact False.elim <|
        (Nat.ne_of_beq_eq_false different) (Nat.eq_of_beq_eq_true reverse).symm

theorem local_action_sender_projects_send
    {sender receiver label schema : Nat} (distinct : Nat.beq sender receiver = false) :
    Choreo.localAction? sender sender receiver label schema =
      some (.send receiver label schema) := by
  unfold Choreo.localAction?
  rw [Nat.beq_refl, distinct]

theorem local_action_receiver_projects_recv
    {sender receiver label schema : Nat} (distinct : Nat.beq sender receiver = false) :
    Choreo.localAction? receiver sender receiver label schema =
      some (.recv sender label schema) := by
  have reverse := nat_beq_false_comm distinct
  unfold Choreo.localAction?
  rw [reverse, Nat.beq_refl]

theorem local_action_send_recv_duality
    {sender receiver label schema : Nat} (distinct : Nat.beq sender receiver = false) :
    Choreo.localAction? sender sender receiver label schema =
      some (.send receiver label schema) /\
    Choreo.localAction? receiver sender receiver label schema =
      some (.recv sender label schema) :=
  ⟨local_action_sender_projects_send distinct,
    local_action_receiver_projects_recv distinct⟩

theorem local_action_uninvolved_projects_none
    {role sender receiver label schema : Nat}
    (notSender : Nat.beq role sender = false)
    (notReceiver : Nat.beq role receiver = false) :
    Choreo.localAction? role sender receiver label schema = none := by
  unfold Choreo.localAction?
  rw [notSender, notReceiver]

theorem local_action_unit_self_send_projects_local (role label : Nat) :
    Choreo.localAction? role role role label 0 = some (.local label 0) := by
  simp [Choreo.localAction?]

theorem local_action_payload_self_send_rejected
    {role label schema : Nat} (payload : schema ≠ 0) :
    Choreo.localAction? role role role label schema = none := by
  simp [Choreo.localAction?, payload]

theorem local_action_preserves_label
    {role sender receiver label schema : Nat} {action : LocalAction}
    (projected : Choreo.localAction? role sender receiver label schema = some action) :
    action.label = label := by
  cases senderCase : Nat.beq role sender with
  | false =>
      cases receiverCase : Nat.beq role receiver with
      | false => simp [Choreo.localAction?, senderCase, receiverCase] at projected
      | true =>
          have actionEq : LocalAction.recv sender label schema = action := by
            simpa [Choreo.localAction?, senderCase, receiverCase] using projected
          cases actionEq
          rfl
  | true =>
      cases receiverCase : Nat.beq role receiver with
      | false =>
          have actionEq : LocalAction.send receiver label schema = action := by
            simpa [Choreo.localAction?, senderCase, receiverCase] using projected
          cases actionEq
          rfl
      | true =>
          by_cases unitSchema : schema = 0
          · have actionEq : LocalAction.local label schema = action := by
              simpa [Choreo.localAction?, senderCase, receiverCase, unitSchema] using projected
            cases actionEq
            rfl
          · simp [Choreo.localAction?, senderCase, receiverCase, unitSchema] at projected

theorem local_action_preserves_schema
    {role sender receiver label schema : Nat} {action : LocalAction}
    (projected : Choreo.localAction? role sender receiver label schema = some action) :
    action.schema = schema := by
  cases senderCase : Nat.beq role sender with
  | false =>
      cases receiverCase : Nat.beq role receiver with
      | false => simp [Choreo.localAction?, senderCase, receiverCase] at projected
      | true =>
          have actionEq : LocalAction.recv sender label schema = action := by
            simpa [Choreo.localAction?, senderCase, receiverCase] using projected
          cases actionEq
          rfl
  | true =>
      cases receiverCase : Nat.beq role receiver with
      | false =>
          have actionEq : LocalAction.send receiver label schema = action := by
            simpa [Choreo.localAction?, senderCase, receiverCase] using projected
          cases actionEq
          rfl
      | true =>
          by_cases unitSchema : schema = 0
          · have actionEq : LocalAction.local label schema = action := by
              simpa [Choreo.localAction?, senderCase, receiverCase, unitSchema] using projected
            cases actionEq
            rfl
          · simp [Choreo.localAction?, senderCase, receiverCase, unitSchema] at projected

end Hibana
