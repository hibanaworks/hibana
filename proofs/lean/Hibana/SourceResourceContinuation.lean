import Hibana.SourceResourcePrefix
import Hibana.SourceResourceCausality
import Init.Data.List.Nat.Pairwise

namespace Hibana
open EndpointDelegation

private theorem successful_option_mapM_has_input
    {α β : Type} {f : α → Option β} {inputs : List α} {outputs : List β}
    (accepted : inputs.mapM f = some outputs) :
    ∀ output ∈ outputs, ∃ input ∈ inputs, f input = some output := by
  induction inputs generalizing outputs with
  | nil => simp at accepted; subst outputs; simp
  | cons head tail ih =>
      cases headAt : f head with
      | none => simp [headAt] at accepted
      | some value =>
          cases tailAt : tail.mapM f with
          | none => simp [headAt, tailAt] at accepted
          | some rest =>
              have same : outputs = value :: rest :=
                (Option.some.inj (by simpa [headAt, tailAt] using accepted)).symm
              rw [same]
              intro output member
              rcases List.mem_cons.mp member with equal | member
              · exact ⟨head, List.mem_cons_self, by simpa [equal] using headAt⟩
              · obtain ⟨input, belongs, mapped⟩ := ih tailAt output member
                exact ⟨input, List.mem_cons_of_mem head belongs, mapped⟩

/-- Each accepted normalized path comes from an actual selected source
elaboration with solved ownership equations and a total binding instantiation.
Continuation proofs therefore do not need a separately assumed source path. -/
theorem accepted_normalized_path_has_a_solved_source_derivation
    {source : Source} {normalized : Normalization} {selected : NormalizedPath}
    (accepted : normalize source = some normalized)
    (member : selected ∈ normalized.paths) :
    ∃ issuance run,
      elaborate source.resources (initialOwners source.resources) selected.selections source 0 = some run ∧
      run.equations.all (equationAccepted issuance) = true ∧
      (run.bindings.mapM fun (event, symbolic) => do
        let binding ← instantiateBinding issuance symbolic
        some (event, binding)) = some selected.bindings := by
  change ((paths (source.scopes 0)).mapM fun selections => do
    let run ← elaborate source.resources (initialOwners source.resources) selections source 0
    some (selections, run)).bind _ = some normalized at accepted
  obtain ⟨runs, runsAt, remaining⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨issuance, _, remaining⟩ := Option.bind_eq_some_iff.mp remaining
  cases solved : (runs.flatMap fun run => run.2.equations).all (equationAccepted issuance) with
  | false => simp [solved] at remaining
  | true =>
      simp only [solved, Bool.not_true, Bool.false_eq_true, ite_false] at remaining
      obtain ⟨normalizedPaths, normalizedAt, written⟩ := Option.bind_eq_some_iff.mp remaining
      have same := Option.some.inj written
      subst normalized
      obtain ⟨⟨selections, run⟩, runMember, pathAt⟩ :=
        successful_option_mapM_has_input normalizedAt selected member
      obtain ⟨bindings, bindingsAt, writtenPath⟩ := Option.bind_eq_some_iff.mp pathAt
      have pathSame := Option.some.inj writtenPath
      subst selected
      obtain ⟨sourceSelections, _, elaborated⟩ :=
        successful_option_mapM_has_input runsAt (selections, run) runMember
      obtain ⟨sourceRun, sourceAt, writtenRun⟩ := Option.bind_eq_some_iff.mp elaborated
      have runSame := Option.some.inj writtenRun
      have selectionsSame := congrArg Prod.fst runSame
      have sourceRunSame := congrArg Prod.snd runSame
      dsimp only at selectionsSame sourceRunSame
      subst sourceSelections
      subst sourceRun
      refine ⟨issuance, run, sourceAt, ?_, bindingsAt⟩
      apply List.all_eq_true.mpr
      intro equation equationMember
      exact List.all_eq_true.mp solved equation
        (List.mem_flatMap.mpr ⟨(selections, run), runMember, equationMember⟩)

