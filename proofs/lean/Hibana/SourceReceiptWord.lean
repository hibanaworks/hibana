import Hibana.SourceActorWord
import Hibana.FifoOccurrenceOrigin
import Init.Data.List.Count

namespace Hibana
open EndpointDelegation

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

private theorem a_source_identity_retains_its_effect
    {image : RustDescriptorImage} {choreo : Choreo} {pathIndex event eff : Nat}
    {binding : Binding} {side : Side}
    (identity : sourceOperationIdentity? image choreo pathIndex event = some (eff, binding, side)) :
    image.eventEffIndex? event = some eff := by
  unfold sourceOperationIdentity? at identity
  simp only [bind, pure, Option.bind] at identity
  cases decoded : image.decodeActorOrder? choreo with
  | none => simp [decoded] at identity
  | some data =>
    cases pathAt : data.normalized.paths[pathIndex]? with
    | none => simp [decoded, pathAt] at identity
    | some path =>
      cases rowAt : data.rows[event]? with
      | none => simp [decoded, pathAt, rowAt] at identity
      | some row =>
        cases entryAt : path.bindings.find? (fun entry => entry.1 == row.actionId.event) with
        | none => simp [decoded, pathAt, rowAt, entryAt] at identity
        | some entry =>
          cases effectAt : image.eventEffIndex? event with
          | none => simp [decoded, pathAt, rowAt, entryAt, effectAt] at identity
          | some actual =>
            have same : actual = eff := by
              simpa [decoded, pathAt, rowAt, entryAt, effectAt] using congrArg (Option.map Prod.fst) identity
            exact congrArg some same

private theorem received_source_word_append (images : Nat → RustDescriptorImage) (choreo : Choreo)
    (left right : List (Nat × ActorCommitRecord)) :
    receivedSourceWord images choreo (left ++ right) =
      receivedSourceWord images choreo left ++ receivedSourceWord images choreo right := by
  exact List.filterMap_append

private theorem a_publication_adds_no_receipt_letter
    {sample : SourceSendAdmission} {record : ActorCommitRecord}
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    (paired : sourcePublicationPairCheck sample record (images actor) choreo session = true) :
    receivedSourceWord images choreo [(actor, record)] = [] := by
  unfold sourcePublicationPairCheck at paired
  have matched := (Bool.and_eq_true_iff.mp paired).2
  cases leftAt : sourceOperationIdentity? sample.image choreo sample.pathIndex sample.event with
  | none => simp [leftAt] at matched
  | some left =>
    rcases left with ⟨eff, binding, side⟩
    cases side <;> simp only [leftAt] at matched
    all_goals
      cases rightAt : sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event with
      | none => simp [rightAt] at matched
      | some right =>
        rcases right with ⟨other, binding', side'⟩
        cases side' <;> simp_all [receivedSourceWord]

private theorem a_paired_receipt_adds_its_accepted_effect
    {sample : SourceSendAdmission} {record : ActorCommitRecord}
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    (paired : SourceReceivePair sample record (images actor) choreo session)
    (sourceAt : sample.effIndex? = some event) :
    receivedSourceWord images choreo [(actor, record)] = [event] := by
  obtain ⟨eff, _, _, sent, received⟩ := a_corresponding_receive_pair_has_its_original_source_halves paired
  have same : eff = event := Option.some.inj ((a_source_identity_retains_its_effect sent).symm.trans sourceAt)
  simp only [receivedSourceWord, List.filterMap_cons, List.filterMap_nil, received, same]

private theorem an_execution_keeps_each_acceptances_actual_source_effect
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : ∀ entry ∈ first.2, entry.sample.effIndex? = some entry.event) :
    ∀ entry ∈ last.2, entry.sample.effIndex? = some entry.event := by
  induction execution with
  | refl => exact initial
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
        exact sourceAt

private theorem receipt_channel_entries_append
    (entries : List SourceAcceptance) (entry : SourceAcceptance) (channel : TransportChannel) :
    sourceChannelEntries (entries ++ [entry]) channel = sourceChannelEntries entries channel ++
      (if entry.channel = channel then [entry] else []) := by
  simp only [sourceChannelEntries, List.filter_append, List.filter_cons, List.filter_nil]
  split <;> simp_all

