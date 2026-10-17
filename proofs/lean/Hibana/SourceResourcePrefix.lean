import Hibana.SymbolicResourceFlow
import Init.Data.List.Nat.TakeDrop

namespace Hibana
open EndpointDelegation

/-- Every symbolic binding reads the owner expressions of its exact source
prefix. A loan also emits the equation tying its resource to that sender.
The prefix is a compile-time enumeration; independent changes commute. -/
theorem elaborated_bindings_read_their_source_prefix_owners
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    (accepted : elaborate resources owners path source start = some run) :
    ∀ index event binding, run.bindings[index]? = some (event, binding) →
      binding.sender = ownerAfterBindings binding.message.sender
        (owners binding.message.sender) (run.bindings.take index) ∧
      binding.receiver = ownerAfterBindings binding.message.receiver
        (owners binding.message.receiver) (run.bindings.take index) ∧
      ∀ key, binding.message.payload = .endpoint key →
        (ownerAfterBindings key (owners key) (run.bindings.take index), binding.sender) ∈ run.equations := by
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
            intro index event binding present
            cases index with
            | zero =>
                have bindingSame := congrArg Prod.snd (Option.some.inj present)
                dsimp only at bindingSame
                subst binding
                refine ⟨rfl, rfl, ?_⟩
                intro key transferred
                simp [payload] at transferred
            | succ index => simp at present
        | endpoint resource =>
            simp only [payload] at accepted
            have same := Option.some.inj accepted
            subst run
            intro index event binding present
            cases index with
            | zero =>
                have bindingSame := congrArg Prod.snd (Option.some.inj present)
                dsimp only at bindingSame
                subst binding
                refine ⟨rfl, rfl, ?_⟩
                intro key transferred
                have keySame : resource = key := by simpa [payload] using transferred
                subst key
                simp [ownerAfterBindings]
            | succ index => simp at present
  | seq left right leftIH rightIH =>
      change (elaborate resources owners path left start).bind _ = some run at accepted
      obtain ⟨leftRun, leftAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
      obtain ⟨rightRun, rightAt, result⟩ := Option.bind_eq_some_iff.mp tail
      have same := Option.some.inj result
      subst run
      intro index event binding present
      by_cases inLeft : index < leftRun.bindings.length
      · have atLeft : leftRun.bindings[index]? = some (event, binding) := by
          simpa [List.getElem?_append, inLeft] using present
        have prefixRows : (leftRun.bindings ++ rightRun.bindings).take index = leftRun.bindings.take index :=
          List.take_append_of_le_length (by omega)
        obtain ⟨sender, receiver, loan⟩ := leftIH leftAt index event binding atLeft
        refine ⟨by simpa only [prefixRows] using sender, by simpa only [prefixRows] using receiver, ?_⟩
        intro key transferred
        apply List.mem_append.mpr
        exact .inl (by simpa only [prefixRows] using loan key transferred)
      · have afterLeft : leftRun.bindings.length ≤ index := by omega
        have atRight : rightRun.bindings[index - leftRun.bindings.length]? = some (event, binding) := by
          simpa [List.getElem?_append, inLeft] using present
        have prefixRows : (leftRun.bindings ++ rightRun.bindings).take index =
            leftRun.bindings ++ rightRun.bindings.take (index - leftRun.bindings.length) := by
          rw [List.take_append, List.take_of_length_le afterLeft]
        have ownerPrefix (key : EndpointKey) :
            ownerAfterBindings key (owners key) ((leftRun.bindings ++ rightRun.bindings).take index) =
              ownerAfterBindings key (leftRun.owners key) (rightRun.bindings.take (index - leftRun.bindings.length)) := by
          rw [prefixRows, symbolic_owner_flow_composes, ← elaborated_owner_is_the_symbolic_binding_flow key leftAt]
        obtain ⟨sender, receiver, loan⟩ := rightIH rightAt (index - leftRun.bindings.length) event binding atRight
        refine ⟨by simpa only [ownerPrefix] using sender, by simpa only [ownerPrefix] using receiver, ?_⟩
        intro key transferred
        apply List.mem_append.mpr
        exact .inr (by simpa only [ownerPrefix] using loan key transferred)
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
        intro index event binding present
        by_cases inLeft : index < leftRun.bindings.length
        · have atLeft : leftRun.bindings[index]? = some (event, binding) := by
            simpa [List.getElem?_append, inLeft] using present
          have prefixRows : (leftRun.bindings ++ rightRun.bindings).take index = leftRun.bindings.take index :=
            List.take_append_of_le_length (by omega)
          obtain ⟨sender, receiver, loan⟩ := leftIH leftAt index event binding atLeft
          refine ⟨by simpa only [prefixRows] using sender, by simpa only [prefixRows] using receiver, ?_⟩
          intro key transferred
          apply List.mem_append.mpr
          exact .inl (by simpa only [prefixRows] using loan key transferred)
        · have afterLeft : leftRun.bindings.length ≤ index := by omega
          have atRight : rightRun.bindings[index - leftRun.bindings.length]? = some (event, binding) := by
            simpa [List.getElem?_append, inLeft] using present
          have prefixRows : (leftRun.bindings ++ rightRun.bindings).take index =
              leftRun.bindings ++ rightRun.bindings.take (index - leftRun.bindings.length) := by
            rw [List.take_append, List.take_of_length_le afterLeft]
          have fromSource : (event, binding.message) ∈ right.messageEntries (start + left.eventCount) := by
            apply (elaboration_retains_exact_source_message_occurrences rightAt).subset
            exact List.mem_map.mpr ⟨(event, binding), List.mem_of_getElem? atRight, rfl⟩
          have unchanged (key : EndpointKey)
              (used : binding.message.sender = key ∨ binding.message.receiver = key ∨ binding.message.payload = .endpoint key) :
              ownerAfterBindings key (owners key) leftRun.bindings = owners key := by
            have usedInRight := source_message_use_is_a_source_use fromSource used
            have notTransferred : key ∉ left.resources := by
              intro member
              exact separated_parallel_forbids_transfer_use_overlap separated member usedInRight
            exact (elaborated_owner_is_the_symbolic_binding_flow key leftAt).symm.trans
              (elaboration_cannot_change_an_untouched_owner leftAt notTransferred)
          have ownerPrefix (key : EndpointKey)
              (used : binding.message.sender = key ∨ binding.message.receiver = key ∨ binding.message.payload = .endpoint key) :
              ownerAfterBindings key (owners key) ((leftRun.bindings ++ rightRun.bindings).take index) =
                ownerAfterBindings key (owners key) (rightRun.bindings.take (index - leftRun.bindings.length)) := by
            rw [prefixRows, symbolic_owner_flow_composes, unchanged key used]
          obtain ⟨sender, receiver, loan⟩ := rightIH rightAt (index - leftRun.bindings.length) event binding atRight
          refine ⟨?_, ?_, ?_⟩
          · rw [ownerPrefix _ (.inl rfl)]
            exact sender
          · rw [ownerPrefix _ (.inr (.inl rfl))]
            exact receiver
          · intro key transferred
            apply List.mem_append.mpr
            right
            rw [ownerPrefix _ (.inr (.inr transferred))]
            exact loan key transferred
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
      intro index event binding present
      obtain ⟨sender, receiver, loan⟩ := bodyIH innerAt index event binding present
      refine ⟨sender, receiver, ?_⟩
      intro key transferred
      exact List.mem_append.mpr (.inl (loan key transferred))

