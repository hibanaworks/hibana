import Hibana.ResourceOccurrenceCausality

namespace Hibana
open EndpointDelegation OwnedCallHistory

private theorem a_receipt_in_the_word_has_its_occurrence
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    ∀ key actor record, DistributedCallBoundary.receive key actor record ∈ word →
      ∃ occurrence, occurrenceCall? calls key = some occurrence ∧
        SourcePoint.committed ⟨occurrence, .receive⟩ ∈ points ∧
        plan.matchesAction (images actor) choreo actor record.step.pathIndex
          record.step.event occurrence .receive = true := by
  induction trace with
  | nil => simp
  | accept prior fresh origin matched ready ih =>
    intro key actor record member
    obtain ⟨occurrence, found, recorded, matched⟩ := ih key actor record (by simpa using member)
    exact ⟨occurrence, a_resolved_occurrence_call_survives_a_prefix
      (List.prefix_append _ _) found, List.mem_append_left _ recorded, matched⟩
  | @receive word calls points key actor occurrence record prior found matched ready ih =>
    intro actualKey actualActor actualRecord member
    rcases List.mem_append.mp member with earlier | current
    · obtain ⟨previous, resolved, recorded, actual⟩ := ih _ _ _ earlier
      exact ⟨previous, resolved, List.mem_append_left _ recorded, actual⟩
    · cases List.mem_singleton.mp current
      exact ⟨occurrence, found, List.mem_append_right _ List.mem_cons_self, matched⟩
  | quiet prior ih | retireView prior ih | close prior ih | abort prior ih
  | observeClosure prior ih | transportFault prior ih =>
    intro key actor record member
    exact ih key actor record (by simpa using member)
  | publish prior found matched ready ih | localCommit prior matched ready ih =>
    intro key actor record member
    obtain ⟨occurrence, resolved, recorded, actual⟩ := ih key actor record (by simpa using member)
    exact ⟨occurrence, resolved, List.mem_append_left _ recorded, actual⟩

private theorem extend_prefixes
    (prior : ∀ n, n ≤ word.length → ∃ earlierCalls earlierPoints,
      SourceOccurrenceTrace images choreo plan (word.take n) earlierCalls earlierPoints ∧
      earlierCalls <+: calls ∧ earlierPoints <+: points)
    (next : SourceOccurrenceTrace images choreo plan (word ++ [boundary]) nextCalls nextPoints)
    (callsKept : calls <+: nextCalls) (pointsKept : points <+: nextPoints) :
    ∀ n, n ≤ (word ++ [boundary]).length → ∃ earlierCalls earlierPoints,
      SourceOccurrenceTrace images choreo plan ((word ++ [boundary]).take n) earlierCalls earlierPoints ∧
      earlierCalls <+: nextCalls ∧ earlierPoints <+: nextPoints := by
  intro n bound
  by_cases inside : n ≤ word.length
  · obtain ⟨earlierCalls, earlierPoints, trace, callsPrefix, pointsPrefix⟩ := prior n inside
    refine ⟨earlierCalls, earlierPoints, ?_, callsPrefix.trans callsKept, pointsPrefix.trans pointsKept⟩
    simpa only [List.take_append_of_le_length inside] using trace
  · have all : n = (word ++ [boundary]).length := by simp only [List.length_append, List.length_singleton] at bound ⊢; omega
    exact ⟨nextCalls, nextPoints, by simpa only [all, List.take_length] using next,
      List.prefix_refl _, List.prefix_refl _⟩

private theorem every_source_prefix_has_its_actual_trace
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    ∀ n, n ≤ word.length → ∃ earlierCalls earlierPoints,
      SourceOccurrenceTrace images choreo plan (word.take n) earlierCalls earlierPoints ∧
      earlierCalls <+: calls ∧ earlierPoints <+: points := by
  induction trace with
  | nil sameSource =>
    intro n bound
    have empty : n = 0 := by simpa using bound
    subst n
    exact ⟨[], [], .nil sameSource, List.prefix_refl _, List.prefix_refl _⟩
  | quiet prior ih => exact extend_prefixes ih (.quiet prior) (List.prefix_refl _) (List.prefix_refl _)
  | retireView prior ih => exact extend_prefixes ih (.retireView prior) (List.prefix_refl _) (List.prefix_refl _)
  | close prior ih => exact extend_prefixes ih (.close prior) (List.prefix_refl _) (List.prefix_refl _)
  | observeClosure prior ih => exact extend_prefixes ih (.observeClosure prior) (List.prefix_refl _) (List.prefix_refl _)
  | transportFault prior ih => exact extend_prefixes ih (.transportFault prior) (List.prefix_refl _) (List.prefix_refl _)
  | abort prior ih => exact extend_prefixes ih (.abort prior) (List.prefix_refl _) (List.prefix_refl _)
  | accept prior fresh origin matched ready ih =>
    exact extend_prefixes ih (.accept prior fresh origin matched ready) (List.prefix_append _ _) (List.prefix_append _ _)
  | publish prior found matched ready ih =>
    exact extend_prefixes ih (.publish prior found matched ready) (List.prefix_refl _) (List.prefix_append _ _)
  | receive prior found matched ready ih =>
    exact extend_prefixes ih (.receive prior found matched ready) (List.prefix_refl _) (List.prefix_append _ _)
  | localCommit prior matched ready ih =>
    exact extend_prefixes ih (.localCommit prior matched ready) (List.prefix_refl _) (List.prefix_append _ _)

