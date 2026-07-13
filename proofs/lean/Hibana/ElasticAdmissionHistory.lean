import Hibana.ElasticRouteHistory

namespace Hibana

/-- Iterations of one static occurrence, extracted from an admitted FIFO
history that may also contain other descriptor occurrences on the same carrier
channel. -/
def occurrenceGenerations
    (accepted : List ElasticOccurrenceKey)
    (globalId : Nat) : List Nat :=
  accepted.filterMap fun key =>
    if key.globalId = globalId then
      some key.iteration
    else
      none

@[simp]
private theorem occurrence_generations_append
    (accepted : List ElasticOccurrenceKey)
    (key : ElasticOccurrenceKey)
    (globalId : Nat) :
    occurrenceGenerations (accepted ++ [key]) globalId =
      occurrenceGenerations accepted globalId ++
        if key.globalId = globalId then
          [key.iteration]
        else
          [] := by
  by_cases same : key.globalId = globalId
  · simp [occurrenceGenerations, same]
  · simp [occurrenceGenerations, same]

/-- Proof-only admission history for one authenticated FIFO direction.
`nextIteration` is derived state: production stores only its descriptor cursor
and the carrier's affine sequence. -/
structure ElasticAdmissionHistory where
  channel : TransportChannel
  accepted : List ElasticOccurrenceKey
  delivered : Nat
  nextIteration : Nat -> Nat

def ElasticAdmissionHistory.MatchesEventChannel
    (history : ElasticAdmissionHistory)
    (event : GlobalEvent) : Prop :=
  history.channel.sender = event.sender /\
    history.channel.receiver = event.receiver /\
    history.channel.lane = event.lane

def ElasticAdmissionHistory.initial
    (channel : TransportChannel) : ElasticAdmissionHistory := {
  channel
  accepted := []
  delivered := 0
  nextIteration := fun _ => 0
}

/-- Every static occurrence contributes exactly generations `0 .. next-1` to
the mixed channel history. Together with `Nodup`, this excludes generation
holes, duplicate admission, and replay while allowing unrelated events to
interleave. -/
structure ElasticAdmissionHistory.WellFormed
    (history : ElasticAdmissionHistory) : Prop where
  deliveryBound : history.delivered <= history.accepted.length
  acceptedNodup : history.accepted.Nodup
  exactGenerations : forall globalId,
    occurrenceGenerations history.accepted globalId =
      List.range (history.nextIteration globalId)

def ElasticAdmissionHistory.nextOccurrence
    (history : ElasticAdmissionHistory)
    (globalId : Nat) : ElasticOccurrenceKey := {
  globalId
  iteration := history.nextIteration globalId
}

def ElasticAdmissionHistory.appendOccurrence
    (history : ElasticAdmissionHistory)
    (globalId : Nat) : ElasticAdmissionHistory :=
  let key := history.nextOccurrence globalId
  {
    history with
    accepted := history.accepted ++ [key]
    nextIteration := fun queryGlobal =>
      if globalId = queryGlobal then
        history.nextIteration queryGlobal + 1
      else
        history.nextIteration queryGlobal
  }

def ElasticAdmissionHistory.receive?
    (history : ElasticAdmissionHistory) :
    Option (ElasticOccurrenceKey × ElasticAdmissionHistory) :=
  match history.accepted[history.delivered]? with
  | none => none
  | some key => some (key, { history with delivered := history.delivered + 1 })

theorem elastic_admission_history_initial_well_formed :
    ∀ channel, (ElasticAdmissionHistory.initial channel).WellFormed := by
  intro channel
  refine {
    deliveryBound := by simp [ElasticAdmissionHistory.initial]
    acceptedNodup := by simp [ElasticAdmissionHistory.initial]
    exactGenerations := ?_
  }
  intro globalId
  simp [ElasticAdmissionHistory.initial, occurrenceGenerations]

theorem elastic_admission_next_occurrence_is_fresh
    {history : ElasticAdmissionHistory}
    (wellFormed : history.WellFormed)
    (globalId : Nat) :
    history.nextOccurrence globalId ∉ history.accepted := by
  intro member
  have generationMember :
      history.nextIteration globalId ∈
        occurrenceGenerations history.accepted globalId := by
    apply List.mem_filterMap.mpr
    exact ⟨history.nextOccurrence globalId, member, by
      simp [ElasticAdmissionHistory.nextOccurrence]⟩
  rw [wellFormed.exactGenerations globalId] at generationMember
  exact Nat.lt_irrefl _ (List.mem_range.mp generationMember)