private theorem a_received_entry_has_its_recorded_channel
    {entries : List SourceAcceptance} {channel : TransportChannel} {index : Nat} {entry : SourceAcceptance}
    (atEntry : (sourceChannelEntries entries channel)[index]? = some entry) : entry.channel = channel := by
  exact of_decide_eq_true (List.mem_filter.mp (List.mem_of_getElem? atEntry)).2

private theorem a_fifo_receipt_extends_exactly_its_delivered_prefix
    {entries : List SourceAcceptance} {channel : TransportChannel} {index : Nat} {entry : SourceAcceptance}
    (atEntry : (sourceChannelEntries entries channel)[index]? = some entry) :
    (sourceChannelEntries entries channel).take (index + 1) =
      (sourceChannelEntries entries channel).take index ++ [entry] := by
  obtain ⟨bound, atIndex⟩ := List.getElem?_eq_some_iff.mp atEntry
  simpa only [atIndex] using List.take_succ_eq_append_getElem bound

/-- The entire source receipt word is reconstructed from the one real
boundary history. For each physical direction it is exactly the delivered
prefix of the complete acceptance list, even when a logical sender changes
physical holder or actors have unrelated branch/roll histories.

Only earlier receive-pair correspondence already present in `execution` is
used. This does not assume a pair check for a new prospective receive, equate
actor visit counters, or require sender publication before delivery. -/
theorem a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records) :
    ∃ receipts : List SourceAcceptance,
      receivedSourceWord images choreo records = receipts.map SourceAcceptance.event ∧
      ∀ channel, sourceChannelEntries receipts channel =
        (sourceChannelEntries last.2 channel).take (last.1.carriers channel).delivered := by
  induction execution with
  | refl => exact ⟨[], rfl, fun _ => rfl⟩
  | @tail middle finish records logged history step ih =>
    obtain ⟨receipts, word, prefixes⟩ := ih
    have effects := an_execution_keeps_each_acceptances_actual_source_effect history (by simp)
    cases step with
    | @quiet cut entries actor after checked => exact ⟨receipts, by simpa using word, prefixes⟩
    | @accept cut entries sample changed sent event previousPath witness label data path row
        imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
      refine ⟨receipts, by simpa using word, ?_⟩
      intro channel
      by_cases same : channel = changed
      · subst changed
        obtain ⟨admissions, trace⟩ := a_distributed_source_execution_projects_its_complete_fifo_histories history channel
        obtain ⟨valid, _, _, _, _, _⟩ := a_checked_source_admission_trace_keeps_exact_frame_and_occurrence_records trace
        have lengthAt := a_checked_source_admission_trace_records_every_frame trace
        have bound : (cut.carriers channel).delivered ≤ (sourceChannelEntries entries channel).length := by
          have sameLength : (sourceChannelEntries entries channel).length = (cut.carriers channel).frames.length := by
            simpa only [sourceChannelRecords, List.length_map] using lengthAt
          simpa only [← sameLength] using valid.1
        unfold TransportState.send at accepted
        split at accepted
        · contradiction
        · have sentAt := Option.some.inj accepted
          subst sent
          simpa only [DistributedSourceCut.carrierAt, if_pos rfl, receipt_channel_entries_append,
            ite_true, List.take_append_of_le_length bound] using prefixes channel
      · simpa only [DistributedSourceCut.carrierAt, if_neg same, receipt_channel_entries_append,
          if_neg (Ne.symm same), List.append_nil] using prefixes channel
    | @publish cut entries actor after cleared record entry member stamp paired committed =>
      refine ⟨receipts, ?_, prefixes⟩
      simpa only [Option.toList_some, received_source_word_append,
        a_publication_adds_no_receipt_letter paired, List.append_nil] using word
    | @localCommit cut entries actor after cleared record eff binding actorRole localAt available committed =>
      refine ⟨receipts, ?_, prefixes⟩
      change receivedSourceWord images choreo (records ++ [(actor, record)]) = _
      rw [received_source_word_append]
      have noLetter : receivedSourceWord images choreo [(actor, record)] = [] := by
        simp only [receivedSourceWord, List.filterMap_cons, List.filterMap_nil, localAt]
      simpa only [noLetter, List.append_nil] using word
    | @receive cut entries actor changed next frame after cleared record entry witness
        actorRole received entryAt identity paired available committed =>
      have sourceAt := effects entry (List.mem_filter.mp (List.mem_of_getElem? entryAt)).1
      have letter := a_paired_receipt_adds_its_accepted_effect paired sourceAt
      have channelAt := a_received_entry_has_its_recorded_channel entryAt
      have nextAt := transport_receive_is_fifo received
      refine ⟨receipts ++ [entry], ?_, ?_⟩
      · change receivedSourceWord images choreo (records ++ [(actor, record)]) = _
        rw [received_source_word_append, letter, List.map_append]
        exact congrArg (fun values => values ++ [entry.event]) word
      · intro query
        by_cases same : query = changed
        · subst query
          simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_pos rfl,
            receipt_channel_entries_append, channelAt, if_pos rfl, ite_true, nextAt.2.2.2,
            a_fifo_receipt_extends_exactly_its_delivered_prefix entryAt] using
              congrArg (fun values => values ++ [entry]) (prefixes changed)
        · simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg same,
            receipt_channel_entries_append, channelAt, if_neg (Ne.symm same), List.append_nil] using prefixes query

