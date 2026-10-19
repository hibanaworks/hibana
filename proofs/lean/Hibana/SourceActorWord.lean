import Hibana.SourceAcceptedWord
import Hibana.SourcePublicationPrefix
import Init.Data.List.Count

namespace Hibana
open EndpointDelegation

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

/-- Source occurrences of the real receive commits, in boundary order.
Publication records and local operations do not add receipt letters. -/
def receivedSourceWord (images : Nat → RustDescriptorImage) (choreo : Choreo)
    (records : List (Nat × ActorCommitRecord)) : List Nat :=
  records.filterMap fun (actor, record) =>
    match sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event with
    | some (event, _, .receive) => some event
    | _ => none

/-- Direction is an immutable row column, independent of branch observations
and of the actor currently holding a logical view. -/
def RustDescriptorImage.sourceRowSide? (image : RustDescriptorImage) (event : Nat) : Option Side :=
  image.eventAction? event |>.map fun action => match action with
    | .send _ _ _ => .send
    | .recv _ _ _ => .receive
    | .local _ _ => .local

/-- A finite translation fact, not a Rust admission guard. Both halves of one
source occurrence may belong to the same actor, so direction is part of the key. -/
def RustDescriptorImage.sourceRowIdentityCheck (image : RustDescriptorImage) : Bool :=
  (List.range image.eventCount).all fun left =>
    (List.range image.eventCount).all fun right =>
      if image.eventEffIndex? left = image.eventEffIndex? right ∧
          image.sourceRowSide? left = image.sourceRowSide? right then left == right else true

private theorem a_source_identity_has_its_actual_row_columns
    {image : RustDescriptorImage} {choreo : Choreo} {pathIndex event eff : Nat}
    {binding : Binding} {side : Side}
    (identity : sourceOperationIdentity? image choreo pathIndex event = some (eff, binding, side)) :
    event < image.eventCount ∧ image.eventEffIndex? event = some eff ∧ image.sourceRowSide? event = some side := by
  unfold sourceOperationIdentity? at identity
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def, Option.some.injEq] at identity
  obtain ⟨data, decoded, path, pathAt, row, rowAt, entry, entryAt, actual, effectAt, same⟩ := identity
  have effectSame := congrArg Prod.fst same
  have sideSame := congrArg (fun triple : Nat × Binding × Side => triple.2.2) same
  have rowsAt := (decoded_actor_order_fields decoded).2.2.2.2.1
  have ownIndex := decoded_actor_row_lookup_preserves_its_index rowsAt rowAt
  obtain ⟨_, action, actionAt, sideAt⟩ := decoded_actor_rows_bind_operation_identity rowsAt (List.mem_of_getElem? rowAt)
  have eventBound : event < image.eventCount := by
    unfold RustDescriptorImage.eventEffIndex? at effectAt
    split at effectAt
    · assumption
    · contradiction
  refine ⟨eventBound, ?_, ?_⟩
  · exact effectAt.trans (congrArg some effectSame)
  · simp only [RustDescriptorImage.sourceRowSide?, ← ownIndex, actionAt, Option.map_some]
    exact congrArg some (sideAt.symm.trans sideSame)

private theorem a_publication_uses_the_same_row
    {sample : SourceSendAdmission} {record : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (sameImage : sample.image = image)
    (distinct : image.sourceRowIdentityCheck = true)
    (paired : sourcePublicationPairCheck sample record image choreo session = true) :
    sample.event = record.step.event := by
  unfold sourcePublicationPairCheck at paired
  have matched := (Bool.and_eq_true_iff.mp paired).2
  cases leftAt : sourceOperationIdentity? sample.image choreo sample.pathIndex sample.event with
  | none => simp [leftAt] at matched
  | some left =>
    rcases left with ⟨eff, binding, side⟩
    cases side <;> try simp only [leftAt] at matched
    all_goals
      cases rightAt : sourceOperationIdentity? image choreo record.step.pathIndex record.step.event with
      | none => simp [rightAt] at matched
      | some right =>
        rcases right with ⟨other, binding', side'⟩
        cases side' <;> try simp only [rightAt] at matched
        all_goals try contradiction
        all_goals
          have equalEffects := beq_iff_eq.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp matched).1).1
          have leftColumns := a_source_identity_has_its_actual_row_columns leftAt
          have rightColumns := a_source_identity_has_its_actual_row_columns rightAt
          rw [sameImage] at leftColumns
          have rowCheck := List.all_eq_true.mp
            (List.all_eq_true.mp distinct sample.event (List.mem_range.mpr leftColumns.1))
            record.step.event (List.mem_range.mpr rightColumns.1)
          have sameKey : image.eventEffIndex? sample.event = image.eventEffIndex? record.step.event ∧
              image.sourceRowSide? sample.event = image.sourceRowSide? record.step.event := by
            exact ⟨leftColumns.2.1.trans ((congrArg some equalEffects).trans rightColumns.2.1.symm),
              leftColumns.2.2.trans rightColumns.2.2.symm⟩
          simpa only [if_pos sameKey, beq_iff_eq] using rowCheck

private theorem a_source_execution_keeps_its_accepted_checks
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : first.2 = []) :
    ∀ entry ∈ last.2, entry.sample.image = images entry.sample.image.role ∧ entry.sample.check choreo session = true ∧
      entry.sample.effIndex? = some entry.event := by
  induction execution with
  | refl => simp [initial]
  | @tail middle finish records logged history step ih =>
    cases step with
    | quiet checked => exact ih
    | publish member stamp paired committed => exact ih
    | localCommit actorRole localAt available committed => exact ih
    | receive actorRole received entryAt identity paired available committed => exact ih
    | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
      intro entry member
      rcases List.mem_append.mp member with old | new
      · exact ih entry old
      · have same := List.mem_singleton.mp new
        subst entry
        exact ⟨imageAt, checked, sourceAt⟩

private theorem a_source_execution_keeps_its_committed_checks
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records) :
    ∀ entry ∈ records, entry.2.check (images entry.1) choreo session = true := by
  induction execution with
  | refl => simp
  | @tail middle finish records logged history step ih =>
    intro entry member
    rcases List.mem_append.mp member with old | new
    · exact ih entry old
    · cases step with
      | quiet checked => simp at new
      | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity => simp at new
      | publish member stamp paired committed =>
        simp only [Option.toList_some, List.mem_singleton] at new
        cases new
        exact (a_measured_commit_retains_its_checked_source_and_wire committed).1
      | localCommit actorRole localAt available committed =>
        simp only [Option.toList_some, List.mem_singleton] at new
        cases new
        exact (a_measured_commit_retains_its_checked_source_and_wire committed).1
      | receive actorRole received entryAt identity paired available committed =>
        simp only [Option.toList_some, List.mem_singleton] at new
        cases new
        exact (a_measured_commit_retains_its_checked_source_and_wire committed).1

private theorem a_send_row_has_the_same_count_in_publications
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    {records : List (Nat × ActorCommitRecord)} {rows : List Nat}
    (checked : ∀ entry ∈ records, entry.2.check (images entry.1) choreo session = true)
    (member : event ∈ rows) (sending : (images actor).sourceRowSide? event = some .send) :
    (committedFrameClassWord rows actor records).count event =
      ((actorPublishedSends images choreo records actor).map (·.step.event)).count event := by
  simp only [committedFrameClassWord, actorPublishedSends, List.map_filterMap, List.count_filterMap]
  apply List.countP_congr
  rintro ⟨owner, record⟩ present
  by_cases owns : owner = actor
  · subst owner
    by_cases same : record.step.event = event
    · obtain ⟨eff, binding, side, identity⟩ := checked_record_has_its_source_identity (checked _ present)
      have columns := a_source_identity_has_its_actual_row_columns identity
      have sideSame : side = .send := Option.some.inj (columns.2.2.symm.trans (same ▸ sending))
      subst side
      have sends : sourceSendRecord (images actor) choreo record = true := by simp [sourceSendRecord, identity]
      simp [same, member, sends]
    · cases kind : sourceSendRecord (images actor) choreo record <;>
        cases inside : rows.contains record.step.event <;>
        simp [same]
  · simp [owns]

