private def peerSourceImages (actor : Nat) : RustDescriptorImage :=
  if actor = 1 then peerWitnessImage1 else if actor = 2 then peerWitnessImage2
  else if actor = 3 then peerWitnessImage3 else if actor = 4 then peerWitnessImage4
  else peerWitnessImage0

private theorem peerSourceMappedActor (actor : Nat)
    (roleAt : (peerSourceImages actor).role = actor) : actor ∈ [0, 1, 2, 3, 4] := by
  by_cases one : actor = 1
  · simp [one]
  · by_cases two : actor = 2
    · simp [two]
    · by_cases three : actor = 3
      · simp [three]
      · by_cases four : actor = 4
        · simp [four]
        · have zero : actor = 0 := by
            simpa only [peerSourceImages, if_neg one, if_neg two, if_neg three, if_neg four]
              using roleAt.symm
          simp [zero]

private def peerLoanSourceRows : Bool :=
  ([0, 1, 2, 3, 4] : List Nat).all fun actor =>
    match (peerSourceImages actor).decodeActorOrder? peerWitnessChoreo with
    | none => false
    | some data => (List.range data.normalized.paths.length).all fun path =>
      (List.range (peerSourceImages actor).eventCount).all fun row =>
        match sourceOperationIdentity? (peerSourceImages actor) peerWitnessChoreo path row with
        | some (eff, binding, .send) =>
          ((peerSourceImages actor).eventLane? row == some 0) &&
            ((eff == 0) == (binding.message.sender.role == 0 && binding.message.receiver.role == 1 &&
              (peerSourceImages actor).eventFrameLabel? row == some 0))
        | _ => true

private theorem peerLoanSourceClassifies
    {sample : SourceSendAdmission} {eff : Nat} {binding : Binding}
    (sameImage : sample.image = peerSourceImages sample.image.role)
    (identity : sourceOperationIdentity? sample.image peerWitnessChoreo sample.pathIndex sample.event =
      some (eff, binding, .send)) :
    sample.image.eventLane? sample.event = some 0 ∧
      (eff == 0) = (binding.message.sender.role == 0 && binding.message.receiver.role == 1 &&
        sample.image.eventFrameLabel? sample.event == some 0) := by
  have roleAt : (peerSourceImages sample.image.role).role = sample.image.role :=
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
  have checked : peerLoanSourceRows = true := by decide
  have allPaths := List.all_eq_true.mp checked sample.image.role (peerSourceMappedActor _ roleAt)
  rw [← sameImage] at allPaths
  simp only [decoded] at allPaths
  have one := List.all_eq_true.mp
    (List.all_eq_true.mp allPaths sample.pathIndex (List.mem_range.mpr (List.getElem?_eq_some_iff.mp pathAt).1))
    sample.event (List.mem_range.mpr bound)
  simp only [identity, Bool.and_eq_true] at one
  exact ⟨beq_iff_eq.mp one.1, beq_iff_eq.mp one.2⟩

private def peerLoanPort (session : Nat) : TransportChannel := ⟨session, 0, 0, 0, 1⟩

private theorem peerLoanOpenedSelector
    {session : Nat} {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance} {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution peerSourceImages peerWitnessChoreo session
      (DistributedSourceCut.initial initial, []) last records)
    (sameRun : ∀ entry ∈ last.2, entry.channel.generation = 0) :
    ∀ entry ∈ last.2,
      (decide (entry.channel = peerLoanPort session) && entry.sample.wire.frameLabel == 0) = (entry.event == 0) := by
  intro entry member
  have facts := a_distributed_acceptance_retains_its_source_and_opened_port history entry member
  obtain ⟨eff, binding, identity, sourceAt, sessionAt, actorAt, senderAt, receiverAt, laneAt, colorAt⟩ :=
    checked_source_send_has_its_bound_wire_columns facts.2.1
  have effect : eff = entry.event := Option.some.inj (sourceAt.symm.trans facts.2.2.1)
  subst eff
  have staticFields := peerLoanSourceClassifies facts.1 identity
  have lane : entry.sample.wire.lane = 0 := Option.some.inj (laneAt.symm.trans staticFields.1)
  have port := facts.2.2.2
  have sessionBound : entry.channel.session = session := port.1.symm.trans sessionAt
  have laneBound : entry.channel.lane = 0 := port.2.1.symm.trans lane
  have senderBound : entry.channel.sender = binding.message.sender.role := port.2.2.1.symm.trans senderAt
  have receiverBound : entry.channel.receiver = binding.message.receiver.role := port.2.2.2.symm.trans receiverAt
  rw [staticFields.2, colorAt, ← senderBound, ← receiverBound]
  have runGeneration := sameRun entry member
  cases channelAt : entry.channel with
  | mk current generation lane sender receiver =>
    simp only [channelAt] at sessionBound laneBound runGeneration ⊢
    subst current; subst generation; subst lane
    by_cases senderIs : sender = 0 <;> by_cases towards : receiver = 1 <;>
      by_cases color : entry.sample.wire.frameLabel = 0 <;>
      simp [peerLoanPort, TransportChannel.mk.injEq, senderIs, towards, color]

/-- Any finite prefix of the three-holder nested choice determines the next
loan source from the actual raw FIFO head. Other branches use different wire
colors on this same port. They remain in its complete history. No receive
commit, matching receiver snapshot, selected holder or common path is supplied.
Earlier receive correspondence remains the explicit execution premise. -/
theorem peerProspectiveLoanHasItsSourceBeforeReceive
    {session : Nat} {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance} {records : List (Nat × ActorCommitRecord)}
    {next : TransportState} {frame : TransportFrame}
    (history : DistributedSourceExecution peerSourceImages peerWitnessChoreo session
      (DistributedSourceCut.initial initial, []) last records)
    (sameRun : ∀ entry ∈ last.2, entry.channel.generation = 0)
    (received : (last.1.carriers (peerLoanPort session)).pollReceive = .delivered frame next)
    (color : frame.frameLabel.val = 0) :
    ∃ sample fifo key,
      SendAdmissionTrace peerWitnessChoreo session (last.1.carriers (peerLoanPort session)) fifo
        (sourceChannelRecords last.2 (peerLoanPort session)) ∧
      fifo.accepted[(last.1.carriers (peerLoanPort session)).delivered]? = some key ∧
      (sourceChannelRecords last.2 (peerLoanPort session))[
        (last.1.carriers (peerLoanPort session)).delivered]? = some (sample, 0) ∧
      sample.check peerWitnessChoreo session = true ∧ sample.effIndex? = some 0 ∧
      sample.wire.identifies frame ∧ key.globalId = 0 := by
  obtain ⟨sample, event, fifo, key, trace, keyAt, recordAt, checked, sourceAt,
    identity, keySource, _, chosen⟩ :=
    a_prospective_port_head_has_its_actual_source_class_position
      (fun eff => eff == 0) (fun wireColor => wireColor == 0) history received
      (peerLoanOpenedSelector history sameRun) (by simp only [color, beq_self_eq_true])
  have same : event = 0 := beq_iff_eq.mp chosen
  rw [same] at recordAt sourceAt keySource
  exact ⟨sample, fifo, key, trace, keyAt, recordAt, checked, sourceAt, identity, keySource⟩
