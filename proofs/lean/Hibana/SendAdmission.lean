import Hibana.OwnedCommit
import Hibana.ElasticAdmissionHistory
import Hibana.ActorOccurrenceHistory
import Hibana.SourceResourceAvailability

namespace Hibana
open EndpointDelegation

/-- A source-prepared send and its actual accepted bytes. It covers ordinary
and owned payloads on the same FIFO direction, without sender publication.
The copied base must correspond to the actual current/fresh preparation. -/
structure SourceSendAdmission where
  image : RustDescriptorImage
  base : ActorProgressObservation
  pathIndex : Nat
  event : Nat
  wire : OwnedWireObservation

def SourceSendAdmission.effIndex? (sample : SourceSendAdmission) : Option Nat :=
  sample.image.eventEffIndex? sample.event

def SourceSendAdmission.ownedFacts? (sample : SourceSendAdmission) (choreo : Choreo) :=
  sample.image.ownedSourceFacts? choreo sample.pathIndex sample.event

/-- Ordinary encoded bytes use the existing codec-conformance contract.
Owned bytes additionally carry the exact resource/source envelope. This
classification comes from the global payload, never the received contents. -/
def SourceSendAdmission.payloadMatches (sample : SourceSendAdmission)
    (choreo : Choreo) (session : Nat) (payload : Payload) : Bool :=
  match payload with
  | .wire _ => true
  | .endpoint _ =>
    match sample.ownedFacts? choreo with
    | none => false
    | some facts => decide (facts.side = .send) &&
        facts.wireMatches sample.base.actor sample.wire session

def SourceSendAdmission.check (sample : SourceSendAdmission) (choreo : Choreo)
    (session : Nat) : Bool :=
  sample.image.actorOrderCheck choreo && sample.base.valid &&
    match sample.image.decodeActorOrder? choreo with
    | none => false
    | some data =>
      match data.normalized.paths[sample.pathIndex]?, data.rows[sample.event]? with
      | some path, some row =>
        match path.bindings.find? (fun entry => entry.1 == row.actionId.event) with
        | none => false
        | some (_, binding) =>
          sample.base.metadataMatches sample.image data.rows &&
          observedChoicesAgree (canonicalProgramSource choreo) path sample.base &&
          decide (row ∈ data.activeRows path) && decide (row.actionId.side = .send) &&
          observedSourcePrefixCheck data path sample.base row.eventId &&
          sourceRowReady data path sample.base row && sourceRowViewsIssued path sample.base row &&
          sample.effIndex?.isSome &&
          sample.wire.session == session && sample.base.actor == binding.senderActor &&
          sample.wire.sender == binding.message.sender.role &&
          sample.wire.receiver == binding.message.receiver.role &&
          sample.image.eventLane? sample.event == some sample.wire.lane &&
          sample.image.eventFrameLabel? sample.event == some sample.wire.frameLabel &&
          sample.payloadMatches choreo session binding.message.payload
      | _, _ => false

theorem checked_source_send_has_its_owned_payload
    {sample : SourceSendAdmission} {choreo : Choreo} {session : Nat} {facts : OwnedSourceFacts}
    (checked : sample.check choreo session = true)
    (factsAt : sample.ownedFacts? choreo = some facts) :
    facts.side = .send ∧ facts.wireMatches sample.base.actor sample.wire session = true := by
  obtain ⟨data, path, row, entry, decoded, pathAt, rowAt, entryAt, payloadAt, _, _⟩ :=
    decoded_owned_source_facts_retain_the_exact_source_row factsAt
  unfold SourceSendAdmission.check at checked
  have rest := (Bool.and_eq_true_iff.mp checked).2
  simp only [decoded, pathAt, rowAt, entryAt] at rest
  have payloadChecked := (Bool.and_eq_true_iff.mp rest).2
  simpa only [SourceSendAdmission.payloadMatches, payloadAt, factsAt,
    Bool.and_eq_true, decide_eq_true_eq] using payloadChecked