/-- Once issuance solves the source equations, a loan's sender is the resource
owner at that source prefix, and its receiver is the owner after the transfer.
These are source-derived actors, not another mutable ownership authority. -/
theorem instantiated_bindings_preserve_source_ownership_transfer
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    {issuance : List Issuance} {index event : Nat} {symbolic : SymbolicBinding}
    {binding : Binding}
    (accepted : elaborate resources owners path source start = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (instantiated : instantiateBinding issuance symbolic = some binding)
    (solved : run.equations.all (equationAccepted issuance) = true) :
    binding.message = symbolic.message ∧
      instantiate issuance (ownerAfterBindings symbolic.message.sender
        (owners symbolic.message.sender) (run.bindings.take index)) = some binding.senderActor ∧
      instantiate issuance (ownerAfterBindings symbolic.message.receiver
        (owners symbolic.message.receiver) (run.bindings.take index)) = some binding.receiverActor ∧
      ∀ key, symbolic.message.payload = .endpoint key →
        instantiate issuance (ownerAfterBindings key (owners key) (run.bindings.take index)) =
          some binding.senderActor ∧
        instantiate issuance (ownerAfterBindings key (owners key)
          (run.bindings.take index ++ [(event, symbolic)])) = some binding.receiverActor := by
  obtain ⟨senderRef, receiverRef, loanEquation⟩ :=
    elaborated_bindings_read_their_source_prefix_owners accepted index event symbolic present
  change (instantiate issuance symbolic.sender).bind _ = some binding at instantiated
  obtain ⟨sender, senderAt, rest⟩ := Option.bind_eq_some_iff.mp instantiated
  obtain ⟨receiver, receiverAt, written⟩ := Option.bind_eq_some_iff.mp rest
  have same := Option.some.inj written
  subst binding
  refine ⟨rfl, ?_, ?_, ?_⟩
  · simpa only [← senderRef] using senderAt
  · simpa only [← receiverRef] using receiverAt
  · intro key transferred
    have equation := List.all_eq_true.mp solved _ (loanEquation key transferred)
    have oldOwner : instantiate issuance (ownerAfterBindings key (owners key) (run.bindings.take index)) = some sender := by
      cases ownerAt : instantiate issuance (ownerAfterBindings key (owners key) (run.bindings.take index)) with
      | none => simp [equationAccepted, ownerAt] at equation
      | some owner =>
          have sameActor : owner = sender := by
            simpa [equationAccepted, ownerAt, senderAt] using equation
          simp only [sameActor]
    refine ⟨oldOwner, ?_⟩
    simpa only [symbolic_owner_flow_composes, ownerAfterBindings, if_pos transferred]
      using receiverAt

end Hibana