private theorem a_checked_source_send_has_its_sending_identity
    {sample : SourceSendAdmission} {choreo : Choreo} {session eff : Nat}
    (checked : sample.check choreo session = true) (effectAt : sample.effIndex? = some eff) :
    ∃ binding, sourceOperationIdentity? sample.image choreo sample.pathIndex sample.event = some (eff, binding, .send) := by
  obtain ⟨data, path, row, entry, decoded, pathAt, rowAt, entryAt, _, sideAt, _, _⟩ :=
    checked_source_send_has_its_ready_source_row checked
  change sample.image.eventEffIndex? sample.event = some eff at effectAt
  refine ⟨entry.2, ?_⟩
  simp only [sourceOperationIdentity?, Option.bind_eq_bind, decoded, pathAt, rowAt, entryAt,
    effectAt, Option.bind_some, Option.pure_def, sideAt]

private theorem filtering_other_pending_rows_keeps_this_count
    {rows : List Nat} (entries : List SourceAcceptance) {event : Nat} (member : event ∈ rows) :
    (entries.filterMap (fun entry => if entry.sample.event ∈ rows then some entry.sample.event else none)).count event =
      (entries.map (fun entry => entry.sample.event)).count event := by
  simp only [List.count_eq_countP, List.countP_filterMap, List.countP_map]
  apply List.countP_congr
  intro entry _
  by_cases same : entry.sample.event = event
  · simp [same, member]
  · by_cases inside : entry.sample.event ∈ rows <;> simp [inside, same]

/-- Within this restricted relation, each actor's source word includes its at most one physically
accepted but unpublished send. Its send-phase counts are the actual accepted
send counts, and its receive-phase counts are the actual committed receive
counts. The restricted relation's publication stamp establishes this refinement;
the general asynchronous same-actor par schedule is outside that relation.
There is no second runtime cursor, visit counter, owner map or publication
barrier for another participant.

`sourceRowIdentityCheck` qualifies finite immutable columns, including both
message halves when an actor owns two views. It is not a new Rust guard or a
restriction on projected protocols. -/
theorem a_distributed_actor_has_its_acceptance_inclusive_source_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initialEntries : first.2 = [])
    (initial : ∀ event ∈ group.rows, (first.1.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true) :
    ∃ pending : List SourceAcceptance,
      pending.length ≤ 1 ∧
      (∀ entry ∈ pending, entry ∈ last.2 ∧ entry.stamps actor (last.1.actors actor).2) ∧
      let word := committedFrameClassWord group.rows actor records ++
        pending.filterMap (fun entry => if entry.sample.event ∈ group.rows then some entry.sample.event else none)
      sourceFrameWord group.rows word ∧
      (∀ event ∈ group.rows, (images actor).sourceRowSide? event = some .send →
        word.count event = ((actorAcceptedSends last.2 actor).map (fun entry => entry.sample.event)).count event) ∧
      (∀ event ∈ group.rows, (images actor).sourceRowSide? event = some .receive →
        word.count event = (committedFrameClassWord group.rows actor records).count event) := by
  obtain ⟨pairs, pending, accepted, published, paired, short, stamped⟩ :=
    a_distributed_actors_acceptances_lead_publications_by_at_most_one (actor := actor) execution initialEntries
  have acceptedChecks := a_source_execution_keeps_its_accepted_checks execution initialEntries
  have pendingPresent : ∀ entry ∈ pending, entry ∈ last.2 ∧ entry.stamps actor (last.1.actors actor).2 := by
    intro entry member
    have belongs : entry ∈ actorAcceptedSends last.2 actor := by
      rw [accepted]
      exact List.mem_append.mpr (.inr member)
    exact ⟨(List.mem_filter.mp belongs).1, stamped entry member⟩
  have sameRows : pairs.map (fun pair => pair.1.sample.event) = pairs.map (fun pair => pair.2.step.event) := by
    apply List.map_congr_left
    intro pair member
    have belongs : pair.1 ∈ actorAcceptedSends last.2 actor := by
      rw [accepted]
      exact List.mem_append.mpr (.inl (List.mem_map.mpr ⟨pair, member, rfl⟩))
    have roleAt := beq_iff_eq.mp (List.mem_filter.mp belongs).2
    have imageAt := (acceptedChecks pair.1 (List.mem_filter.mp belongs).1).1
    have sameImage : pair.1.sample.image = images actor := by simpa only [roleAt] using imageAt
    exact a_publication_uses_the_same_row sameImage distinct (paired pair member).1
  have acceptedRows : (actorAcceptedSends last.2 actor).map (fun entry => entry.sample.event) =
      (actorPublishedSends images choreo records actor).map (fun record => record.step.event) ++
        pending.map (fun entry => entry.sample.event) := by
    rw [accepted, published, List.map_append, List.map_map, List.map_map]
    exact congrArg (fun rows => rows ++ pending.map (fun entry => entry.sample.event)) sameRows
  refine ⟨pending, short, pendingPresent, ?_, ?_, ?_⟩
  · cases pending with
    | nil =>
      simpa using (a_measured_distributed_class_has_its_source_word_and_exact_prefix
        (group := group) execution initial).1
    | cons entry rest =>
      have empty : rest = [] := List.eq_nil_of_length_eq_zero (by have bound := short; simp only [List.length_cons] at bound; omega)
      subst rest
      have present := pendingPresent entry (by simp)
      have next := (a_pending_acceptance_extends_its_current_actor_source_word
        (group := group) execution initialEntries initial present.1 present.2).1
      by_cases inside : entry.sample.event ∈ group.rows <;>
        simpa [List.filterMap_cons, inside] using next
  · intro event member sending
    have countAt := a_send_row_has_the_same_count_in_publications
      (a_source_execution_keeps_its_committed_checks execution) member sending
    rw [List.count_append, filtering_other_pending_rows_keeps_this_count pending member, countAt,
      acceptedRows, List.count_append]
  · intro event member receiving
    have noPending : ∀ entry ∈ pending, entry.sample.event ≠ event := by
      intro entry member same
      have present := pendingPresent entry member
      have facts := acceptedChecks entry present.1
      obtain ⟨binding, identity⟩ := a_checked_source_send_has_its_sending_identity facts.2.1 facts.2.2
      have columns := a_source_identity_has_its_actual_row_columns identity
      have sameImage : entry.sample.image = images actor := by simpa only [present.2.1] using facts.1
      rw [sameImage, same, receiving] at columns
      cases Option.some.inj columns.2.2
    have zero : (pending.map (fun entry => entry.sample.event)).count event = 0 := by
      apply List.count_eq_zero.mpr
      intro present
      obtain ⟨entry, member, same⟩ := List.mem_map.mp present
      exact noPending entry member same
    rw [List.count_append, filtering_other_pending_rows_keeps_this_count pending member, zero, Nat.add_zero]

/-- A completed receive phase of one actor contributes to the same source
occurrence's actual receipt word. Other actors and directions may contribute
additional receipts; no common iteration is needed. -/
theorem a_distributed_receiving_row_is_counted_by_the_source_receipt_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor row effect : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {rows : List Nat}
    (execution : DistributedSourceExecution images choreo session first last records)
    (receiving : (images actor).sourceRowSide? row = some .receive)
    (effectAt : (images actor).eventEffIndex? row = some effect) :
    (committedFrameClassWord rows actor records).count row ≤
      (receivedSourceWord images choreo records).count effect := by
  have checks := a_source_execution_keeps_its_committed_checks execution
  simp only [committedFrameClassWord, receivedSourceWord, List.count_filterMap]
  apply List.countP_mono_left
  rintro ⟨owner, record⟩ present selected
  by_cases owns : owner = actor
  · subst owner
    by_cases same : record.step.event = row
    · obtain ⟨actual, binding, side, identity⟩ := checked_record_has_its_source_identity (checks _ present)
      have columns := a_source_identity_has_its_actual_row_columns identity
      have sideSame : side = .receive := Option.some.inj (columns.2.2.symm.trans (same ▸ receiving))
      have effectSame : actual = effect := Option.some.inj (columns.2.1.symm.trans (same ▸ effectAt))
      simp only [identity, sideSame, effectSame, beq_self_eq_true]
    · by_cases inside : record.step.event ∈ rows <;> simp [same, inside] at selected
  · simp [owns] at selected

/-- Count accepted source occurrences using a qualified immutable row
classification. This derives the source/actor/row link from the actual
acceptances in the same execution, including unpublished sends. -/
theorem a_distributed_acceptance_source_count_has_its_static_row_classification
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session effect : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : first.2 = []) (selected : Nat → Nat → Bool)
    (classifies : ∀ actor row actual, (images actor).role = actor → row < (images actor).eventCount →
      (images actor).eventEffIndex? row = some actual → (images actor).sourceRowSide? row = some .send →
      (actual == effect) = selected actor row) :
    (last.2.map SourceAcceptance.event).count effect =
      last.2.countP (fun entry => selected entry.sample.image.role entry.sample.event) := by
  have checks := a_source_execution_keeps_its_accepted_checks execution initial
  simp only [List.count_eq_countP, List.countP_map]
  apply List.countP_congr
  intro entry member
  have facts := checks entry member
  obtain ⟨binding, identity⟩ := a_checked_source_send_has_its_sending_identity facts.2.1 facts.2.2
  have columns := a_source_identity_has_its_actual_row_columns identity
  have roleAt : (images entry.sample.image.role).role = entry.sample.image.role :=
    (congrArg RustDescriptorImage.role facts.1).symm
  rw [facts.1] at columns
  have equal := classifies entry.sample.image.role entry.sample.event entry.event roleAt
    columns.1 columns.2.1 columns.2.2
  change (entry.event == effect) = true ↔ selected entry.sample.image.role entry.sample.event = true
  rw [equal]

