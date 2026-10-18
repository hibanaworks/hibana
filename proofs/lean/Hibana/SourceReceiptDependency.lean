import Hibana.SourceHandoffAvailability

namespace Hibana
open EndpointDelegation

private theorem ordered_member_is_the_key_lookup
    {α : Type} {rows : List (Nat × α)} {event : Nat} {value : α}
    (ordered : rows.Pairwise (fun left right => left.1 < right.1))
    (member : (event, value) ∈ rows) :
    rows.find? (fun entry => entry.1 == event) = some (event, value) := by
  induction rows with
  | nil => simp at member
  | cons head tail ih =>
      have fields := List.pairwise_cons.mp ordered
      rcases List.mem_cons.mp member with same | member
      · subst head
        have identity : (event == event) = true := decide_eq_true rfl
        simp only [List.find?_cons, identity]
      · have less := fields.1 _ member
        have different : head.1 ≠ event := by omega
        simpa [different] using ih fields.2 member

private theorem instantiated_binding_retains_key_lookup
    {inputs : List (Nat × SymbolicBinding)} {outputs : List (Nat × Binding)}
    {issuance : List Issuance} {event : Nat} {symbolic : SymbolicBinding} {binding : Binding}
    (mapped : (inputs.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some outputs)
    (located : inputs.find? (fun entry => entry.1 == event) = some (event, symbolic))
    (instantiated : instantiateBinding issuance symbolic = some binding) :
    outputs.find? (fun entry => entry.1 == event) = some (event, binding) := by
  change (inputs.mapM fun entry => (instantiateBinding issuance entry.2).bind
    fun binding => some (entry.1, binding)) = some outputs at mapped
  induction inputs generalizing outputs with
  | nil => simp at located
  | cons head tail ih =>
      cases headAt : instantiateBinding issuance head.2 with
      | none => simp [headAt] at mapped
      | some headBinding =>
          cases tailAt : (tail.mapM fun entry => (instantiateBinding issuance entry.2).bind
              fun binding => some (entry.1, binding)) with
          | none => simp [headAt, tailAt] at mapped
          | some rest =>
              have outputAt : outputs = (head.1, headBinding) :: rest := by
                simpa [headAt, tailAt] using mapped.symm
              rw [outputAt]
              by_cases same : head.1 = event
              · have guard : (head.1 == event) = true := decide_eq_true same
                have pair : head = (event, symbolic) :=
                  Option.some.inj (by simpa only [List.find?_cons, guard] using located)
                cases pair
                have bindingSame := Option.some.inj (headAt.symm.trans instantiated)
                have identity : (event == event) = true := decide_eq_true rfl
                simp only [List.find?_cons, identity, bindingSame]
              · have tailLocated : tail.find? (fun entry => entry.1 == event) =
                    some (event, symbolic) := by simpa [same] using located
                simpa [same] using ih tailLocated tailAt

private theorem elaboration_only_emits_valid_messages
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start event : Nat} {run : Elaboration} {binding : SymbolicBinding}
    (accepted : elaborate resources owners path source start = some run)
    (member : (event, binding) ∈ run.bindings) : validMessage binding.message = true := by
  induction source generalizing owners start run with
  | send message =>
      cases validAt : validMessage message with
      | false => simp [elaborate, validAt] at accepted
      | true =>
          cases payloadAt : message.payload with
          | wire schema | endpoint key =>
              simp only [elaborate, validAt, Bool.not_true, Bool.false_eq_true,
                ite_false, payloadAt] at accepted
              have same := Option.some.inj accepted
              subst run
              have pair := List.mem_singleton.mp member
              have messageSame := congrArg (fun entry => entry.2.message) pair
              change binding.message = message at messageSame
              rw [messageSame]
              exact validAt
  | seq left right leftIH rightIH =>
      change (elaborate resources owners path left start).bind _ = some run at accepted
      obtain ⟨first, firstAt, remaining⟩ := Option.bind_eq_some_iff.mp accepted
      obtain ⟨second, secondAt, written⟩ := Option.bind_eq_some_iff.mp remaining
      have same := Option.some.inj written
      subst run
      rcases List.mem_append.mp member with leftMember | rightMember
      · exact leftIH firstAt leftMember
      · exact rightIH secondAt rightMember
  | par left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next incompatible => simp at accepted
      next compatible =>
        obtain ⟨first, firstAt, remaining⟩ := Option.bind_eq_some_iff.mp accepted
        obtain ⟨second, secondAt, written⟩ := Option.bind_eq_some_iff.mp remaining
        have same := Option.some.inj written
        subst run
        rcases List.mem_append.mp member with leftMember | rightMember
        · exact leftIH firstAt leftMember
        · exact rightIH secondAt rightMember
  | route resolver left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next invalid => simp at accepted
      next valid =>
        obtain ⟨arm, armAt, selected⟩ := Option.bind_eq_some_iff.mp accepted
        cases arm with
        | left => exact leftIH selected member
        | right => exact rightIH selected member
  | roll body bodyIH =>
      change (elaborate resources owners path body start).bind _ = some run at accepted
      obtain ⟨inner, innerAt, written⟩ := Option.bind_eq_some_iff.mp accepted
      have same := Option.some.inj written
      subst run
      exact bodyIH innerAt member