/-- Recover the same source row and its ready causal prefix before I/O.
This applies to ordinary and owned frames, and supplies the predecessor input
to the measured-history origin theorem without asserting a live value. -/
theorem checked_source_send_has_its_ready_source_row
    {sample : SourceSendAdmission} {choreo : Choreo} {session : Nat}
    (checked : sample.check choreo session = true) :
    ∃ data path row entry,
      sample.image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[sample.pathIndex]? = some path ∧ data.rows[sample.event]? = some row ∧
      path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry ∧
      row ∈ data.activeRows path ∧ row.actionId.side = .send ∧
      sourceRowReady data path sample.base row = true ∧
      observedSourcePrefixCheck data path sample.base row.eventId = true := by
  unfold SourceSendAdmission.check at checked
  have rest := (Bool.and_eq_true_iff.mp checked).2
  cases decoded : sample.image.decodeActorOrder? choreo with
  | none => simp [decoded] at rest
  | some data =>
    cases pathAt : data.normalized.paths[sample.pathIndex]? with
    | none => simp [decoded, pathAt] at rest
    | some path =>
      cases rowAt : data.rows[sample.event]? with
      | none => simp [decoded, pathAt, rowAt] at rest
      | some row =>
        cases entryAt : path.bindings.find? (fun entry => entry.1 == row.actionId.event) with
        | none => simp [decoded, pathAt, rowAt, entryAt] at rest
        | some entry =>
          simp only [decoded, pathAt, rowAt, entryAt, Bool.and_eq_true, and_assoc] at rest
          obtain ⟨_, _, member, side, prefixChecked, ready, _⟩ := rest
          exact ⟨data, path, row, entry, rfl, pathAt, rowAt, entryAt,
            of_decide_eq_true member, of_decide_eq_true side, ready, prefixChecked⟩

/-- Link a real accepted frame's source preparation to the measured origins
of its prerequisites. Current and fresh bases use the same existing relation;
there is no successful sender snapshot or available-value premise. -/
theorem an_accepted_source_send_has_actual_prerequisite_origins
    {sample : SourceSendAdmission} {choreo : Choreo} {session previousPath event : Nat}
    {cut : ActorAdmissionObservation} {data : DecodedActorOrder} {path : NormalizedPath}
    {row : DecodedActorRow} {first : ActorProgressObservation}
    {firstCount finalCount commits : Nat} {cleared : List Nat}
    (accepted : sample.check choreo session = true)
    (decoded : sample.image.decodeActorOrder? choreo = some data)
    (samePath : sample.pathIndex = cut.pathIndex)
    (pathAt : data.normalized.paths[cut.pathIndex]? = some path)
    (rowAt : data.rows[sample.event]? = some row)
    (history : ActorMeasuredHistory sample.image choreo session
      (first, firstCount) (cut.snapshot, finalCount) cleared commits)
    (initial : first.doneState.done event = false)
    (origin : ActorSourceCandidateBase cut sample.image choreo previousPath path row sample.base)
    (required : event ∈ data.sourcePredecessors path row.eventId) :
    ∃ before after prior marked later priorCommits laterCommits record,
      ActorMeasuredHistory sample.image choreo session (first, firstCount) before prior priorCommits ∧
      ActorMeasuredCommit sample.image choreo session before after marked record ∧
      ActorMeasuredHistory sample.image choreo session after (cut.snapshot, finalCount) later laterCommits ∧
      record.step.event = event ∧ event ∉ later ∧
      record.check sample.image choreo session = true ∧ record.step.SourceOrdered sample.image choreo := by
  obtain ⟨actualData, actualPath, actualRow, _, actualAt, chosenAt, actualRowAt, _, _, _, ready, _⟩ :=
    checked_source_send_has_its_ready_source_row accepted
  have dataSame := Option.some.inj (actualAt.symm.trans decoded)
  subst actualData
  have chosen : data.normalized.paths[cut.pathIndex]? = some actualPath := by
    simpa only [samePath] using chosenAt
  have pathSame := Option.some.inj (chosen.symm.trans pathAt)
  subst actualPath
  have rowSame := Option.some.inj (actualRowAt.symm.trans rowAt)
  subst actualRow
  exact a_ready_candidate_predecessor_has_its_mixed_history_origin history initial origin required ready

