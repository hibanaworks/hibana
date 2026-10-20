import Hibana.OccurrenceResourceOrigin

namespace Hibana
open EndpointDelegation

private theorem a_source_receive_has_its_send_half
    {bindings : List (Nat × Binding)} {event : Nat} {action : Action}
    (received : action? bindings ⟨event, .receive⟩ = some action) :
    ∃ sender, action? bindings ⟨event, .send⟩ = some sender := by
  cases found : bindings.find? (fun entry => entry.1 == event) with
  | none => simp [action?, found] at received
  | some pair =>
    by_cases same : pair.2.message.sender = pair.2.message.receiver
    · simp [action?, found, same] at received
    · exact ⟨{ actor := pair.2.senderActor, endpoint := pair.2.message.sender, side := .send },
        by simp [action?, found, same]⟩

/-- The accepted source orders transfers of one resource through receipt
edges, even when physical actors and roll copies differ. Other parallel
resources acquire no dependency. Sender publication is not on this path.
This closes the source-side premise needed to rule out an old acceptance
inside the same word's last-receipt-to-pending-use interval. -/
theorem earlier_resource_receipts_causally_precede_a_later_handoff
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {run : Elaboration} {issuance : List Issuance}
    {bindings : List (Nat × Binding)} {resource : EndpointKey}
    (elaborated : elaborate resources owners path source 0 = some run)
    (solved : run.equations.all (equationAccepted issuance) = true)
    (mapped : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some bindings) :
    ∀ event binding action,
      bindings.find? (fun entry => entry.1 == event) = some (event, binding) →
      binding.message.payload = .endpoint resource →
      action? bindings ⟨event, .send⟩ = some action →
      ∀ earlier previous,
        bindings.find? (fun entry => entry.1 == earlier) = some (earlier, previous) →
        previous.message.payload = .endpoint resource → earlier < event →
        Relation.TransGen (SourcePointEdge source bindings)
          (.committed ⟨earlier, .receive⟩) (.accepted event) := by
  intro event
  induction event using Nat.strongRecOn with
  | ind event ih =>
    intro binding action located payload decoded earlier previous previousAt previousPayload less
    obtain ⟨symbolic, member, instantiated⟩ :=
      instantiated_path_binding_has_source_input mapped located
    obtain ⟨index, bound, atIndex⟩ := List.mem_iff_getElem.mp member
    have sourceAt : run.bindings[index]? = some (event, symbolic) := by
      rw [List.getElem?_eq_getElem bound, atIndex]
    have symbolicPayload : symbolic.message.payload = .endpoint resource := by
      simpa only [(instantiated_binding_retains_message_and_actor_references instantiated).1] using payload
    obtain ⟨oldSymbolic, oldMember, oldInstantiated⟩ :=
      instantiated_path_binding_has_source_input mapped previousAt
    have oldPayload : oldSymbolic.message.payload = .endpoint resource := by
      simpa only [(instantiated_binding_retains_message_and_actor_references oldInstantiated).1] using previousPayload
    rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
        elaborated sourceAt instantiated solved mapped (Or.inr symbolicPayload) decoded with
      ⟨initialActor, initialAt, actorAt, untouched⟩ |
      ⟨priorEvent, priorBinding, priorAction, priorAt, priorPayload, priorDecoded,
        sameActor, priorBound, priorOrder, edge, beforeRows, priorSymbolic, afterRows,
        shape, transferred, untouched, priorInstantiated⟩
    · rcases an_initial_resource_has_no_handoff_before_its_target elaborated sourceAt
        untouched oldMember oldPayload (Or.inr (Or.inr symbolicPayload)) with same | after
      · exfalso
        omega
      · have := source_order_strictly_increases_event after
        exfalso
        omega
    · have priorLess : priorEvent < event := by
        have increasing := source_point_dependency_increases_rank edge
        simp only [SourcePoint.rank, ActionId.entryPoint] at increasing
        omega
      have outside := a_resource_handoff_is_outside_its_last_receipt_to_target_interval
        elaborated sourceAt shape transferred untouched oldMember oldPayload
        (Or.inr (Or.inr symbolicPayload))
      have atMostPrior : earlier ≤ priorEvent := by
        rcases outside with before | same | same | after
        · exact Nat.le_of_lt (source_order_strictly_increases_event before)
        · omega
        · omega
        · have := source_order_strictly_increases_event after
          omega
      by_cases same : earlier = priorEvent
      · subst earlier
        exact .single edge
      · obtain ⟨priorSend, priorSendAt⟩ := a_source_receive_has_its_send_half priorDecoded
        have earlierThanPrior : earlier < priorEvent := by omega
        have causalPrefix := ih priorEvent priorLess priorBinding priorSend priorAt priorPayload
          priorSendAt earlier previous previousAt previousPayload earlierThanPrior
        exact .tail (.tail causalPrefix (.deliver priorEvent)) edge