/-- A resource action's receipt dependency is derived from the accepted
source, including the previous binding's lookup, receive operation and actual
actor. The last handoff is not supplied by the caller. Initial issuance is the
only alternative. This concerns one selected visit, not runtime liveness. -/
theorem accepted_resource_action_has_initial_authority_or_a_receipt_dependency
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {run : Elaboration} {issuance : List Issuance}
    {index event : Nat} {symbolic : SymbolicBinding} {binding : Binding}
    {bindings : List (Nat × Binding)} {key : EndpointKey} {side : Side} {action : Action}
    (accepted : elaborate resources owners path source 0 = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (instantiated : instantiateBinding issuance symbolic = some binding)
    (solved : run.equations.all (equationAccepted issuance) = true)
    (mapped : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some bindings)
    (required : match side with
      | .receive => symbolic.message.receiver = key
      | .send => symbolic.message.sender = key ∨ symbolic.message.payload = .endpoint key
      | .local => symbolic.message.sender = key)
    (decoded : action? bindings ⟨event, side⟩ = some action) :
    (∃ initialActor, instantiate issuance (owners key) = some initialActor ∧ action.actor = initialActor ∧
      ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint key) ∨
    ∃ previousEvent previousBinding beforeAction,
      bindings.find? (fun entry => entry.1 == previousEvent) = some (previousEvent, previousBinding) ∧
      previousBinding.message.payload = .endpoint key ∧
      action? bindings ⟨previousEvent, .receive⟩ = some beforeAction ∧
      beforeAction.actor = action.actor ∧
      previousEvent < source.eventCount ∧
      precedes source bindings ⟨previousEvent, .receive⟩ ⟨event, side⟩ = true ∧
      SourcePointEdge source bindings (.committed ⟨previousEvent, .receive⟩)
        (ActionId.entryPoint ⟨event, side⟩) ∧
      ∃ earlierRows previous laterRows,
        run.bindings.take index = earlierRows ++ (previousEvent, previous) :: laterRows ∧
        previous.message.payload = .endpoint key ∧
        (∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) ∧
        instantiateBinding issuance previous = some previousBinding := by
  have used : symbolic.message.sender = key ∨ symbolic.message.receiver = key ∨
      symbolic.message.payload = .endpoint key := by
    cases side with
    | receive => exact .inr (.inl required)
    | send => exact required.elim Or.inl (fun payload => .inr (.inr payload))
    | «local» => exact .inl required
  have ordered := accepted_binding_occurrence_enumeration_increases accepted
  have located := instantiated_binding_retains_key_lookup mapped
    (ordered_member_is_the_key_lookup ordered (List.mem_of_getElem? present)) instantiated
  have actualActor := source_action_lookup_uses_the_instantiated_actor located decoded
  rcases accepted_resource_use_derives_its_initial_or_last_receiver accepted present
      instantiated solved mapped used with
    ⟨untouched, sender, receiver, payload⟩ |
    ⟨earlierRows, previousEvent, previous, laterRows, previousBinding,
      shape, transferred, untouched, previousAt, sender, receiver, payload, causal⟩
  · cases side with
    | receive => exact .inl ⟨binding.receiverActor, receiver required, actualActor, untouched⟩
    | send =>
        rcases required with sending | sending
        · exact .inl ⟨binding.senderActor, sender sending, actualActor, untouched⟩
        · exact .inl ⟨binding.senderActor, payload sending, actualActor, untouched⟩
    | «local» => exact .inl ⟨binding.senderActor, sender required, actualActor, untouched⟩
  · have previousMember : (previousEvent, previous) ∈ run.bindings := by
      apply List.mem_of_mem_take
      rw [shape]
      exact List.mem_append.mpr (.inr (List.mem_cons_self))
    have previousLocated := instantiated_binding_retains_key_lookup mapped
      (ordered_member_is_the_key_lookup ordered previousMember) previousAt
    have valid := elaboration_only_emits_valid_messages accepted previousMember
    have different : previous.message.sender ≠ previous.message.receiver := by
      unfold validMessage at valid
      rw [transferred] at valid
      have external := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp valid).2).2
      intro same
      simp [same] at external
    have fields := instantiated_binding_retains_message_and_actor_references previousAt
    let beforeAction : Action := {
      actor := previousBinding.receiverActor
      endpoint := previousBinding.message.receiver
      side := .receive
    }
    have beforeDecoded : action? bindings ⟨previousEvent, .receive⟩ = some beforeAction := by
      simp [action?, previousLocated, fields.1, different, beforeAction]
    have sameActor : beforeAction.actor = action.actor := by
      change previousBinding.receiverActor = action.actor
      cases side with
      | receive => exact ((actualActor.trans (receiver required))).symm
      | send =>
          rcases required with sending | sending
          · exact ((actualActor.trans (sender sending))).symm
          · exact ((actualActor.trans (payload sending))).symm
      | «local» => exact ((actualActor.trans (sender required))).symm
    have beforeBound := (elaborated_occurrence_is_within_its_fragment accepted previousMember).2
    have differentEvents : previousEvent ≠ event :=
      Nat.ne_of_lt (source_order_strictly_increases_event causal)
    have sameActorBool : (beforeAction.actor == action.actor) = true := decide_eq_true sameActor
    have before : precedes source bindings ⟨previousEvent, .receive⟩ ⟨event, side⟩ = true := by
      simp only [precedes, beforeDecoded, decoded, sameActorBool,
        Bool.true_and, beq_eq_false_iff_ne.mpr differentEvents]
      exact causal
    exact .inr ⟨previousEvent, previousBinding, beforeAction, previousLocated,
      by simpa only [fields.1] using transferred,
      beforeDecoded, sameActor, by simpa only [Nat.zero_add] using beforeBound,
      before, SourcePointEdge.ordered before,
      earlierRows, previous, laterRows, shape, transferred, untouched, previousAt⟩

