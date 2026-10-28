private theorem logicalMappedActor (actor : Nat)
    (roleAt : (distributedImages actor).role = actor) : actor ∈ [0, 1, 3] := by
  by_cases one : actor = 1
  · simp [one]
  · by_cases three : actor = 3
    · simp [three]
    · have zero : actor = 0 := by simpa only [distributedImages, if_neg one, if_neg three] using roleAt.symm
      simp [zero]

private def logicalRequestReceiptRows : Bool :=
  ([0, 1, 3] : List Nat).all fun actor =>
    (List.range (distributedImages actor).eventCount).all fun row =>
      match (distributedImages actor).eventEffIndex? row, (distributedImages actor).sourceRowSide? row with
      | some eff, some side =>
        (if side = .receive then eff == 0 || eff == 3 else false) ==
          (actor == 3 && (row == 0 || row == 2))
      | _, _ => false

private theorem logicalRequestReceiptClassifies
    (actor row eff : Nat) (side : Side) (roleAt : (distributedImages actor).role = actor)
    (bound : row < (distributedImages actor).eventCount)
    (effectAt : (distributedImages actor).eventEffIndex? row = some eff)
    (sideAt : (distributedImages actor).sourceRowSide? row = some side) :
    (if side = .receive then eff == 0 || eff == 3 else false) =
      (actor == 3 && (row == 0 || row == 2)) := by
  have checked : logicalRequestReceiptRows = true := by decide
  have one := List.all_eq_true.mp
    (List.all_eq_true.mp checked actor (logicalMappedActor actor roleAt)) row (List.mem_range.mpr bound)
  exact beq_iff_eq.mp (by simpa only [effectAt, sideAt] using one)

theorem logicalRequestReceiptCountIsTheServiceClassLength
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records) :
    (receivedSourceWord distributedImages distributedChoreo records).countP (fun eff => eff == 0 || eff == 3) =
      (committedFrameClassWord [0, 2] 3 records).length := by
  have classified := a_distributed_receipt_source_count_has_its_static_row_classification
    history rfl (fun eff => eff == 0 || eff == 3) (fun actor row => actor == 3 && (row == 0 || row == 2))
    logicalRequestReceiptClassifies
  rw [classified]
  simp only [committedFrameClassWord, List.length_filterMap_eq_countP]
  apply List.countP_congr
  rintro ⟨actor, record⟩ _
  by_cases owns : actor = 3 <;> by_cases zero : record.step.event = 0 <;>
    by_cases two : record.step.event = 2 <;> simp [owns, zero, two]

private def logicalRequestSourceRows : Bool :=
  ([0, 1, 3] : List Nat).all fun actor =>
    match (distributedImages actor).decodeActorOrder? distributedChoreo with
    | none => false
    | some data => (List.range data.normalized.paths.length).all fun path =>
      (List.range (distributedImages actor).eventCount).all fun row =>
        match sourceOperationIdentity? (distributedImages actor) distributedChoreo path row with
        | some (eff, binding, .send) =>
          ((distributedImages actor).eventLane? row == some 0) &&
            ((eff == 0 || eff == 3) ==
              (binding.message.sender.role == 10 && binding.message.receiver.role == 3))
        | _ => true

private theorem logicalRequestSourceClassifies
    {sample : SourceSendAdmission} {eff : Nat} {binding : Binding}
    (sameImage : sample.image = distributedImages sample.image.role)
    (identity : sourceOperationIdentity? sample.image distributedChoreo sample.pathIndex sample.event =
      some (eff, binding, .send)) :
    sample.image.eventLane? sample.event = some 0 ∧
      (eff == 0 || eff == 3) = (binding.message.sender.role == 10 && binding.message.receiver.role == 3) := by
  have roleAt : (distributedImages sample.image.role).role = sample.image.role :=
    (congrArg RustDescriptorImage.role sameImage).symm
  have decomposed := identity
  unfold sourceOperationIdentity? at decomposed
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def, Option.some.injEq] at decomposed
  obtain ⟨data, decoded, path, pathAt, row, rowAt, entry, entryAt, actual, effectAt, same⟩ := decomposed
  have bound : sample.event < sample.image.eventCount := by
    unfold RustDescriptorImage.eventEffIndex? at effectAt
    split at effectAt
    · assumption
    · contradiction
  have checked : logicalRequestSourceRows = true := by decide
  have allPaths := List.all_eq_true.mp checked sample.image.role (logicalMappedActor _ roleAt)
  rw [← sameImage] at allPaths
  simp only [decoded] at allPaths
  have one := List.all_eq_true.mp
    (List.all_eq_true.mp allPaths sample.pathIndex (List.mem_range.mpr (List.getElem?_eq_some_iff.mp pathAt).1))
    sample.event (List.mem_range.mpr bound)
  simp only [identity, Bool.and_eq_true] at one
  exact ⟨beq_iff_eq.mp one.1, beq_iff_eq.mp one.2⟩

