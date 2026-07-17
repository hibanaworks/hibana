import Hibana.Refinement

namespace Hibana

structure DecodedRouteResolver where
  scope : Nat
  authority : RouteAuthority
  controller : Option Nat
  leftParticipants : List Nat
  rightParticipants : List Nat
  deriving Repr, DecidableEq

def DecodedRouteResolver.rebase
    (resolver : DecodedRouteResolver) (scopeOffset : Nat) : DecodedRouteResolver :=
  { resolver with scope := resolver.scope + scopeOffset }

def Choreo.participants : Choreo -> List Nat
  | .send sender receiver _ _ => [sender, receiver].eraseDups
  | .seq left right | .par left right | .route _ left right =>
      (left.participants ++ right.participants).eraseDups
  | .roll body => body.participants

def insertParticipant (role : Nat) : List Nat -> List Nat
  | [] => [role]
  | head :: tail =>
      if role < head then role :: head :: tail
      else if role = head then head :: tail
      else head :: insertParticipant role tail

def canonicalParticipants (roles : List Nat) : List Nat :=
  roles.foldl (fun sorted role => insertParticipant role sorted) []

def uniqueController (roles : List Nat) : Option Nat :=
  resolveUnique? roles

def validRouteParticipants
    (participants : List Nat) (controller roleCount : Nat) : Bool :=
  decide (
    participants ≠ [] ∧
    participants.Pairwise (· < ·) ∧
    (∀ role ∈ participants, role < roleCount) ∧
    controller ∈ participants)

example : uniqueController [255] = some 255 := by decide

example : uniqueController [0, 255] = none := by decide

example : canonicalParticipants [255, 0, 255] = [0, 255] := by decide

example : validRouteParticipants [255] 255 256 = true := by decide

end Hibana