private theorem the_last_acceptance_has_its_projected_prefix
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    ∀ earlier entry, word = earlier ++ [DistributedCallBoundary.accept entry] →
      ∃ priorCalls priorPoints occurrence,
        SourceOccurrenceTrace images choreo plan earlier priorCalls priorPoints ∧
        priorCalls <+: calls ∧
        occurrenceCall? calls entry.callKey = some occurrence ∧
        plan.matchesAction entry.sample.image choreo entry.sample.image.role
          entry.sample.pathIndex entry.sample.event occurrence .send = true ∧
        (occurrenceCut plan priorPoints).ready (.accepted occurrence) = true := by
  induction trace with
  | nil =>
    intro earlier entry equal
    exfalso
    have lengths := congrArg List.length equal
    simp only [List.length_nil, List.length_append, List.length_singleton] at lengths
    omega
  | @accept word calls points actual occurrence prior fresh origin matched ready ih =>
    intro earlier entry equal
    obtain ⟨sameWord, sameLast⟩ := List.append_inj' equal rfl
    have same : actual = entry := by simpa using sameLast
    subst entry
    subst earlier
    have absent : calls.find? (fun pair => decide (pair.1 = actual.callKey)) = none := by
      apply List.find?_eq_none.mpr
      intro pair present
      have different := of_decide_eq_true (List.all_eq_true.mp fresh pair present)
      exact fun equal => different (of_decide_eq_true equal)
    refine ⟨calls, points, occurrence, prior, List.prefix_append _ _, ?_, matched, ?_⟩
    · simp [occurrenceCall?, List.find?_append, absent]
    · exact (projected_occurrence_readiness_is_exact _ _ _).mpr ⟨_, ready, rfl⟩
  | quiet prior ih | retireView prior ih | close prior ih | abort prior ih
  | observeClosure prior ih | transportFault prior ih
  | publish prior found matched ready ih | receive prior found matched ready ih
  | localCommit prior matched ready ih =>
    intro earlier entry equal
    exfalso
    have sameLast := (List.append_inj' equal rfl).2
    simp at sameLast


private theorem a_match_has_its_exact_binding_and_action
    {plan : SourceOccurrenceCut source}
    (matched : plan.matchesAction image choreo actor pathIndex row occurrence side = true) :
    ∃ binding action eff,
      plan.path.bindings.find? (fun entry => entry.1 == occurrence) = some (occurrence, binding) ∧
      action? plan.path.bindings ⟨occurrence, side⟩ = some action ∧
      sourceOperationIdentity? image choreo pathIndex row = some (eff, binding, side) := by
  unfold SourceOccurrenceCut.matchesAction at matched
  cases originAt : (plan.unfolding.occurrences 0)[occurrence]? with
  | none => simp only [originAt, Bool.false_eq_true] at matched
  | some origin =>
    cases bindingAt : plan.path.bindings.find? (fun entry => entry.1 == occurrence) with
    | none => simp only [originAt, bindingAt, Bool.false_eq_true] at matched
    | some pair =>
      have eventAt : pair.1 = occurrence := beq_iff_eq.mp
        (List.find?_some (p := fun entry : Nat × Binding => entry.1 == occurrence) bindingAt)
      obtain ⟨event, binding⟩ := pair
      dsimp only at eventAt
      subst event
      cases identityAt : sourceOperationIdentity? image choreo pathIndex row with
      | none => simp only [originAt, bindingAt, identityAt, Bool.false_eq_true] at matched
      | some actual =>
        obtain ⟨eff, actualBinding, actualSide⟩ := actual
        cases actionAt : action? plan.path.bindings ⟨occurrence, side⟩ with
        | none => simp only [originAt, bindingAt, identityAt, actionAt, Bool.false_eq_true] at matched
        | some action =>
          simp only [originAt, bindingAt, identityAt, actionAt, decide_eq_true_eq] at matched
          exact ⟨binding, action, eff, rfl, rfl,
            by simp only [matched.2.2.1, matched.2.2.2.1]⟩

private theorem an_old_handoff_is_blocked_after_a_resource_receipt
    {plan : SourceOccurrenceCut source} {current earlier : Nat}
    {binding previous : Binding} {resource : EndpointKey}
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (located : plan.path.bindings.find? (fun entry => entry.1 == current) = some (current, binding))
    (payload : binding.message.payload = .endpoint resource)
    (previousAt : plan.path.bindings.find? (fun entry => entry.1 == earlier) = some (earlier, previous))
    (previousPayload : previous.message.payload = .endpoint resource)
    (atMost : earlier ≤ current)
    (received : SourcePoint.committed ⟨current, .receive⟩ ∈ points) :
    (occurrenceCut plan points).ready (.accepted earlier) = false := by
  have accepted := a_source_occurrence_trace_retains_its_causal_predecessors trace
    (.single (.deliver current)) received
  by_cases same : earlier = current
  · subst earlier
    have marked : (occurrenceCut plan points).observed (.accepted current) = true :=
      List.contains_iff_mem.mpr accepted
    simp only [SourceOccurrenceCut.ready, marked, Bool.not_true, Bool.and_false, Bool.false_and]
  · obtain ⟨key, actor, record, _, _, callAt⟩ :=
      an_observed_occurrence_receipt_has_its_same_word_boundary trace received
    obtain ⟨_, _, entry, _, _, _, matched⟩ :=
      a_resolved_occurrence_call_has_its_same_word_acceptance trace callAt
    obtain ⟨sentBinding, sender, _, sentAt, sent, _⟩ :=
      a_match_has_its_exact_binding_and_action matched
    have sameBinding : sentBinding = binding :=
      congrArg Prod.snd (Option.some.inj (sentAt.symm.trans located))
    subst sentBinding
    exact (a_recorded_handoff_keeps_earlier_receipts_and_excludes_old_acceptances
      trace located payload sent previousAt previousPayload (by omega) accepted).2


/-- A physical acceptance after this receipt resolves to a strictly later
handoff of the same resource. Chronological prefix and call correspondence
are retained through independent visits; no publication order is assumed. -/
theorem a_handoff_after_a_resource_receipt_has_a_later_occurrence
    {plan : SourceOccurrenceCut source} {receiptBinding : Binding} {facts : OwnedSourceFacts}
    (trace : SourceOccurrenceTrace images choreo plan
      (word ++ [DistributedCallBoundary.accept entry]) calls points)
    (callsKept : calls <+: finalCalls)
    (received : DistributedCallBoundary.receive key actor record ∈ word)
    (resolved : occurrenceCall? finalCalls key = some receipt)
    (located : plan.path.bindings.find? (fun row => row.1 == receipt) = some (receipt, receiptBinding))
    (payload : receiptBinding.message.payload = .endpoint resource)
    (owned : entry.sample.ownedFacts? choreo = some facts)
    (resourceAt : facts.resource = resource) (sending : facts.side = .send) :
    ∃ occurrence binding, receipt < occurrence ∧
      occurrenceCall? finalCalls entry.callKey = some occurrence ∧
      plan.path.bindings.find? (fun row => row.1 == occurrence) = some (occurrence, binding) ∧
      binding.message.payload = .endpoint resource := by
  obtain ⟨priorCalls, priorPoints, occurrence, prior, priorKept, accepted, matched, ready⟩ :=
    the_last_acceptance_has_its_projected_prefix trace _ _ rfl
  obtain ⟨actualReceipt, receivedCall, receivedPoint, matchedReceipt⟩ :=
    a_receipt_in_the_word_has_its_occurrence prior _ _ _ received
  have same : actualReceipt = receipt := Option.some.inj
    ((a_resolved_occurrence_call_survives_a_prefix (priorKept.trans callsKept) receivedCall).symm.trans resolved)
  subst actualReceipt
  obtain ⟨binding, receiver, eff, bindingAt, decoded, identity⟩ :=
    a_match_has_its_exact_binding_and_action matchedReceipt
  have sameBinding : binding = receiptBinding :=
    congrArg Prod.snd (Option.some.inj (bindingAt.symm.trans located))
  subst binding
  obtain ⟨releaseBinding, sender, releaseEff, releaseAt, sent, releaseIdentity⟩ :=
    a_match_has_its_exact_binding_and_action matched
  have actualFacts := owned_source_facts_retain_identity_and_payload owned
  rw [sending] at actualFacts
  have sameRelease : releaseBinding = facts.binding :=
    congrArg (fun value : Nat × Binding × Side => value.2.1)
      (Option.some.inj (releaseIdentity.symm.trans actualFacts.1))
  have releasePayload : releaseBinding.message.payload = .endpoint resource := by
    simpa only [sameRelease, resourceAt] using actualFacts.2
  have notOld : ¬ occurrence ≤ receipt := by
    intro atMost
    have blocked := an_old_handoff_is_blocked_after_a_resource_receipt prior
      located payload releaseAt releasePayload atMost receivedPoint
    rw [blocked] at ready
    contradiction
  exact ⟨occurrence, releaseBinding, by omega,
    a_resolved_occurrence_call_survives_a_prefix callsKept accepted, releaseAt, releasePayload⟩


/-- The concrete tag issued by this receipt is retained now, passed onward by
an actual strictly later source occurrence, or retired at its checked complete
or faulted boundary. All three alternatives use this one physical/custody word.
An old equal envelope, sender-witness drop or I/O error cannot stand in for
that release. There is no live Rust-value or successful final-admission premise.
This is a source/custody interval result, not full implementation refinement. -/
theorem an_owned_occurrence_receipt_has_its_value_later_handoff_or_retirement
    {plan : SourceOccurrenceCut source} {binding : Binding}
    (custody : OwnedCallHistory.History images resource choreo initialValue events cut)
    (physical : DistributedPhysicalExecution images choreo session first last
      (sourceWord events) entries published pending)
    (trace : SourceOccurrenceTrace images choreo plan (sourceWord events) calls points)
    (located : plan.path.bindings.find? (fun entry => entry.1 == occurrence) =
      some (occurrence, binding))
    (payload : binding.message.payload = .endpoint resource)
    (received : DistributedCallBoundary.receive key actor record ∈ sourceWord events)
    (callAt : occurrenceCall? calls key = some occurrence) :
    ∃ frame facts,
      OwnedCallEvent.receive key actor record frame (actor, facts.afterView.header) ∈ events ∧
      record.step.ownedFacts? (images actor) choreo = some facts ∧ facts.resource = resource ∧
      facts.side = .receive ∧ record.step.after.actor = actor ∧ record.wire.identifies frame ∧
      (TransferValueLocated (actor, facts.afterView.header) cut.custody ∨
        ∃ earlier later event before after,
          events = earlier ++ event :: later ∧
          OwnedCallEvent.receive key actor record frame (actor, facts.afterView.header) ∈ earlier ∧
          OwnedCallHistory.History images resource choreo initialValue earlier before ∧
          OwnedCallHistory.Boundary images resource choreo earlier event before after ∧
          TransferValueLocated (actor, facts.afterView.header) before.custody ∧
          event.releasedValue? = some (actor, facts.afterView.header) ∧
          ((∃ entry request outgoing next nextBinding,
              event = .accept entry request outgoing ∧ occurrence < next ∧
              occurrenceCall? calls entry.callKey = some next ∧
              plan.path.bindings.find? (fun row => row.1 == next) = some (next, nextBinding) ∧
              nextBinding.message.payload = .endpoint resource) ∨
            ∃ sample, event.retirement? = some ((actor, facts.afterView.header), sample) ∧
              retirementMatches resource (actor, facts.afterView.header) sample
                (images sample.before.actor) choreo = true ∧
              (sample.after.fault ≠ none ∨
                sourceViewComplete? sample.before (images sample.before.actor) choreo sample.view = some true))) := by
  obtain ⟨frame, facts, present, factsAt, resourceAt, receiving, actorAt, wire, outcome⟩ :=
    an_owned_occurrence_receipt_has_its_actual_value_or_subsequent_release
      custody physical trace located payload received callAt
  refine ⟨frame, facts, present, factsAt, resourceAt, receiving, actorAt, wire, ?_⟩
  rcases outcome with retained | ⟨earlier, later, event, before, after,
    wordAt, receiptAt, history, boundary, priorLocation, released⟩
  · exact .inl retained
  · refine .inr ⟨earlier, later, event, before, after,
      wordAt, receiptAt, history, boundary, priorLocation, released, ?_⟩
    cases boundary <;> simp only [OwnedCallEvent.releasedValue?, reduceCtorEq] at released
    case accept state left right requests entry request outgoing sendFacts sendFactsAt
        sentResource sending requested matched wireAt channel position =>
      have sourceReceipt : DistributedCallBoundary.receive key actor record ∈ sourceWord earlier :=
        List.mem_filterMap.mpr ⟨_, receiptAt, rfl⟩
      have wordShape : sourceWord events =
          (sourceWord earlier ++ [DistributedCallBoundary.accept entry]) ++ sourceWord later := by
        simp [wordAt, sourceWord, OwnedCallEvent.source?, List.append_assoc]
      have bound : (sourceWord earlier ++ [DistributedCallBoundary.accept entry]).length ≤
          (sourceWord events).length := by
        simp only [wordShape, List.length_append, List.length_singleton]
        omega
      obtain ⟨prefixCalls, prefixPoints, prefixTrace, callsPrefix, pointsPrefix⟩ :=
        every_source_prefix_has_its_actual_trace trace _ bound
      have takeWord : (sourceWord events).take
          (sourceWord earlier ++ [DistributedCallBoundary.accept entry]).length =
          sourceWord earlier ++ [DistributedCallBoundary.accept entry] := by
        rw [wordShape, List.take_left]
      rw [takeWord] at prefixTrace
      obtain ⟨next, nextBinding, forward, accepted, nextAt, nextPayload⟩ :=
        a_handoff_after_a_resource_receipt_has_a_later_occurrence prefixTrace callsPrefix
          sourceReceipt callAt located payload sendFactsAt sentResource sending
      exact .inl ⟨entry, request, outgoing, next, nextBinding, rfl, forward, accepted, nextAt, nextPayload⟩
    case abandon state left right priorRequests laterRequests request sample checked =>
      have same := Option.some.inj released
      refine .inr ⟨sample, by simp only [OwnedCallEvent.retirement?, same], ?_, ?_⟩
      · simpa only [same] using checked
      · rcases a_joint_value_retirement_is_complete_or_faulted
          (Boundary.abandon earlier state left right priorRequests laterRequests request sample checked) rfl with
          live | faulted
        · exact .inr live.2.1
        · obtain ⟨cause, faultAt, _⟩ := faulted
          exact .inl (by rw [faultAt]; simp)
    case dropValue state left right requests value sample checked =>
      have same := Option.some.inj released
      refine .inr ⟨sample, by simp only [OwnedCallEvent.retirement?, same], ?_, ?_⟩
      · simpa only [same] using checked
      · rcases a_joint_value_retirement_is_complete_or_faulted
          (Boundary.dropValue earlier state left right requests value sample checked) rfl with
          live | faulted
        · exact .inr live.2.1
        · obtain ⟨cause, faultAt, _⟩ := faulted
          exact .inl (by rw [faultAt]; simp)


private theorem an_unfinished_resource_action_excludes_the_same_handoff
    {plan : SourceOccurrenceCut source} {target : ActionId} {binding : Binding}
    {action : Action} {resource : EndpointKey} {points : List SourcePoint}
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) =
      some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (unfinished : (if target.side = .send ∧ binding.message.payload = .endpoint resource
      then SourcePoint.accepted target.event else .committed target) ∉ points)
    (payload : binding.message.payload = .endpoint resource) :
    SourcePoint.accepted target.event ∉ points := by
  obtain ⟨run, elaborated, solved, mapped⟩ :=
    accepted_normalized_path_has_its_exact_issuance_derivation plan.accepted plan.selected
  obtain ⟨symbolic, member, instantiated⟩ := instantiated_path_binding_has_source_input mapped located
  have messageAt := (instantiated_binding_retains_message_and_actor_references instantiated).1
  have valid : validMessage binding.message = true := by
    rw [messageAt]
    exact elaboration_only_emits_valid_messages elaborated member
  have fields : resource ≠ binding.message.receiver ∧
      binding.message.sender ≠ binding.message.receiver := by
    simp only [validMessage, payload, Bool.and_eq_true, bne_iff_ne] at valid
    exact ⟨valid.2.1.2, valid.2.2⟩
  cases sideAt : target.side with
  | send => simpa only [sideAt, payload, and_self, ite_true] using unfinished
  | receive =>
    have uses : binding.message.receiver = resource := by simpa only [sideAt] using required
    exact False.elim (fields.1 uses.symm)
  | «local» => simp [action?, located, fields.2, sideAt] at decoded

