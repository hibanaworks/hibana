import Hibana.SourceResourceOrder

namespace Hibana.EndpointDelegation

/-- A characterization of compile-time symbolic ownership for one resource.
It is not an execution schedule or a new runtime ownership record. -/
def ownerAfterBindings (key : EndpointKey) : OwnerExpr → List (Nat × SymbolicBinding) → OwnerExpr
  | owner, [] => owner
  | owner, (_, binding) :: rest =>
      ownerAfterBindings key
        (if binding.message.payload = .endpoint key then binding.receiver else owner) rest

end Hibana.EndpointDelegation

namespace Hibana
open EndpointDelegation

theorem symbolic_owner_flow_composes
    (key : EndpointKey) (owner : OwnerExpr) (first second : List (Nat × SymbolicBinding)) :
    ownerAfterBindings key owner (first ++ second) =
      ownerAfterBindings key (ownerAfterBindings key owner first) second := by
  induction first generalizing owner with
  | nil => rfl
  | cons head tail ih => exact ih _

theorem symbolic_owner_flow_preserves_an_untouched_resource
    {key : EndpointKey} {owner : OwnerExpr} {bindings : List (Nat × SymbolicBinding)}
    (untouched : ∀ entry ∈ bindings, entry.2.message.payload ≠ .endpoint key) :
    ownerAfterBindings key owner bindings = owner := by
  induction bindings generalizing owner with
  | nil => rfl
  | cons head tail ih =>
      have atHead := untouched head (by simp)
      have atTail : ∀ entry ∈ tail, entry.2.message.payload ≠ .endpoint key := by
        intro entry member
        exact untouched entry (List.mem_cons_of_mem head member)
      simpa only [ownerAfterBindings, if_neg atHead] using (ih (owner := owner) atTail)

/-- The normalizer's owner expression is exactly the source's per-resource
transfer fold. Independent parallel changes commute because the branches
cannot transfer/use the same resource. This adds no ordering between them. -/
theorem elaborated_owner_is_the_symbolic_binding_flow
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration} (key : EndpointKey)
    (accepted : elaborate resources owners path source start = some run) :
    run.owners key = ownerAfterBindings key (owners key) run.bindings := by
  induction source generalizing owners start run with
  | send message =>
      simp only [elaborate] at accepted
      split at accepted
      next invalid => simp at accepted
      next valid =>
        cases payload : message.payload with
        | wire schema =>
            simp only [payload] at accepted
            have same := Option.some.inj accepted
            subst run
            simp [ownerAfterBindings, payload]
        | endpoint resource =>
            simp only [payload] at accepted
            have same := Option.some.inj accepted
            subst run
            simp [ownerAfterBindings, withOwner, payload, eq_comm]
  | seq left right leftIH rightIH =>
      change (elaborate resources owners path left start).bind _ = some run at accepted
      obtain ⟨leftRun, leftAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
      obtain ⟨rightRun, rightAt, result⟩ := Option.bind_eq_some_iff.mp tail
      have same := Option.some.inj result
      subst run
      have leftFlow := leftIH leftAt
      have rightFlow := rightIH rightAt
      simpa only [symbolic_owner_flow_composes, ← leftFlow] using rightFlow
  | par left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next incompatible => simp at accepted
      next compatible =>
        have separated : incompatibleParallel left right = false := by
          cases result : incompatibleParallel left right <;> simp_all
        obtain ⟨leftRun, leftAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
        obtain ⟨rightRun, rightAt, result⟩ := Option.bind_eq_some_iff.mp tail
        have same := Option.some.inj result
        subst run
        by_cases inLeft : key ∈ left.resources
        · have excluded := separated_parallel_forbids_transfer_use_overlap separated inLeft
          have noRight : ∀ entry ∈ rightRun.bindings, entry.2.message.payload ≠ .endpoint key := by
            intro entry member transfer
            rcases entry with ⟨event, binding⟩
            have fromSource : (event, binding.message) ∈ right.messageEntries (start + left.eventCount) := by
              apply (elaboration_retains_exact_source_message_occurrences rightAt).subset
              exact List.mem_map.mpr ⟨(event, binding), member, rfl⟩
            exact excluded (source_message_use_is_a_source_use fromSource (.inr (.inr transfer)))
          have ignores : ∀ owner, ownerAfterBindings key owner rightRun.bindings = owner := by
            intro owner
            exact symbolic_owner_flow_preserves_an_untouched_resource noRight
          have contains : left.resources.contains key = true := by simpa using inLeft
          simpa only [contains, Bool.true_eq, if_true, symbolic_owner_flow_composes, ignores]
            using leftIH leftAt
        · have noLeft : ∀ entry ∈ leftRun.bindings, entry.2.message.payload ≠ .endpoint key := by
            intro entry member transfer
            rcases entry with ⟨event, binding⟩
            have fromSource : (event, binding.message) ∈ left.messageEntries start := by
              apply (elaboration_retains_exact_source_message_occurrences leftAt).subset
              exact List.mem_map.mpr ⟨(event, binding), member, rfl⟩
            exact inLeft (source_message_transfer_is_a_source_resource fromSource transfer)
          have ignores : ownerAfterBindings key (owners key) leftRun.bindings = owners key :=
            symbolic_owner_flow_preserves_an_untouched_resource noLeft
          have contains : left.resources.contains key = false := by simpa using inLeft
          simpa only [contains, Bool.false_eq, if_false, symbolic_owner_flow_composes, ignores]
            using rightIH rightAt
  | route resolver left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next invalid => simp at accepted
      next valid =>
        obtain ⟨arm, armAt, selected⟩ := Option.bind_eq_some_iff.mp accepted
        cases arm with
        | left => exact leftIH selected
        | right => exact rightIH selected
  | roll body bodyIH =>
      change (elaborate resources owners path body start).bind _ = some run at accepted
      obtain ⟨inner, innerAt, result⟩ := Option.bind_eq_some_iff.mp accepted
      have same := Option.some.inj result
      subst run
      have innerFlow : inner.owners key = ownerAfterBindings key (owners key) inner.bindings :=
        bodyIH innerAt
      exact innerFlow

theorem elaboration_cannot_change_an_untouched_owner
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration} {key : EndpointKey}
    (accepted : elaborate resources owners path source start = some run)
    (untouched : key ∉ source.resources) : run.owners key = owners key := by
  have noTransfer : ∀ entry ∈ run.bindings, entry.2.message.payload ≠ .endpoint key := by
    intro entry member transfer
    rcases entry with ⟨event, binding⟩
    have fromSource : (event, binding.message) ∈ source.messageEntries start := by
      apply (elaboration_retains_exact_source_message_occurrences accepted).subset
      exact List.mem_map.mpr ⟨(event, binding), member, rfl⟩
    exact untouched (source_message_transfer_is_a_source_resource fromSource transfer)
  rw [elaborated_owner_is_the_symbolic_binding_flow key accepted]
  exact symbolic_owner_flow_preserves_an_untouched_resource noTransfer

end Hibana
