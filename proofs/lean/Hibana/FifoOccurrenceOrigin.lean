import Hibana.DistributedSourceExecution

namespace Hibana

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

/-- The ordinal of a source occurrence is recovered from all preceding
physical acceptances. Ordinary frames are retained in this prefix. Neither
actor-local mutation counts nor a common roll visit enter this definition. -/
def sourceOccurrenceOrdinal (records : List (SourceSendAdmission × Nat))
    (index event : Nat) : Nat :=
  ((records.take index).filter (fun record => record.2 == event)).length

private theorem aligned_source_record_lengths
    {records : List (SourceSendAdmission × Nat)} {keys : List ElasticOccurrenceKey}
    (aligned : AlignedRecords (fun record key => key.globalId = record.2) records keys) :
    records.length = keys.length := by
  induction aligned with
  | nil => rfl
  | cons one rest ih => exact congrArg Nat.succ ih

private theorem aligned_source_records_have_the_same_occurrence_count
    {records : List (SourceSendAdmission × Nat)} {keys : List ElasticOccurrenceKey}
    (aligned : AlignedRecords (fun record key => key.globalId = record.2) records keys)
    (event : Nat) :
    (records.filter (fun record => record.2 == event)).length =
      (occurrenceGenerations keys event).length := by
  induction aligned with
  | nil => rfl
  | @cons record key records keys same rest ih =>
    simp only [List.filter_cons, occurrenceGenerations, List.filterMap_cons]
    by_cases matched : record.2 = event
    · simp only [same, matched, beq_self_eq_true, if_true, List.length_cons]
      exact congrArg Nat.succ ih
    · simp only [same, matched, beq_iff_eq, if_false]
      exact ih

/-- Numbering an accepted frame is not an assumption about the receiver's
iteration. It follows from the actual append history and the per-occurrence
range invariant, including when sends run ahead of receives. -/
theorem an_accepted_source_occurrence_has_its_exact_fifo_prefix_ordinal
    {choreo : Choreo} {session : Nat} {transport : TransportState}
    {history : ElasticAdmissionHistory} {records : List (SourceSendAdmission × Nat)}
    (trace : SendAdmissionTrace choreo session transport history records) :
    ∀ index record key,
      records[index]? = some record → history.accepted[index]? = some key →
      key.globalId = record.2 ∧ key.iteration = sourceOccurrenceOrdinal records index record.2 := by
  induction trace with
  | initial channel => intro index record key missing; simp at missing
  | close previous ih => exact ih
  | @accept transport sent history records sample event label previous checked sourceAt accepted identity ih =>
    obtain ⟨_, valid, _, _, frames, aligned⟩ :=
      a_checked_source_admission_trace_keeps_exact_frame_and_occurrence_records previous
    have lengthAligned := aligned_source_record_lengths aligned
    intro index record key recordAt keyAt
    by_cases earlier : index < records.length
    · have oldRecord : records[index]? = some record := by
        simpa only [List.getElem?_append_left earlier] using recordAt
      have oldKey : history.accepted[index]? = some key := by
        simpa only [ElasticAdmissionHistory.appendOccurrence,
          List.getElem?_append_left (lengthAligned ▸ earlier)] using keyAt
      have exactAt := ih index record key oldRecord oldKey
      refine ⟨exactAt.1, ?_⟩
      simpa only [sourceOccurrenceOrdinal, List.take_append_of_le_length (Nat.le_of_lt earlier)] using exactAt.2
    · have bound : index < records.length + 1 := by
        have := (List.getElem?_eq_some_iff.mp recordAt).1
        simpa only [List.length_append, List.length_singleton] using this
      have position : index = records.length := by omega
      subst index
      have recordSame : (sample, event) = record := by simpa using recordAt
      subst record
      have keySame : history.nextOccurrence event = key := by
        simpa only [ElasticAdmissionHistory.appendOccurrence, lengthAligned,
          List.getElem?_append_right (Nat.le_refl _), Nat.sub_self,
          List.getElem?_cons_zero, Option.some.injEq] using keyAt
      subst key
      refine ⟨rfl, ?_⟩
      have counts := aligned_source_records_have_the_same_occurrence_count aligned event
      rw [valid.exactGenerations event, List.length_range] at counts
      simpa only [sourceOccurrenceOrdinal, List.take_left, ElasticAdmissionHistory.nextOccurrence]
        using counts.symm
  | @receive transport next history following records frame key previous received ghost ih =>
    have exactAt := elastic_admission_receive_is_exact_fifo ghost
    simpa only [exactAt.2.2.1] using ih

/-- Reconstruct the source occurrence at an actual FIFO receipt. The receiver
can lag behind arbitrarily many accepted visits, and no sender publication is
required. The ordinal is a count in the complete physical acceptance prefix,
not a comparison of the two actors' counters. -/
theorem a_fifo_receipt_reconstructs_its_source_occurrence_without_a_shared_visit
    {choreo : Choreo} {session : Nat} {transport next : TransportState}
    {history : ElasticAdmissionHistory} {records : List (SourceSendAdmission × Nat)}
    {frame : TransportFrame}
    (trace : SendAdmissionTrace choreo session transport history records)
    (received : transport.pollReceive = .delivered frame next) :
    ∃ sample event key following,
      records[transport.delivered]? = some (sample, event) ∧
      history.accepted[transport.delivered]? = some key ∧
      sample.check choreo session = true ∧ sample.effIndex? = some event ∧
      sample.wire.identifies frame ∧ key.globalId = event ∧
      key.iteration = sourceOccurrenceOrdinal records transport.delivered event ∧
      history.receive? = some (key, following) ∧
      SendAdmissionTrace choreo session next following records := by
  obtain ⟨sample, event, key, following, recordAt, keyAt, checked, sourceAt,
      identity, same, _, _, ghost, after⟩ :=
    a_fifo_receive_has_its_actual_accepted_source_occurrence trace received
  have ordinal := an_accepted_source_occurrence_has_its_exact_fifo_prefix_ordinal trace
    transport.delivered (sample, event) key recordAt keyAt
  exact ⟨sample, event, key, following, recordAt, keyAt, checked, sourceAt,
    identity, same, ordinal.2, ghost, after⟩

