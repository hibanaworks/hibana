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
  | send (sender receiver label : Nat)
  | seq (left right : Choreo)
  | par (left right : Choreo)
  | route (authority : RouteAuthority) (left right : Choreo)
  | roll (body : Choreo)
  deriving Repr, DecidableEq, BEq

/-- One role-local interpretation of a global send. -/
inductive LocalAction where
  | send (peer label : Nat)
  | recv (peer label : Nat)
  | local (label : Nat)
  deriving Repr, DecidableEq, BEq

def LocalAction.label : LocalAction -> Nat
  | .send _ label | .recv _ label | .local label => label

def Choreo.localAction? (role sender receiver label : Nat) : Option LocalAction :=
  match Nat.beq role sender, Nat.beq role receiver with
  | true, true => some (.local label)
  | true, false => some (.send receiver label)
  | false, true => some (.recv sender label)
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
    {sender receiver label : Nat} (distinct : Nat.beq sender receiver = false) :
    Choreo.localAction? sender sender receiver label = some (.send receiver label) := by
  unfold Choreo.localAction?
  rw [Nat.beq_refl, distinct]

theorem local_action_receiver_projects_recv
    {sender receiver label : Nat} (distinct : Nat.beq sender receiver = false) :
    Choreo.localAction? receiver sender receiver label = some (.recv sender label) := by
  have reverse := nat_beq_false_comm distinct
  unfold Choreo.localAction?
  rw [reverse, Nat.beq_refl]

theorem local_action_send_recv_duality
    {sender receiver label : Nat} (distinct : Nat.beq sender receiver = false) :
    Choreo.localAction? sender sender receiver label = some (.send receiver label) /\
    Choreo.localAction? receiver sender receiver label = some (.recv sender label) :=
  ⟨local_action_sender_projects_send distinct,
    local_action_receiver_projects_recv distinct⟩

theorem local_action_uninvolved_projects_none
    {role sender receiver label : Nat}
    (notSender : Nat.beq role sender = false)
    (notReceiver : Nat.beq role receiver = false) :
    Choreo.localAction? role sender receiver label = none := by
  unfold Choreo.localAction?
  rw [notSender, notReceiver]

theorem local_action_self_send_projects_local (role label : Nat) :
    Choreo.localAction? role role role label = some (.local label) := by
  unfold Choreo.localAction?
  rw [Nat.beq_refl]

theorem local_action_preserves_label
    {role sender receiver label : Nat} {action : LocalAction}
    (projected : Choreo.localAction? role sender receiver label = some action) :
    action.label = label := by
  unfold Choreo.localAction? at projected
  split at projected <;> cases projected <;> rfl

end Hibana