theorem elastic_admission_preserves_well_formed
    {history : ElasticAdmissionHistory}
    (wellFormed : history.WellFormed)
    (globalId : Nat) :
    (history.appendOccurrence globalId).WellFormed := by
  let key := history.nextOccurrence globalId
  have fresh : key ∉ history.accepted :=
    elastic_admission_next_occurrence_is_fresh wellFormed globalId
  refine {
    deliveryBound := ?_
    acceptedNodup := ?_
    exactGenerations := ?_
  }
  · change history.delivered <=
      (history.accepted ++ [history.nextOccurrence globalId]).length
    have bound := wellFormed.deliveryBound
    simp
    omega
  · change (history.accepted ++ [key]).Nodup
    rw [List.nodup_append]
    refine ⟨wellFormed.acceptedNodup, by simp, ?_⟩
    intro prior priorMember appended appendedMember
    have appendedEq : appended = key := by simpa using appendedMember
    subst appended
    intro same
    apply fresh
    simpa [same] using priorMember
  · intro queryGlobal
    change occurrenceGenerations (history.accepted ++ [key]) queryGlobal =
      List.range (if globalId = queryGlobal then
        history.nextIteration queryGlobal + 1
      else
        history.nextIteration queryGlobal)
    rw [occurrence_generations_append]
    by_cases sameGlobal : globalId = queryGlobal
    · subst queryGlobal
      simp [key, ElasticAdmissionHistory.nextOccurrence,
        wellFormed.exactGenerations, List.range_succ]
    · simp [key, ElasticAdmissionHistory.nextOccurrence,
        sameGlobal, wellFormed.exactGenerations]

theorem elastic_admission_appends_exact_occurrence
    (history : ElasticAdmissionHistory)
    (globalId : Nat) :
    (history.appendOccurrence globalId).accepted =
        history.accepted ++ [history.nextOccurrence globalId] /\
      (history.appendOccurrence globalId).channel = history.channel /\
      (history.nextOccurrence globalId).iteration =
        history.nextIteration globalId := by
  exact ⟨rfl, rfl, rfl⟩

theorem elastic_admission_receive_is_exact_fifo
    {history next : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey}
    (received : history.receive? = some (key, next)) :
    history.accepted[history.delivered]? = some key /\
      next.channel = history.channel /\
      next.accepted = history.accepted /\
      next.delivered = history.delivered + 1 /\
      next.nextIteration = history.nextIteration := by
  unfold ElasticAdmissionHistory.receive? at received
  cases present : history.accepted[history.delivered]? with
  | none => simp [present] at received
  | some candidate =>
      have pairEq :
          (candidate, { history with delivered := history.delivered + 1 }) =
            (key, next) := by
        exact Option.some.inj (by simpa [present] using received)
      cases pairEq
      exact ⟨rfl, rfl, rfl, rfl, rfl⟩

theorem elastic_admission_receive_preserves_well_formed
    {history next : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey}
    (wellFormed : history.WellFormed)
    (received : history.receive? = some (key, next)) :
    next.WellFormed := by
  obtain ⟨present, _, acceptedEq, deliveredEq, iterationEq⟩ :=
    elastic_admission_receive_is_exact_fifo received
  refine {
    deliveryBound := ?_
    acceptedNodup := ?_
    exactGenerations := ?_
  }
  · have bound := (List.getElem?_eq_some_iff.mp present).1
    rw [acceptedEq, deliveredEq]
    omega
  · simpa [acceptedEq] using wellFormed.acceptedNodup
  · intro globalId
    rw [acceptedEq, iterationEq]
    exact wellFormed.exactGenerations globalId

theorem elastic_admission_receive_advances_affinely
    {history next : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey}
    (received : history.receive? = some (key, next)) :
    history.delivered < next.delivered := by
  rw [(elastic_admission_receive_is_exact_fifo received).2.2.2.1]
  omega

theorem elastic_admission_consecutive_receives_are_distinct
    {history next after : ElasticAdmissionHistory}
    {first second : ElasticOccurrenceKey}
    (wellFormed : history.WellFormed)
    (receivedFirst : history.receive? = some (first, next))
    (receivedSecond : next.receive? = some (second, after)) :
    first ≠ second := by
  have firstExact := elastic_admission_receive_is_exact_fifo receivedFirst
  have secondExact := elastic_admission_receive_is_exact_fifo receivedSecond
  have firstAt := List.getElem?_eq_some_iff.mp firstExact.1
  have secondPresent :
      history.accepted[history.delivered + 1]? = some second := by
    simpa [firstExact.2.2.1, firstExact.2.2.2.1] using secondExact.1
  have secondAt := List.getElem?_eq_some_iff.mp secondPresent
  obtain ⟨firstBound, firstValue⟩ := firstAt
  obtain ⟨secondBound, secondValue⟩ := secondAt
  have distinctAt :=
    (List.pairwise_iff_getElem.mp wellFormed.acceptedNodup)
      history.delivered (history.delivered + 1)
      firstBound secondBound (by omega)
  simpa [firstValue, secondValue] using distinctAt