theorem decoded_actor_order_retains_its_normalized_source
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    (decoded : image.decodeActorOrder? choreo = some data) :
    choreo.ownershipSource? = some data.source ∧
      normalize data.source = some data.normalized := by
  cases recoveryAt : image.ownedRecoveryIndexCheck with
  | false => simp [RustDescriptorImage.decodeActorOrder?, recoveryAt] at decoded
  | true =>
      cases entryAt : image.ownedEntryResourcesCheck with
      | false => simp [RustDescriptorImage.decodeActorOrder?, recoveryAt, entryAt] at decoded
      | true =>
          simp only [RustDescriptorImage.decodeActorOrder?, recoveryAt, entryAt,
            ite_true, Option.pure_def, Option.bind_eq_bind,
            Option.bind_some] at decoded
          simp only [Option.bind_eq_some_iff, Option.some.injEq] at decoded
          rcases decoded with ⟨source, sourceAt, normalized, normalizedAt,
            columns, _, markers, _, rows, _, lanes, _, eventDependencies, _, dependencies, _, rfl⟩
          exact ⟨sourceAt, normalizedAt⟩

/-- Apply the source derivation to the same decoded image and selected path
used by actor admission and owned commit checks. -/
theorem decoded_actor_path_has_a_solved_source_derivation
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {selected : NormalizedPath}
    (decoded : image.decodeActorOrder? choreo = some data)
    (member : selected ∈ data.normalized.paths) :
    ∃ issuance run,
      elaborate data.source.resources (initialOwners data.source.resources)
        selected.selections data.source 0 = some run ∧
      run.equations.all (equationAccepted issuance) = true ∧
      (run.bindings.mapM fun (event, symbolic) => do
        let binding ← instantiateBinding issuance symbolic
        some (event, binding)) = some selected.bindings :=
  accepted_normalized_path_has_a_solved_source_derivation
    (decoded_actor_order_retains_its_normalized_source decoded).2 member

/-- Source coordinates enumerate occurrences strictly, including both route
arms. This concerns identity only, not execution order of parallel work. -/
theorem source_occurrence_enumeration_increases
    (source : Source) (start : Nat) :
    (source.messageEntries start).Pairwise (fun left right => left.1 < right.1) := by
  induction source generalizing start with
  | send message => simp [Source.messageEntries]
  | seq left right leftIH rightIH | par left right leftIH rightIH | route resolver left right leftIH rightIH =>
      change (left.messageEntries start ++ right.messageEntries (start + left.eventCount)).Pairwise _
      refine List.pairwise_append.mpr ⟨leftIH start, rightIH _, ?_⟩
      intro first firstMember second secondMember
      have firstBound := source_occurrence_is_within_its_fragment firstMember
      have secondBound := source_occurrence_is_within_its_fragment secondMember
      exact Nat.lt_of_lt_of_le firstBound.2 secondBound.1
  | roll body bodyIH => exact bodyIH start

theorem accepted_binding_occurrence_enumeration_increases
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    (accepted : elaborate resources owners path source start = some run) :
    run.bindings.Pairwise (fun left right => left.1 < right.1) := by
  have sub := elaboration_retains_exact_source_message_occurrences accepted
  have ordered := (source_occurrence_enumeration_increases source start).sublist sub
  simpa only [Elaboration.messageEntries, List.pairwise_map] using ordered