/-- One authenticated FIFO direction with complete accepted source records.
The occurrence ordinal is reconstructed by the existing erased history;
neither sender publication nor a receiver ACK is a transition premise. -/
inductive SendAdmissionTrace (choreo : Choreo) (session : Nat) :
    TransportState → ElasticAdmissionHistory → List (SourceSendAdmission × Nat) → Prop where
  | initial (channel : TransportChannel) :
      SendAdmissionTrace choreo session (TransportState.initial channel)
        (ElasticAdmissionHistory.initial channel) []
  | accept {transport sent : TransportState} {history : ElasticAdmissionHistory}
      {records : List (SourceSendAdmission × Nat)}
      {sample : SourceSendAdmission} {eventId : Nat} {label : Fin 256}
      (previous : SendAdmissionTrace choreo session transport history records)
      (checked : sample.check choreo session = true)
      (factsAt : sample.effIndex? = some eventId)
      (accepted : transport.send label = some sent)
      (identity : sample.wire.identifies {
        channel := transport.channel, sequence := transport.frames.length, frameLabel := label }) :
      SendAdmissionTrace choreo session sent (history.appendOccurrence eventId)
        (records ++ [(sample, eventId)])
  | receive {transport next : TransportState} {history following : ElasticAdmissionHistory}
      {records : List (SourceSendAdmission × Nat)}
      {frame : TransportFrame} {key : ElasticOccurrenceKey}
      (previous : SendAdmissionTrace choreo session transport history records)
      (received : transport.pollReceive = .delivered frame next)
      (ghost : history.receive? = some (key, following)) :
      SendAdmissionTrace choreo session next following records

def AcceptedSourceFrame (choreo : Choreo) (session : Nat)
    (record : SourceSendAdmission × Nat) (frame : TransportFrame) : Prop :=
  record.1.check choreo session = true ∧ record.1.effIndex? = some record.2 ∧
    record.1.wire.identifies frame

/-- Erased, complete index correspondence between two observed traces. -/
inductive AlignedRecords {α β : Type} (relation : α → β → Prop) : List α → List β → Prop where
  | nil : AlignedRecords relation [] []
  | cons {left right lefts rights} : relation left right → AlignedRecords relation lefts rights →
      AlignedRecords relation (left :: lefts) (right :: rights)

private theorem AlignedRecords.append {α β : Type} {relation : α → β → Prop}
    {left more : List α} {right rest : List β}
    (before : AlignedRecords relation left right) (after : AlignedRecords relation more rest) :
    AlignedRecords relation (left ++ more) (right ++ rest) := by
  induction before with
  | nil => exact after
  | cons one tail ih => exact .cons one ih

theorem a_checked_source_admission_trace_keeps_exact_frame_and_occurrence_records
    {choreo : Choreo} {session : Nat} {transport : TransportState}
    {history : ElasticAdmissionHistory} {records : List (SourceSendAdmission × Nat)}
    (trace : SendAdmissionTrace choreo session transport history records) :
    transport.WellFormed ∧ history.WellFormed ∧ history.channel = transport.channel ∧
      history.delivered = transport.delivered ∧
      AlignedRecords (AcceptedSourceFrame choreo session) records transport.frames ∧
      AlignedRecords (fun record key => key.globalId = record.2) records history.accepted := by
  induction trace with
  | initial channel =>
    exact ⟨initial_transport_is_well_formed channel,
      elastic_admission_history_initial_well_formed channel, rfl, rfl, .nil, .nil⟩
  | @accept transport sent history records sample eventId label previous checked factsAt accepted identity ih =>
    obtain ⟨transportValid, historyValid, channel, delivered, frames, occurrences⟩ := ih
    have nextTransportValid := transport_send_preserves_well_formed transportValid accepted
    have nextHistoryValid := elastic_admission_preserves_well_formed historyValid eventId
    unfold TransportState.send at accepted
    cases closed : transport.peerClosed with
    | true => simp [closed] at accepted
    | false =>
      have same := Option.some.inj (by simpa only [closed, Bool.false_eq_true, ite_false] using accepted)
      subst sent
      refine ⟨nextTransportValid, nextHistoryValid, channel, delivered, ?_, ?_⟩
      · exact frames.append (.cons ⟨checked, factsAt, identity⟩ .nil)
      · exact occurrences.append (.cons rfl .nil)
  | @receive transport next history following records frame key previous received ghost ih =>
    obtain ⟨transportValid, historyValid, channel, delivered, frames, occurrences⟩ := ih
    have transportAt := transport_receive_is_fifo received
    have historyAt := elastic_admission_receive_is_exact_fifo ghost
    refine ⟨transport_receive_preserves_well_formed transportValid received,
      elastic_admission_receive_preserves_well_formed historyValid ghost, ?_, ?_, ?_, ?_⟩
    · exact historyAt.2.1.trans (channel.trans transportAt.2.1.symm)
    · rw [historyAt.2.2.2.1, transportAt.2.2.2, delivered]
    · simpa only [transportAt.2.2.1] using frames
    · simpa only [historyAt.2.2.1] using occurrences

