import Hibana.ElasticAdmissionHistory

namespace Hibana

/-- Production-visible identity of an elastic occurrence. The proof ordinal is
not retained. -/
def ElasticOccurrenceKey.erase
    (key : ElasticOccurrenceKey) : Nat :=
  key.globalId

def ElasticAdmissionHistory.erasedTrace
    (history : ElasticAdmissionHistory) : List Nat :=
  history.accepted.map ElasticOccurrenceKey.erase

/-- Exact alignment with the existing carrier state. `frameLabelOf` is derived
from the accepted descriptor, not stored in the carrier. The relation checks
both histories, channel identity, affine delivery, and the complete admitted
label trace while retaining no roll iteration or per-occurrence epoch. -/
def ElasticAdmissionHistory.ErasesToTransport
    (frameLabelOf : Nat -> Fin 256)
    (history : ElasticAdmissionHistory)
    (transport : TransportState) : Prop :=
  history.WellFormed /\
    transport.WellFormed /\
    history.channel = transport.channel /\
    history.delivered = transport.delivered /\
    history.erasedTrace.map frameLabelOf =
      transport.frames.map TransportFrame.frameLabel

theorem elastic_occurrence_erasure_forgets_iteration
    {left right : ElasticOccurrenceKey}
    (sameEvent : left.globalId = right.globalId) :
    left.erase = right.erase :=
  sameEvent

theorem elastic_admission_append_commutes_with_trace_erasure
    (history : ElasticAdmissionHistory)
    (globalId : Nat) :
    (history.appendOccurrence globalId).erasedTrace =
      history.erasedTrace ++ [globalId] := by
  simp [ElasticAdmissionHistory.erasedTrace,
    ElasticAdmissionHistory.appendOccurrence,
    ElasticAdmissionHistory.nextOccurrence,
    ElasticOccurrenceKey.erase]

theorem elastic_admission_append_erasure_is_iteration_independent
    (history : ElasticAdmissionHistory)
    (globalId : Nat) :
    (history.appendOccurrence globalId).erasedTrace =
      history.erasedTrace ++ [globalId] /\
    (history.appendOccurrence globalId).delivered = history.delivered := by
  exact ⟨elastic_admission_append_commutes_with_trace_erasure history globalId,
    rfl⟩

theorem elastic_admission_receive_commutes_with_trace_erasure
    {history next : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey}
    (received : history.receive? = some (key, next)) :
    next.erasedTrace = history.erasedTrace /\
      next.delivered = history.delivered + 1 := by
  have exactReceive := elastic_admission_receive_is_exact_fifo received
  exact ⟨by simp [ElasticAdmissionHistory.erasedTrace, exactReceive.2.2.1],
    exactReceive.2.2.2.1⟩

theorem elastic_transport_erasure_initial
    (frameLabelOf : Nat -> Fin 256)
    (channel : TransportChannel) :
    (ElasticAdmissionHistory.initial channel).ErasesToTransport frameLabelOf
      (TransportState.initial channel) := by
  exact ⟨
    elastic_admission_history_initial_well_formed channel,
    initial_transport_is_well_formed channel,
    rfl,
    rfl,
    rfl
  ⟩

theorem elastic_transport_trace_is_exact_erasure
    {frameLabelOf : Nat -> Fin 256}
    {history : ElasticAdmissionHistory}
    {transport : TransportState}
    (erases : history.ErasesToTransport frameLabelOf transport) :
    history.erasedTrace.map frameLabelOf =
      transport.frames.map TransportFrame.frameLabel :=
  erases.2.2.2.2

theorem elastic_transport_erasure_preserved_by_send
    {transport sent : TransportState}
    {history : ElasticAdmissionHistory}
    {frameLabel : Fin 256}
    {globalId : Nat}
    {frameLabelOf : Nat -> Fin 256}
    (erases : history.ErasesToTransport frameLabelOf transport)
    (labelExact : frameLabelOf globalId = frameLabel)
    (accepted : transport.send frameLabel = some sent) :
    (history.appendOccurrence globalId).ErasesToTransport frameLabelOf sent := by
  obtain ⟨historyWellFormed, transportWellFormed, channelAligned,
    deliveryAligned, traceAligned⟩ := erases
  have historyNextWellFormed :=
    elastic_admission_preserves_well_formed historyWellFormed globalId
  have transportNextWellFormed :=
    transport_send_preserves_well_formed transportWellFormed accepted
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
    exact ⟨
      historyNextWellFormed,
      transportNextWellFormed,
      channelAligned,
      deliveryAligned,
      by
        rw [elastic_admission_append_commutes_with_trace_erasure]
        simp [traceAligned, labelExact]
    ⟩