private theorem a_source_execution_retains_its_dependency_closure
    (execution : SourceOccurrenceExecution first last points)
    (initialClosed : ∀ before after,
      SourcePointEdge first.expanded first.path.bindings before after →
      first.observed after = true → first.observed before = true) :
    ∀ before after, SourcePointEdge last.expanded last.path.bindings before after →
      last.observed after = true → last.observed before = true := by
  induction execution with
  | nil => exact initialClosed
  | @tail beforeCut priorPoints point afterCut earlier step ih =>
    cases step with
    | advance present pending ready =>
      intro before after edge observed
      by_cases same : after = point
      · subst after
        have kept := ready before edge
        simp only [kept, ite_self]
      · have kept := ih before after edge (by simpa only [if_neg same] using observed)
        simp only [kept, ite_self]

private theorem a_closed_cut_retains_every_causal_predecessor
    {cut : SourceOccurrenceCut source}
    (closed : ∀ before after, SourcePointEdge cut.expanded cut.path.bindings before after →
      cut.observed after = true → cut.observed before = true)
    (path : Relation.TransGen (SourcePointEdge cut.expanded cut.path.bindings) before after) :
    cut.observed after = true → cut.observed before = true := by
  induction path with
  | single edge => exact closed _ _ edge
  | tail earlier edge ih =>
    intro seen
    exact ih (closed _ _ edge seen)

/-- Closure is derived for this very chronological source word, not a guessed
final bitmap. Independent visits do not clear another occurrence's facts. -/
theorem a_source_occurrence_trace_retains_its_causal_predecessors
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (path : Relation.TransGen (SourcePointEdge plan.expanded plan.path.bindings) before after)
    (seen : after ∈ points) : before ∈ points := by
  have closed := a_source_execution_retains_its_dependency_closure
    (a_source_occurrence_trace_is_its_reconstructed_execution trace)
    (by intro before after edge observed; simp [occurrenceCut] at observed)
  have retained := a_closed_cut_retains_every_causal_predecessor closed path
    (by simpa only [occurrenceCut] using List.contains_iff_mem.mpr seen)
  exact List.contains_iff_mem.mp retained



/-- Resolve the static chain against THIS actual occurrence word. Every
resource receipt before the recorded later handoff is retained, so an old
handoff cannot be accepted again in this cut. No shared visit, sender
publication order, live Rust value, or final admission result is assumed. -/
theorem a_recorded_handoff_keeps_earlier_receipts_and_excludes_old_acceptances
    {plan : SourceOccurrenceCut source} {later earlier : Nat}
    {binding previous : Binding} {sender : Action} {resource : EndpointKey}
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (located : plan.path.bindings.find? (fun entry => entry.1 == later) =
      some (later, binding))
    (payload : binding.message.payload = .endpoint resource)
    (decoded : action? plan.path.bindings ⟨later, .send⟩ = some sender)
    (previousAt : plan.path.bindings.find? (fun entry => entry.1 == earlier) =
      some (earlier, previous))
    (previousPayload : previous.message.payload = .endpoint resource)
    (less : earlier < later)
    (recorded : SourcePoint.accepted later ∈ points) :
    SourcePoint.committed ⟨earlier, .receive⟩ ∈ points ∧
      (occurrenceCut plan points).ready (.accepted earlier) = false := by
  obtain ⟨run, elaborated, solved, mapped⟩ :=
    accepted_normalized_path_has_its_exact_issuance_derivation plan.accepted plan.selected
  have causal := earlier_resource_receipts_causally_precede_a_later_handoff
    elaborated solved mapped later binding sender located payload decoded
    earlier previous previousAt previousPayload less
  have received := a_source_occurrence_trace_retains_its_causal_predecessors trace causal recorded
  have accepted := a_source_occurrence_trace_retains_its_causal_predecessors trace
    (.single (.deliver earlier)) received
  have marked : (occurrenceCut plan points).observed (.accepted earlier) = true :=
    List.contains_iff_mem.mpr accepted
  exact ⟨received, by simp only [SourceOccurrenceCut.ready, marked, Bool.not_true,
    Bool.and_false, Bool.false_and]⟩