private theorem AlignedRecords.length_eq {α β : Type} {relation : α → β → Prop}
    {left : List α} {right : List β} (aligned : AlignedRecords relation left right) :
    left.length = right.length := by
  induction aligned with
  | nil => rfl
  | cons one rest ih => exact congrArg Nat.succ ih

/-- Complete physical acceptance coverage includes ordinary messages between
loans. Omitting one record cannot still qualify the same carrier history. -/
theorem a_checked_source_admission_trace_records_every_frame
    {choreo : Choreo} {session : Nat} {transport : TransportState}
    {history : ElasticAdmissionHistory} {records : List (SourceSendAdmission × Nat)}
    (trace : SendAdmissionTrace choreo session transport history records) :
    records.length = transport.frames.length := by
  obtain ⟨_, _, _, _, frames, _⟩ :=
    a_checked_source_admission_trace_keeps_exact_frame_and_occurrence_records trace
  exact frames.length_eq

private theorem AlignedRecords.right_at {α β : Type} {relation : α → β → Prop}
    {left : List α} {right : List β} {index : Nat} {value : β}
    (aligned : AlignedRecords relation left right) (found : right[index]? = some value) :
    ∃ origin, left[index]? = some origin ∧ relation origin value := by
  induction aligned generalizing index with
  | nil => simp at found
  | @cons head tail lefts rights one rest ih =>
    cases index with
    | zero =>
      change some tail = some value at found
      cases Option.some.inj found
      exact ⟨head, rfl, one⟩
    | succ index => exact ih found

private theorem AlignedRecords.left_at {α β : Type} {relation : α → β → Prop}
    {left : List α} {right : List β} {index : Nat} {value : α}
    (aligned : AlignedRecords relation left right) (found : left[index]? = some value) :
    ∃ corresponding, right[index]? = some corresponding ∧ relation value corresponding := by
  induction aligned generalizing index with
  | nil => simp at found
  | @cons head tail lefts rights one rest ih =>
    cases index with
    | zero =>
      change some head = some value at found
      cases Option.some.inj found
      exact ⟨tail, rfl, one⟩
    | succ index => exact ih found

/-- The actual FIFO receive ordinal selects its unique accepted source
record and erased occurrence. The sender need not have published a commit,
and unrelated static occurrences may be interleaved on the direction. -/
theorem a_fifo_receive_has_its_actual_accepted_source_occurrence
    {choreo : Choreo} {session : Nat} {transport next : TransportState}
    {history : ElasticAdmissionHistory} {records : List (SourceSendAdmission × Nat)}
    {frame : TransportFrame}
    (trace : SendAdmissionTrace choreo session transport history records)
    (received : transport.pollReceive = .delivered frame next) :
    ∃ sample eventId key following,
      records[transport.delivered]? = some (sample, eventId) ∧
      history.accepted[transport.delivered]? = some key ∧
      sample.check choreo session = true ∧ sample.effIndex? = some eventId ∧
      sample.wire.identifies frame ∧ key.globalId = eventId ∧
      frame.sequence = history.delivered ∧ key.iteration < history.nextIteration key.globalId ∧
      history.receive? = some (key, following) ∧
      SendAdmissionTrace choreo session next following records := by
  obtain ⟨transportValid, historyValid, channel, delivered, frames, occurrences⟩ :=
    a_checked_source_admission_trace_keeps_exact_frame_and_occurrence_records trace
  have frameAt := (transport_receive_is_fifo received).1
  obtain ⟨⟨sample, eventId⟩, recordAt, checked, factsAt, identity⟩ := frames.right_at frameAt
  obtain ⟨key, keyAt, sourceAt⟩ := occurrences.left_at recordAt
  have present : key ∈ history.accepted := List.mem_of_getElem? keyAt
  have generation : key.iteration ∈ occurrenceGenerations history.accepted key.globalId := by
    apply List.mem_filterMap.mpr
    exact ⟨key, present, by simp⟩
  rw [historyValid.exactGenerations key.globalId] at generation
  let following := { history with delivered := history.delivered + 1 }
  have ghost : history.receive? = some (key, following) := by
    simp only [ElasticAdmissionHistory.receive?, following, delivered, keyAt]
  exact ⟨sample, eventId, key, following, recordAt, keyAt, checked, factsAt, identity, sourceAt,
    ((transportValid.2 _ _ frameAt).2).trans delivered.symm,
    List.mem_range.mp generation, ghost, .receive trace received ghost⟩