private theorem a_pending_resource_use_excludes_a_handoff_after_its_last_receipt
    {plan : SourceOccurrenceCut source} {target : ActionId} {binding : Binding}
    {action : Action} {resource : EndpointKey} {run : Elaboration} {symbolic previous : SymbolicBinding}
    {nextBinding : Binding} {index receipt next : Nat} {earlier later : List (Nat × SymbolicBinding)}
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (elaborated : elaborate plan.expanded.resources (initialOwners plan.expanded.resources)
      plan.path.selections plan.expanded 0 = some run)
    (mapped : (run.bindings.mapM fun (event, item) => do
      let value ← instantiateBinding plan.normalized.issuance item
      some (event, value)) = some plan.path.bindings)
    (sourceAt : run.bindings[index]? = some (target.event, symbolic))
    (instantiated : instantiateBinding plan.normalized.issuance symbolic = some binding)
    (shape : run.bindings.take index = earlier ++ (receipt, previous) :: later)
    (transferred : previous.message.payload = .endpoint resource)
    (untouched : ∀ entry ∈ later, entry.2.message.payload ≠ .endpoint resource)
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) =
      some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (unfinished : (if target.side = .send ∧ binding.message.payload = .endpoint resource
      then SourcePoint.accepted target.event else .committed target) ∉ points)
    (nextAt : plan.path.bindings.find? (fun row => row.1 == next) = some (next, nextBinding))
    (nextPayload : nextBinding.message.payload = .endpoint resource)
    (forward : receipt < next) : SourcePoint.accepted next ∉ points := by
  obtain ⟨nextSymbolic, member, nextInstantiated⟩ := instantiated_path_binding_has_source_input mapped nextAt
  have messageAt := (instantiated_binding_retains_message_and_actor_references instantiated).1
  have nextMessageAt := (instantiated_binding_retains_message_and_actor_references nextInstantiated).1
  have nextTransferred : nextSymbolic.message.payload = .endpoint resource := by
    simpa only [nextMessageAt] using nextPayload
  have sourceRequired : symbolic.message.sender = resource ∨ symbolic.message.receiver = resource ∨
      symbolic.message.payload = .endpoint resource := by
    rw [← messageAt]
    cases sideAt : target.side with
    | send =>
      have uses : binding.message.sender = resource ∨ binding.message.payload = .endpoint resource := by
        simpa only [sideAt] using required
      exact uses.elim Or.inl (fun h => .inr (.inr h))
    | receive => exact .inr (.inl (by simpa only [sideAt] using required))
    | «local» => exact .inl (by simpa only [sideAt] using required)
  rcases a_resource_handoff_is_outside_its_last_receipt_to_target_interval
      elaborated sourceAt shape transferred untouched member nextTransferred sourceRequired with
    before | sameReceipt | sameTarget | after
  · have := source_order_strictly_increases_event before
    omega
  · omega
  · subst next
    have sameBinding : nextBinding = binding :=
      congrArg Prod.snd (Option.some.inj (nextAt.symm.trans located))
    exact an_unfinished_resource_action_excludes_the_same_handoff located decoded required
      unfinished (by simpa only [sameBinding] using nextPayload)
  · have valid := elaboration_only_emits_valid_messages elaborated member
    have distinct : nextBinding.message.sender ≠ nextBinding.message.receiver := by
      rw [nextMessageAt]
      simp only [validMessage, nextTransferred, Bool.and_eq_true, bne_iff_ne] at valid
      exact valid.2.2
    have nextDecoded : action? plan.path.bindings ⟨next, .send⟩ = some
        { actor := nextBinding.senderActor, endpoint := nextBinding.message.sender, side := .send } := by
      simp [action?, nextAt, distinct]
    exact an_unfinished_resource_operation_excludes_a_later_accepted_handoff trace located decoded
      required unfinished nextAt nextPayload nextDecoded (source_order_strictly_increases_event after)

