import Hibana.EndpointOwnership

namespace Hibana.EndpointDelegation

/-- Proof-only enumeration of the source's message occurrences. It includes
both route arms; elaboration keeps only the selected arm's occurrences. -/
def Source.messageEntries : Source → Nat → List (Nat × Message)
  | .send message, start => [(start, message)]
  | .seq left right, start | .par left right, start | .route _ left right, start =>
      left.messageEntries start ++ right.messageEntries (start + left.eventCount)
  | .roll body, start => body.messageEntries start

def Elaboration.messageEntries (run : Elaboration) : List (Nat × Message) :=
  run.bindings.map fun (event, binding) => (event, binding.message)

end Hibana.EndpointDelegation

namespace Hibana
open EndpointDelegation

/-- A parallel path joins only the selected children's effects. -/
theorem accepted_parallel_elaboration_has_separated_children
    {left right : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    (accepted : elaborate resources owners path (.par left right) start = some run) :
    ∃ first second,
      elaborate resources owners path left start = some first ∧
      elaborate resources owners path right (start + left.eventCount) = some second ∧
      incompatibleElaborations first second = false ∧
      run = {
        owners := fun key => if first.resources.contains key then first.owners key else second.owners key
        bindings := first.bindings ++ second.bindings
        equations := first.equations ++ second.equations
      } := by
  change (elaborate resources owners path left start).bind _ = some run at accepted
  obtain ⟨first, firstAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨second, secondAt, result⟩ := Option.bind_eq_some_iff.mp tail
  cases separated : incompatibleElaborations first second with
  | true => simp [separated] at result
  | false =>
    simp only [separated, Bool.false_eq_true, ite_false] at result
    exact ⟨first, second, firstAt, secondAt, separated, (Option.some.inj result).symm⟩

/-- Elaboration cannot invent a source event, message, or resource use. The
sublist retains exact occurrence identifiers, including across parallel parts.
This is a source property, not a mutable execution or owner table. Enumeration order is not a runtime
schedule for independent parallel branches. -/
theorem elaboration_retains_exact_source_message_occurrences
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    (accepted : elaborate resources owners path source start = some run) :
    run.messageEntries.Sublist (source.messageEntries start) := by
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
            exact .refl _
        | endpoint key =>
            simp only [payload] at accepted
            have same := Option.some.inj accepted
            subst run
            exact .refl _
  | seq left right leftIH rightIH =>
      change (elaborate resources owners path left start).bind _ = some run at accepted
      obtain ⟨first, firstAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
      change (elaborate resources first.owners path right (start + left.eventCount)).bind _ =
        some run at tail
      obtain ⟨second, secondAt, result⟩ := Option.bind_eq_some_iff.mp tail
      have same := Option.some.inj result
      subst run
      simpa [Elaboration.messageEntries, Source.messageEntries, List.map_append] using
        List.Sublist.append (leftIH firstAt) (rightIH secondAt)
  | par left right leftIH rightIH =>
      obtain ⟨first, second, firstAt, secondAt, _, rfl⟩ :=
        accepted_parallel_elaboration_has_separated_children accepted
      simpa [Elaboration.messageEntries, Source.messageEntries, List.map_append] using
        List.Sublist.append (leftIH firstAt) (rightIH secondAt)
  | route resolver left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next invalid => simp at accepted
      next valid =>
        obtain ⟨arm, armAt, selected⟩ := Option.bind_eq_some_iff.mp accepted
        cases arm with
        | left =>
            have chosen := leftIH selected
            exact chosen.trans (List.sublist_append_left _ _)
        | right =>
            have chosen := rightIH selected
            exact chosen.trans (List.sublist_append_right _ _)
  | roll body bodyIH =>
      change (elaborate resources owners path body start).bind _ = some run at accepted
      obtain ⟨inner, innerAt, result⟩ := Option.bind_eq_some_iff.mp accepted
      have same := Option.some.inj result
      subst run
      have inside : inner.messageEntries.Sublist (body.messageEntries start) := bodyIH innerAt
      simpa only [Elaboration.messageEntries, Source.messageEntries] using inside

/-- Parallel ownership is checked against all endpoint uses, including a
resource used as an ordinary sender/receiver or as another loan's carrier. -/
theorem separated_parallel_forbids_transfer_use_overlap
    {left right : Source} {key : EndpointKey}
    (accepted : incompatibleParallel left right = false)
    (transferred : key ∈ left.resources) : key ∉ right.uses := by
  intro used
  have overlap : left.resources.any right.uses.contains = true := by
    apply List.any_eq_true.mpr
    exact ⟨key, transferred, by simpa using used⟩
  simp [incompatibleParallel, overlap] at accepted

/-- An emitted transfer refers to a resource named by the same source. -/
theorem source_message_transfer_is_a_source_resource
    {source : Source} {start event : Nat} {message : Message} {key : EndpointKey}
    (present : (event, message) ∈ source.messageEntries start)
    (transferred : message.payload = .endpoint key) : key ∈ source.resources := by
  induction source generalizing start with
  | send original =>
      have same := congrArg Prod.snd (List.mem_singleton.mp present)
      dsimp only at same
      subst message
      simp [Source.resources, transferred]
  | seq left right leftIH rightIH | par left right leftIH rightIH | route resolver left right leftIH rightIH =>
      rcases List.mem_append.mp present with fromLeft | fromRight
      · have member := leftIH fromLeft
        have combined : key ∈ left.resources ++ right.resources :=
          List.mem_append.mpr (.inl member)
        simpa only [Source.resources, List.mem_eraseDups] using combined
      · have member := rightIH fromRight
        have combined : key ∈ left.resources ++ right.resources :=
          List.mem_append.mpr (.inr member)
        simpa only [Source.resources, List.mem_eraseDups] using combined
  | roll body bodyIH => exact bodyIH present

/-- This includes resource use as either a peer or another loan's payload. -/
theorem source_message_use_is_a_source_use
    {source : Source} {start event : Nat} {message : Message} {key : EndpointKey}
    (present : (event, message) ∈ source.messageEntries start)
    (used : message.sender = key ∨ message.receiver = key ∨
      message.payload = .endpoint key) : key ∈ source.uses := by
  induction source generalizing start with
  | send original =>
      have same := congrArg Prod.snd (List.mem_singleton.mp present)
      dsimp only at same
      subst message
      rcases used with sender | receiver | transferred
      · subst key
        simp [Source.uses]
      · subst key
        simp [Source.uses]
      · simp [Source.uses, transferred]
  | seq left right leftIH rightIH | par left right leftIH rightIH | route resolver left right leftIH rightIH =>
      rcases List.mem_append.mp present with fromLeft | fromRight
      · have member := leftIH fromLeft
        have combined : key ∈ left.uses ++ right.uses := List.mem_append.mpr (.inl member)
        simpa only [Source.uses, List.mem_eraseDups] using combined
      · have member := rightIH fromRight
        have combined : key ∈ left.uses ++ right.uses := List.mem_append.mpr (.inr member)
        simpa only [Source.uses, List.mem_eraseDups] using combined
  | roll body bodyIH => exact bodyIH present

theorem selected_transfer_is_an_elaboration_resource
    {run : Elaboration} {event : Nat} {binding : SymbolicBinding} {key : EndpointKey}
    (present : (event, binding) ∈ run.bindings)
    (transferred : binding.message.payload = .endpoint key) : key ∈ run.resources := by
  simp only [Elaboration.resources, List.mem_eraseDups, List.mem_flatMap]
  exact ⟨(event, binding), present,
    source_message_transfer_is_a_source_resource (source := .send binding.message)
      (start := 0) (event := 0) (message := binding.message)
      (by simp [Source.messageEntries]) transferred⟩

theorem selected_use_is_an_elaboration_use
    {run : Elaboration} {event : Nat} {binding : SymbolicBinding} {key : EndpointKey}
    (present : (event, binding) ∈ run.bindings)
    (used : binding.message.sender = key ∨ binding.message.receiver = key ∨
      binding.message.payload = .endpoint key) : key ∈ run.uses := by
  simp only [Elaboration.uses, List.mem_eraseDups, List.mem_flatMap]
  exact ⟨(event, binding), present,
    source_message_use_is_a_source_use (source := .send binding.message)
      (start := 0) (event := 0) (message := binding.message)
      (by simp [Source.messageEntries]) used⟩

theorem separated_elaborations_forbid_transfer_use_overlap
    {left right : Elaboration} {key : EndpointKey}
    (accepted : incompatibleElaborations left right = false)
    (transferred : key ∈ left.resources) : key ∉ right.uses := by
  intro used
  have overlap : left.resources.any right.uses.contains = true := by
    apply List.any_eq_true.mpr
    exact ⟨key, transferred, by simpa using used⟩
  simp [incompatibleElaborations, overlap] at accepted

/-- A selected transfer cannot overlap any selected use in the sibling. -/
theorem accepted_parallel_elaborations_cannot_alias_transfer_and_use
    {leftEvent rightEvent : Nat}
    {leftRun rightRun : Elaboration} {leftBinding rightBinding : SymbolicBinding}
    {key : EndpointKey}
    (separated : incompatibleElaborations leftRun rightRun = false)
    (leftPresent : (leftEvent, leftBinding) ∈ leftRun.bindings)
    (rightPresent : (rightEvent, rightBinding) ∈ rightRun.bindings)
    (transferred : leftBinding.message.payload = .endpoint key)
    (used : rightBinding.message.sender = key ∨ rightBinding.message.receiver = key ∨
      rightBinding.message.payload = .endpoint key) : False := by
  exact separated_elaborations_forbid_transfer_use_overlap separated
    (selected_transfer_is_an_elaboration_resource leftPresent transferred)
    (selected_use_is_an_elaboration_use rightPresent used)

end Hibana
