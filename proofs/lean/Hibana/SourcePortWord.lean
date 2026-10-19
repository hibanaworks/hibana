import Hibana.SourceReceiptWord
import Hibana.SourceHandoffWord

namespace Hibana
open EndpointDelegation

private theorem a_source_execution_preserves_each_opened_direction
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records) :
    ∀ channel, (last.1.carriers channel).channel = channel := by
  induction execution with
  | refl => intro channel; rfl
  | @tail middle finish records logged history step ih =>
    cases step with
    | quiet checked => exact ih
    | publish member stamp paired committed => exact ih
    | localCommit actorRole localAt available committed => exact ih
    | @accept cut entries sample changed sent event previousPath witness label data path row
        imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
      intro channel
      by_cases same : channel = changed
      · subst changed
        unfold TransportState.send at accepted
        split at accepted
        · contradiction
        · have sameState := Option.some.inj accepted
          subst sent
          simpa only [DistributedSourceCut.carrierAt, if_pos rfl] using ih channel
      · simpa only [DistributedSourceCut.carrierAt, if_neg same] using ih channel
    | @receive cut entries actor changed next frame after cleared record entry witness
        actorRole received entryAt identity paired available committed =>
      intro channel
      by_cases same : channel = changed
      · subst changed
        simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_pos rfl]
          using (transport_receive_is_fifo received).2.1.trans (ih channel)
      · simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg same]
          using ih channel

/-- Accepted observations are bound to the opened logical direction. The
carrier-instance generation is deliberately not inferred from wire bytes. -/
def SourceAcceptance.boundToPort (entry : SourceAcceptance) : Prop :=
  entry.sample.wire.session = entry.channel.session ∧
  entry.sample.wire.lane = entry.channel.lane ∧
  entry.sample.wire.sender = entry.channel.sender ∧
  entry.sample.wire.receiver = entry.channel.receiver

/-- The same complete history supplies both source provenance and actual
port binding for every accepted frame, including an unpublished send. -/
theorem a_distributed_acceptance_retains_its_source_and_opened_port
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records) :
    ∀ entry ∈ last.2, entry.sample.image = images entry.sample.image.role ∧
      entry.sample.check choreo session = true ∧ entry.sample.effIndex? = some entry.event ∧
      entry.boundToPort := by
  induction execution with
  | refl => simp
  | @tail middle finish records logged history step ih =>
    cases step with
    | quiet checked => exact ih
    | publish member stamp paired committed => exact ih
    | localCommit actorRole localAt available committed => exact ih
    | receive actorRole received entryAt identity paired available committed => exact ih
    | @accept cut entries sample channel sent event previousPath witness label data path row
        imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
      intro entry member
      rcases List.mem_append.mp member with old | new
      · exact ih entry old
      · cases List.mem_singleton.mp new
        have port := a_source_execution_preserves_each_opened_direction history channel
        change (cut.carriers channel).channel = channel at port
        unfold OwnedWireObservation.identifies at identity
        dsimp only at identity
        rw [port] at identity
        exact ⟨imageAt, checked, sourceAt,
          identity.1, identity.2.1, identity.2.2.1, identity.2.2.2.1⟩

private theorem a_selected_acceptance_word_keeps_its_complete_port_subsequence
    {entries : List SourceAcceptance} {channel : TransportChannel} {selected : Nat → Bool}
    (bound : ∀ entry ∈ entries, selected entry.event = true → entry.channel = channel) :
    (sourceChannelEntries entries channel).filter (fun entry => selected entry.event) =
      entries.filter (fun entry => selected entry.event) := by
  unfold sourceChannelEntries
  rw [List.filter_filter]
  apply List.filter_congr
  intro entry member
  cases chosen : selected entry.event with
  | false => simp only [Bool.false_and]
  | true => simp only [bound entry member chosen, decide_true, Bool.true_and]

