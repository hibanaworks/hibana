import Hibana.EndpointOwnership

namespace Hibana.EndpointDelegation

/-- A local edge belongs to the actor, even when its two actions use different
logical endpoints. Message delivery adds a send-to-receive edge separately. -/
inductive CausalEdge (source : Source) (bindings : List (Nat × Binding)) :
    ActionId → ActionId → Prop where
  | local {before after} (ordered : precedes source bindings before after = true) :
      CausalEdge source bindings before after
  | delivery {event : Nat} {binding : Binding}
      (present : (event, binding) ∈ bindings)
      (nonlocal : binding.message.sender ≠ binding.message.receiver) :
      CausalEdge source bindings ⟨event, .send⟩ ⟨event, .receive⟩

abbrev CausalPath (source : Source) (bindings : List (Nat × Binding)) :=
  Relation.TransGen (CausalEdge source bindings)

theorem shared_holder_preserves_order_between_different_endpoint_views
    (source : Source) (bindings : List (Nat × Binding))
    (before after : ActionId) (left right : Action)
    (first : action? bindings before = some left)
    (second : action? bindings after = some right)
    (holder : left.actor = right.actor)
    (differentEvents : before.event ≠ after.event)
    (ordered : source.before before.event after.event = true) :
    CausalPath source bindings before after := by
  apply Relation.TransGen.single
  apply CausalEdge.local
  have sameActor : (left.actor == right.actor) = true := by
    change decide (left.actor = right.actor) = true
    exact decide_eq_true holder
  have differentEvent : ¬ (before.event == after.event) = true := by
    intro equal
    exact differentEvents (of_decide_eq_true equal)
  simp only [precedes, first, second, sameActor, if_neg differentEvent,
    ordered, Bool.and_self]

theorem a_parallel_sibling_cannot_supply_that_local_edge
    (left right : Source) (bindings : List (Nat × Binding))
    (before after : ActionId)
    (inLeft : before.event < left.eventCount)
    (inRight : left.eventCount ≤ after.event) :
    precedes (.par left right) bindings before after = false := by
  have distinct : before.event ≠ after.event := by omega
  have unordered := independent_parallel_siblings_have_no_local_order
    left right inLeft inRight
  unfold precedes
  split <;> simp_all

theorem a_received_handoff_can_continue_on_another_view_of_its_holder
    (source : Source) (bindings : List (Nat × Binding)) (origin receive send : ActionId)
    (received : CausalPath source bindings origin receive)
    (localOrder : precedes source bindings receive send = true) :
    CausalPath source bindings origin send :=
  Relation.TransGen.tail received (CausalEdge.local localOrder)

theorem a_relay_extends_the_path_only_after_its_send
    (source : Source) (bindings : List (Nat × Binding)) (origin : ActionId) (event : Nat)
    (sent : CausalPath source bindings origin ⟨event, .send⟩)
    (binding : Binding) (present : (event, binding) ∈ bindings)
    (nonlocal : binding.message.sender ≠ binding.message.receiver) :
    CausalPath source bindings origin ⟨event, .receive⟩ :=
  Relation.TransGen.tail sent (CausalEdge.delivery present nonlocal)

#print axioms shared_holder_preserves_order_between_different_endpoint_views
#print axioms a_parallel_sibling_cannot_supply_that_local_edge
#print axioms a_received_handoff_can_continue_on_another_view_of_its_holder
#print axioms a_relay_extends_the_path_only_after_its_send

end Hibana.EndpointDelegation