private theorem a_source_execution_logs_only_its_actual_actor_images
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : first.2 = []) :
    ∀ entry ∈ records, (images entry.1).role = entry.1 := by
  induction execution with
  | refl => simp
  | @tail middle finish records logged history step ih =>
    intro entry present
    rcases List.mem_append.mp present with old | new
    · exact ih entry old
    · cases step with
      | quiet checked => simp at new
      | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity => simp at new
      | @publish cut entries actor after cleared record accepted member stamp paired committed =>
        simp only [Option.toList_some, List.mem_singleton] at new
        cases new
        have imageAt := (a_source_execution_keeps_its_accepted_checks history initial accepted member).1
        simpa only [stamp.1] using (congrArg RustDescriptorImage.role imageAt).symm
      | localCommit actorRole localAt available committed =>
        simp only [Option.toList_some, List.mem_singleton] at new
        cases new
        exact actorRole
      | receive actorRole received entryAt identity paired available committed =>
        simp only [Option.toList_some, List.mem_singleton] at new
        cases new
        exact actorRole

/-- A complete immutable row classification counts exactly the corresponding
past receives in the same execution. Non-receiving rows must classify false;
ordinary and owned messages are counted together. This replaces a one-sided
count bound when a prospective receive needs its exact FIFO position. -/
theorem a_distributed_receipt_source_count_has_its_static_row_classification
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : first.2 = []) (selectedSource : Nat → Bool) (selectedRow : Nat → Nat → Bool)
    (classifies : ∀ actor row eff side, (images actor).role = actor → row < (images actor).eventCount →
      (images actor).eventEffIndex? row = some eff → (images actor).sourceRowSide? row = some side →
      (if side = .receive then selectedSource eff else false) = selectedRow actor row) :
    (receivedSourceWord images choreo records).countP selectedSource =
      records.countP (fun entry => selectedRow entry.1 entry.2.step.event) := by
  have checks := a_source_execution_keeps_its_committed_checks execution
  have roles := a_source_execution_logs_only_its_actual_actor_images execution initial
  simp only [receivedSourceWord, List.countP_filterMap]
  apply List.countP_congr
  intro entry present
  obtain ⟨eff, binding, side, identity⟩ := checked_record_has_its_source_identity (checks entry present)
  have columns := a_source_identity_has_its_actual_row_columns identity
  have classified := classifies entry.1 entry.2.step.event eff side (roles entry present)
    columns.1 columns.2.1 columns.2.2
  cases side <;> simp only [identity, reduceCtorEq, if_false, if_true, Option.map_none,
    Option.map_some, Option.getD_none, Option.getD_some] at classified ⊢ <;>
      simp only [← classified, Bool.false_eq_true, iff_self]

private def callCommittedWord (rows : List Nat) (actor : Nat)
    (word : List DistributedCallBoundary) : List Nat :=
  committedFrameClassWord rows actor (word.filterMap DistributedCallBoundary.record?)

private theorem call_committed_word_append
    (rows : List Nat) (actor : Nat) (word : List DistributedCallBoundary)
    (boundary : DistributedCallBoundary) :
    callCommittedWord rows actor (word ++ [boundary]) = callCommittedWord rows actor word ++
      committedFrameClassWord rows actor boundary.record?.toList := by
  cases boundary <;> simp [callCommittedWord, committedFrameClassWord,
    List.filterMap_append, DistributedCallBoundary.record?]

private theorem a_pending_call_has_its_actual_send_side
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    {entry : SourceAcceptance} (member : entry ∈ pending) (owns : entry.sample.image.role = actor) :
    (images actor).sourceRowSide? entry.sample.event = some .send := by
  have partition := (a_distributed_call_execution_partitions_its_actual_acceptances execution).1
  have present : entry ∈ entries := partition.mem_iff.mpr (List.mem_append.mpr (.inr member))
  have checked := a_distributed_call_execution_retains_each_accepted_source execution entry present
  have sameImage : entry.sample.image = images actor := by simpa only [owns] using checked.1
  obtain ⟨eff, binding, identity, _⟩ := checked_source_send_has_its_bound_wire_columns checked.2.1
  have columns := a_source_identity_has_its_actual_row_columns identity
  simpa only [sameImage] using columns.2.2

private theorem a_retained_publication_uses_its_actual_row
    {entry : SourceAcceptance} {record : ActorCommitRecord} {choreo : Choreo} {session : Nat}
    (distinct : entry.sample.image.sourceRowIdentityCheck = true)
    (paired : retainedSendPublicationCheck entry record choreo session = true) :
    entry.sample.event = record.step.event := by
  obtain ⟨_, _, _, eff, binding, sent, committed, _⟩ :=
    a_retained_publication_preserves_its_source_and_wire paired
  have left := a_source_identity_has_its_actual_row_columns sent
  have right := a_source_identity_has_its_actual_row_columns committed
  have checked := List.all_eq_true.mp
    (List.all_eq_true.mp distinct entry.sample.event (List.mem_range.mpr left.1))
    record.step.event (List.mem_range.mpr right.1)
  have sameKey : entry.sample.image.eventEffIndex? entry.sample.event =
      entry.sample.image.eventEffIndex? record.step.event ∧
      entry.sample.image.sourceRowSide? entry.sample.event =
      entry.sample.image.sourceRowSide? record.step.event :=
    ⟨left.2.1.trans right.2.1.symm, left.2.2.trans right.2.2.symm⟩
  simpa only [if_pos sameKey, beq_iff_eq] using checked

private theorem a_pending_publication_matches_its_source_row
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    {entry : SourceAcceptance} {record : ActorCommitRecord}
    (member : entry ∈ pending) (owns : entry.sample.image.role = actor)
    (distinct : (images actor).sourceRowIdentityCheck = true)
    (paired : retainedSendPublicationCheck entry record choreo session = true) :
    entry.sample.event = record.step.event := by
  have partition := (a_distributed_call_execution_partitions_its_actual_acceptances execution).1
  have accepted : entry ∈ entries := partition.mem_iff.mpr (List.mem_append.mpr (.inr member))
  have facts := a_distributed_call_execution_retains_each_accepted_source execution entry accepted
  have sameImage : entry.sample.image = images actor := by simpa only [owns] using facts.1
  exact a_retained_publication_uses_its_actual_row (by simpa only [sameImage] using distinct) paired