/-- Descriptor admission is the only source of an elastic occurrence identity.
The carrier contributes authenticated peer/lane/label evidence; the canonical
descriptor contributes `globalId`; FIFO history contributes the erased roll
generation. -/
theorem transport_admission_extends_exact_elastic_occurrence
    {choreo : Choreo}
    {frame : TransportFrame}
    {globalId : Nat}
    {event : GlobalEvent}
    {history : ElasticAdmissionHistory}
    (wellFormed : history.WellFormed)
    (sameChannel : history.channel = frame.channel)
    (admission : TransportAdmission choreo frame globalId event) :
    let key := history.nextOccurrence globalId
    let next := history.appendOccurrence globalId
    next.WellFormed /\
      next.channel = history.channel /\
      next.accepted = history.accepted ++ [key] /\
      key.globalId = globalId /\
      key.iteration = history.nextIteration globalId /\
      history.channel = frame.channel /\
      frame.channel.sender = event.sender /\
      frame.channel.receiver = event.receiver /\
      frame.channel.lane = event.lane := by
  obtain ⟨_, _, _, sender, receiver, lane, _⟩ :=
    transport_admission_binds_exact_descriptor_occurrence admission
  exact ⟨
    elastic_admission_preserves_well_formed wellFormed globalId,
    rfl,
    rfl,
    rfl,
    rfl,
    sameChannel,
    sender,
    receiver,
    lane
  ⟩

/-- One physical carrier append and one admitted ghost occurrence preserve
length and affine-cursor alignment. The generation stays proof-only; no field
is added to `TransportFrame`. -/
theorem elastic_admission_tracks_transport_send
    {transport sent : TransportState}
    {history : ElasticAdmissionHistory}
    {frameLabel : Fin 256}
    {globalId : Nat}
    (channelAligned : history.channel = transport.channel)
    (lengthAligned : history.accepted.length = transport.frames.length)
    (deliveryAligned : history.delivered = transport.delivered)
    (accepted : transport.send frameLabel = some sent) :
    let nextHistory := history.appendOccurrence globalId
    nextHistory.channel = sent.channel /\
      nextHistory.accepted.length = sent.frames.length /\
      nextHistory.delivered = sent.delivered := by
  unfold TransportState.send at accepted
  by_cases closed : transport.peerClosed
  · simp [closed] at accepted
  · have sentEq :
        { transport with
          frames := transport.frames ++ [{
            channel := transport.channel
            sequence := transport.frames.length
            frameLabel
          }] } = sent := by
      exact Option.some.inj (by simpa [closed] using accepted)
    subst sent
    simp [ElasticAdmissionHistory.appendOccurrence, channelAligned,
      lengthAligned, deliveryAligned]

/-- Mixed descriptor occurrences do not perturb each other's generation. The
same static event can be accepted twice before any receive, while an
unrelated occurrence remains generation zero. -/
theorem elastic_admission_history_models_mixed_pipelining :
    let channel : TransportChannel := {
      session := 7
      generation := 3
      lane := 0
      sender := 0
      receiver := 1
    }
    let start := ElasticAdmissionHistory.initial channel
    let first := start.appendOccurrence 4
    let mixed := first.appendOccurrence 9
    let pipelined := mixed.appendOccurrence 4
    pipelined.WellFormed /\
      pipelined.channel = channel /\
      occurrenceGenerations pipelined.accepted 4 = [0, 1] /\
      occurrenceGenerations pipelined.accepted 9 = [0] /\
      pipelined.delivered = 0 := by
  let channel : TransportChannel := {
    session := 7
    generation := 3
    lane := 0
    sender := 0
    receiver := 1
  }
  let start := ElasticAdmissionHistory.initial channel
  let first := start.appendOccurrence 4
  let mixed := first.appendOccurrence 9
  let pipelined := mixed.appendOccurrence 4
  have startWellFormed : start.WellFormed :=
    elastic_admission_history_initial_well_formed channel
  have firstWellFormed : first.WellFormed :=
    elastic_admission_preserves_well_formed startWellFormed 4
  have mixedWellFormed : mixed.WellFormed :=
    elastic_admission_preserves_well_formed firstWellFormed 9
  have pipelinedWellFormed : pipelined.WellFormed :=
    elastic_admission_preserves_well_formed mixedWellFormed 4
  refine ⟨pipelinedWellFormed, ?_⟩
  decide

