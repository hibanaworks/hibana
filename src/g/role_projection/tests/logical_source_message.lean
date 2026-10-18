private def logicalSharedSourceAtoms := (distributedImage0.decodeAtoms?).get (by decide)

private theorem logicalImagesShareSourceAtoms (actor : Nat) :
    (distributedImages actor).decodeAtoms? = some logicalSharedSourceAtoms := by
  by_cases one : actor = 1
  · simp only [distributedImages, if_pos one]
    decide
  · by_cases three : actor = 3
    · simp only [distributedImages, if_neg one, if_pos three]
      decide
    · simp only [distributedImages, if_neg one, if_neg three]
      decide

/-- For every finite history of the holder-changing protocol, the raw head
has the logical message selected by the independently prepared service row.
The receive source identity is immutable local projection data. The head's
correspondence and sender kind are outputs, not a receive-pair premise or a
classification of its bytes. Sender publication is not required. -/
theorem logicalProspectiveRequestMessageMatchesBeforeReceive
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    {previousPath pathIndex event localSource : Nat} {base : ActorProgressObservation}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {next : TransportState} {frame : TransportFrame} {receiverBinding : Binding}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records)
    (sameRun : ∀ entry ∈ entries, entry.channel.generation = 0)
    (decoded : distributedImage3.decodeActorOrder? distributedChoreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path)
    (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := (cut.actors 3).1, pathIndex, admitted := [] }
      distributedImage3 distributedChoreo previousPath path row base)
    (selector : distributedImage3.frameKey? event = some ⟨.receive, 3, 10, 0, 0⟩)
    (received : (cut.carriers logicalRequestPort).pollReceive = .delivered frame next)
    (localIdentity : sourceOperationIdentity? distributedImage3 distributedChoreo pathIndex event =
      some (localSource, receiverBinding, .receive)) :
    ∃ sample senderBinding fifo key,
      SendAdmissionTrace distributedChoreo 9064 (cut.carriers logicalRequestPort) fifo
        (sourceChannelRecords entries logicalRequestPort) ∧
      fifo.accepted[(cut.carriers logicalRequestPort).delivered]? = some key ∧
      (sourceChannelRecords entries logicalRequestPort)[(cut.carriers logicalRequestPort).delivered]? =
        some (sample, localSource) ∧
      sample.check distributedChoreo 9064 = true ∧ key.globalId = localSource ∧
      sample.wire.identifies frame ∧
      sourceOperationIdentity? sample.image distributedChoreo sample.pathIndex sample.event =
        some (localSource, senderBinding, .send) ∧
      senderBinding.message = receiverBinding.message := by
  obtain ⟨sample, headSource, fifo, key, trace, keyAt, recordAt, checked, sourceAt,
    identity, keySource, receiverSource⟩ :=
    logicalProspectiveRequestSourceMatchesBeforeReceive history sameRun decoded pathAt rowAt
      active ready origin selector received
  have own := localIdentity
  unfold sourceOperationIdentity? at own
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def,
    Option.some.injEq] at own
  obtain ⟨actualData, _, actualPath, _, actualRow, _, entry, _, eff, effectAt, same⟩ := own
  have ownSource : distributedImage3.eventEffIndex? event = some localSource :=
    effectAt.trans (congrArg some (congrArg Prod.fst same))
  have matching : headSource = localSource := Option.some.inj (receiverSource.symm.trans ownSource)
  obtain ⟨sentSource, senderBinding, senderIdentity, senderEffect, _⟩ :=
    checked_source_send_has_its_bound_wire_columns checked
  have senderMatches : sentSource = localSource :=
    Option.some.inj (senderEffect.symm.trans (sourceAt.trans (congrArg some matching)))
  rw [senderMatches] at senderIdentity
  have senderImage : sample.image = distributedImages sample.image.role := by
    have present : (sample, headSource) ∈ sourceChannelRecords entries logicalRequestPort :=
      List.mem_of_getElem? recordAt
    obtain ⟨accepted, member, sameSample⟩ := List.mem_map.mp present
    have acceptedImage := (a_distributed_acceptance_retains_its_source_and_opened_port
      history accepted (List.mem_filter.mp member).1).1
    have sampleSame : accepted.sample = sample := congrArg Prod.fst sameSample
    simpa only [sampleSame] using acceptedImage
  have senderAtoms : sample.image.decodeAtoms? = some logicalSharedSourceAtoms := by
    rw [senderImage]
    exact logicalImagesShareSourceAtoms sample.image.role
  have message := source_matched_operations_have_the_same_logical_message
    senderIdentity localIdentity senderAtoms (logicalImagesShareSourceAtoms 3)
  exact ⟨sample, senderBinding, fifo, key, trace, keyAt,
    by simpa only [matching] using recordAt,
    checked, keySource.trans matching, identity, senderIdentity, message⟩