private theorem a_removed_pending_row_has_no_remaining_alias
    {left right : List SourceAcceptance} {entry other : SourceAcceptance}
    (distinct : ((left ++ entry :: right).map SourceAcceptance.callRow).Nodup)
    (present : other ∈ left ++ right) (same : other.callRow = entry.callRow) : False := by
  have parts := List.nodup_append.mp
    (show (left.map SourceAcceptance.callRow ++ entry.callRow :: right.map SourceAcceptance.callRow).Nodup by
      simpa only [List.map_append, List.map_cons] using distinct)
  rcases List.mem_append.mp present with earlier | later
  · exact parts.2.2 other.callRow (List.mem_map.mpr ⟨other, earlier, rfl⟩)
      entry.callRow (by simp) same
  · have member : other.callRow ∈ right.map SourceAcceptance.callRow := List.mem_map.mpr ⟨other, later, rfl⟩
    rw [same] at member
    exact (List.nodup_cons.mp parts.2.1).1 member

private theorem the_last_source_letter_is_its_next_row
    {rows word : List Nat} {event : Nat} (ordered : sourceFrameWord rows (word ++ [event])) :
    rows[word.length % rows.length]? = some event := by
  exact ordered word.length event (by simp)

private def pendingGroupNext (rows : List Nat) (actor : Nat)
    (word : List DistributedCallBoundary) (pending : List SourceAcceptance) : Prop :=
  ∀ entry ∈ pending, entry.sample.image.role = actor → entry.sample.event ∈ rows →
    rows[(callCommittedWord rows actor word).length % rows.length]? = some entry.sample.event

private theorem pending_group_rows_keep_their_next_source_letter
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true) :
    pendingGroupNext group.rows actor word pending := by
  induction execution with
  | refl => simp [pendingGroupNext]
  | quiet history checked _headers ih =>
      simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
        committedFrameClassWord] using ih
  | @accept cut word entries published pending sample channel sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      intro entry present owns inside
      rw [call_committed_word_append]
      simp only [DistributedCallBoundary.record?, Option.toList_none, committedFrameClassWord,
        List.filterMap_nil, List.append_nil]
      rcases List.mem_append.mp present with old | current
      · exact ih entry old owns inside
      · have same := List.mem_singleton.mp current
        subst entry
        change sample.image.role = actor at owns
        change sample.event ∈ group.rows at inside
        obtain ⟨actualData, actualPath, actualRow, sourceEntry, actualDecoded, actualPathAt, actualRowAt,
            _, active, _, ready, _⟩ := checked_source_send_has_its_ready_source_row checked
        have sameData := Option.some.inj (actualDecoded.symm.trans decoded)
        subst actualData
        have samePath := Option.some.inj (actualPathAt.symm.trans pathAt)
        subst actualPath
        have sameRow := Option.some.inj (actualRowAt.symm.trans rowAt)
        subst actualRow
        have sameImage : sample.image = images actor := by simpa only [owns] using imageAt
        have preparedAt := prepared
        rw [owns, sameImage] at preparedAt
        rw [sameImage] at decoded
        exact (a_prepared_call_operation_has_its_next_committed_source_letter
          (actor := actor) (group := group) (previousPath := previousPath)
          history initial decoded pathAt rowAt active ready preparedAt inside).2
  | @publish cut word entries published left right changed after cleared record sentEntry
      history actorAt paired committed ih =>
      have keeps : pendingGroupNext group.rows actor word (left ++ right) := by
        intro entry present
        exact ih entry (((List.sublist_cons_self sentEntry right).append_left left).subset present)
      by_cases sameActor : changed = actor
      · have sourceActor := actorAt.trans sameActor
        clear actorAt
        subst changed
        by_cases inGroup : record.step.event ∈ group.rows
        · intro entry present owns inside
          have afterExecution := DistributedCallExecution.publish
            (left := left) (right := right) (entry := sentEntry) history sourceActor paired committed
          have ordered := (a_distributed_call_execution_keeps_its_committed_source_group_word
            (actor := actor) (group := group) afterExecution initial).1
          change sourceFrameWord group.rows
            (callCommittedWord group.rows actor (word ++ [.publish sentEntry.callKey actor record])) at ordered
          have appended : callCommittedWord group.rows actor (word ++ [.publish sentEntry.callKey actor record]) =
              callCommittedWord group.rows actor word ++ [record.step.event] := by
            rw [call_committed_word_append]
            simp [DistributedCallBoundary.record?, committedFrameClassWord, inGroup]
          rw [appended] at ordered
          have next := the_last_source_letter_is_its_next_row ordered
          have sameEvent := Option.some.inj ((keeps entry present owns inside).symm.trans next)
          have sentPresent : sentEntry ∈ left ++ sentEntry :: right := by simp
          have rowMatch := a_pending_publication_matches_its_source_row history sentPresent sourceActor distinct paired
          have sameSlot : entry.callRow = sentEntry.callRow :=
            Prod.ext (owns.trans sourceActor.symm) (sameEvent.trans rowMatch.symm)
          have unique := a_source_call_history_has_unique_outstanding_rows
            (a_distributed_call_execution_has_its_same_history_publication_matching history)
          exact False.elim (a_removed_pending_row_has_no_remaining_alias unique present sameSlot)
        · simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
            committedFrameClassWord, inGroup] using keeps
      · simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
          committedFrameClassWord, sameActor] using keeps
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
      by_cases sameActor : changed = actor
      · subst changed
        by_cases inGroup : record.step.event ∈ group.rows
        · intro entry present owns inside
          have afterExecution := DistributedCallExecution.localCommit history actorRole localAt committed
          have ordered := (a_distributed_call_execution_keeps_its_committed_source_group_word
            (actor := actor) (group := group) afterExecution initial).1
          change sourceFrameWord group.rows
            (callCommittedWord group.rows actor (word ++ [.localCommit actor record])) at ordered
          have appended : callCommittedWord group.rows actor (word ++ [.localCommit actor record]) =
              callCommittedWord group.rows actor word ++ [record.step.event] := by
            rw [call_committed_word_append]
            simp [DistributedCallBoundary.record?, committedFrameClassWord, inGroup]
          rw [appended] at ordered
          have next := the_last_source_letter_is_its_next_row ordered
          have sameEvent := Option.some.inj ((ih entry present owns inside).symm.trans next)
          have sendSide := a_pending_call_has_its_actual_send_side history present owns
          have localSide := (a_source_identity_has_its_actual_row_columns localAt).2.2
          rw [sameEvent, localSide] at sendSide
          cases Option.some.inj sendSide
        · simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
            committedFrameClassWord, inGroup] using ih
      · simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
          committedFrameClassWord, sameActor] using ih
  | @receive cut word entries published pending changed channel next frame after cleared record sentEntry witness
      history actorRole received entryAt identity paired committed ih =>
      by_cases sameActor : changed = actor
      · subst changed
        by_cases inGroup : record.step.event ∈ group.rows
        · intro entry present owns inside
          have afterExecution := DistributedCallExecution.receive
            (entry := sentEntry) (witness := witness) history actorRole received entryAt identity paired committed
          have ordered := (a_distributed_call_execution_keeps_its_committed_source_group_word
            (actor := actor) (group := group) afterExecution initial).1
          change sourceFrameWord group.rows
            (callCommittedWord group.rows actor (word ++ [.receive sentEntry.callKey actor record])) at ordered
          have appended : callCommittedWord group.rows actor (word ++ [.receive sentEntry.callKey actor record]) =
              callCommittedWord group.rows actor word ++ [record.step.event] := by
            rw [call_committed_word_append]
            simp [DistributedCallBoundary.record?, committedFrameClassWord, inGroup]
          rw [appended] at ordered
          have expected := the_last_source_letter_is_its_next_row ordered
          have sameEvent := Option.some.inj ((ih entry present owns inside).symm.trans expected)
          have sendSide := a_pending_call_has_its_actual_send_side history present owns
          obtain ⟨_, _, _, _, receiveIdentity⟩ := a_corresponding_receive_pair_has_its_original_source_halves paired
          have receiveSide := (a_source_identity_has_its_actual_row_columns receiveIdentity).2.2
          rw [sameEvent, receiveSide] at sendSide
          cases Option.some.inj sendSide
        · simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
            committedFrameClassWord, inGroup] using ih
      · simpa [pendingGroupNext, call_committed_word_append, DistributedCallBoundary.record?,
          committedFrameClassWord, sameActor] using ih