/-- Bind a checked receiver's resource and static event to the bytes accepted
by the source send. Exact payload preservation belongs to that same FIFO
frame, not to a presumed sender completion or a logical-label match alone. -/
theorem accepted_owned_source_bytes_identify_the_checked_receiver
    {sender : SourceSendAdmission} {receiver : ActorCommitObservation}
    {receiverImage : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {sentFacts : OwnedSourceFacts} {receivedFacts : OwnedCommitFacts} {wire : OwnedWireObservation}
    (accepted : sender.check choreo session = true)
    (sentAt : sender.ownedFacts? choreo = some sentFacts)
    (checked : receiver.ownedCheck receiverImage choreo wire session = true)
    (receivedAt : receiver.ownedFacts? receiverImage choreo = some receivedFacts)
    (preserved : sender.wire.payload = wire.payload) :
    sentFacts.resource = receivedFacts.resource ∧ sentFacts.effIndex = receivedFacts.effIndex := by
  have wireChecked := (checked_source_send_has_its_owned_payload accepted sentAt).2
  have fields : sentFacts.resource.component = 0 ∧ sentFacts.resource.role < 256 ∧
      sentFacts.effIndex < 65536 ∧ sentFacts.actor? = some sender.base.actor ∧
      sender.wire.session = session ∧ sender.wire.lane = sentFacts.lane ∧
      sender.wire.frameLabel = sentFacts.frameLabel ∧
      sender.wire.sender = sentFacts.binding.message.sender.role ∧
      sender.wire.receiver = sentFacts.binding.message.receiver.role ∧
      sender.wire.payload = sentFacts.envelope := by
    simpa only [OwnedSourceFacts.wireMatches, Bool.and_eq_true, beq_iff_eq,
      decide_eq_true_eq, and_assoc] using wireChecked
  obtain ⟨component, _, _, _, _, _, _, _, _, envelope⟩ := fields
  obtain ⟨_, actualReceiver, actualReceiverAt, receiverFields⟩ :=
    accepted_owned_commit_binds_source_view_and_wire checked
  have sameReceiver := Option.some.inj (actualReceiverAt.symm.trans receivedAt)
  subst actualReceiver
  obtain ⟨receiverComponent, _, _, _, _, _, _, _, _, receiverEnvelope⟩ := receiverFields
  have sameEnvelope : sentFacts.envelope = receivedFacts.sourceFacts.envelope :=
    envelope.symm.trans (preserved.trans receiverEnvelope)
  obtain ⟨role, event⟩ := owned_source_envelope_identifies_resource_and_occurrence sameEnvelope
  change sentFacts.resource.role = receivedFacts.resource.role at role
  change sentFacts.effIndex = receivedFacts.effIndex at event
  have resource : sentFacts.resource = receivedFacts.resource := by
    change EndpointKey.mk sentFacts.resource.component sentFacts.resource.role =
      EndpointKey.mk receivedFacts.resource.component receivedFacts.resource.role
    rw [component, receiverComponent, role]
  exact ⟨resource, event⟩

/-- Source preparation already suffices at the real carrier acceptance
boundary: authority moves to that exact frame before sender publication.
The complete transfer history and ordered-carrier refinement remain explicit
implementation contracts. No after-send snapshot or remote ACK is needed. -/
theorem accepted_source_send_keeps_its_exact_authority_until_receive_or_fence
    {sample : SourceSendAdmission} {choreo : Choreo} {session : Nat} {facts : OwnedSourceFacts}
    (checked : sample.check choreo session = true)
    (decoded : sample.ownedFacts? choreo = some facts)
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current accepted : carrier.State} {before : TransportState}
    {state : EndpointTransfer.Observation (Nat × Nat × Nat × Nat) TransportFrame}
    {left right : List (Nat × Nat × Nat × Nat)}
    (initialView sending : Nat × Nat × Nat × Nat) (label : Fin 256)
    (held : (observedActorView? sample.base facts.resource.role).map
      (fun view => (sample.base.actor, view.header)) = some sending)
    (execution : EndpointTransfer.Execution (EndpointTransfer.initial initialView) state)
    (preparing : state.preparing = left ++ sending :: right)
    (currentExact : abstraction current = .idle before)
    (sent : carrier.send current label = some accepted)
    (identity : sample.wire.identifies {
      channel := before.channel, sequence := before.frames.length, frameLabel := label }) :
    sending.1 = facts.binding.senderActor ∧ sample.wire.payload = facts.envelope ∧
      ∃ next frame after, EndpointTransfer.Step .accept state next ∧ TransferFrameLocated frame next ∧
        abstraction accepted = .idle after ∧ sample.wire.identifies frame ∧
        ∀ final, EndpointTransfer.Execution next final → TransferFrameLocated frame final ∨
          ∃ prior following action, EndpointTransfer.Execution next prior ∧
            EndpointTransfer.Step action prior following ∧ TransferFrameLocated frame prior ∧
            (action = .receive ∨ action = .fenceFrame) ∧
            EndpointTransfer.Execution following final := by
  obtain ⟨side, wireChecked⟩ := checked_source_send_has_its_owned_payload checked decoded
  have fields : facts.resource.component = 0 ∧ facts.resource.role < 256 ∧
      facts.effIndex < 65536 ∧ facts.actor? = some sample.base.actor ∧
      sample.wire.session = session ∧ sample.wire.lane = facts.lane ∧
      sample.wire.frameLabel = facts.frameLabel ∧
      sample.wire.sender = facts.binding.message.sender.role ∧
      sample.wire.receiver = facts.binding.message.receiver.role ∧
      sample.wire.payload = facts.envelope := by
    simpa only [OwnedSourceFacts.wireMatches, Bool.and_eq_true, beq_iff_eq,
      decide_eq_true_eq, and_assoc] using wireChecked
  obtain ⟨_, _, _, actor, _, _, _, _, _, envelope⟩ := fields
  have sendingActor : sending.1 = sample.base.actor := by
    cases headerAt : observedActorView? sample.base facts.resource.role with
    | none => simp [headerAt] at held
    | some header =>
      simp only [headerAt, Option.map_some, Option.some.injEq] at held
      exact (congrArg Prod.fst held).symm
  simp only [OwnedSourceFacts.actor?, side, Option.some.injEq] at actor
  obtain ⟨next, frame, after, step, bound, acceptedAt, _, channelAt, sequenceAt, labelAt, queued⟩ :=
    EndpointTransfer.ordered_carrier_send_preserves_unique_authority profile initialView sending label
      execution preparing currentExact sent
  have located := one_queued_authority_is_the_exact_frame bound (by
    rw [queued]
    exact List.mem_append.mpr (.inr List.mem_cons_self))
  have frameExact : frame = {
      channel := before.channel, sequence := before.frames.length, frameLabel := label } := by
    cases frame
    simp_all
  refine ⟨sendingActor.trans actor.symm, envelope,
    next, frame, after, step, located, acceptedAt, ?_, ?_⟩
  · rw [frameExact]
    exact identity
  · intro final suffix
    exact transfer_execution_keeps_the_frame_until_receive_or_fence suffix located

end Hibana