private def logicalRequestPort : TransportChannel := ⟨9064, 0, 0, 10, 3⟩

private theorem logicalOpenedRequestDirection
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records)
    (sameRun : ∀ entry ∈ entries, entry.channel.generation = 0) :
    ∀ entry ∈ entries, decide (entry.channel = logicalRequestPort) = (entry.event == 0 || entry.event == 3) := by
  intro entry member
  have facts := a_distributed_acceptance_retains_its_source_and_opened_port history entry member
  obtain ⟨eff, binding, identity, sourceAt, sessionAt, actorAt, senderAt, receiverAt, laneAt, colorAt⟩ :=
    checked_source_send_has_its_bound_wire_columns facts.2.1
  have effect : eff = entry.event := Option.some.inj (sourceAt.symm.trans facts.2.2.1)
  subst eff
  have staticFields := logicalRequestSourceClassifies facts.1 identity
  have lane : entry.sample.wire.lane = 0 := Option.some.inj (laneAt.symm.trans staticFields.1)
  have port := facts.2.2.2
  have sessionBound : entry.channel.session = 9064 := port.1.symm.trans sessionAt
  have laneBound : entry.channel.lane = 0 := port.2.1.symm.trans lane
  have senderBound : entry.channel.sender = binding.message.sender.role := port.2.2.1.symm.trans senderAt
  have receiverBound : entry.channel.receiver = binding.message.receiver.role := port.2.2.2.symm.trans receiverAt
  rw [staticFields.2, ← senderBound, ← receiverBound]
  have runGeneration := sameRun entry member
  cases channelAt : entry.channel with
  | mk session generation lane sender receiver =>
    simp only [channelAt] at sessionBound laneBound runGeneration ⊢
    subst session; subst generation; subst lane
    by_cases senderIs : sender = 10 <;> by_cases towards : receiver = 3 <;>
      simp [logicalRequestPort, TransportChannel.mk.injEq, senderIs, towards]

private def logicalRequestGroup : SourceOrderGroup distributedImage3 distributedChoreo := {
  rows := [0, 2]
  distinct := by decide
  data := (distributedImage3.decodeActorOrder? distributedChoreo).get (by decide)
  rolls := distributedImage3.decodeRollRows?.get (by decide)
  choices := []
  decoded := (Option.some_get _).symm
  decodedRolls := (Option.some_get _).symm
  nonempty := by decide
  paths := by decide
  choiceMembers := by decide
  slots := by decide
  active := by decide
  ordered := by decide
  uniformReset := by decide
  outsideChoices := by decide
}