/-- A pending accepted send stays the next source letter of its ordered
group across arbitrary independent commits and fresh body resets. This follows
from the same chronology, not equality of whole-actor snapshots or stamps. -/
theorem a_pending_call_keeps_its_ordered_groups_next_source_letter
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true) :
    ∀ entry ∈ pending, entry.sample.image.role = actor → entry.sample.event ∈ group.rows →
      group.rows[(committedFrameClassWord group.rows actor
        (word.filterMap DistributedCallBoundary.record?)).length % group.rows.length]? = some entry.sample.event := by
  exact pending_group_rows_keep_their_next_source_letter execution initial distinct

private def callPendingGroup (rows : List Nat) (actor : Nat) (pending : List SourceAcceptance) :
    List SourceAcceptance :=
  pending.filter fun entry => decide (entry.sample.image.role = actor ∧ entry.sample.event ∈ rows)

/-- At most one call can be outstanding in this ordered source group. An
actor can simultaneously retain calls from other independent groups; there is
no actor-wide bound of one and no added Rust admission check. -/
theorem an_ordered_source_group_has_at_most_one_pending_call
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true) :
    (pending.filter fun entry => decide (entry.sample.image.role = actor ∧ entry.sample.event ∈ group.rows)).length ≤ 1 := by
  change (callPendingGroup group.rows actor pending).length ≤ 1
  cases selected : callPendingGroup group.rows actor pending with
  | nil => simp
  | cons firstCall rest =>
    cases rest with
    | nil => simp
    | cons secondCall tail =>
      have firstMember : firstCall ∈ callPendingGroup group.rows actor pending := by rw [selected]; simp
      have secondMember : secondCall ∈ callPendingGroup group.rows actor pending := by rw [selected]; simp
      have firstFacts := List.mem_filter.mp firstMember
      have secondFacts := List.mem_filter.mp secondMember
      have firstGroup := of_decide_eq_true firstFacts.2
      have secondGroup := of_decide_eq_true secondFacts.2
      have next := pending_group_rows_keep_their_next_source_letter execution initial distinct
      have firstNext := next firstCall firstFacts.1 firstGroup.1 firstGroup.2
      have secondNext := next secondCall secondFacts.1 secondGroup.1 secondGroup.2
      have sameEvent := Option.some.inj (firstNext.symm.trans secondNext)
      have sameSlot : firstCall.callRow = secondCall.callRow :=
        Prod.ext (firstGroup.1.trans secondGroup.1.symm) sameEvent
      have sublist : (callPendingGroup group.rows actor pending).Sublist pending := List.filter_sublist
      have slots := a_source_call_history_has_unique_outstanding_rows
        (a_distributed_call_execution_has_its_same_history_publication_matching execution)
      have filteredSlots := (sublist.map SourceAcceptance.callRow).nodup slots
      simp only [selected, List.map_cons] at filteredSlots
      exact False.elim ((List.nodup_cons.mp filteredSlots).1 (by simp [sameSlot]))

private def callLogicalWord (rows : List Nat) (actor : Nat)
    (word : List DistributedCallBoundary) : List Nat :=
  word.filterMap fun boundary => boundary.sourceLetter? actor |>.bind fun event =>
    if event ∈ rows then some event else none

private theorem call_logical_word_append
    (rows : List Nat) (actor : Nat) (word : List DistributedCallBoundary)
    (boundary : DistributedCallBoundary) :
    callLogicalWord rows actor (word ++ [boundary]) = callLogicalWord rows actor word ++
      ((boundary.sourceLetter? actor).bind fun event => if event ∈ rows then some event else none).toList := by
  unfold callLogicalWord
  rw [List.filterMap_append]
  rfl

private theorem call_word_length_balance
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (rows : List Nat) (distinct : (images actor).sourceRowIdentityCheck = true) :
    (callLogicalWord rows actor word).length =
      (callCommittedWord rows actor word).length + (callPendingGroup rows actor pending).length := by
  induction execution with
  | refl => simp [callLogicalWord, callCommittedWord, committedFrameClassWord, callPendingGroup]
  | quiet history checked _headers ih =>
      simpa [call_logical_word_append, call_committed_word_append, DistributedCallBoundary.sourceLetter?,
        DistributedCallBoundary.record?, committedFrameClassWord] using ih
  | @accept cut word entries published pending sample channel sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      rw [call_logical_word_append, call_committed_word_append]
      by_cases owns : sample.image.role = actor <;> by_cases inside : sample.event ∈ rows <;>
        simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
          callPendingGroup, List.filter_append, owns, inside] at ih ⊢ <;> omega
  | @publish cut word entries published left right changed after cleared record sentEntry
      history actorAt paired committed ih =>
      rw [call_logical_word_append, call_committed_word_append]
      by_cases sameActor : changed = actor
      · have sourceActor := actorAt.trans sameActor
        clear actorAt
        subst changed
        have rowMatch := a_pending_publication_matches_its_source_row history
          (by simp) sourceActor distinct paired
        by_cases inside : record.step.event ∈ rows <;>
          simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
            callPendingGroup, List.filter_append, sourceActor, rowMatch, inside] at ih ⊢ <;> omega
      · simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
          callPendingGroup, List.filter_append, actorAt, sameActor] at ih ⊢
        exact ih
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
      rw [call_logical_word_append, call_committed_word_append]
      by_cases sameActor : changed = actor <;> by_cases inside : record.step.event ∈ rows <;>
        simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
          sameActor, inside] at ih ⊢ <;> omega
  | @receive cut word entries published pending changed channel next frame after cleared record sentEntry witness
      history actorRole received entryAt identity paired committed ih =>
      rw [call_logical_word_append, call_committed_word_append]
      by_cases sameActor : changed = actor <;> by_cases inside : record.step.event ∈ rows <;>
        simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
          sameActor, inside] at ih ⊢ <;> omega

private theorem a_non_send_next_row_has_no_pending_call
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    {group : SourceOrderGroup (images actor) choreo} {side : Side}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true)
    (next : group.rows[(callCommittedWord group.rows actor word).length % group.rows.length]? = some event)
    (sideAt : (images actor).sourceRowSide? event = some side) (notSend : side ≠ .send) :
    callPendingGroup group.rows actor pending = [] := by
  cases selected : callPendingGroup group.rows actor pending with
  | nil => rfl
  | cons entry rest =>
    have member : entry ∈ callPendingGroup group.rows actor pending := by rw [selected]; simp
    have facts := List.mem_filter.mp member
    have belongs := of_decide_eq_true facts.2
    have expected := pending_group_rows_keep_their_next_source_letter execution initial distinct
      entry facts.1 belongs.1 belongs.2
    have same := Option.some.inj (expected.symm.trans next)
    have actual := a_pending_call_has_its_actual_send_side execution facts.1 belongs.1
    rw [same, sideAt] at actual
    exact False.elim (notSend (Option.some.inj actual))

/-- Count a wire send at its physical acceptance, even while its sender has
not published that operation. Each ordered source group follows its cyclic
source word in this actual chronological order. Independent groups may have
simultaneous outstanding calls and publish them in either order; no common
actor visit, whole-snapshot equality or actor-wide single-call guard is used.