theorem elastic_transport_erasure_preserved_by_receive
    {transport nextTransport : TransportState}
    {frame : TransportFrame}
    {history nextHistory : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey}
    {frameLabelOf : Nat -> Fin 256}
    (erases : history.ErasesToTransport frameLabelOf transport)
    (historyReceived : history.receive? = some (key, nextHistory))
    (transportReceived :
      transport.pollReceive = .delivered frame nextTransport) :
    nextHistory.ErasesToTransport frameLabelOf nextTransport := by
  obtain ⟨historyWellFormed, transportWellFormed, channelAligned,
    deliveryAligned, traceAligned⟩ := erases
  have historyExact := elastic_admission_receive_is_exact_fifo historyReceived
  have transportExact := transport_receive_is_fifo transportReceived
  have erasedExact :=
    (elastic_admission_receive_commutes_with_trace_erasure historyReceived).1
  exact ⟨
    elastic_admission_receive_preserves_well_formed
      historyWellFormed historyReceived,
    transport_receive_preserves_well_formed
      transportWellFormed transportReceived,
    historyExact.2.1.trans (channelAligned.trans transportExact.2.1.symm),
    by rw [historyExact.2.2.2.1, transportExact.2.2.2, deliveryAligned],
    by rw [erasedExact, transportExact.2.2.1, traceAligned]
  ⟩

/-- Erased route publication identity. Production retains conflict and branch
evidence but not the proof ordinal. -/
def ElasticRoutePublication.erase
    (publication : ElasticRoutePublication) : Nat × RouteArm :=
  (publication.key.conflict, publication.arm)

def ElasticRouteHistory.erasedTrace
    (history : ElasticRouteHistory) : List (Nat × RouteArm) :=
  history.published.map ElasticRoutePublication.erase

theorem elastic_route_publish_commutes_with_trace_erasure
    (history : ElasticRouteHistory)
    (conflict : Nat)
    (arm : RouteArm) :
    (history.publish conflict arm).erasedTrace =
      history.erasedTrace ++ [(conflict, arm)] := by
  simp [ElasticRouteHistory.erasedTrace, ElasticRouteHistory.publish,
    ElasticRouteHistory.nextPublication, ElasticRoutePublication.erase]

/-- General erasure/refinement law consumed by the end-to-end theorem. All
elastic identity is confined to the Lean witness; the resulting traces use
only static descriptor identity, route conflict/arm, and carrier cursors. -/
structure ElasticErasureRefinement : Prop where
  admissionAppend : forall (history : ElasticAdmissionHistory) globalId,
    (history.appendOccurrence globalId).erasedTrace =
      history.erasedTrace ++ [globalId]
  admissionReceive : forall
    {history next : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey},
    history.receive? = some (key, next) ->
      next.erasedTrace = history.erasedTrace /\
      next.delivered = history.delivered + 1
  transportInitial : forall frameLabelOf channel,
    (ElasticAdmissionHistory.initial channel).ErasesToTransport frameLabelOf
      (TransportState.initial channel)
  transportSend : forall
    {transport sent : TransportState}
    {history : ElasticAdmissionHistory}
    {frameLabel : Fin 256}
    {globalId : Nat}
    {frameLabelOf : Nat -> Fin 256},
    history.ErasesToTransport frameLabelOf transport ->
    frameLabelOf globalId = frameLabel ->
    transport.send frameLabel = some sent ->
      (history.appendOccurrence globalId).ErasesToTransport frameLabelOf sent
  transportReceive : forall
    {transport nextTransport : TransportState}
    {frame : TransportFrame}
    {history nextHistory : ElasticAdmissionHistory}
    {key : ElasticOccurrenceKey}
    {frameLabelOf : Nat -> Fin 256},
    history.ErasesToTransport frameLabelOf transport ->
    history.receive? = some (key, nextHistory) ->
    transport.pollReceive = .delivered frame nextTransport ->
      nextHistory.ErasesToTransport frameLabelOf nextTransport
  transportTraceExact : forall
    {frameLabelOf : Nat -> Fin 256}
    {history : ElasticAdmissionHistory}
    {transport : TransportState},
    history.ErasesToTransport frameLabelOf transport ->
      history.erasedTrace.map frameLabelOf =
        transport.frames.map TransportFrame.frameLabel
  routePublish : forall (history : ElasticRouteHistory) conflict arm,
    (history.publish conflict arm).erasedTrace =
      history.erasedTrace ++ [(conflict, arm)]

theorem elastic_erasure_refinement_holds : ElasticErasureRefinement := {
  admissionAppend := elastic_admission_append_commutes_with_trace_erasure
  admissionReceive := fun received =>
    elastic_admission_receive_commutes_with_trace_erasure received
  transportInitial := elastic_transport_erasure_initial
  transportSend := elastic_transport_erasure_preserved_by_send
  transportReceive := elastic_transport_erasure_preserved_by_receive
  transportTraceExact := elastic_transport_trace_is_exact_erasure
  routePublish := elastic_route_publish_commutes_with_trace_erasure
}

end Hibana