theorem a_binding_in_the_source_prefix_has_an_earlier_occurrence
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    {index event previousEvent : Nat} {symbolic previous : SymbolicBinding}
    (accepted : elaborate resources owners path source start = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (previousPresent : (previousEvent, previous) ∈ run.bindings.take index) : previousEvent < event := by
  obtain ⟨bound, currentAt⟩ := List.getElem?_eq_some_iff.mp present
  obtain ⟨earlier, earlierBound, previousAt⟩ := List.mem_take_iff_getElem.mp previousPresent
  have ordered := List.pairwise_iff_getElem.mp
    (accepted_binding_occurrence_enumeration_increases accepted)
    earlier index (by omega) bound (by omega)
  simpa only [currentAt, previousAt] using ordered

/-- Unrelated communication after a handoff cannot change its receiver's
resource continuation. This folds immutable source bindings, not runtime state. -/
theorem symbolic_owner_after_last_handoff
    {key : EndpointKey} {owner : OwnerExpr}
    {earlierRows laterRows : List (Nat × SymbolicBinding)} {event : Nat} {loan : SymbolicBinding}
    (transferred : loan.message.payload = .endpoint key)
    (untouched : ∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) :
    ownerAfterBindings key owner (earlierRows ++ (event, loan) :: laterRows) = loan.receiver := by
  rw [symbolic_owner_flow_composes]
  simp only [ownerAfterBindings, if_pos transferred]
  exact symbolic_owner_flow_preserves_an_untouched_resource untouched

/-- Every finite source prefix either has no handoff of this resource or has
a last one. No order is imposed on unrelated parallel communication. -/
theorem symbolic_owner_prefix_has_initial_or_last_handoff
    (key : EndpointKey) (owner : OwnerExpr) (bindings : List (Nat × SymbolicBinding)) :
    (ownerAfterBindings key owner bindings = owner ∧
      ∀ entry ∈ bindings, entry.2.message.payload ≠ .endpoint key) ∨
    ∃ earlierRows event loan laterRows,
      bindings = earlierRows ++ (event, loan) :: laterRows ∧
      loan.message.payload = .endpoint key ∧
      (∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) ∧
      ownerAfterBindings key owner bindings = loan.receiver := by
  induction bindings generalizing owner with
  | nil => exact .inl ⟨rfl, by simp⟩
  | cons head tail ih =>
      let next := if head.2.message.payload = .endpoint key then head.2.receiver else owner
      rcases ih next with ⟨flow, untouched⟩ | ⟨earlierRows, event, loan, laterRows, shape, transfer, untouched, flow⟩
      · by_cases transfer : head.2.message.payload = .endpoint key
        · refine .inr ⟨[], head.1, head.2, tail, ?_, transfer, untouched, ?_⟩
          · simp
          · simpa only [ownerAfterBindings, next, if_pos transfer] using flow
        · refine .inl ⟨?_, ?_⟩
          · simpa only [ownerAfterBindings, next, if_neg transfer] using flow
          · intro entry member
            rcases List.mem_cons.mp member with same | rest
            · simpa [same] using transfer
            · exact untouched entry rest
      · refine .inr ⟨head :: earlierRows, event, loan, laterRows, ?_, transfer, untouched, ?_⟩
        · simp only [List.cons_append, shape]
        · simpa only [ownerAfterBindings, next] using flow

theorem instantiated_binding_retains_message_and_actor_references
    {issuance : List Issuance} {symbolic : SymbolicBinding} {binding : Binding}
    (accepted : instantiateBinding issuance symbolic = some binding) :
    binding.message = symbolic.message ∧
      instantiate issuance symbolic.sender = some binding.senderActor ∧
      instantiate issuance symbolic.receiver = some binding.receiverActor := by
  change (instantiate issuance symbolic.sender).bind _ = some binding at accepted
  obtain ⟨sender, senderAt, rest⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨receiver, receiverAt, written⟩ := Option.bind_eq_some_iff.mp rest
  have same := Option.some.inj written
  subst binding
  exact ⟨rfl, senderAt, receiverAt⟩

/-- For an arbitrary accepted source and selected path, every later use and
the next loan run at the previous receiver. That equality is derived from
source-prefix ownership equations, not assumed as a runtime owner table. -/
theorem source_resource_continuation_uses_previous_receiver
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    {issuance : List Issuance} {index event previousEvent : Nat}
    {symbolic previous : SymbolicBinding} {binding previousBinding : Binding}
    {key : EndpointKey} {earlierRows laterRows : List (Nat × SymbolicBinding)}
    (accepted : elaborate resources owners path source start = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (instantiated : instantiateBinding issuance symbolic = some binding)
    (previousInstantiated : instantiateBinding issuance previous = some previousBinding)
    (solved : run.equations.all (equationAccepted issuance) = true)
    (prefixShape : run.bindings.take index = earlierRows ++ (previousEvent, previous) :: laterRows)
    (transferred : previous.message.payload = .endpoint key)
    (untouched : ∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key) :
    (symbolic.message.sender = key → binding.senderActor = previousBinding.receiverActor) ∧
    (symbolic.message.receiver = key → binding.receiverActor = previousBinding.receiverActor) ∧
    (symbolic.message.payload = .endpoint key → binding.senderActor = previousBinding.receiverActor) := by
  obtain ⟨_, sender, receiver, loan⟩ :=
    instantiated_bindings_preserve_source_ownership_transfer accepted present instantiated solved
  have previousReceiver :=
    (instantiated_binding_retains_message_and_actor_references previousInstantiated).2.2
  have ownerFlow : ownerAfterBindings key (owners key) (run.bindings.take index) =
      previous.receiver := by
    rw [prefixShape]
    exact symbolic_owner_after_last_handoff transferred untouched
  refine ⟨?_, ?_, ?_⟩
  · intro uses
    have ownerAt : instantiate issuance (ownerAfterBindings key (owners key)
        (run.bindings.take index)) = some binding.senderActor := by
      simpa only [uses] using sender
    rw [ownerFlow] at ownerAt
    exact (Option.some.inj (ownerAt.symm.trans previousReceiver))
  · intro uses
    have ownerAt : instantiate issuance (ownerAfterBindings key (owners key)
        (run.bindings.take index)) = some binding.receiverActor := by
      simpa only [uses] using receiver
    rw [ownerFlow] at ownerAt
    exact (Option.some.inj (ownerAt.symm.trans previousReceiver))
  · intro uses
    have ownerAt := (loan key uses).1
    rw [ownerFlow] at ownerAt
    exact (Option.some.inj (ownerAt.symm.trans previousReceiver))

/-- The last source handoff is causally before the continuation, even if
unrelated parallel communication is interleaved in the source enumeration. -/
theorem a_resource_continuation_follows_its_last_handoff
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start : Nat} {run : Elaboration}
    {index event previousEvent : Nat} {symbolic previous : SymbolicBinding}
    {key : EndpointKey} {earlierRows laterRows : List (Nat × SymbolicBinding)}
    (accepted : elaborate resources owners path source start = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (prefixShape : run.bindings.take index = earlierRows ++ (previousEvent, previous) :: laterRows)
    (transferred : previous.message.payload = .endpoint key)
    (used : symbolic.message.sender = key ∨ symbolic.message.receiver = key ∨
      symbolic.message.payload = .endpoint key) :
    source.before (previousEvent - start) (event - start) = true := by
  have previousInPrefix : (previousEvent, previous) ∈ run.bindings.take index := by
    rw [prefixShape]
    exact List.mem_append.mpr (.inr (List.mem_cons_self))
  exact an_earlier_transfer_causally_precedes_a_resource_use accepted
    (List.mem_of_mem_take previousInPrefix) (List.mem_of_getElem? present) transferred used
    (a_binding_in_the_source_prefix_has_an_earlier_occurrence accepted present previousInPrefix)

theorem source_action_lookup_uses_the_instantiated_actor
    {bindings : List (Nat × Binding)} {id : ActionId} {binding : Binding} {action : Action}
    (located : bindings.find? (fun row => row.1 == id.event) = some (id.event, binding))
    (decoded : action? bindings id = some action) :
    action.actor = match id.side with
      | .receive => binding.receiverActor
      | .send | .local => binding.senderActor := by
  cases side : id.side with
  | send =>
      simp only [action?, located, side] at decoded
      change (if binding.message.sender == binding.message.receiver then none else
        some { actor := binding.senderActor, endpoint := binding.message.sender, side := Side.send }) =
        some action at decoded
      split at decoded
      · simp at decoded
      · have same := Option.some.inj decoded
        subst action
        rfl
  | receive =>
      simp only [action?, located, side] at decoded
      change (if binding.message.sender == binding.message.receiver then none else
        some { actor := binding.receiverActor, endpoint := binding.message.receiver, side := Side.receive }) =
        some action at decoded
      split at decoded
      · simp at decoded
      · have same := Option.some.inj decoded
        subst action
        rfl
  | «local» =>
      simp only [action?, located, side] at decoded
      change (if binding.message.sender == binding.message.receiver then
        some { actor := binding.senderActor, endpoint := binding.message.sender, side := Side.local }
        else none) = some action at decoded
      split at decoded
      · have same := Option.some.inj decoded
        subst action
        rfl
      · simp at decoded

/-- A resource continuation depends on the previous handoff's receive at the
same actual actor. This derives the actor equality and causal edge from an
accepted source; it does not assume a ready-operation implementation oracle. -/
theorem last_handoff_receipt_is_a_source_point_dependency
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {run : Elaboration}
    {issuance : List Issuance} {index event previousEvent : Nat}
    {symbolic previous : SymbolicBinding} {binding previousBinding : Binding}
    {key : EndpointKey} {earlierRows laterRows : List (Nat × SymbolicBinding)}
    {bindings : List (Nat × Binding)} {side : Side} {beforeAction afterAction : Action}
    (accepted : elaborate resources owners path source 0 = some run)
    (present : run.bindings[index]? = some (event, symbolic))
    (instantiated : instantiateBinding issuance symbolic = some binding)
    (previousInstantiated : instantiateBinding issuance previous = some previousBinding)
    (solved : run.equations.all (equationAccepted issuance) = true)
    (prefixShape : run.bindings.take index = earlierRows ++ (previousEvent, previous) :: laterRows)
    (transferred : previous.message.payload = .endpoint key)
    (untouched : ∀ entry ∈ laterRows, entry.2.message.payload ≠ .endpoint key)
    (previousLocated : bindings.find? (fun row => row.1 == previousEvent) =
      some (previousEvent, previousBinding))
    (located : bindings.find? (fun row => row.1 == event) = some (event, binding))
    (previousDecoded : action? bindings ⟨previousEvent, .receive⟩ = some beforeAction)
    (decoded : action? bindings ⟨event, side⟩ = some afterAction)
    (required : match side with
      | .receive => symbolic.message.receiver = key
      | .send => symbolic.message.sender = key ∨ symbolic.message.payload = .endpoint key
      | .local => symbolic.message.sender = key) :
    SourcePointEdge source bindings (.committed ⟨previousEvent, .receive⟩)
      (ActionId.entryPoint ⟨event, side⟩) := by
  have actors := source_resource_continuation_uses_previous_receiver accepted present
    instantiated previousInstantiated solved prefixShape transferred untouched
  have beforeActor := source_action_lookup_uses_the_instantiated_actor previousLocated previousDecoded
  have afterActor := source_action_lookup_uses_the_instantiated_actor located decoded
  have sameActor : beforeAction.actor = afterAction.actor := by
    cases side with
    | receive => exact beforeActor.trans ((actors.2.1 required).symm.trans afterActor.symm)
    | send =>
        rcases required with sender | payload
        · exact beforeActor.trans ((actors.1 sender).symm.trans afterActor.symm)
        · exact beforeActor.trans ((actors.2.2 payload).symm.trans afterActor.symm)
    | «local» => exact beforeActor.trans ((actors.1 required).symm.trans afterActor.symm)
  have used : symbolic.message.sender = key ∨ symbolic.message.receiver = key ∨
      symbolic.message.payload = .endpoint key := by
    cases side with
    | receive => exact .inr (.inl required)
    | send => exact required.elim Or.inl (fun payload => .inr (.inr payload))
    | «local» => exact .inl required
  have causal := a_resource_continuation_follows_its_last_handoff accepted present prefixShape transferred used
  have increasing := source_order_strictly_increases_event causal
  have different : previousEvent ≠ event := by omega
  have actorSame : (beforeAction.actor == afterAction.actor) = true :=
    decide_eq_true sameActor
  have eventsDifferent : (previousEvent == event) = false := decide_eq_false different
  apply SourcePointEdge.ordered
  simpa only [precedes, previousDecoded, decoded, actorSame, eventsDifferent,
    Bool.true_and, Bool.false_eq_true, ite_false, Nat.sub_zero] using causal

end Hibana