/-- Structural readiness therefore entails the actual source receipt of the
last handoff at this actor, rather than a guessed earlier event or a header's
mere existence. Value/future availability and fresh visits remain separate. -/
theorem ready_resource_action_has_initial_authority_or_an_observed_receipt
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {run : Elaboration} {issuance : List Issuance}
    {index event : Nat} {symbolic : SymbolicBinding} {binding : Binding}
    {bindings : List (Nat × Binding)} {key : EndpointKey} {side : Side} {action : Action}
    {observed : SourcePoint → Bool}
    (accepted : elaborate resources owners path source 0 = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (instantiated : instantiateBinding issuance symbolic = some binding)
    (solved : run.equations.all (equationAccepted issuance) = true)
    (mapped : (run.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)) = some bindings)
    (required : match side with
      | .receive => symbolic.message.receiver = key
      | .send => symbolic.message.sender = key ∨ symbolic.message.payload = .endpoint key
      | .local => symbolic.message.sender = key)
    (decoded : action? bindings ⟨event, side⟩ = some action)
    (ready : ∀ before, SourcePointEdge source bindings before (ActionId.entryPoint ⟨event, side⟩) →
      observed before = true) :
    (∃ initialActor, instantiate issuance (owners key) = some initialActor ∧ action.actor = initialActor) ∨
    ∃ previousEvent previousBinding beforeAction,
      bindings.find? (fun entry => entry.1 == previousEvent) = some (previousEvent, previousBinding) ∧
      previousBinding.message.payload = .endpoint key ∧
      action? bindings ⟨previousEvent, .receive⟩ = some beforeAction ∧
      beforeAction.actor = action.actor ∧ observed (.committed ⟨previousEvent, .receive⟩) = true := by
  rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
      accepted present instantiated solved mapped required decoded with
    ⟨initialActor, issued, atActor, _⟩ |
    ⟨previousEvent, previousBinding, beforeAction, located, payload, decodedBefore, sameActor, bound, before, edge, _⟩
  · exact .inl ⟨initialActor, issued, atActor⟩
  · exact .inr ⟨previousEvent, previousBinding, beforeAction, located, payload, decodedBefore,
      sameActor, ready _ edge⟩