/-- Connect a checked owned receipt to the exact accepted source instance,
using preserved bytes of that physical frame. There is no receive-pair check,
common normalized path, shared visit or sender-publication premise here.

The accepted frame must be an owned source send. Proving that a conforming
ordinary/owned mixed receiver always selects the right source kind remains a
separate source-word refinement obligation; this theorem does not assume that
all frames with the same wire label have that kind. -/
theorem a_checked_owned_fifo_receipt_retains_its_actual_source_instance
    {choreo : Choreo} {session : Nat} {transport next : TransportState}
    {history : ElasticAdmissionHistory} {records : List (SourceSendAdmission × Nat)}
    {frame : TransportFrame} {sender : SourceSendAdmission} {event : Nat}
    {sentFacts : OwnedSourceFacts} {receiver : ActorCommitObservation}
    {image : RustDescriptorImage} {wire : OwnedWireObservation} {receivedFacts : OwnedCommitFacts}
    (trace : SendAdmissionTrace choreo session transport history records)
    (received : transport.pollReceive = .delivered frame next)
    (recordAt : records[transport.delivered]? = some (sender, event))
    (sourceAt : sender.ownedFacts? choreo = some sentFacts)
    (checked : receiver.ownedCheck image choreo wire session = true)
    (receiverAt : receiver.ownedFacts? image choreo = some receivedFacts)
    (preserved : sender.wire = wire) :
    ∃ key following,
      history.accepted[transport.delivered]? = some key ∧
      key.globalId = event ∧
      key.iteration = sourceOccurrenceOrdinal records transport.delivered event ∧
      sender.effIndex? = some event ∧ wire.identifies frame ∧
      sentFacts.resource = receivedFacts.resource ∧ sentFacts.effIndex = receivedFacts.effIndex ∧
      history.receive? = some (key, following) ∧
      SendAdmissionTrace choreo session next following records := by
  obtain ⟨actual, actualEvent, key, following, actualAt, keyAt, accepted, effAt,
      identity, same, ordinal, ghost, after⟩ :=
    a_fifo_receipt_reconstructs_its_source_occurrence_without_a_shared_visit trace received
  have recordSame := Option.some.inj (actualAt.symm.trans recordAt)
  cases recordSame
  have binding := accepted_owned_source_bytes_identify_the_checked_receiver
    accepted sourceAt checked receiverAt (congrArg OwnedWireObservation.payload preserved)
  exact ⟨key, following, keyAt, same, ordinal, effAt, preserved ▸ identity,
    binding.1, binding.2, ghost, after⟩

/-- Apply the receipt result at a prospective boundary of the same measured
distributed execution. The new receipt is deliberately not a
`DistributedSourceStep.receive` premise: its source-instance correspondence
is derived before such a step's receive-pair obligation can be discharged.
Earlier checked boundaries remain part of the execution's explicit contract. -/
theorem a_prospective_owned_receipt_has_its_distributed_fifo_source_instance
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {channel : TransportChannel}
    {frame : TransportFrame} {next : TransportState} {entry : SourceAcceptance}
    {sentFacts : OwnedSourceFacts} {receiver : ActorCommitObservation}
    {image : RustDescriptorImage} {wire : OwnedWireObservation} {receivedFacts : OwnedCommitFacts}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records)
    (received : (last.1.carriers channel).pollReceive = .delivered frame next)
    (entryAt : (sourceChannelEntries last.2 channel)[(last.1.carriers channel).delivered]? = some entry)
    (sourceAt : entry.sample.ownedFacts? choreo = some sentFacts)
    (checked : receiver.ownedCheck image choreo wire session = true)
    (receiverAt : receiver.ownedFacts? image choreo = some receivedFacts)
    (preserved : entry.sample.wire = wire) :
    ∃ history key following,
      SendAdmissionTrace choreo session (last.1.carriers channel) history
        (sourceChannelRecords last.2 channel) ∧
      history.accepted[(last.1.carriers channel).delivered]? = some key ∧
      key.globalId = entry.event ∧
      key.iteration = sourceOccurrenceOrdinal (sourceChannelRecords last.2 channel)
        (last.1.carriers channel).delivered entry.event ∧
      entry.sample.effIndex? = some entry.event ∧ wire.identifies frame ∧
      sentFacts.resource = receivedFacts.resource ∧ sentFacts.effIndex = receivedFacts.effIndex ∧
      history.receive? = some (key, following) ∧
      SendAdmissionTrace choreo session next following (sourceChannelRecords last.2 channel) := by
  obtain ⟨history, trace⟩ :=
    a_distributed_source_execution_projects_its_complete_fifo_histories execution channel
  have recordAt : (sourceChannelRecords last.2 channel)[(last.1.carriers channel).delivered]? =
      some (entry.sample, entry.event) := by
    simp only [sourceChannelRecords, List.getElem?_map, entryAt, Option.map_some]
  obtain ⟨key, following, keyAt, same, ordinal, effAt, identity, resource, source, ghost, after⟩ :=
    a_checked_owned_fifo_receipt_retains_its_actual_source_instance
      trace received recordAt sourceAt checked receiverAt preserved
  exact ⟨history, key, following, trace, keyAt, same, ordinal, effAt,
    identity, resource, source, ghost, after⟩

end Hibana