/-- At an unfinished resource operation, the last actual receipt's authority
is still in application/private-future custody, or that exact value has a
checked complete/faulted retirement. No later handoff can take it away first.
This composes the same source, physical and custody word, not a postulated
live value or a sender-publication barrier. -/
private theorem a_pending_resource_use_keeps_its_received_value_or_checked_retirement
    {plan : SourceOccurrenceCut source} {target : ActionId} {binding receiptBinding : Binding}
    {action : Action} {run : Elaboration} {symbolic previous : SymbolicBinding}
    {index receipt : Nat} {earlier later : List (Nat × SymbolicBinding)}
    (custody : OwnedCallHistory.History images resource choreo initialValue events cut)
    (physical : DistributedPhysicalExecution images choreo session first last
      (sourceWord events) entries published pending)
    (trace : SourceOccurrenceTrace images choreo plan (sourceWord events) calls points)
    (elaborated : elaborate plan.expanded.resources (initialOwners plan.expanded.resources)
      plan.path.selections plan.expanded 0 = some run)
    (mapped : (run.bindings.mapM fun (event, item) => do
      let value ← instantiateBinding plan.normalized.issuance item
      some (event, value)) = some plan.path.bindings)
    (sourceAt : run.bindings[index]? = some (target.event, symbolic))
    (instantiated : instantiateBinding plan.normalized.issuance symbolic = some binding)
    (shape : run.bindings.take index = earlier ++ (receipt, previous) :: later)
    (transferred : previous.message.payload = .endpoint resource)
    (untouched : ∀ entry ∈ later, entry.2.message.payload ≠ .endpoint resource)
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) =
      some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (unfinished : (if target.side = .send ∧ binding.message.payload = .endpoint resource
      then SourcePoint.accepted target.event else .committed target) ∉ points)
    (receiptAt : plan.path.bindings.find? (fun row => row.1 == receipt) = some (receipt, receiptBinding))
    (receiptPayload : receiptBinding.message.payload = .endpoint resource)
    (received : DistributedCallBoundary.receive key action.actor record ∈ sourceWord events)
    (resolved : occurrenceCall? calls key = some receipt) :
    ∃ frame facts,
      OwnedCallEvent.receive key action.actor record frame (action.actor, facts.afterView.header) ∈ events ∧
      record.step.ownedFacts? (images action.actor) choreo = some facts ∧ facts.resource = resource ∧
      (TransferValueLocated (action.actor, facts.afterView.header) cut.custody ∨
        ∃ event ∈ events, ∃ sample,
          event.retirement? = some ((action.actor, facts.afterView.header), sample) ∧
          retirementMatches resource (action.actor, facts.afterView.header) sample
            (images sample.before.actor) choreo = true ∧
          (sample.after.fault ≠ none ∨
            sourceViewComplete? sample.before (images sample.before.actor) choreo sample.view = some true)) := by
  obtain ⟨frame, facts, present, factsAt, resourceAt, receiving, actorAt, wire, outcome⟩ :=
    an_owned_occurrence_receipt_has_its_value_later_handoff_or_retirement custody physical trace
      receiptAt receiptPayload received resolved
  refine ⟨frame, facts, present, factsAt, resourceAt, ?_⟩
  rcases outcome with retained | ⟨prior, suffix, event, before, after,
    wordAt, receiptInPrefix, priorHistory, boundary, priorLocation, released, actual⟩
  · exact .inl retained
  · rcases actual with handoff | retirement
    · obtain ⟨entry, request, outgoing, next, nextBinding,
        eventAt, forward, callAt, nextAt, nextPayload⟩ := handoff
      have excluded := a_pending_resource_use_excludes_a_handoff_after_its_last_receipt
        trace elaborated mapped sourceAt instantiated shape transferred untouched
        located decoded required unfinished nextAt nextPayload forward
      exact False.elim (excluded (List.contains_iff_mem.mp
        (a_resolved_call_has_actual_acceptance_evidence trace callAt)))
    · refine .inr ⟨event, ?_, retirement⟩
      rw [wordAt]
      exact List.mem_append.mpr (.inr List.mem_cons_self)