private theorem a_source_fifo_prefix_counts_its_receipt_events
    (entries : List SourceAcceptance) (channel : TransportChannel) (index event : Nat) :
    sourceOccurrenceOrdinal (sourceChannelRecords entries channel) index event =
      (((sourceChannelEntries entries channel).take index).map SourceAcceptance.event).count event := by
  simp only [sourceOccurrenceOrdinal, sourceChannelRecords, ← List.map_take,
    List.count_eq_length_filter, List.filter_map, List.length_map, Function.comp_def]

/-- A new raw FIFO head gets its source occurrence ordinal from the actual
past receipts in the same distributed execution. Its sender may be a different
physical holder from an earlier occurrence, and need not have published.
There is no current receive commit, successful after-snapshot or current
receive-pair premise. Earlier pair correspondence remains explicit in the
history; matching the new receiver's local source row is a separate step. -/
theorem a_prospective_fifo_head_has_its_actual_receipt_word_ordinal
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records)
    (received : (last.1.carriers channel).pollReceive = .delivered frame next) :
    ∃ receipts : List SourceAcceptance, ∃ history : ElasticAdmissionHistory, ∃ sample event key,
      receivedSourceWord images choreo records = receipts.map SourceAcceptance.event ∧
      sourceChannelEntries receipts channel =
        (sourceChannelEntries last.2 channel).take (last.1.carriers channel).delivered ∧
      SendAdmissionTrace choreo session (last.1.carriers channel) history (sourceChannelRecords last.2 channel) ∧
      history.accepted[(last.1.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords last.2 channel)[(last.1.carriers channel).delivered]? = some (sample, event) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some event ∧
      sample.wire.identifies frame ∧ key.globalId = event ∧
      key.iteration = ((sourceChannelEntries receipts channel).map SourceAcceptance.event).count event := by
  obtain ⟨receipts, word, prefixes⟩ := a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix execution
  obtain ⟨history, trace⟩ := a_distributed_source_execution_projects_its_complete_fifo_histories execution channel
  obtain ⟨sample, event, key, following, recordAt, keyAt, checked, sourceAt, identity,
    same, ordinal, ghost, advanced⟩ :=
    a_fifo_receipt_reconstructs_its_source_occurrence_without_a_shared_visit trace received
  refine ⟨receipts, history, sample, event, key, word, prefixes channel, trace, keyAt,
    recordAt, checked, sourceAt, identity, same, ?_⟩
  rw [ordinal, prefixes channel]
  exact a_source_fifo_prefix_counts_its_receipt_events last.2 channel _ event

/-- The general asynchronous chronology reconstructs exactly the delivered
prefix of every full logical direction. Source words include unpublished sends,
while receipt letters come only from actual receives. Independent calls may
publish out of acceptance order without changing this FIFO correspondence. -/
theorem a_distributed_call_receipt_word_is_the_exact_accepted_fifo_prefix
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {word : List DistributedCallBoundary} {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session
      (.initial initial) last word entries published pending) :
    ∃ receipts : List SourceAcceptance,
      receivedSourceWord images choreo (word.filterMap DistributedCallBoundary.record?) =
        receipts.map SourceAcceptance.event ∧
      ∀ channel, sourceChannelEntries receipts channel =
        (sourceChannelEntries entries channel).take (last.carriers channel).delivered := by
  induction execution with
  | refl => exact ⟨[], rfl, fun _ => rfl⟩
  | quiet history checked ih =>
    obtain ⟨receipts, word, prefixes⟩ := ih
    refine ⟨receipts, ?_, prefixes⟩
    simpa [List.filterMap_append, DistributedCallBoundary.record?] using word
  | @accept cut boundaries entries published pending sample changed sent event previousPath witness label data path row
      history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    obtain ⟨receipts, word, prefixes⟩ := ih
    refine ⟨receipts, ?_, ?_⟩
    · simpa [List.filterMap_append, DistributedCallBoundary.record?] using word
    · intro channel
      by_cases same : channel = changed
      · subst changed
        obtain ⟨admissions, trace⟩ := a_distributed_call_execution_projects_its_complete_fifo_histories history channel
        obtain ⟨valid, _, _, _, _, _⟩ := a_checked_source_admission_trace_keeps_exact_frame_and_occurrence_records trace
        have lengthAt := a_checked_source_admission_trace_records_every_frame trace
        have bound : (cut.carriers channel).delivered ≤ (sourceChannelEntries entries channel).length := by
          have sameLength : (sourceChannelEntries entries channel).length = (cut.carriers channel).frames.length := by
            simpa only [sourceChannelRecords, List.length_map] using lengthAt
          simpa only [← sameLength] using valid.1
        unfold TransportState.send at accepted
        split at accepted
        · contradiction
        · have sentAt := Option.some.inj accepted
          subst sent
          simpa only [DistributedSourceCut.carrierAt, if_pos rfl, receipt_channel_entries_append,
            ite_true, List.take_append_of_le_length bound] using prefixes channel
      · simpa only [DistributedSourceCut.carrierAt, if_neg same, receipt_channel_entries_append,
          if_neg (Ne.symm same), List.append_nil] using prefixes channel
  | @publish cut boundaries entries published left right actor after cleared record entry
      history actorAt paired committed ih =>
    obtain ⟨receipts, word, prefixes⟩ := ih
    refine ⟨receipts, ?_, prefixes⟩
    have member : entry ∈ entries :=
      (a_distributed_call_execution_partitions_its_actual_acceptances history).1.mem_iff.mpr
        (List.mem_append.mpr (.inr (by simp)))
    have facts := a_distributed_call_execution_retains_each_accepted_source history entry member
    have imageAt : entry.sample.image = images actor := by simpa only [actorAt] using facts.1
    obtain ⟨_, _, _, _, _, _, sendIdentity, _⟩ := a_retained_publication_preserves_its_source_and_wire paired
    rw [imageAt] at sendIdentity
    simpa [List.filterMap_append, DistributedCallBoundary.record?, received_source_word_append,
      receivedSourceWord, sendIdentity] using word
  | @localCommit cut boundaries entries published pending actor after cleared record eff binding
      history actorRole localAt committed ih =>
    obtain ⟨receipts, word, prefixes⟩ := ih
    refine ⟨receipts, ?_, prefixes⟩
    simpa [List.filterMap_append, DistributedCallBoundary.record?, received_source_word_append,
      receivedSourceWord, localAt] using word
  | @receive cut boundaries entries published pending actor changed next frame after cleared record entry witness
      history actorRole received entryAt identity paired committed ih =>
    obtain ⟨receipts, word, prefixes⟩ := ih
    have sourceAt := (a_distributed_call_execution_retains_each_accepted_source history entry
      (List.mem_filter.mp (List.mem_of_getElem? entryAt)).1).2.2
    have letter := a_paired_receipt_adds_its_accepted_effect paired sourceAt
    have channelAt := a_received_entry_has_its_recorded_channel entryAt
    have nextAt := transport_receive_is_fifo received
    refine ⟨receipts ++ [entry], ?_, ?_⟩
    · simp only [List.filterMap_append, DistributedCallBoundary.record?, List.filterMap_cons,
        List.filterMap_nil, received_source_word_append, letter, List.map_append, List.map_cons, List.map_nil]
      exact congrArg (fun values => values ++ [entry.event]) word
    · intro query
      by_cases same : query = changed
      · subst query
        simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_pos rfl,
          receipt_channel_entries_append, channelAt, if_pos rfl, ite_true, nextAt.2.2.2,
          a_fifo_receipt_extends_exactly_its_delivered_prefix entryAt] using
            congrArg (fun values => values ++ [entry]) (prefixes changed)
      · simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg same,
          receipt_channel_entries_append, channelAt, if_neg (Ne.symm same), List.append_nil] using prefixes query

end Hibana