This is source-order refinement under the checked static group facts and
actual call-history premises. It does not by itself establish ownership,
enabled-step lifting, complete Rust history extraction or failure progress. -/
theorem a_distributed_call_execution_has_its_acceptance_inclusive_source_group_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true) :
    sourceFrameWord group.rows (word.filterMap fun boundary =>
      (boundary.sourceLetter? actor).bind fun event => if event ∈ group.rows then some event else none) := by
  change sourceFrameWord group.rows (callLogicalWord group.rows actor word)
  induction execution with
  | refl => simp [sourceFrameWord, callLogicalWord]
  | quiet history checked _headers ih =>
    simpa [call_logical_word_append, DistributedCallBoundary.sourceLetter?] using ih
  | @accept cut word entries published pending sample channel sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    rw [call_logical_word_append]
    by_cases owns : sample.image.role = actor
    · by_cases inside : sample.event ∈ group.rows
      · simp only [DistributedCallBoundary.sourceLetter?, beq_iff_eq, owns, ↓reduceIte,
          Option.bind_some, if_pos inside, Option.toList_some]
        have afterExecution := DistributedCallExecution.accept
          (previousPath := previousPath) (witness := witness) history imageAt checked decoded pathAt rowAt
          prepared slotFree freshWitness sourceAt accepted identity
        have bound := an_ordered_source_group_has_at_most_one_pending_call
          (actor := actor) (group := group) afterExecution initial distinct
        have emptyLength : (callPendingGroup group.rows actor pending).length = 0 := by
          have lengthBound : (callPendingGroup group.rows actor pending).length + 1 ≤ 1 := by
            simpa only [callPendingGroup, List.filter_append, List.filter_cons, List.filter_nil,
              owns, inside, and_self, decide_true, ite_true, List.length_append,
              List.length_cons, List.length_nil, Nat.zero_add] using bound
          omega
        have balance := call_word_length_balance history group.rows distinct
        rw [emptyLength, Nat.add_zero] at balance
        obtain ⟨actualData, actualPath, actualRow, sourceEntry, actualDecoded, actualPathAt, actualRowAt,
            _, active, _, ready, _⟩ := checked_source_send_has_its_ready_source_row checked
        have sameData := Option.some.inj (actualDecoded.symm.trans decoded)
        subst actualData
        have samePath := Option.some.inj (actualPathAt.symm.trans pathAt)
        subst actualPath
        have sameRow := Option.some.inj (actualRowAt.symm.trans rowAt)
        subst actualRow
        have sameImage : sample.image = images actor := by simpa only [owns] using imageAt
        have preparedAt := prepared
        rw [owns, sameImage] at preparedAt
        rw [sameImage] at decoded
        have next := (a_prepared_call_operation_has_its_next_committed_source_letter
          (actor := actor) (group := group) (previousPath := previousPath)
          history initial decoded pathAt rowAt active ready preparedAt inside).2
        exact a_source_word_extends_by_its_next_actual_letter ih (by simpa only [balance] using next)
      · simpa [DistributedCallBoundary.sourceLetter?, owns, inside] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, owns] using ih
  | publish history actorAt paired committed ih =>
    simpa [call_logical_word_append, DistributedCallBoundary.sourceLetter?] using ih
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
    rw [call_logical_word_append]
    by_cases sameActor : changed = actor
    · subst changed
      by_cases inside : record.step.event ∈ group.rows
      · simp only [DistributedCallBoundary.sourceLetter?, beq_self_eq_true, ↓reduceIte,
          Option.bind_some, if_pos inside, Option.toList_some]
        have afterExecution := DistributedCallExecution.localCommit history actorRole localAt committed
        have ordered := (a_distributed_call_execution_keeps_its_committed_source_group_word
          (actor := actor) (group := group) afterExecution initial).1
        change sourceFrameWord group.rows
          (callCommittedWord group.rows actor (word ++ [.localCommit actor record])) at ordered
        rw [call_committed_word_append] at ordered
        simp [DistributedCallBoundary.record?, committedFrameClassWord, inside] at ordered
        have next := the_last_source_letter_is_its_next_row ordered
        have sideAt := (a_source_identity_has_its_actual_row_columns localAt).2.2
        have emptyPending := a_non_send_next_row_has_no_pending_call history initial distinct next sideAt (by decide)
        have balance := call_word_length_balance history group.rows distinct
        rw [emptyPending, List.length_nil, Nat.add_zero] at balance
        exact a_source_word_extends_by_its_next_actual_letter ih (by simpa only [balance] using next)
      · simpa [DistributedCallBoundary.sourceLetter?, inside] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, sameActor] using ih
  | @receive cut word entries published pending changed channel nextState frame after cleared record sentEntry witness
      history actorRole received entryAt identity paired committed ih =>
    rw [call_logical_word_append]
    by_cases sameActor : changed = actor
    · subst changed
      by_cases inside : record.step.event ∈ group.rows
      · simp only [DistributedCallBoundary.sourceLetter?, beq_self_eq_true, ↓reduceIte,
          Option.bind_some, if_pos inside, Option.toList_some]
        have afterExecution := DistributedCallExecution.receive
          (entry := sentEntry) (witness := witness) history actorRole received entryAt identity paired committed
        have ordered := (a_distributed_call_execution_keeps_its_committed_source_group_word
          (actor := actor) (group := group) afterExecution initial).1
        change sourceFrameWord group.rows
          (callCommittedWord group.rows actor (word ++ [.receive sentEntry.callKey actor record])) at ordered
        rw [call_committed_word_append] at ordered
        simp [DistributedCallBoundary.record?, committedFrameClassWord, inside] at ordered
        have next := the_last_source_letter_is_its_next_row ordered
        obtain ⟨_, _, _, _, receiveIdentity⟩ := a_corresponding_receive_pair_has_its_original_source_halves paired
        have sideAt := (a_source_identity_has_its_actual_row_columns receiveIdentity).2.2
        have emptyPending := a_non_send_next_row_has_no_pending_call history initial distinct next sideAt (by decide)
        have balance := call_word_length_balance history group.rows distinct
        rw [emptyPending, List.length_nil, Nat.add_zero] at balance
        exact a_source_word_extends_by_its_next_actual_letter ih (by simpa only [balance] using next)
      · simpa [DistributedCallBoundary.sourceLetter?, inside] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, sameActor] using ih

private theorem a_call_word_counts_each_actual_send_row
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (rows : List Nat) (inside : event ∈ rows)
    (sending : (images actor).sourceRowSide? event = some .send) :
    (callLogicalWord rows actor word).count event =
      ((actorAcceptedSends entries actor).map fun entry => entry.sample.event).count event := by
  induction execution with
  | refl => simp [callLogicalWord, actorAcceptedSends]
  | quiet history checked _headers ih =>
    simpa [call_logical_word_append, DistributedCallBoundary.sourceLetter?] using ih
  | @accept cut word entries published pending sample channel sent sourceEvent previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    rw [call_logical_word_append]
    by_cases owns : sample.image.role = actor <;> by_cases same : sample.event = event <;>
      by_cases member : sample.event ∈ rows <;>
      simp [DistributedCallBoundary.sourceLetter?, actorAcceptedSends, List.filter_append, List.map_append,
        List.count_append, owns, same, member, inside] at ih ⊢ <;> omega
  | publish history actorAt paired committed ih =>
    simpa [call_logical_word_append, DistributedCallBoundary.sourceLetter?] using ih
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
    rw [call_logical_word_append]
    by_cases sameActor : changed = actor
    · subst changed
      have other : record.step.event ≠ event := by
        intro same
        have sideAt := (a_source_identity_has_its_actual_row_columns localAt).2.2
        rw [same, sending] at sideAt
        cases Option.some.inj sideAt
      by_cases member : record.step.event ∈ rows <;>
        simpa [DistributedCallBoundary.sourceLetter?, List.count_append, member, other, Ne.symm other] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, sameActor] using ih
  | @receive cut word entries published pending changed channel nextState frame after cleared record sentEntry witness
      history actorRole received entryAt identity paired committed ih =>
    rw [call_logical_word_append]
    by_cases sameActor : changed = actor
    · subst changed
      have other : record.step.event ≠ event := by
        intro same
        obtain ⟨_, _, _, _, receiveIdentity⟩ := a_corresponding_receive_pair_has_its_original_source_halves paired
        have sideAt := (a_source_identity_has_its_actual_row_columns receiveIdentity).2.2
        rw [same, sending] at sideAt
        cases Option.some.inj sideAt
      by_cases member : record.step.event ∈ rows <;>
        simpa [DistributedCallBoundary.sourceLetter?, List.count_append, member, other, Ne.symm other] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, sameActor] using ih

