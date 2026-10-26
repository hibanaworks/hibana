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

/-- A parallel or choice split separates two events exactly when the split,
but neither enclosing boundary, lies between them. -/
theorem source_scope_separation_uses_only_boundaries_between_the_events
    (start split stop before after : Nat)
    (left : start < split) (right : split < stop) :
    (start ≤ before ∧ before < split ∧ split ≤ after ∧ after < stop) ↔
      (before < split ∧ split ≤ after ∧
        ¬ (before < start ∧ start ≤ after) ∧
        ¬ (before < stop ∧ stop ≤ after)) := by
  constructor
  · rintro ⟨startBefore, beforeSplit, splitAfter, afterStop⟩
    exact ⟨beforeSplit, splitAfter,
      fun boundary => Nat.not_lt_of_ge startBefore boundary.1,
      fun boundary => Nat.not_le_of_gt afterStop boundary.2⟩
  · rintro ⟨beforeSplit, splitAfter, noStart, noStop⟩
    have startBefore : start ≤ before := Nat.le_of_not_gt fun boundary =>
      noStart ⟨boundary, Nat.le_trans (Nat.le_of_lt left) splitAfter⟩
    have afterStop : after < stop := Nat.lt_of_not_ge fun boundary =>
      noStop ⟨Nat.lt_trans beforeSplit right, boundary⟩
    exact ⟨startBefore, beforeSplit, splitAfter, afterStop⟩

/-- In source preorder, a later scope either nests in an earlier scope or
starts after it. Once that earlier scope enters or closes in the query window,
no later scope can separate the queried events. -/
theorem source_preorder_elides_entered_or_closed_scopes
    (start stop laterStart laterSplit laterStop before after : Nat)
    (ordered : start ≤ laterStart)
    (nested : laterStart < stop → laterStop ≤ stop)
    (boundary : before < start ∨ (before < stop ∧ stop ≤ after)) :
    ¬ (laterStart ≤ before ∧ before < laterSplit ∧
      laterSplit ≤ after ∧ after < laterStop) := by
  intro separation
  rcases boundary with entered | closed
  · exact Nat.not_lt_of_ge (Nat.le_trans ordered separation.1) entered
  · have inside := Nat.lt_of_le_of_lt separation.1 closed.1
    exact Nat.not_lt_of_ge (Nat.le_trans (nested inside) closed.2) separation.2.2.2

/-- A receive needs causal evidence only up to the last change of sender on
its lane. The lookup's bound excludes no obligation in that source range. -/
theorem the_last_sender_change_bounds_every_causal_obligation
    (changed witnessed : Nat → Prop) (first stop cutoff : Nat)
    (within : cutoff ≤ stop)
    (lastChange : ∀ i, first ≤ i → i < stop → changed i → i < cutoff) :
    (∀ i, first ≤ i → i < stop → changed i → witnessed i) ↔
      (∀ i, first ≤ i → i < cutoff → changed i → witnessed i) := by
  constructor
  · intro all i lower upper change
    exact all i lower (Nat.lt_of_lt_of_le upper within) change
  · intro bounded i lower upper change
    exact bounded i lower (lastChange i lower upper change) change

#print axioms shared_holder_preserves_order_between_different_endpoint_views
#print axioms a_parallel_sibling_cannot_supply_that_local_edge
#print axioms a_received_handoff_can_continue_on_another_view_of_its_holder
#print axioms a_relay_extends_the_path_only_after_its_send
#print axioms source_scope_separation_uses_only_boundaries_between_the_events
#print axioms source_preorder_elides_entered_or_closed_scopes
#print axioms the_last_sender_change_bounds_every_causal_obligation

end Hibana.EndpointDelegation