private theorem a_selected_element_has_its_filtered_prefix_position
    {α : Type} {entries : List α} {index : Nat} {entry : α} {selected : α → Bool}
    (atEntry : entries[index]? = some entry) (chosen : selected entry = true) :
    (entries.filter selected)[((entries.take index).filter selected).length]? = some entry := by
  induction entries generalizing index with
  | nil => simp only [List.getElem?_nil] at atEntry; cases atEntry
  | cons head tail ih =>
    cases index with
    | zero =>
      have same : head = entry := Option.some.inj atEntry
      subst head
      simp only [List.filter_cons, chosen, if_true, List.take_zero, List.filter_nil,
        List.length_nil, List.getElem?_cons_zero]
    | succ index =>
      have rest : tail[index]? = some entry := atEntry
      have next := ih rest
      cases headSelected : selected head <;>
        simpa only [List.filter_cons, headSelected, Bool.false_eq_true, if_false, if_true,
          List.take_succ_cons, List.length_cons, List.getElem?_cons_succ] using next

private theorem receipt_prefixes_contain_only_actual_acceptances
    {receipts entries : List SourceAcceptance} {cut : DistributedSourceCut}
    (prefixes : ∀ channel, sourceChannelEntries receipts channel =
      (sourceChannelEntries entries channel).take (cut.carriers channel).delivered) :
    ∀ entry ∈ receipts, entry ∈ entries := by
  intro entry member
  have own : entry ∈ sourceChannelEntries receipts entry.channel :=
    List.mem_filter.mpr ⟨member, by simp⟩
  rw [prefixes entry.channel] at own
  exact (List.mem_filter.mp (List.mem_of_mem_take own)).1