private theorem a_call_word_counts_each_actual_receive_row
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (rows : List Nat)
    (receiving : (images actor).sourceRowSide? event = some .receive) :
    (callLogicalWord rows actor word).count event = (callCommittedWord rows actor word).count event := by
  induction execution with
  | refl => simp [callLogicalWord, callCommittedWord, committedFrameClassWord]
  | quiet history checked _headers ih =>
    simpa [call_logical_word_append, call_committed_word_append, DistributedCallBoundary.sourceLetter?,
      DistributedCallBoundary.record?, committedFrameClassWord] using ih
  | @accept cut word entries published pending sample channel sent sourceEvent previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    rw [call_logical_word_append, call_committed_word_append]
    by_cases owns : sample.image.role = actor
    · have other : sample.event ≠ event := by
        intro same
        obtain ⟨eff, binding, sendIdentity, _⟩ := checked_source_send_has_its_bound_wire_columns checked
        have sideAt := (a_source_identity_has_its_actual_row_columns sendIdentity).2.2
        have sameImage : sample.image = images actor := by simpa only [owns] using imageAt
        rw [sameImage, same, receiving] at sideAt
        cases Option.some.inj sideAt
      by_cases member : sample.event ∈ rows <;>
        simpa [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
          List.count_append, owns, member, other, Ne.symm other] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord, owns] using ih
  | @publish cut word entries published left right changed after cleared record sentEntry
      history actorAt paired committed ih =>
    rw [call_logical_word_append, call_committed_word_append]
    by_cases sameActor : changed = actor
    · have sourceActor := actorAt.trans sameActor
      clear actorAt
      subst changed
      have other : record.step.event ≠ event := by
        intro same
        obtain ⟨_, _, _, eff, binding, sent, sendIdentity, _⟩ :=
          a_retained_publication_preserves_its_source_and_wire paired
        have sideAt := (a_source_identity_has_its_actual_row_columns sendIdentity).2.2
        have acceptedFacts := a_distributed_call_execution_retains_each_accepted_source history sentEntry
          ((a_distributed_call_execution_partitions_its_actual_acceptances history).1.mem_iff.mpr
            (List.mem_append.mpr (.inr (by simp))))
        have sameImage : sentEntry.sample.image = images actor := by simpa only [sourceActor] using acceptedFacts.1
        rw [sameImage, same, receiving] at sideAt
        cases Option.some.inj sideAt
      by_cases member : record.step.event ∈ rows <;>
        simpa [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
          List.count_append, member, other, Ne.symm other] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord, sameActor] using ih
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
    rw [call_logical_word_append, call_committed_word_append]
    by_cases sameActor : changed = actor <;> by_cases member : record.step.event ∈ rows <;>
      simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
        List.count_append, sameActor, member, ih]
  | @receive cut word entries published pending changed channel nextState frame after cleared record sentEntry witness
      history actorRole received entryAt identity paired committed ih =>
    rw [call_logical_word_append, call_committed_word_append]
    by_cases sameActor : changed = actor <;> by_cases member : record.step.event ∈ rows <;>
      simp [DistributedCallBoundary.sourceLetter?, DistributedCallBoundary.record?, committedFrameClassWord,
        List.count_append, sameActor, member, ih]

/-- The acceptance-inclusive group word counts every actual send acceptance
once and every actual receive commit once, including independent unpublished
sends. The row's immutable direction rules out contributions by another kind
of operation. These counts and the source-word result use the same chronology. -/
theorem a_distributed_call_source_group_counts_its_actual_sends_and_receives
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    let logicalWord := word.filterMap fun boundary =>
      (boundary.sourceLetter? actor).bind fun event => if event ∈ group.rows then some event else none
    (∀ event ∈ group.rows, (images actor).sourceRowSide? event = some .send →
      logicalWord.count event = ((actorAcceptedSends entries actor).map fun entry => entry.sample.event).count event) ∧
    (∀ event ∈ group.rows, (images actor).sourceRowSide? event = some .receive →
      logicalWord.count event = (committedFrameClassWord group.rows actor
        (word.filterMap DistributedCallBoundary.record?)).count event) := by
  exact ⟨fun _ member sideAt => a_call_word_counts_each_actual_send_row execution group.rows member sideAt,
    fun _ _ sideAt => a_call_word_counts_each_actual_receive_row execution group.rows sideAt⟩

private theorem a_call_execution_logs_its_checked_actor_images
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    ∀ entry ∈ word.filterMap DistributedCallBoundary.record?,
      (images entry.1).role = entry.1 ∧ entry.2.check (images entry.1) choreo session = true := by
  induction execution with
  | refl => simp
  | quiet history checked _headers ih =>
    simpa [List.filterMap_append, DistributedCallBoundary.record?] using ih
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    simpa [List.filterMap_append, DistributedCallBoundary.record?] using ih
  | @publish cut word entries published left right actor after cleared record sentEntry
      history actorAt paired committed ih =>
    intro entry member
    simp only [List.filterMap_append, DistributedCallBoundary.record?, List.filterMap_cons,
      List.filterMap_nil, List.mem_append, List.mem_singleton] at member
    rcases member with earlier | current
    · exact ih entry earlier
    · subst entry
      have source := a_distributed_call_execution_retains_each_accepted_source history sentEntry
        ((a_distributed_call_execution_partitions_its_actual_acceptances history).1.mem_iff.mpr
          (List.mem_append.mpr (.inr (by simp))))
      have roleAt := (congrArg RustDescriptorImage.role source.1).symm
      exact ⟨by simpa only [actorAt] using roleAt,
        (a_measured_commit_retains_its_checked_source_and_wire committed).1⟩
  | localCommit history actorRole localAt committed ih =>
    intro entry member
    simp only [List.filterMap_append, DistributedCallBoundary.record?, List.filterMap_cons,
      List.filterMap_nil, List.mem_append, List.mem_singleton] at member
    rcases member with earlier | current
    · exact ih entry earlier
    · subst entry
      exact ⟨actorRole, (a_measured_commit_retains_its_checked_source_and_wire committed).1⟩
  | receive history actorRole received entryAt identity paired committed ih =>
    intro entry member
    simp only [List.filterMap_append, DistributedCallBoundary.record?, List.filterMap_cons,
      List.filterMap_nil, List.mem_append, List.mem_singleton] at member
    rcases member with earlier | current
    · exact ih entry earlier
    · subst entry
      exact ⟨actorRole, (a_measured_commit_retains_its_checked_source_and_wire committed).1⟩

/-- Count past receipts directly from checked records of the asynchronous
chronology and a finite immutable row classification. The classifier includes
every row and maps non-receiving rows to false; it contains no trace order or
prospective receive-pair assumption. Ordinary and owned messages count alike. -/
theorem a_distributed_call_receipt_source_count_has_its_static_row_classification
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (selectedSource : Nat → Bool) (selectedRow : Nat → Nat → Bool)
    (classifies : ∀ actor row eff side, (images actor).role = actor → row < (images actor).eventCount →
      (images actor).eventEffIndex? row = some eff → (images actor).sourceRowSide? row = some side →
      (if side = .receive then selectedSource eff else false) = selectedRow actor row) :
    (receivedSourceWord images choreo (word.filterMap DistributedCallBoundary.record?)).countP selectedSource =
      (word.filterMap DistributedCallBoundary.record?).countP (fun entry => selectedRow entry.1 entry.2.step.event) := by
  have checked := a_call_execution_logs_its_checked_actor_images execution
  generalize recordAt : word.filterMap DistributedCallBoundary.record? = records at checked ⊢
  simp only [receivedSourceWord, List.countP_filterMap]
  apply List.countP_congr
  intro entry present
  have facts := checked entry present
  obtain ⟨eff, binding, side, identity⟩ := checked_record_has_its_source_identity facts.2
  have columns := a_source_identity_has_its_actual_row_columns identity
  have classified := classifies entry.1 entry.2.step.event eff side facts.1 columns.1 columns.2.1 columns.2.2
  cases side <;> simp only [identity, reduceCtorEq, if_false, if_true, Option.map_none,
    Option.map_some, Option.getD_none, Option.getD_some] at classified ⊢ <;>
      simp only [← classified, Bool.false_eq_true, iff_self]

