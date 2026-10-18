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

private theorem a_checked_record_has_its_source_identity
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat} {record : ActorCommitRecord}
    (checked : record.check image choreo session = true) :
    ∃ eff binding side, sourceOperationIdentity? image choreo record.step.pathIndex record.step.event =
      some (eff, binding, side) := by
  obtain ⟨_, data, path, row, entry, decoded, pathAt, rowAt, _, _, _, entryAt,
    _, _, _, _, _, _, _, publication⟩ :=
    accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
  obtain ⟨eff, effectAt, _⟩ := Option.bind_eq_some_iff.mp publication
  refine ⟨eff, entry.2, row.actionId.side, ?_⟩
  simp only [sourceOperationIdentity?, Option.bind_eq_bind, decoded, pathAt, rowAt, entryAt,
    effectAt, Option.bind_some, Option.pure_def]

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
    · obtain ⟨eff, binding, side, identity⟩ := a_checked_record_has_its_source_identity (checked _ present)
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

/-- Each actor's actual source word includes its at most one physically
accepted but unpublished send. Its send-phase counts are the actual accepted
send counts, and its receive-phase counts are the actual committed receive
counts. The original stamp and mutation lease establish this refinement;
there is no second runtime cursor, visit counter, owner map or publication
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
    · obtain ⟨actual, binding, side, identity⟩ := a_checked_record_has_its_source_identity (checks _ present)
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
  obtain ⟨eff, binding, side, identity⟩ := a_checked_record_has_its_source_identity (checks entry present)
  have columns := a_source_identity_has_its_actual_row_columns identity
  have classified := classifies entry.1 entry.2.step.event eff side (roles entry present)
    columns.1 columns.2.1 columns.2.2
  cases side <;> simp only [identity, reduceCtorEq, if_false, if_true, Option.map_none,
    Option.map_some, Option.getD_none, Option.getD_some] at classified ⊢ <;>
      simp only [← classified, Bool.false_eq_true, iff_self]

end Hibana