/-- Normative elastic semantics used by the message-erased endpoint runtime.
It is host-only evidence: no counter, history, or key becomes a Pico resident
field or a transport header field. -/
structure ElasticPipelinedSemantics : Prop where
  initialWellFormed : forall channel,
    (ElasticAdmissionHistory.initial channel).WellFormed
  admissionPreserves : forall
    {history : ElasticAdmissionHistory},
    history.WellFormed -> forall globalId,
      (history.appendOccurrence globalId).WellFormed
  receivePreserves : forall
    {history next : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey},
    history.WellFormed ->
    history.receive? = some (key, next) ->
    next.WellFormed
  consecutiveReceivesDistinct : forall
    {history next after : ElasticAdmissionHistory}
    {first second : ElasticOccurrenceKey},
    history.WellFormed ->
    history.receive? = some (first, next) ->
    next.receive? = some (second, after) ->
    first ≠ second
  transportSendAligned : forall
    {transport sent : TransportState}
    {history : ElasticAdmissionHistory}
    {frameLabel : Fin 256}
    {globalId : Nat},
    history.channel = transport.channel ->
    history.accepted.length = transport.frames.length ->
    history.delivered = transport.delivered ->
    transport.send frameLabel = some sent ->
    let nextHistory := history.appendOccurrence globalId
    nextHistory.channel = sent.channel /\
      nextHistory.accepted.length = sent.frames.length /\
      nextHistory.delivered = sent.delivered
  transportAdmissionExtends : forall
    {choreo : Choreo}
    {frame : TransportFrame}
    {globalId : Nat}
    {event : GlobalEvent}
    {history : ElasticAdmissionHistory},
    history.WellFormed ->
    history.channel = frame.channel ->
    TransportAdmission choreo frame globalId event ->
    let key := history.nextOccurrence globalId
    let next := history.appendOccurrence globalId
    next.WellFormed /\
      next.channel = history.channel /\
      next.accepted = history.accepted ++ [key] /\
      key.globalId = globalId /\
      key.iteration = history.nextIteration globalId /\
      history.channel = frame.channel /\
      frame.channel.sender = event.sender /\
      frame.channel.receiver = event.receiver /\
      frame.channel.lane = event.lane
  mixedPipelining :
    let channel : TransportChannel := {
      session := 7
      generation := 3
      lane := 0
      sender := 0
      receiver := 1
    }
    let start := ElasticAdmissionHistory.initial channel
    let first := start.appendOccurrence 4
    let mixed := first.appendOccurrence 9
    let pipelined := mixed.appendOccurrence 4
    pipelined.WellFormed /\
      pipelined.channel = channel /\
      occurrenceGenerations pipelined.accepted 4 = [0, 1] /\
      occurrenceGenerations pipelined.accepted 9 = [0] /\
      pipelined.delivered = 0
  routeHistoryInitial : ElasticRouteHistory.initial.WellFormed
  routePublicationPreserves : forall
    {history : ElasticRouteHistory},
    history.WellFormed -> forall conflict arm,
      (history.publish conflict arm).WellFormed
  alternatingRoutePipelining :
    let first := ElasticRouteHistory.initial.publish 0 .left
    let second := first.publish 0 .right
    second.WellFormed /\
      second.published.map ElasticRoutePublication.arm = [.left, .right] /\
      routePublicationIterations second.published 0 = [0, 1]

theorem elastic_pipelined_semantics_holds : ElasticPipelinedSemantics := {
  initialWellFormed := elastic_admission_history_initial_well_formed
  admissionPreserves := fun wellFormed globalId =>
    elastic_admission_preserves_well_formed wellFormed globalId
  receivePreserves := fun wellFormed received =>
    elastic_admission_receive_preserves_well_formed wellFormed received
  consecutiveReceivesDistinct := fun wellFormed first second =>
    elastic_admission_consecutive_receives_are_distinct wellFormed first second
  transportSendAligned := fun channelAligned lengthAligned deliveryAligned accepted =>
    elastic_admission_tracks_transport_send
      channelAligned lengthAligned deliveryAligned accepted
  transportAdmissionExtends := fun wellFormed sameChannel admission =>
    transport_admission_extends_exact_elastic_occurrence
      wellFormed sameChannel admission
  mixedPipelining := elastic_admission_history_models_mixed_pipelining
  routeHistoryInitial := elastic_route_history_initial_well_formed
  routePublicationPreserves := fun wellFormed conflict arm =>
    elastic_route_publish_preserves_well_formed wellFormed conflict arm
  alternatingRoutePipelining :=
    elastic_route_history_models_alternating_pipelining
}

end Hibana