private theorem shared_actor_order_preserves_the_resource_use_boundary
    {source : Source} {bindings : List (Nat × Binding)} {resource : EndpointKey}
    {earlier later : Nat} {side : Side} {previous : Binding} {before after : Action}
    (decoded : action? bindings ⟨earlier, side⟩ = some before)
    (laterDecoded : action? bindings ⟨later, .send⟩ = some after)
    (sameActor : before.actor = after.actor)
    (causal : source.before earlier later = true) :
    Relation.TransGen (SourcePointEdge source bindings)
      (if side = .send ∧ previous.message.payload = .endpoint resource
       then .accepted earlier else .committed ⟨earlier, side⟩) (.accepted later) := by
  have distinct : earlier ≠ later := Nat.ne_of_lt (source_order_strictly_increases_event causal)
  have sameActorBool : (before.actor == after.actor) = true := decide_eq_true sameActor
  have ordered : precedes source bindings ⟨earlier, side⟩ ⟨later, .send⟩ = true := by
    simpa only [precedes, decoded, laterDecoded, sameActorBool,
      beq_eq_false_iff_ne.mpr distinct, Bool.true_and, Bool.false_eq_true, ite_false] using causal
  have edge := SourcePointEdge.ordered ordered
  split
  next owned =>
    obtain ⟨rfl, payload⟩ := owned
    exact .tail (.single (.publish earlier)) edge
  next ordinary => exact .single edge