private theorem a_pending_resource_use_keeps_its_initial_value_or_checked_retirement
    {plan : SourceOccurrenceCut source} {target : ActionId} {binding : Binding}
    {action : Action} {run : Elaboration} {symbolic : SymbolicBinding} {index : Nat}
    (custody : OwnedCallHistory.History images resource choreo initialValue events cut)
    (trace : SourceOccurrenceTrace images choreo plan (sourceWord events) calls points)
    (elaborated : elaborate plan.expanded.resources (initialOwners plan.expanded.resources)
      plan.path.selections plan.expanded 0 = some run)
    (mapped : (run.bindings.mapM fun (event, item) => do
      let value ← instantiateBinding plan.normalized.issuance item
      some (event, value)) = some plan.path.bindings)
    (sourceAt : run.bindings[index]? = some (target.event, symbolic))
    (instantiated : instantiateBinding plan.normalized.issuance symbolic = some binding)
    (untouched : ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint resource)
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) =
      some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (unfinished : (if target.side = .send ∧ binding.message.payload = .endpoint resource
      then SourcePoint.accepted target.event else .committed target) ∉ points) :
    TransferValueLocated initialValue cut.custody ∨
      ∃ event ∈ events, ∃ sample,
        event.retirement? = some (initialValue, sample) ∧
        retirementMatches resource initialValue sample (images sample.before.actor) choreo = true ∧
        (sample.after.fault ≠ none ∨
          sourceViewComplete? sample.before (images sample.before.actor) choreo sample.view = some true) := by
  rcases a_joint_initial_value_has_its_current_location_or_actual_release custody with
    retained | ⟨prior, suffix, event, before, after, wordAt, priorHistory, boundary, priorLocation, released⟩
  · exact .inl retained
  · have present : event ∈ events := by
      rw [wordAt]
      exact List.mem_append.mpr (.inr List.mem_cons_self)
    cases boundary <;> simp only [OwnedCallEvent.releasedValue?, reduceCtorEq] at released
    case accept state left right requests entry request outgoing sendFacts sendFactsAt
        sentResource sending requested matched wireAt channel position =>
      have acceptedInWord : DistributedCallBoundary.accept entry ∈ sourceWord events :=
        List.mem_filterMap.mpr ⟨_, present, rfl⟩
      obtain ⟨next, callAt, sourceMatch⟩ := an_accepted_occurrence_keeps_its_call_and_source trace _ acceptedInWord
      obtain ⟨nextBinding, sender, eff, nextAt, nextDecoded, identity⟩ :=
        a_match_has_its_exact_binding_and_action sourceMatch
      have actualFacts := owned_source_facts_retain_identity_and_payload sendFactsAt
      rw [sending] at actualFacts
      have sameBinding : nextBinding = sendFacts.binding :=
        congrArg (fun value : Nat × Binding × Side => value.2.1)
          (Option.some.inj (identity.symm.trans actualFacts.1))
      have nextPayload : nextBinding.message.payload = .endpoint resource := by
        rw [sameBinding, actualFacts.2, sentResource]
      obtain ⟨nextSymbolic, nextMember, nextInstantiated⟩ :=
        instantiated_path_binding_has_source_input mapped nextAt
      have nextTransferred : nextSymbolic.message.payload = .endpoint resource := by
        simpa only [(instantiated_binding_retains_message_and_actor_references nextInstantiated).1] using nextPayload
      have used : symbolic.message.sender = resource ∨ symbolic.message.receiver = resource ∨
          symbolic.message.payload = .endpoint resource := by
        rw [← (instantiated_binding_retains_message_and_actor_references instantiated).1]
        cases sideAt : target.side with
        | send =>
          have uses : binding.message.sender = resource ∨ binding.message.payload = .endpoint resource := by
            simpa only [sideAt] using required
          exact uses.elim Or.inl (fun h => .inr (.inr h))
        | receive => exact .inr (.inl (by simpa only [sideAt] using required))
        | «local» => exact .inl (by simpa only [sideAt] using required)
      have accepted := List.contains_iff_mem.mp (a_resolved_call_has_actual_acceptance_evidence trace callAt)
      rcases an_initial_resource_has_no_handoff_before_its_target
          elaborated sourceAt untouched nextMember nextTransferred used with same | later
      · subst next
        have same : nextBinding = binding :=
          congrArg Prod.snd (Option.some.inj (nextAt.symm.trans located))
        exact False.elim (an_unfinished_resource_action_excludes_the_same_handoff located decoded required
          unfinished (by simpa only [same] using nextPayload) accepted)
      · exact False.elim (an_unfinished_resource_operation_excludes_a_later_accepted_handoff
          trace located decoded required unfinished nextAt nextPayload nextDecoded
          (source_order_strictly_increases_event later) accepted)
    case abandon state left right priorRequests laterRequests request sample checked =>
      have same := Option.some.inj released
      refine .inr ⟨_, present, sample, by simp only [OwnedCallEvent.retirement?, same], ?_, ?_⟩
      · simpa only [same] using checked
      · rcases a_joint_value_retirement_is_complete_or_faulted
          (Boundary.abandon prior state left right priorRequests laterRequests request sample checked) rfl with live | faulted
        · exact .inr live.2.1
        · obtain ⟨cause, faultAt, _⟩ := faulted
          exact .inl (by rw [faultAt]; simp)
    case dropValue state left right requests value sample checked =>
      have same := Option.some.inj released
      refine .inr ⟨_, present, sample, by simp only [OwnedCallEvent.retirement?, same], ?_, ?_⟩
      · simpa only [same] using checked
      · rcases a_joint_value_retirement_is_complete_or_faulted
          (Boundary.dropValue prior state left right requests value sample checked) rfl with live | faulted
        · exact .inr live.2.1
        · obtain ⟨cause, faultAt, _⟩ := faulted
          exact .inl (by rw [faultAt]; simp)