/-- A prospective raw head is at the number of actual past receives in its
complete wire-selector class. The supplied classification concerns static
source rows, opened port identity and wire-visible colors, not the expected
source order. Other colors on the same FIFO remain in the raw history and
contribute only when selected. No frame is skipped or received speculatively.
No current receive-pair or successful receiver snapshot is assumed. -/
theorem a_prospective_port_head_has_its_actual_source_class_position
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame} (selected colors : Nat → Bool)
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records)
    (received : (last.1.carriers channel).pollReceive = .delivered frame next)
    (classifies : ∀ entry ∈ last.2,
      (decide (entry.channel = channel) && colors entry.sample.wire.frameLabel) = selected entry.event)
    (visible : colors frame.frameLabel.val = true) :
    ∃ sample event history key,
      SendAdmissionTrace choreo session (last.1.carriers channel) history (sourceChannelRecords last.2 channel) ∧
      history.accepted[(last.1.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords last.2 channel)[(last.1.carriers channel).delivered]? = some (sample, event) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some event ∧
      sample.wire.identifies frame ∧ key.globalId = event ∧
      (last.2.filterMap (fun entry => if selected entry.event then some entry.event else none))[
        (receivedSourceWord images choreo records).countP selected]? = some event ∧
      selected event = true := by
  obtain ⟨receipts, word, prefixes⟩ := a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix execution
  obtain ⟨history, trace⟩ := a_distributed_source_execution_projects_its_complete_fifo_histories execution channel
  obtain ⟨sample, event, key, _, recordAt, keyAt, checked, sourceAt, identity, sourceKey, _, _, _⟩ :=
    a_fifo_receipt_reconstructs_its_source_occurrence_without_a_shared_visit trace received
  have onlyPort : ∀ entry ∈ last.2, selected entry.event = true → entry.channel = channel := by
    intro entry member chosen
    exact of_decide_eq_true (Bool.and_eq_true_iff.mp ((classifies entry member).trans chosen)).1
  have receiptPort : ∀ entry ∈ receipts, selected entry.event = true → entry.channel = channel :=
    fun entry member => onlyPort entry (receipt_prefixes_contain_only_actual_acceptances prefixes entry member)
  have position : (receivedSourceWord images choreo records).countP selected =
      (((sourceChannelEntries last.2 channel).take (last.1.carriers channel).delivered).filter
        (fun entry => selected entry.event)).length := by
    rw [word, List.countP_map, List.countP_eq_length_filter]
    simp only [Function.comp_def]
    rw [
      ← a_selected_acceptance_word_keeps_its_complete_port_subsequence receiptPort, prefixes channel]
  have rawAt : ∃ entry,
      (sourceChannelEntries last.2 channel)[(last.1.carriers channel).delivered]? = some entry ∧
        entry.sample = sample ∧ entry.event = event := by
    simp only [sourceChannelRecords, List.getElem?_map] at recordAt
    cases actual : (sourceChannelEntries last.2 channel)[(last.1.carriers channel).delivered]? with
    | none => simp only [actual, Option.map_none] at recordAt; contradiction
    | some entry =>
      have same := Option.some.inj (by simpa only [actual, Option.map_some] using recordAt)
      exact ⟨entry, rfl, congrArg Prod.fst same, congrArg Prod.snd same⟩
  obtain ⟨entry, entryAt, sampleSame, eventSame⟩ := rawAt
  have member := List.mem_filter.mp (List.mem_of_getElem? entryAt)
  have selectedHead : selected entry.event = true := by
    rw [← classifies entry member.1, member.2, Bool.true_and, sampleSame,
      identity.2.2.2.2, visible]
  have nextEntry := a_selected_element_has_its_filtered_prefix_position
    (selected := fun entry : SourceAcceptance => selected entry.event) entryAt selectedHead
  have nextSource := congrArg (Option.map SourceAcceptance.event) nextEntry
  simp only [← List.getElem?_map, Option.map_some, eventSame] at nextSource
  refine ⟨sample, event, history, key, trace, keyAt, recordAt, checked, sourceAt,
    identity, sourceKey, ?_, by simpa only [eventSame] using selectedHead⟩
  rw [position]
  rw [a_selected_acceptance_word_keeps_its_complete_port_subsequence onlyPort] at nextSource
  simpa only [← List.filterMap_eq_map', List.filterMap_filter] using nextSource

/-- Compose the accepted logical source word with the receiver's independent
current/fresh preparation. Its next source identity is derived before any
receive commit. The accepted word and exact receipt count are intermediate
history invariants, not a per-frame correspondence premise. -/
theorem a_prospective_source_word_head_matches_its_prepared_receiver
    {images : Nat → RustDescriptorImage} {choreo : Choreo}
    {session actor previousPath pathIndex event : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame}
    {group : SourceOrderGroup (images actor) choreo} {sourceRows : List Nat}
    {base : ActorProgressObservation} {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    (selected colors : Nat → Bool)
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records)
    (empty : ∀ event ∈ group.rows, (initial actor).1.doneState.done event = false)
    (decoded : (images actor).decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path)
    (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := (last.1.actors actor).1, pathIndex, admitted := [] }
      (images actor) choreo previousPath path row base)
    (member : event ∈ group.rows)
    (received : (last.1.carriers channel).pollReceive = .delivered frame next)
    (classifies : ∀ entry ∈ last.2,
      (decide (entry.channel = channel) && colors entry.sample.wire.frameLabel) = selected entry.event)
    (visible : colors frame.frameLabel.val = true)
    (cycle : sourceRows.map some = group.rows.map (images actor).eventEffIndex?)
    (sentOrdered : sourceFrameWord sourceRows
      (last.2.filterMap fun entry => if selected entry.event then some entry.event else none))
    (receiptCount : (receivedSourceWord images choreo records).countP selected =
      (committedFrameClassWord group.rows actor records).length) :
    ∃ sample source history key,
      SendAdmissionTrace choreo session (last.1.carriers channel) history (sourceChannelRecords last.2 channel) ∧
      history.accepted[(last.1.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords last.2 channel)[(last.1.carriers channel).delivered]? = some (sample, source) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some source ∧
      sample.wire.identifies frame ∧ key.globalId = source ∧
      (images actor).eventEffIndex? event = some source := by
  obtain ⟨receiverOrdered, _⟩ := a_prepared_wire_operation_extends_its_actors_measured_source_word
    execution empty decoded pathAt rowAt active ready origin member
  obtain ⟨sample, source, history, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, nextAt, _⟩ :=
    a_prospective_port_head_has_its_actual_source_class_position selected colors execution received classifies visible
  rw [receiptCount] at nextAt
  have receiverAt : (committedFrameClassWord group.rows actor records ++ [event])[
      (committedFrameClassWord group.rows actor records).length]? = some event := by simp
  have matched := source_frame_words_match_at_each_common_class_position cycle sentOrdered receiverOrdered nextAt receiverAt
  exact ⟨sample, source, history, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, matched.symm⟩

/-- Locate the next raw FIFO head in its complete selector class using the
asynchronous call chronology. Earlier receipts provide its ordinal; the
prospective receive needs no pair check or successful after-snapshot. -/
theorem a_prospective_call_port_head_has_its_actual_source_class_position
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {boundaries : List DistributedCallBoundary} {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame} (selected colors : Nat → Bool)
    (execution : DistributedCallExecution images choreo session
      (.initial initial) last boundaries entries published pending)
    (received : (last.carriers channel).pollReceive = .delivered frame next)
    (classifies : ∀ entry ∈ entries,
      (decide (entry.channel = channel) && colors entry.sample.wire.frameLabel) = selected entry.event)
    (visible : colors frame.frameLabel.val = true) :
    ∃ sample event history key,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) ∧
      history.accepted[(last.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords entries channel)[(last.carriers channel).delivered]? = some (sample, event) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some event ∧
      sample.wire.identifies frame ∧ key.globalId = event ∧
      (entries.filterMap (fun entry => if selected entry.event then some entry.event else none))[
        (receivedSourceWord images choreo (boundaries.filterMap DistributedCallBoundary.record?)).countP selected]? = some event ∧
      selected event = true := by
  obtain ⟨receipts, word, prefixes⟩ := a_distributed_call_receipt_word_is_the_exact_accepted_fifo_prefix execution
  obtain ⟨history, trace⟩ := a_distributed_call_execution_projects_its_complete_fifo_histories execution channel
  obtain ⟨sample, event, key, _, recordAt, keyAt, checked, sourceAt, identity, sourceKey, _, _, _⟩ :=
    a_fifo_receipt_reconstructs_its_source_occurrence_without_a_shared_visit trace received
  have onlyPort : ∀ entry ∈ entries, selected entry.event = true → entry.channel = channel := by
    intro entry member chosen
    exact of_decide_eq_true (Bool.and_eq_true_iff.mp ((classifies entry member).trans chosen)).1
  have receiptPort : ∀ entry ∈ receipts, selected entry.event = true → entry.channel = channel :=
    fun entry member => onlyPort entry (receipt_prefixes_contain_only_actual_acceptances prefixes entry member)
  have position : (receivedSourceWord images choreo (boundaries.filterMap DistributedCallBoundary.record?)).countP selected =
      (((sourceChannelEntries entries channel).take (last.carriers channel).delivered).filter
        (fun entry => selected entry.event)).length := by
    rw [word, List.countP_map, List.countP_eq_length_filter]
    simp only [Function.comp_def]
    rw [← a_selected_acceptance_word_keeps_its_complete_port_subsequence receiptPort, prefixes channel]
  have rawAt : ∃ entry,
      (sourceChannelEntries entries channel)[(last.carriers channel).delivered]? = some entry ∧
        entry.sample = sample ∧ entry.event = event := by
    simp only [sourceChannelRecords, List.getElem?_map] at recordAt
    cases actual : (sourceChannelEntries entries channel)[(last.carriers channel).delivered]? with
    | none => simp only [actual, Option.map_none] at recordAt; contradiction
    | some entry =>
      have same := Option.some.inj (by simpa only [actual, Option.map_some] using recordAt)
      exact ⟨entry, rfl, congrArg Prod.fst same, congrArg Prod.snd same⟩
  obtain ⟨entry, entryAt, sampleSame, eventSame⟩ := rawAt
  have member := List.mem_filter.mp (List.mem_of_getElem? entryAt)
  have selectedHead : selected entry.event = true := by
    rw [← classifies entry member.1, member.2, Bool.true_and, sampleSame,
      identity.2.2.2.2, visible]
  have nextEntry := a_selected_element_has_its_filtered_prefix_position
    (selected := fun entry : SourceAcceptance => selected entry.event) entryAt selectedHead
  have nextSource := congrArg (Option.map SourceAcceptance.event) nextEntry
  simp only [← List.getElem?_map, Option.map_some, eventSame] at nextSource
  refine ⟨sample, event, history, key, trace, keyAt, recordAt, checked, sourceAt,
    identity, sourceKey, ?_, by simpa only [eventSame] using selectedHead⟩
  rw [position]
  rw [a_selected_acceptance_word_keeps_its_complete_port_subsequence onlyPort] at nextSource
  simpa only [← List.filterMap_eq_map', List.filterMap_filter] using nextSource

/-- The next raw head agrees with the independently prepared receiver's source
identity in the asynchronous call model. Its ordinal comes from actual past
receipts, not sender publication. Static cycle classification and the accepted
word/count invariants are explicit inputs; a current receive-pair is not. -/
theorem a_prospective_call_source_word_head_matches_its_prepared_receiver
    {images : Nat → RustDescriptorImage} {choreo : Choreo}
    {session actor previousPath pathIndex event : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {boundaries : List DistributedCallBoundary} {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame}
    {group : SourceOrderGroup (images actor) choreo} {sourceRows : List Nat}
    {base : ActorProgressObservation} {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    (selected colors : Nat → Bool)
    (execution : DistributedCallExecution images choreo session
      (.initial initial) last boundaries entries published pending)
    (empty : ∀ event ∈ group.rows, (initial actor).1.doneState.done event = false)
    (decoded : (images actor).decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path)
    (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := (last.actors actor).1, pathIndex, admitted := [] }
      (images actor) choreo previousPath path row base)
    (member : event ∈ group.rows)
    (received : (last.carriers channel).pollReceive = .delivered frame next)
    (classifies : ∀ entry ∈ entries,
      (decide (entry.channel = channel) && colors entry.sample.wire.frameLabel) = selected entry.event)
    (visible : colors frame.frameLabel.val = true)
    (cycle : sourceRows.map some = group.rows.map (images actor).eventEffIndex?)
    (sentOrdered : sourceFrameWord sourceRows
      (entries.filterMap fun entry => if selected entry.event then some entry.event else none))
    (receiptCount : (receivedSourceWord images choreo
        (boundaries.filterMap DistributedCallBoundary.record?)).countP selected =
      (committedFrameClassWord group.rows actor (boundaries.filterMap DistributedCallBoundary.record?)).length) :
    ∃ sample source history key,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) ∧
      history.accepted[(last.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords entries channel)[(last.carriers channel).delivered]? = some (sample, source) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some source ∧
      sample.wire.identifies frame ∧ key.globalId = source ∧
      (images actor).eventEffIndex? event = some source := by
  obtain ⟨receiverOrdered, _⟩ := a_prepared_call_operation_has_its_next_committed_source_letter
    execution empty decoded pathAt rowAt active ready origin member
  obtain ⟨sample, source, history, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, nextAt, _⟩ :=
    a_prospective_call_port_head_has_its_actual_source_class_position selected colors execution received classifies visible
  rw [receiptCount] at nextAt
  have receiverAt : (committedFrameClassWord group.rows actor
      (boundaries.filterMap DistributedCallBoundary.record?) ++ [event])[
        (committedFrameClassWord group.rows actor (boundaries.filterMap DistributedCallBoundary.record?)).length]? = some event := by simp
  have matched := source_frame_words_match_at_each_common_class_position cycle sentOrdered receiverOrdered nextAt receiverAt
  exact ⟨sample, source, history, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, matched.symm⟩

/-- Derive the prospective head/receiver source match without assuming the
accepted word is ordered or supplying a trace-dependent receipt count. Both
come from the same asynchronous execution and immutable row classifications.

This composition identifies one sending group with one receiving group. Other
independent work, overlapping calls and holder changes remain in the history;
combining a selector spread over several physical-holder groups is a further
custody/continuity obligation. No shared visit or current pair is assumed. -/
theorem a_prospective_call_head_matches_its_statically_classified_receiver
    {images : Nat → RustDescriptorImage} {choreo : Choreo}
    {session sender actor previousPath pathIndex event : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {boundaries : List DistributedCallBoundary} {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame}
    {sentGroup : SourceOrderGroup (images sender) choreo}
    {group : SourceOrderGroup (images actor) choreo} {sourceRows : List Nat}
    {base : ActorProgressObservation} {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    (selected colors : Nat → Bool)
    (execution : DistributedCallExecution images choreo session
      (.initial initial) last boundaries entries published pending)
    (emptySent : ∀ event ∈ sentGroup.rows, (initial sender).1.doneState.done event = false)
    (distinctSent : (images sender).sourceRowIdentityCheck = true)
    (sending : ∀ event ∈ sentGroup.rows, (images sender).sourceRowSide? event = some .send)
    (sendClassifies : ∀ owner row eff, (images owner).role = owner → row < (images owner).eventCount →
      (images owner).eventEffIndex? row = some eff → (images owner).sourceRowSide? row = some .send →
      selected eff = (owner == sender && decide (row ∈ sentGroup.rows)))
    (sentCycle : sentGroup.rows.map (images sender).eventEffIndex? = sourceRows.map some)
    (receiveClassifies : ∀ owner row eff side, (images owner).role = owner → row < (images owner).eventCount →
      (images owner).eventEffIndex? row = some eff → (images owner).sourceRowSide? row = some side →
      (if side = .receive then selected eff else false) = (owner == actor && decide (row ∈ group.rows)))
    (empty : ∀ event ∈ group.rows, (initial actor).1.doneState.done event = false)
    (decoded : (images actor).decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path)
    (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := (last.actors actor).1, pathIndex, admitted := [] }
      (images actor) choreo previousPath path row base)
    (member : event ∈ group.rows)
    (received : (last.carriers channel).pollReceive = .delivered frame next)
    (portClassifies : ∀ entry ∈ entries,
      (decide (entry.channel = channel) && colors entry.sample.wire.frameLabel) = selected entry.event)
    (visible : colors frame.frameLabel.val = true)
    (cycle : sourceRows.map some = group.rows.map (images actor).eventEffIndex?) :
    ∃ sample source history key,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) ∧
      history.accepted[(last.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords entries channel)[(last.carriers channel).delivered]? = some (sample, source) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some source ∧
      sample.wire.identifies frame ∧ key.globalId = source ∧
      (images actor).eventEffIndex? event = some source := by
  have sentOrdered := a_distributed_call_send_group_derives_its_accepted_source_word
    execution emptySent distinctSent sending selected sendClassifies sentCycle
  have receiptCount := a_distributed_call_receipt_source_count_has_its_static_row_classification execution selected
    (fun owner row => owner == actor && decide (row ∈ group.rows)) receiveClassifies
  have exactLength : (boundaries.filterMap DistributedCallBoundary.record?).countP
      (fun entry => entry.1 == actor && decide (entry.2.step.event ∈ group.rows)) =
        (committedFrameClassWord group.rows actor (boundaries.filterMap DistributedCallBoundary.record?)).length := by
    generalize recordsAt : boundaries.filterMap DistributedCallBoundary.record? = records
    simp only [committedFrameClassWord, List.length_filterMap_eq_countP]
    apply List.countP_congr
    intro entry member
    by_cases owns : entry.1 = actor <;> by_cases inside : entry.2.step.event ∈ group.rows <;>
      simp [owns, inside]
  rw [exactLength] at receiptCount
  exact a_prospective_call_source_word_head_matches_its_prepared_receiver
    selected colors execution empty decoded pathAt rowAt active ready origin member received
      portClassifies visible cycle sentOrdered receiptCount

end Hibana