/-- A required resource operation cannot be overtaken by a later transfer.
An ordinary operation must commit first. An owned send of this very resource
needs only acceptance: its former sender publication is not a remote barrier.
The chain follows derived holders and receipts, never a mutable owner table. -/
theorem required_resource_operations_causally_precede_a_later_handoff
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {run : Elaboration} {issuance : List Issuance}
    {bindings : List (Nat × Binding)} {resource : EndpointKey}
    (elaborated : elaborate resources owners path source 0 = some run)
    (solved : run.equations.all (equationAccepted issuance) = true)
    (mapped : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some bindings) :
    ∀ later binding action,
      bindings.find? (fun entry => entry.1 == later) = some (later, binding) →
      binding.message.payload = .endpoint resource →
      action? bindings ⟨later, .send⟩ = some action →
      ∀ earlier previous side before,
        bindings.find? (fun entry => entry.1 == earlier) = some (earlier, previous) →
        (match side with
         | .receive => previous.message.receiver = resource
         | .send => previous.message.sender = resource ∨ previous.message.payload = .endpoint resource
         | .local => previous.message.sender = resource) →
        action? bindings ⟨earlier, side⟩ = some before → earlier < later →
        Relation.TransGen (SourcePointEdge source bindings)
          (if side = .send ∧ previous.message.payload = .endpoint resource
           then .accepted earlier else .committed ⟨earlier, side⟩) (.accepted later) := by
  intro later
  induction later using Nat.strongRecOn with
  | ind later ih =>
    intro binding action located payload decoded earlier previous side before previousAt required beforeDecoded less
    obtain ⟨symbolic, member, instantiated⟩ := instantiated_path_binding_has_source_input mapped located
    obtain ⟨index, bound, atIndex⟩ := List.mem_iff_getElem.mp member
    have sourceAt : run.bindings[index]? = some (later, symbolic) := by
      rw [List.getElem?_eq_getElem bound, atIndex]
    have transferred : symbolic.message.payload = .endpoint resource := by
      simpa only [(instantiated_binding_retains_message_and_actor_references instantiated).1] using payload
    obtain ⟨oldSymbolic, oldMember, oldInstantiated⟩ :=
      instantiated_path_binding_has_source_input mapped previousAt
    obtain ⟨oldIndex, oldBound, oldAt⟩ := List.mem_iff_getElem.mp oldMember
    have oldSourceAt : run.bindings[oldIndex]? = some (earlier, oldSymbolic) := by
      rw [List.getElem?_eq_getElem oldBound, oldAt]
    have oldRequired : match (generalizing := false) side with
        | .receive => oldSymbolic.message.receiver = resource
        | .send => oldSymbolic.message.sender = resource ∨ oldSymbolic.message.payload = .endpoint resource
        | .local => oldSymbolic.message.sender = resource := by
      simpa only [(instantiated_binding_retains_message_and_actor_references oldInstantiated).1] using required
    have oldUsed : oldSymbolic.message.sender = resource ∨ oldSymbolic.message.receiver = resource ∨
        oldSymbolic.message.payload = .endpoint resource := by
      cases side with
      | receive => exact .inr (.inl oldRequired)
      | send => exact oldRequired.elim Or.inl (fun h => .inr (.inr h))
      | «local» => exact .inl oldRequired
    have causal : source.before earlier later = true := by
      simpa only [Nat.sub_zero] using an_earlier_resource_use_causally_precedes_a_transfer
        elaborated member oldMember transferred oldUsed less
    have sameActorPath (same : before.actor = action.actor) :=
      shared_actor_order_preserves_the_resource_use_boundary (previous := previous)
        (resource := resource) beforeDecoded decoded same causal
    rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
        elaborated sourceAt instantiated solved mapped (Or.inr transferred) decoded with
      ⟨initialActor, initialAt, actorAt, untouched⟩ |
      ⟨priorEvent, priorBinding, priorAction, priorAt, priorPayload, priorDecoded,
        sameActor, priorBound, priorOrder, edge, beforeRows, priorSymbolic, afterRows,
        shape, priorTransferred, untouched, priorInstantiated⟩
    · rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
          elaborated oldSourceAt oldInstantiated solved mapped oldRequired beforeDecoded with
        ⟨oldInitial, oldInitialAt, oldActorAt, oldUntouched⟩ |
        ⟨oldPriorEvent, oldPriorBinding, oldPriorAction, oldPriorAt, oldPriorPayload, oldPriorDecoded,
          oldSameActor, oldPriorBound, oldPriorOrder, oldEdge, oldBeforeRows, oldPriorSymbolic, oldAfterRows,
          oldShape, oldTransferred, oldUntouched, oldPriorInstantiated⟩
      · have same := Option.some.inj (oldInitialAt.symm.trans initialAt)
        exact sameActorPath (oldActorAt.trans (same.trans actorAt.symm))
      · have oldPriorInPrefix : (oldPriorEvent, oldPriorSymbolic) ∈ run.bindings.take oldIndex := by
          rw [oldShape]
          exact List.mem_append.mpr (.inr List.mem_cons_self)
        have oldPriorLess := a_binding_in_the_source_prefix_has_an_earlier_occurrence
          elaborated oldSourceAt oldPriorInPrefix
        exact False.elim (untouched _
          (an_earlier_source_binding_is_in_the_source_prefix elaborated sourceAt
            (List.mem_of_mem_take oldPriorInPrefix) (Nat.lt_trans oldPriorLess less)) oldTransferred)
    · have priorInPrefix : (priorEvent, priorSymbolic) ∈ run.bindings.take index := by
        rw [shape]
        exact List.mem_append.mpr (.inr List.mem_cons_self)
      have priorLess := a_binding_in_the_source_prefix_has_an_earlier_occurrence
        elaborated sourceAt priorInPrefix
      rcases Nat.lt_trichotomy earlier priorEvent with beforePrior | same | afterPrior
      · obtain ⟨priorSend, priorSendAt⟩ := a_source_receive_has_its_send_half priorDecoded
        have chain := ih priorEvent priorLess priorBinding priorSend priorAt priorPayload priorSendAt
          earlier previous side before previousAt required beforeDecoded beforePrior
        exact .tail (.tail chain (.deliver priorEvent)) edge
      · subst earlier
        have sameBinding : previous = priorBinding :=
          congrArg Prod.snd (Option.some.inj (previousAt.symm.trans priorAt))
        have previousPayload : previous.message.payload = .endpoint resource := by
          rw [sameBinding]
          exact priorPayload
        cases side with
        | send =>
          rw [if_pos ⟨rfl, previousPayload⟩]
          exact .tail (.single (.deliver priorEvent)) edge
        | receive =>
          simpa only [reduceCtorEq, false_and, ite_false] using Relation.TransGen.single edge
        | «local» =>
          by_cases localMessage : priorBinding.message.sender = priorBinding.message.receiver
          · simp [action?, priorAt, localMessage] at priorDecoded
          · simp [action?, priorAt, localMessage] at beforeDecoded
      · rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
            elaborated oldSourceAt oldInstantiated solved mapped oldRequired beforeDecoded with
          ⟨oldInitial, oldInitialAt, oldActorAt, oldUntouched⟩ |
          ⟨oldPriorEvent, oldPriorBinding, oldPriorAction, oldPriorAt, oldPriorPayload, oldPriorDecoded,
            oldSameActor, oldPriorBound, oldPriorOrder, oldEdge, oldBeforeRows, oldPriorSymbolic, oldAfterRows,
            oldShape, oldTransferred, oldUntouched, oldPriorInstantiated⟩
        · exact False.elim (oldUntouched _
            (an_earlier_source_binding_is_in_the_source_prefix elaborated oldSourceAt
              (List.mem_of_mem_take priorInPrefix) afterPrior) priorTransferred)
        · have oldPriorInPrefix : (oldPriorEvent, oldPriorSymbolic) ∈ run.bindings.take oldIndex := by
            rw [oldShape]
            exact List.mem_append.mpr (.inr List.mem_cons_self)
          have oldPriorLess := a_binding_in_the_source_prefix_has_an_earlier_occurrence
            elaborated oldSourceAt oldPriorInPrefix
          have oldAtMost : oldPriorEvent ≤ priorEvent := by
            rcases a_resource_handoff_is_outside_its_last_receipt_to_target_interval
                elaborated sourceAt shape priorTransferred untouched (List.mem_of_mem_take oldPriorInPrefix)
                oldTransferred (Or.inr (Or.inr transferred)) with first | same | same | last
            · exact Nat.le_of_lt (source_order_strictly_increases_event first)
            · omega
            · omega
            · have := source_order_strictly_increases_event last
              omega
          have priorAtMost : priorEvent ≤ oldPriorEvent := by
            rcases a_resource_handoff_is_outside_its_last_receipt_to_target_interval
                elaborated oldSourceAt oldShape oldTransferred oldUntouched
                (List.mem_of_mem_take priorInPrefix) priorTransferred oldUsed with first | same | same | last
            · exact Nat.le_of_lt (source_order_strictly_increases_event first)
            · omega
            · omega
            · have := source_order_strictly_increases_event last
              omega
          have same : oldPriorEvent = priorEvent := by omega
          subst oldPriorEvent
          have samePrevious : oldPriorAction = priorAction :=
            Option.some.inj (oldPriorDecoded.symm.trans priorDecoded)
          exact sameActorPath (oldSameActor.symm.trans
            ((congrArg Action.actor samePrevious).trans sameActor))