/-- Compose projected readiness, exact initial issuance/last receipt, and the
same physical/custody word. Every resource needed by this ready operation has
its current actor-local authority in application/private-future custody, or
its exact value was retired at a checked complete/faulted boundary. No later
transfer, equal rolled envelope, error alone or completed sender witness can
replace that authority. The initial tag is tied only to its unique issuer,
not postulated available at the final cut. Application retention and polling
remain separate premises; this is not arbitrary Rust callability. -/
theorem a_ready_resource_operation_has_its_authority_or_checked_retirement
    {plan : SourceOccurrenceCut source} {target : ActionId} {binding : Binding} {action : Action}
    (custody : OwnedCallHistory.History images resource choreo initialValue events cut)
    (physical : DistributedPhysicalExecution images choreo session first last
      (sourceWord events) entries published pending)
    (trace : SourceOccurrenceTrace images choreo plan (sourceWord events) calls points)
    (initialOwner : instantiate plan.original.issuance (initialOwners source.resources resource) = some initialValue.1)
    (located : plan.path.bindings.find? (fun entry => entry.1 == target.event) = some (target.event, binding))
    (decoded : action? plan.path.bindings target = some action)
    (required : match target.side with
      | .receive => binding.message.receiver = resource
      | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
      | .local => binding.message.sender = resource)
    (localReady : occurrenceLocalReady plan action.actor
      (occurrenceLocalHistory plan action.actor points) target.entryPoint = true)
    (wire : occurrenceWireReady points target.entryPoint = true) :
    ∃ value : Tag, value.1 = action.actor ∧
      (TransferValueLocated value cut.custody ∨
        ∃ event ∈ events, ∃ sample,
          event.retirement? = some (value, sample) ∧
          retirementMatches resource value sample (images sample.before.actor) choreo = true ∧
          (sample.after.fault ≠ none ∨
            sourceViewComplete? sample.before (images sample.before.actor) choreo sample.view = some true)) := by
  have globallyReady := (projected_occurrence_readiness_is_exact plan points target.entryPoint).mpr
    ⟨action.actor, localReady, wire⟩
  obtain ⟨next, step⟩ := a_checked_source_occurrence_has_its_exact_transition
    (occurrenceCut plan points) target.entryPoint globallyReady
  have notEntry : target.entryPoint ∉ points := by
    cases step with
    | advance present pending ready =>
      intro marked
      have seen := List.contains_iff_mem.mpr marked
      change points.contains target.entryPoint = false at pending
      rw [pending] at seen
      contradiction
  have unfinished : (if target.side = .send ∧ binding.message.payload = .endpoint resource
      then SourcePoint.accepted target.event else .committed target) ∉ points := by
    intro seen
    apply notEntry
    split at seen
    next owned => simpa only [ActionId.entryPoint, owned.1] using seen
    next other =>
      cases sideAt : target.side with
      | send =>
        have finished : SourcePoint.committed ⟨target.event, .send⟩ ∈ points := by
          simpa only [← sideAt] using seen
        have accepted := a_source_occurrence_trace_retains_its_causal_predecessors trace
          (.single (.publish target.event)) finished
        simpa only [ActionId.entryPoint, sideAt] using accepted
      | receive | «local» => simpa only [ActionId.entryPoint, sideAt] using seen
  obtain ⟨run, index, symbolic, elaborated, sourceAt, instantiated, solved, mapped, origin⟩ :=
    a_ready_occurrence_resource_has_its_initial_owner_or_same_word_receipt trace located decoded required localReady wire
  rcases origin with ⟨issuerAt, untouched⟩ | ⟨receipt, receiptBinding, receiver,
    earlier, previous, later, key, record, shape, transferred, untouched, previousInstantiated,
    receiptAt, receiptPayload, receivedAction, sameActor, edge, observed, inWord, matched, resolved⟩
  · exact ⟨initialValue, Option.some.inj (initialOwner.symm.trans issuerAt),
      a_pending_resource_use_keeps_its_initial_value_or_checked_retirement custody trace elaborated mapped
        sourceAt instantiated untouched located decoded required unfinished⟩
  · obtain ⟨frame, facts, present, factsAt, resourceAt, outcome⟩ :=
      a_pending_resource_use_keeps_its_received_value_or_checked_retirement custody physical trace
        elaborated mapped sourceAt instantiated shape transferred untouched located decoded required unfinished
        receiptAt receiptPayload inWord resolved
    exact ⟨(action.actor, facts.afterView.header), rfl, outcome⟩