private theorem decoded_receipt_is_an_actor_source_action
    {source : Source} {path : NormalizedPath} {event actor : Nat} {action : Action}
    (bound : event < source.eventCount)
    (decoded : action? path.bindings ⟨event, .receive⟩ = some action)
    (atActor : action.actor = actor) :
    (⟨event, .receive⟩, action) ∈ source.actorActions path actor := by
  have quotient : (3 * event + 1) / 3 = event := by omega
  have remainder : (3 * event + 1) % 3 = 1 := by omega
  have identity : decodeActionId (3 * event + 1) = ⟨event, .receive⟩ := by
    simp only [decodeActionId, quotient, remainder]
  apply List.mem_filterMap.mpr
  refine ⟨3 * event + 1, List.mem_range.mpr (by omega), ?_⟩
  have itself : (actor == actor) = true := decide_eq_true rfl
  simp only [identity, decoded, Option.bind_eq_bind, Option.bind_some, atActor, itself, ite_true]

/-- A ready decoded resource row either has its source-derived initial issuer
or marks the prior receipt complete at this actual actor. No issued-header or
value-availability premise is used. This is a selected-visit readiness result;
it does not turn a done bit into an application value or cover fresh resets. -/
theorem source_ready_resource_row_has_initial_authority_or_a_completed_receipt
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {row : DecodedActorRow} {binding : Binding}
    {key : EndpointKey} {snapshot : ActorProgressObservation}
    (checked : image.actorRowsCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths)
    (member : row ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (required : match row.actionId.side with
      | .receive => binding.message.receiver = key
      | .send => binding.message.sender = key ∨ binding.message.payload = .endpoint key
      | .local => binding.message.sender = key)
    (ready : sourceRowReady data path snapshot row = true) :
    ∃ (issuance : List Issuance) (run : Elaboration) (index : Nat) (symbolic : SymbolicBinding),
      elaborate data.source.resources (initialOwners data.source.resources)
        path.selections data.source 0 = some run ∧
      run.bindings[index]? = some (row.actionId.event, symbolic) ∧
      instantiateBinding issuance symbolic = some binding ∧
      run.equations.all (equationAccepted issuance) = true ∧
      (run.bindings.mapM fun (event, symbolic) => do
        let binding ← instantiateBinding issuance symbolic
        some (event, binding)) = some path.bindings ∧
      ((instantiate issuance ((initialOwners data.source.resources) key) = some data.actor ∧
          ∀ entry ∈ run.bindings.take index, entry.2.message.payload ≠ .endpoint key) ∨
        ∃ previousRow previousBinding,
          previousRow ∈ data.activeRows path ∧ previousRow.actionId.side = .receive ∧
          path.bindings.find? (fun entry => entry.1 == previousRow.actionId.event) =
            some (previousRow.actionId.event, previousBinding) ∧
          previousBinding.message.payload = .endpoint key ∧ previousBinding.receiverActor = data.actor ∧
          previousRow.eventId ∈ data.sourcePredecessors path row.eventId ∧
          snapshot.doneState.done previousRow.eventId = true ∧
          ∃ earlierRows previous laterRows,
            run.bindings.take index = earlierRows ++ (previousRow.actionId.event, previous) :: laterRows ∧
            previous.message.payload = .endpoint key ∧
            (∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) ∧
            instantiateBinding issuance previous = some previousBinding) := by
  obtain ⟨issuance, run, accepted, solved, mapped⟩ :=
    decoded_actor_path_has_a_solved_source_derivation decoded selected
  obtain ⟨⟨occurrence, symbolic⟩, sourceMember, lowered⟩ := successful_option_mapM_has_input
    mapped (row.actionId.event, binding) (List.mem_of_find?_eq_some located)
  cases bindingAt : instantiateBinding issuance symbolic with
  | none => simp [bindingAt] at lowered
  | some actual =>
      have pair : (occurrence, actual) = (row.actionId.event, binding) := by
        simpa [bindingAt] using lowered
      cases pair
      obtain ⟨index, bound, indexAt⟩ := List.mem_iff_getElem.mp sourceMember
      have present : run.bindings[index]? = some (row.actionId.event, symbolic) := by
        rw [List.getElem?_eq_getElem bound, indexAt]
      have messageAt := (instantiated_binding_retains_message_and_actor_references bindingAt).1
      have sourceRequired : match row.actionId.side with
          | .receive => symbolic.message.receiver = key
          | .send => symbolic.message.sender = key ∨ symbolic.message.payload = .endpoint key
          | .local => symbolic.message.sender = key := by
        simpa only [messageAt] using required
      have actionAt := active_decoded_row_has_its_exact_source_action checked decoded selected member
      refine ⟨issuance, run, index, symbolic, accepted, present, bindingAt, solved, mapped, ?_⟩
      rcases accepted_resource_action_has_initial_authority_or_a_receipt_dependency
          (key := key) (side := row.actionId.side) (action := (row.sourceAction data.actor).2)
          accepted present bindingAt solved mapped sourceRequired actionAt with
        ⟨initialActor, initialAt, actorAt, untouched⟩ |
        ⟨previousEvent, previousBinding, beforeAction, beforeLocated, payload,
          beforeDecoded, sameActor, beforeBound, before, edge, handoffPrefix⟩
      · have same : data.actor = initialActor := actorAt
        exact .inl ⟨by simpa only [← same] using initialAt, untouched⟩
      · have atActor : beforeAction.actor = data.actor := sameActor
        have beforeMember := decoded_receipt_is_an_actor_source_action beforeBound beforeDecoded atActor
        have coverage := checked_active_rows_are_exactly_the_source_actions checked decoded selected
        rw [← coverage] at beforeMember
        obtain ⟨previousRow, previousMember, identity⟩ := List.mem_map.mp beforeMember
        have actionIdentity : previousRow.actionId = ⟨previousEvent, .receive⟩ := congrArg Prod.fst identity
        have previousOrdered : precedes data.source path.bindings previousRow.actionId row.actionId = true := by
          simpa only [actionIdentity] using before
        have predecessor := actor_precedence_is_a_source_predecessor previousMember member previousOrdered
        have done := List.all_eq_true.mp (Bool.and_eq_true_iff.mp ready).2 previousRow.eventId predecessor
        have previousActor : previousBinding.receiverActor = data.actor :=
          (source_action_lookup_uses_the_instantiated_actor beforeLocated beforeDecoded).symm.trans atActor
        exact .inr ⟨previousRow, previousBinding, previousMember, congrArg ActionId.side actionIdentity,
          by simpa only [actionIdentity] using beforeLocated, payload, previousActor, predecessor, done,
          by simpa only [actionIdentity] using handoffPrefix⟩

end Hibana