/-- In an actual occurrence word, an unfinished resource operation rules out
acceptance of every later transfer of that resource. Completion means the
ordinary operation's commit, but only acceptance for sending the resource
itself. Independent resources and old sender publications remain unconstrained. -/
theorem an_unfinished_resource_operation_excludes_a_later_accepted_handoff
    {plan : SourceOccurrenceCut source} {resource : EndpointKey}
    {target : ActionId} {binding nextBinding : Binding} {action sender : Action}
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) =
      some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (unfinished : (if target.side = .send ∧ binding.message.payload = .endpoint resource
      then SourcePoint.accepted target.event else .committed target) ∉ points)
    (nextAt : plan.path.bindings.find? (fun entry => entry.1 == next) = some (next, nextBinding))
    (nextPayload : nextBinding.message.payload = .endpoint resource)
    (nextDecoded : action? plan.path.bindings ⟨next, .send⟩ = some sender)
    (later : target.event < next) : SourcePoint.accepted next ∉ points := by
  obtain ⟨run, elaborated, solved, mapped⟩ :=
    accepted_normalized_path_has_its_exact_issuance_derivation plan.accepted plan.selected
  have causal := required_resource_operations_causally_precede_a_later_handoff
    elaborated solved mapped next nextBinding sender nextAt nextPayload nextDecoded
    target.event binding target.side action located required decoded later
  exact fun seen => unfinished
    (a_source_occurrence_trace_retains_its_causal_predecessors trace causal seen)

end Hibana