private theorem a_call_word_selects_its_actual_send_rows
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (rows : List Nat) (sending : ∀ event ∈ rows, (images actor).sourceRowSide? event = some .send) :
    callLogicalWord rows actor word = (actorAcceptedSends entries actor).filterMap
      (fun entry => if entry.sample.event ∈ rows then some entry.sample.event else none) := by
  induction execution with
  | refl => simp [callLogicalWord, actorAcceptedSends]
  | quiet history checked _headers ih =>
    simpa [call_logical_word_append, DistributedCallBoundary.sourceLetter?] using ih
  | @accept cut word entries published pending sample channel sent sourceEvent previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    rw [call_logical_word_append]
    by_cases owns : sample.image.role = actor <;> by_cases member : sample.event ∈ rows <;>
      simpa [DistributedCallBoundary.sourceLetter?, actorAcceptedSends, List.filter_append,
        List.filterMap_append, owns, member] using ih
  | publish history actorAt paired committed ih =>
    simpa [call_logical_word_append, DistributedCallBoundary.sourceLetter?] using ih
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
    rw [call_logical_word_append]
    by_cases sameActor : changed = actor
    · subst changed
      have outside : record.step.event ∉ rows := by
        intro member
        have sideAt := (a_source_identity_has_its_actual_row_columns localAt).2.2
        rw [sending _ member] at sideAt
        cases Option.some.inj sideAt
      simpa [DistributedCallBoundary.sourceLetter?, outside] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, sameActor] using ih
  | @receive cut word entries published pending changed channel nextState frame after cleared record sentEntry witness
      history actorRole received entryAt identity paired committed ih =>
    rw [call_logical_word_append]
    by_cases sameActor : changed = actor
    · subst changed
      have outside : record.step.event ∉ rows := by
        intro member
        obtain ⟨_, _, _, _, receiveIdentity⟩ := a_corresponding_receive_pair_has_its_original_source_halves paired
        have sideAt := (a_source_identity_has_its_actual_row_columns receiveIdentity).2.2
        rw [sending _ member] at sideAt
        cases Option.some.inj sideAt
      simpa [DistributedCallBoundary.sourceLetter?, outside] using ih
    · simpa [DistributedCallBoundary.sourceLetter?, sameActor] using ih

private theorem an_exact_partial_map_preserves_a_source_word
    {rows word sourceRows sourceWord : List Nat} {source : Nat → Option Nat}
    (ordered : sourceFrameWord rows word)
    (rowMap : rows.map source = sourceRows.map some)
    (wordMap : word.map source = sourceWord.map some) :
    sourceFrameWord sourceRows sourceWord := by
  have rowLength := congrArg List.length rowMap
  simp only [List.length_map] at rowLength
  intro index event atEvent
  have mapped : (word.map source)[index]? = some (some event) := by
    rw [wordMap]
    simp only [List.getElem?_map, atEvent, Option.map_some]
  cases rowAt : word[index]? with
  | none => simp only [List.getElem?_map, rowAt, Option.map_none] at mapped; contradiction
  | some row =>
    have sourceAt : source row = some event := by
      simpa only [List.getElem?_map, rowAt, Option.map_some, Option.some.injEq] using mapped
    have position := ordered index row rowAt
    have mappedRow : (rows.map source)[index % rows.length]? = some (some event) := by
      simp only [List.getElem?_map, position, Option.map_some, sourceAt]
    rw [rowMap, rowLength] at mappedRow
    cases nextAt : sourceRows[index % sourceRows.length]? with
    | none => simp only [List.getElem?_map, nextAt, Option.map_none] at mappedRow; contradiction
    | some actual =>
      have same : actual = event := by
        simpa only [List.getElem?_map, nextAt, Option.map_some, Option.some.injEq] using mappedRow
      simp only [same]

/-- A sending group's actual accepted source occurrences follow the immutable
source cycle. The accepted word is derived from the chronology, not assumed
to be ordered. Classification is over finite source columns of every actor;
it identifies this selector with this sending group and contains no visits,
trace positions, whole-actor stamps or prospective receive-pair premise. -/
theorem a_distributed_call_send_group_derives_its_accepted_source_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo} {sourceRows : List Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (distinct : (images actor).sourceRowIdentityCheck = true)
    (sending : ∀ event ∈ group.rows, (images actor).sourceRowSide? event = some .send)
    (selected : Nat → Bool)
    (classifies : ∀ owner row eff, (images owner).role = owner → row < (images owner).eventCount →
      (images owner).eventEffIndex? row = some eff → (images owner).sourceRowSide? row = some .send →
      selected eff = (owner == actor && decide (row ∈ group.rows)))
    (cycle : group.rows.map (images actor).eventEffIndex? = sourceRows.map some) :
    sourceFrameWord sourceRows
      (entries.filterMap fun entry => if selected entry.event then some entry.event else none) := by
  have ordered := a_distributed_call_execution_has_its_acceptance_inclusive_source_group_word execution initial distinct
  change sourceFrameWord group.rows (callLogicalWord group.rows actor word) at ordered
  rw [a_call_word_selects_its_actual_send_rows execution group.rows sending] at ordered
  apply an_exact_partial_map_preserves_a_source_word ordered cycle
  simp only [List.map_filterMap, actorAcceptedSends, List.filterMap_filter]
  have mapped : entries.map (fun entry =>
      if entry.sample.image.role == actor then
        (if entry.sample.event ∈ group.rows then some entry.sample.event else none).map (images actor).eventEffIndex?
      else none) = entries.map (fun entry =>
        (if selected entry.event then some entry.event else none).map some) := by
    apply List.map_congr_left
    intro entry member
    have facts := a_distributed_call_execution_retains_each_accepted_source execution entry member
    obtain ⟨binding, identity⟩ := a_checked_source_send_has_its_sending_identity facts.2.1 facts.2.2
    have columns := a_source_identity_has_its_actual_row_columns identity
    have roleAt := (congrArg RustDescriptorImage.role facts.1).symm
    rw [facts.1] at columns
    have classified := classifies entry.sample.image.role entry.sample.event entry.event roleAt
      columns.1 columns.2.1 columns.2.2
    by_cases owns : entry.sample.image.role = actor
    · rw [owns] at columns
      by_cases inside : entry.sample.event ∈ group.rows <;>
        simp [classified, owns, inside, columns.2.1]
    · simp [classified, owns]
  have result := congrArg (List.filterMap id) mapped
  simpa only [List.filterMap_map, Function.comp_def, id_eq, apply_ite, Option.map_none, Option.map_some] using result

/-- Classify actual physical acceptances by immutable source rows. This covers
arbitrary predicates and any set of physical actors, without a common visit or
sender-publication requirement. It does not classify frames from payload bytes. -/
theorem a_distributed_call_acceptance_predicate_has_its_static_row_classification
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (selectedSource : Nat → Bool) (selectedRow : Nat → Nat → Bool)
    (classifies : ∀ owner row eff, (images owner).role = owner → row < (images owner).eventCount →
      (images owner).eventEffIndex? row = some eff → (images owner).sourceRowSide? row = some .send →
      selectedSource eff = selectedRow owner row) :
    (entries.map SourceAcceptance.event).countP selectedSource =
      entries.countP (fun entry => selectedRow entry.sample.image.role entry.sample.event) := by
  simp only [List.countP_map]
  apply List.countP_congr
  intro entry member
  have facts := a_distributed_call_execution_retains_each_accepted_source execution entry member
  obtain ⟨binding, identity⟩ := a_checked_source_send_has_its_sending_identity facts.2.1 facts.2.2
  have columns := a_source_identity_has_its_actual_row_columns identity
  have roleAt := (congrArg RustDescriptorImage.role facts.1).symm
  rw [facts.1] at columns
  have classified := classifies entry.sample.image.role entry.sample.event entry.event roleAt
    columns.1 columns.2.1 columns.2.2
  change selectedSource entry.event = true ↔ selectedRow entry.sample.image.role entry.sample.event = true
  rw [classified]

end Hibana
