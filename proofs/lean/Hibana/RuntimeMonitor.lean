import Hibana.Commit

namespace Hibana

/-- Descriptor resolution identifies one exact event before runtime state may
commit. The action carries direction, peer, label, and payload schema. -/
structure MonitorRequest where
  eventId : Nat
  action : LocalAction
  deriving Repr, DecidableEq

/-- Result of checking one runtime-erased endpoint operation. -/
inductive MonitorResult where
  | accepted (state : CommitState)
  | rejected

/-- Resolve only the requested descriptor event, rejecting stale IDs and every
operation-contract mismatch before commit. -/
def matchingMonitorEvent?
    (graph : EventGraph) (request : MonitorRequest) : Option Event :=
  match graph.events[request.eventId]? with
  | none => none
  | some event =>
      if event.id = request.eventId ∧ event.action = request.action then
        some event
      else
        none

def monitorStep
    (graph : EventGraph) (state : CommitState) (request : MonitorRequest) : MonitorResult :=
  match matchingMonitorEvent? graph request with
  | none => .rejected
  | some _ =>
      match commitEvent graph state request.eventId with
      | none => .rejected
      | some next => .accepted next

/-- Rejection retains the exact pre-operation commit state. -/
def applyMonitor
    (graph : EventGraph) (state : CommitState) (request : MonitorRequest) : CommitState :=
  match monitorStep graph state request with
  | .accepted next => next
  | .rejected => state

theorem matching_monitor_event_is_exact
    {graph : EventGraph} {request : MonitorRequest} {event : Event}
    (matched : matchingMonitorEvent? graph request = some event) :
    graph.events[request.eventId]? = some event ∧
      event.id = request.eventId ∧ event.action = request.action := by
  unfold matchingMonitorEvent? at matched
  cases eventCase : graph.events[request.eventId]? with
  | none => simp [eventCase] at matched
  | some candidate =>
      by_cases exact : candidate.id = request.eventId ∧ candidate.action = request.action
      · have matchedOptions : some candidate = some event := by
          simpa [eventCase, exact] using matched
        have candidateEq : candidate = event := Option.some.inj matchedOptions
        subst event
        exact ⟨rfl, exact⟩
      · simp [eventCase, exact] at matched

theorem monitor_rejects_missing_event
    {graph : EventGraph} {state : CommitState} {request : MonitorRequest}
    (missing : graph.events[request.eventId]? = none) :
    monitorStep graph state request = .rejected := by
  simp [monitorStep, matchingMonitorEvent?, missing]

theorem monitor_rejects_descriptor_id_mismatch
    {graph : EventGraph} {state : CommitState} {request : MonitorRequest} {event : Event}
    (present : graph.events[request.eventId]? = some event)
    (mismatch : event.id ≠ request.eventId) :
    monitorStep graph state request = .rejected := by
  simp [monitorStep, matchingMonitorEvent?, present, mismatch]

theorem monitor_rejects_action_mismatch
    {graph : EventGraph} {state : CommitState} {request : MonitorRequest} {event : Event}
    (present : graph.events[request.eventId]? = some event)
    (mismatch : event.action ≠ request.action) :
    monitorStep graph state request = .rejected := by
  simp [monitorStep, matchingMonitorEvent?, present, mismatch]

theorem monitor_rejection_preserves_state
    {graph : EventGraph} {state : CommitState} {request : MonitorRequest}
    (rejected : monitorStep graph state request = .rejected) :
    applyMonitor graph state request = state := by
  simp [applyMonitor, rejected]

theorem monitor_acceptance_is_exact_commit
    {graph : EventGraph} {state next : CommitState} {request : MonitorRequest}
    (accepted : monitorStep graph state request = .accepted next) :
    ∃ event,
      matchingMonitorEvent? graph request = some event ∧
      graph.events[request.eventId]? = some event ∧
      event.id = request.eventId ∧
      event.action = request.action ∧
      commitEvent graph state request.eventId = some next := by
  unfold monitorStep at accepted
  cases matchCase : matchingMonitorEvent? graph request with
  | none => simp [matchCase] at accepted
  | some event =>
      cases commitCase : commitEvent graph state request.eventId with
      | none => simp [matchCase, commitCase] at accepted
      | some candidate =>
          have acceptedStates : MonitorResult.accepted candidate = .accepted next := by
            simpa [matchCase, commitCase] using accepted
          have candidateEq : candidate = next := MonitorResult.accepted.inj acceptedStates
          subst next
          obtain ⟨present, eventId, action⟩ := matching_monitor_event_is_exact matchCase
          exact ⟨event, rfl, present, eventId, action, rfl⟩

theorem monitor_rejects_send_schema_mismatch
    {graph : EventGraph} {state : CommitState} {event : Event}
    {eventId peer label expectedSchema actualSchema : Nat}
    (present : graph.events[eventId]? = some event)
    (expected : event.action = .send peer label expectedSchema)
    (mismatch : actualSchema ≠ expectedSchema) :
    monitorStep graph state { eventId, action := .send peer label actualSchema } = .rejected := by
  apply monitor_rejects_action_mismatch present
  rw [expected]
  change LocalAction.send peer label expectedSchema ≠
    LocalAction.send peer label actualSchema
  intro same
  have schemaEq : expectedSchema = actualSchema := by injection same
  exact mismatch schemaEq.symm

theorem monitor_rejects_send_peer_mismatch
    {graph : EventGraph} {state : CommitState} {event : Event}
    {eventId expectedPeer actualPeer label schema : Nat}
    (present : graph.events[eventId]? = some event)
    (expected : event.action = .send expectedPeer label schema)
    (mismatch : actualPeer ≠ expectedPeer) :
    monitorStep graph state { eventId, action := .send actualPeer label schema } = .rejected := by
  apply monitor_rejects_action_mismatch present
  rw [expected]
  change LocalAction.send expectedPeer label schema ≠
    LocalAction.send actualPeer label schema
  intro same
  have peerEq : expectedPeer = actualPeer := by injection same
  exact mismatch peerEq.symm

theorem monitor_rejects_wrong_direction
    {graph : EventGraph} {state : CommitState} {event : Event}
    {eventId peer label schema : Nat}
    (present : graph.events[eventId]? = some event)
    (expected : event.action = .recv peer label schema) :
    monitorStep graph state { eventId, action := .send peer label schema } = .rejected := by
  apply monitor_rejects_action_mismatch present
  rw [expected]
  simp

end Hibana
