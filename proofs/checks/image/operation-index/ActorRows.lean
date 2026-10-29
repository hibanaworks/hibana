import Std

namespace Hibana.ActorRows

inductive Side where
  | local | send | recv
  deriving DecidableEq

/-- Logical self messages use one local operation. Distinct logical endpoints
retain both directions even when one physical actor handles both. -/
def required (self sender receiver : Bool) : List Side :=
  if self then (if sender then [.local] else [])
  else (if sender then [.send] else []) ++ (if receiver then [.recv] else [])

theorem required_directions_exact (self sender receiver : Bool) (side : Side) :
    side ∈ required self sender receiver ↔
      (side = .local ∧ self = true ∧ sender = true) ∨
      (side = .send ∧ self = false ∧ sender = true) ∨
      (side = .recv ∧ self = false ∧ receiver = true) := by
  cases self <;> cases sender <;> cases receiver <;> cases side <;>
    decide

theorem no_direction_is_duplicated (self sender receiver : Bool) :
    (required self sender receiver).Nodup := by
  cases self <;> cases sender <;> cases receiver <;> decide

theorem one_actor_retains_both_logical_facets :
    required false true true = [.send, .recv] := rfl

theorem logical_self_uses_one_local_operation :
    required true true true = [.local] := rfl

end Hibana.ActorRows

#print axioms Hibana.ActorRows.required_directions_exact
#print axioms Hibana.ActorRows.no_direction_is_duplicated
#print axioms Hibana.ActorRows.one_actor_retains_both_logical_facets
#print axioms Hibana.ActorRows.logical_self_uses_one_local_operation
