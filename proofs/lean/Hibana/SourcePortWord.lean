import Hibana.SourceReceiptWord
import Hibana.SourceHandoffWord

namespace Hibana
open EndpointDelegation

/-- The accepted bytes retain their exact source binding and static wire
columns. This extracts existing checks; it is not an additional admission
condition or a comparison with a prospective receive. -/
theorem checked_source_send_has_its_bound_wire_columns
    {sample : SourceSendAdmission} {choreo : Choreo} {session : Nat}
    (checked : sample.check choreo session = true) :
    ∃ eff binding,
      sourceOperationIdentity? sample.image choreo sample.pathIndex sample.event =
        some (eff, binding, .send) ∧
      sample.effIndex? = some eff ∧ sample.wire.session = session ∧
      sample.base.actor = binding.senderActor ∧
      sample.wire.sender = binding.message.sender.role ∧
      sample.wire.receiver = binding.message.receiver.role ∧
      sample.image.eventLane? sample.event = some sample.wire.lane ∧
      sample.image.eventFrameLabel? sample.event = some sample.wire.frameLabel := by
  obtain ⟨data, path, row, entry, decoded, pathAt, rowAt, entryAt, _, side, _, _⟩ :=
    checked_source_send_has_its_ready_source_row checked
  unfold SourceSendAdmission.check at checked
  have rest := (Bool.and_eq_true_iff.mp checked).2
  simp only [decoded, pathAt, rowAt, entryAt, Bool.and_eq_true, and_assoc] at rest
  obtain ⟨_, _, _, _, _, _, _, someEff, sessionAt, actorAt, senderAt, receiverAt,
    laneAt, colorAt, _⟩ := rest
  obtain ⟨eff, effectAt⟩ := Option.isSome_iff_exists.mp someEff
  refine ⟨eff, entry.2, ?_, effectAt, beq_iff_eq.mp sessionAt, beq_iff_eq.mp actorAt,
    beq_iff_eq.mp senderAt, beq_iff_eq.mp receiverAt, beq_iff_eq.mp laneAt, beq_iff_eq.mp colorAt⟩
  change sample.image.eventEffIndex? sample.event = some eff at effectAt
  simp only [sourceOperationIdentity?, Option.bind_eq_bind, decoded, pathAt, rowAt, entryAt,
    effectAt, side, Option.bind_some, Option.pure_def]

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

private theorem a_complete_direction_word_keeps_every_selected_acceptance
    {entries : List SourceAcceptance} {channel : TransportChannel} {selected : Nat → Bool}
    (classifies : ∀ entry ∈ entries, decide (entry.channel = channel) = selected entry.event) :
    (sourceChannelEntries entries channel).map SourceAcceptance.event =
      entries.filterMap (fun entry => if selected entry.event then some entry.event else none) := by
  induction entries with
  | nil => rfl
  | cons entry entries ih =>
    have head := classifies entry (List.mem_cons_self ..)
    have rest := ih (fun other member => classifies other (List.mem_cons_of_mem _ member))
    by_cases same : entry.channel = channel
    · have chosen : selected entry.event = true := by simpa only [same, decide_true] using head.symm
      simpa only [sourceChannelEntries, List.filter_cons, same, decide_true, if_true,
        List.map_cons, List.filterMap_cons, chosen] using congrArg (fun word => entry.event :: word) rest
    · have omitted : selected entry.event = false := by simpa only [same, decide_false] using head.symm
      simpa only [sourceChannelEntries, List.filter_cons, same, decide_false, if_false,
        List.filterMap_cons, omitted, Bool.false_eq_true] using rest

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
complete source class. The supplied classification concerns static source
rows and opened port identity, not the expected source order. All accepted
ordinary/owned frames sharing the direction must be included. No current
receive-pair or successful receiver snapshot is assumed. -/
theorem a_prospective_port_head_has_its_actual_source_class_position
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {channel : TransportChannel}
    {next : TransportState} {frame : TransportFrame} (selected : Nat → Bool)
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records)
    (received : (last.1.carriers channel).pollReceive = .delivered frame next)
    (classifies : ∀ entry ∈ last.2, decide (entry.channel = channel) = selected entry.event) :
    ∃ sample event history key,
      SendAdmissionTrace choreo session (last.1.carriers channel) history (sourceChannelRecords last.2 channel) ∧
      history.accepted[(last.1.carriers channel).delivered]? = some key ∧
      (sourceChannelRecords last.2 channel)[(last.1.carriers channel).delivered]? = some (sample, event) ∧
      sample.check choreo session = true ∧ sample.effIndex? = some event ∧
      sample.wire.identifies frame ∧ key.globalId = event ∧
      (last.2.filterMap (fun entry => if selected entry.event then some entry.event else none))[
        (receivedSourceWord images choreo records).countP selected]? = some event := by
  obtain ⟨receipts, word, prefixes⟩ := a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix execution
  obtain ⟨history, trace⟩ := a_distributed_source_execution_projects_its_complete_fifo_histories execution channel
  obtain ⟨sample, event, key, _, recordAt, keyAt, checked, sourceAt, identity, sourceKey, _, _, _⟩ :=
    a_fifo_receipt_reconstructs_its_source_occurrence_without_a_shared_visit trace received
  have receiptClass : ∀ entry ∈ receipts, decide (entry.channel = channel) = selected entry.event :=
    fun entry member => classifies entry (receipt_prefixes_contain_only_actual_acceptances prefixes entry member)
  have receiptCount : (receipts.map SourceAcceptance.event).countP selected =
      (sourceChannelEntries receipts channel).length := by
    simp only [sourceChannelEntries, ← List.countP_eq_length_filter, List.countP_map]
    apply List.countP_congr
    intro entry member
    rw [receiptClass entry member]
    rfl
  have bound : (last.1.carriers channel).delivered < (sourceChannelEntries last.2 channel).length := by
    have actual := (List.getElem?_eq_some_iff.mp recordAt).1
    simpa only [sourceChannelRecords, List.length_map] using actual
  have position : (receivedSourceWord images choreo records).countP selected =
      (last.1.carriers channel).delivered := by
    rw [word, receiptCount, prefixes channel, List.length_take, Nat.min_eq_left (Nat.le_of_lt bound)]
  have nextSource : ((sourceChannelEntries last.2 channel).map SourceAcceptance.event)[
      (last.1.carriers channel).delivered]? = some event := by
    have actual := congrArg (Option.map Prod.snd) recordAt
    simpa only [sourceChannelRecords, List.getElem?_map, Option.map_map, Option.map_some,
      Function.comp_def] using actual
  refine ⟨sample, event, history, key, trace, keyAt, recordAt, checked, sourceAt,
    identity, sourceKey, ?_⟩
  rw [position, ← a_complete_direction_word_keeps_every_selected_acceptance classifies]
  exact nextSource

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
    (selected : Nat → Bool)
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
    (classifies : ∀ entry ∈ last.2, decide (entry.channel = channel) = selected entry.event)
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
  obtain ⟨sample, source, history, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, nextAt⟩ :=
    a_prospective_port_head_has_its_actual_source_class_position selected execution received classifies
  rw [receiptCount] at nextAt
  have receiverAt : (committedFrameClassWord group.rows actor records ++ [event])[
      (committedFrameClassWord group.rows actor records).length]? = some event := by simp
  have matched := source_frame_words_match_at_each_common_class_position cycle sentOrdered receiverOrdered nextAt receiverAt
  exact ⟨sample, source, history, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, matched.symm⟩

end Hibana
