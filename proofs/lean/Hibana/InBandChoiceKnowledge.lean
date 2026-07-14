import Hibana.StaticProjectability

namespace Hibana

/-- Runtime-local role membership is open only while endpoints are attached.
Invoking a dynamic resolver seals that exact set before external code runs. -/
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

end Hibana
