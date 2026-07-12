import Hibana.Syntax

namespace Hibana

/-- Runtime-local role membership is open only while endpoints are being
attached. Dynamic resolution seals the exact role set before invoking external
decision code. -/
inductive LocalMembership where
  | open (roles : List Nat)
  | sealed (roles : List Nat)
  deriving Repr, DecidableEq

def sealLocalMembership : LocalMembership -> LocalMembership
  | .open roles | .sealed roles => .sealed roles

def attachLocalRole? (membership : LocalMembership) (role : Nat) : Option LocalMembership :=
  match membership with
  | .open roles => some (.open ((role :: roles).eraseDups))
  | .sealed _ => none

theorem sealed_local_membership_rejects_late_attach
    (membership : LocalMembership) (role : Nat) :
    attachLocalRole? (sealLocalMembership membership) role = none := by
  cases membership <;> rfl

/-- One published dynamic route decision. `pending` contains exactly the roles
in the selected arm that have not yet obtained the decision from local
publication or an admitted frame. -/
structure AffineRouteCell where
  arm : RouteArm
  pending : List Nat
  deriving Repr, DecidableEq

/-- The resident route cell coordinates only selected roles attached to this
runtime. Roles on another runtime learn the arm from admitted frame evidence
and cannot retain local publication authority. -/
def selectedLocalRouteParticipants
    (selectedParticipants attachedRoles : List Nat) : List Nat :=
  selectedParticipants.eraseDups.filter fun role => attachedRoles.contains role

theorem selected_local_route_participants_are_exact
    {role : Nat} {selectedParticipants attachedRoles : List Nat} :
    role ∈ selectedLocalRouteParticipants selectedParticipants attachedRoles ↔
      role ∈ selectedParticipants ∧ role ∈ attachedRoles := by
  simp [selectedLocalRouteParticipants]

def pendingRouteParticipants (participants observed : List Nat) : List Nat :=
  participants.eraseDups.filter fun role => !observed.contains role

def beginRouteDecision?
    (current : Option AffineRouteCell)
    (arm : RouteArm)
    (localParticipants observed : List Nat) : Option (Option AffineRouteCell) :=
  match current with
  | some _ => none
  | none =>
      let pending := pendingRouteParticipants localParticipants observed
      if pending.isEmpty then some none else some (some { arm, pending })

def observeRouteDecision?
    (role : Nat)
    (current : Option AffineRouteCell) : Option (Option AffineRouteCell) :=
  match current with
  | none => none
  | some cell =>
      if role ∈ cell.pending then
        let pending := cell.pending.filter fun candidate => candidate != role
        if pending.isEmpty then some none else some (some { cell with pending })
      else
        none

def routeRolePending (role : Nat) : Option AffineRouteCell -> Bool
  | none => false
  | some cell => cell.pending.contains role

theorem active_route_decision_cannot_be_overwritten
    (cell : AffineRouteCell) (arm : RouteArm)
    (participants observed : List Nat) :
    beginRouteDecision? (some cell) arm participants observed = none := rfl

theorem begun_route_decision_tracks_exact_unobserved_participants
    {arm : RouteArm} {participants observed : List Nat}
    {next : Option AffineRouteCell}
    (begun : beginRouteDecision? none arm participants observed = some next) :
    next =
      let pending := pendingRouteParticipants participants observed
      if pending.isEmpty then none else some { arm, pending } := by
  by_cases empty : (pendingRouteParticipants participants observed).isEmpty
  · simp [beginRouteDecision?, empty] at begun ⊢
    exact begun.symm
  · simp [beginRouteDecision?, empty] at begun ⊢
    exact begun.symm

theorem route_observation_is_affine
    {role : Nat} {cell : AffineRouteCell}
    {next : Option AffineRouteCell}
    (observed : observeRouteDecision? role (some cell) = some next) :
    routeRolePending role next = false := by
  let remaining := cell.pending.filter fun candidate => candidate != role
  by_cases pending : role ∈ cell.pending
  · by_cases empty : remaining.isEmpty
    · simp [observeRouteDecision?, pending, remaining, empty] at observed
      subst next
      rfl
    · simp [observeRouteDecision?, pending, remaining, empty] at observed
      subst next
      simp [routeRolePending]
  · simp [observeRouteDecision?, pending] at observed

theorem route_observation_rejects_duplicate
    {role : Nat} {cell : AffineRouteCell}
    {next : Option AffineRouteCell}
    (observed : observeRouteDecision? role (some cell) = some next) :
    observeRouteDecision? role next = none := by
  let remaining := cell.pending.filter fun candidate => candidate != role
  by_cases pending : role ∈ cell.pending
  · by_cases empty : remaining.isEmpty
    · simp [observeRouteDecision?, pending, remaining, empty] at observed
      subst next
      rfl
    · simp [observeRouteDecision?, pending, remaining, empty] at observed
      subst next
      simp [observeRouteDecision?]
  · simp [observeRouteDecision?, pending] at observed

theorem route_observation_preserves_selected_arm
    {role : Nat} {cell next : AffineRouteCell}
    (observed : observeRouteDecision? role (some cell) = some (some next)) :
    next.arm = cell.arm := by
  let remaining := cell.pending.filter fun candidate => candidate != role
  by_cases pending : role ∈ cell.pending
  · by_cases empty : remaining.isEmpty
    · simp [observeRouteDecision?, pending, remaining, empty] at observed
    · simp [observeRouteDecision?, pending, remaining, empty] at observed
      exact observed ▸ rfl
  · simp [observeRouteDecision?, pending] at observed

theorem retired_route_decision_rejects_duplicate_observation
    (role : Nat) : observeRouteDecision? role none = none := rfl

end Hibana