/-- This covers arbitrary finite histories and the next current or fresh
receive preparation. The only deployment premise is one carrier namespace
for this run; no expected source identity, common visit or new pair-check is
assumed. A sender may have accepted without publishing its operation. -/
theorem logicalProspectiveRequestSourceMatchesBeforeReceive
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    {previousPath pathIndex event wirePosition : Nat} {base : ActorProgressObservation}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {next : TransportState} {frame : TransportFrame}
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
    (selector : distributedImage3.frameKey? event = some ⟨.receive, 3, 10, 0, wirePosition⟩)
    (received : (cut.carriers logicalRequestPort).pollReceive = .delivered frame next) :
    ∃ sample source fifo key,
      SendAdmissionTrace distributedChoreo 9064 (cut.carriers logicalRequestPort) fifo
        (sourceChannelRecords entries logicalRequestPort) ∧
      fifo.accepted[(cut.carriers logicalRequestPort).delivered]? = some key ∧
      (sourceChannelRecords entries logicalRequestPort)[(cut.carriers logicalRequestPort).delivered]? = some (sample, source) ∧
      sample.check distributedChoreo 9064 = true ∧ sample.effIndex? = some source ∧
      sample.wire.identifies frame ∧ key.globalId = source ∧
      distributedImage3.eventEffIndex? event = some source := by
  have bounded : event < distributedImage3.eventCount := by
    unfold RustDescriptorImage.frameKey? at selector
    simp only [Option.bind_eq_bind, Option.bind_eq_some_iff] at selector
    obtain ⟨_, _, _, laneAt, _⟩ := selector
    unfold RustDescriptorImage.eventLane? at laneAt
    split at laneAt
    · assumption
    · contradiction
  have complete : logicalRequestGroup.rows =
      (List.range distributedImage3.eventCount).filter (fun index =>
        (distributedImage3.frameKey? index).any (fun key =>
          decide (key.side = .receive ∧ key.view = 3 ∧ key.peer = 10 ∧ key.lane = 0))) := by decide
  have member : event ∈ logicalRequestGroup.rows := by
    rw [complete]
    exact List.mem_filter.mpr ⟨List.mem_range.mpr bounded, by simp [selector]⟩
  exact a_prospective_source_word_head_matches_its_prepared_receiver
    (actor := 3) (group := logicalRequestGroup) (sourceRows := [0, 3])
    (fun eff => eff == 0 || eff == 3) (fun _ => true)
    history (by decide) decoded pathAt rowAt active ready origin member received
    (by simpa only [Bool.and_true] using logicalOpenedRequestDirection history sameRun)
    rfl (by decide) (logicalRequestSourceWordAtEveryCut history)
    (logicalRequestReceiptCountIsTheServiceClassLength history)

/-- At the observed second worker-held request, the generic result applies
before that sender publishes and before the service receives. Only the
receiver's existing before-cut is used for preparation. -/
theorem logicalSecondHolderMatchesReceiverBeforePublication
    {frame : TransportFrame} {next : TransportState}
    (received : (distributedCut28.carriers distributedChannel0).pollReceive = .delivered frame next) :
    ∃ fifo key,
      SendAdmissionTrace distributedChoreo 9064 (distributedCut28.carriers distributedChannel0) fifo
        (sourceChannelRecords distributedEntries28 distributedChannel0) ∧
      fifo.accepted[3]? = some key ∧ key.globalId = 3 ∧
      distributedSend18.wire.identifies frame ∧
      ∃ binding,
        sourceOperationIdentity? distributedSend18.image distributedChoreo
          distributedSend18.pathIndex distributedSend18.event = some (3, binding, .send) ∧
        sourceOperationIdentity? distributedImage3 distributedChoreo 0 2 = some (3, binding, .receive) ∧
        binding.receiverActor = 3 := by
  have current : ActorSourceCandidateBase
      { snapshot := (distributedCut28.actors 3).1, pathIndex := 0, admitted := [] }
      distributedImage3 distributedChoreo 0
      ((distributedWordData3.normalized.paths[0]?).get (by decide))
      ((distributedWordData3.rows[2]?).get (by decide))
      (distributedCut28.actors 3).1 := .current (by decide)
  obtain ⟨sample, source, fifo, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource, receiverSource⟩ :=
    logicalProspectiveRequestSourceMatchesBeforeReceive
      (previousPath := 0) (pathIndex := 0) (event := 2)
      (wirePosition := distributedFrameKey3_2.color) (data := distributedWordData3)
      logicalBeforeSecondHolderReceipt (by decide)
      distributedWordData3Decoded (Option.some_get _).symm (Option.some_get _).symm
      (by decide) (by decide) current (by decide) received
  have actualSource : distributedImage3.eventEffIndex? 2 = some 3 := by decide
  have sourceSame : source = 3 := Option.some.inj (receiverSource.symm.trans actualSource)
  have keyMatches : key.globalId = 3 := keySource.trans sourceSame
  have actualAt : (sourceChannelRecords distributedEntries28 distributedChannel0)[
      (distributedCut28.carriers distributedChannel0).delivered]? = some (distributedSend18, 3) := by decide
  have same := Option.some.inj (recordAt.symm.trans actualAt)
  have sampleSame : sample = distributedSend18 := congrArg Prod.fst same
  subst sample
  refine ⟨fifo, key, trace, keyAt, keyMatches, identity, ?_⟩
  let sourcePath := (distributedWordData3.normalized.paths[0]?).get (by decide)
  let sourceBinding := (sourcePath.bindings[3]?).get (by decide)
  exact ⟨sourceBinding.2, by decide, by decide, by decide⟩