/-- The model-level reduction on ONE chronological word: affine custody,
physical FIFO/call matching, reconstructed source execution, and an enabled
projected transition whose required retained resource is at its actual actor.
Retention excludes recorded retirement of the tracked active resource; it does
not assume a final live value, future, admission result or common roll visit.
Completed sender publication needs no resource authority and is not blocked
by its new holder. This is protocol-model progress, not a formalization of
arbitrary Rust applications or their executor. -/
theorem a_joint_occurrence_execution_has_retained_resource_authority_and_source_progress
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {resource : EndpointKey} {initialValue : Tag} {events : List OwnedCallEvent} {cut : Cut}
    {source : Source} {plan : SourceOccurrenceCut source}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    {calls : OccurrenceCalls} {points : List SourcePoint}
    (custody : OwnedCallHistory.History images resource choreo initialValue events cut)
    (physical : DistributedPhysicalExecution images choreo session (.initial initial) last
      (sourceWord events) entries published pending)
    (trace : SourceOccurrenceTrace images choreo plan (sourceWord events) calls points)
    (initialOwner : instantiate plan.original.issuance (initialOwners source.resources resource) = some initialValue.1)
    (retained : ∀ event ∈ events, event.retirement? = none)
    (unfinished : (occurrenceCut plan points).Unfinished) :
    cut.custody.authorityCount ≤ 1 ∧
      entries.Perm (published.map Prod.fst ++ pending) ∧
      SourceOccurrenceExecution (occurrenceCut plan []) (occurrenceCut plan points) points ∧
      (∀ channel, QualifiedDirectionHistory choreo session
        (last.carriers channel) (sourceChannelRecords entries channel)) ∧
      ∃ actor point next,
        occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points) point = true ∧
        occurrenceWireReady points point = true ∧
        (occurrenceCut plan (occurrenceLocalHistory plan actor points)).resumptionReady point = true ∧
        SourceOccurrenceStep (occurrenceCut plan points) point next ∧
        ∀ target binding action,
          target.entryPoint = point → action.actor = actor →
          plan.path.bindings.find? (fun row => row.1 == target.event) = some (target.event, binding) →
          action? plan.path.bindings target = some action →
          (match target.side with
            | .receive => binding.message.receiver = resource
            | .send => binding.message.sender = resource ∨ binding.message.payload = .endpoint resource
            | .local => binding.message.sender = resource) →
          ∃ value : Tag, value.1 = actor ∧ TransferValueLocated value cut.custody := by
  obtain ⟨affine, partitioned, execution, fifo, actor, point, next, localReady, wire, resumed, step⟩ :=
    a_joint_physical_occurrence_refinement_has_custody_fifo_and_source_progress
      custody physical trace unfinished
  refine ⟨affine, partitioned, execution, fifo, actor, point, next, localReady, wire, resumed, step, ?_⟩
  intro target binding action samePoint sameActor located decoded required
  obtain ⟨value, atActor, outcome⟩ := a_ready_resource_operation_has_its_authority_or_checked_retirement
    custody physical trace initialOwner located decoded required
    (by simpa only [samePoint, sameActor] using localReady)
    (by simpa only [samePoint] using wire)
  rcases outcome with current | ⟨event, present, sample, retired, _⟩
  · exact ⟨value, atActor.trans sameActor, current⟩
  · rw [retained event present] at retired
    contradiction

end Hibana
