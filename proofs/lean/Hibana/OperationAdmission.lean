import Hibana.Commit

namespace Hibana

/-- Result of checking one runtime-erased endpoint operation. -/
inductive AdmissionResult where
  | accepted (state : CommitState)
  | rejected

def admitOperation
    (graph : EventGraph) (state : CommitState) (request : OperationRequest) : AdmissionResult :=
  match matchingOperationEvent? graph request with
  | none => .rejected
  | some _ =>
      match commitEvent graph state request.eventId with
      | none => .rejected
      | some next => .accepted next

/-- Exact decision domain of mediated endpoint operation admission. This is
relative to one descriptor operation and commit state; it is not black-box
process monitorability. -/
def OperationAdmissible
    (graph : EventGraph) (state : CommitState) (request : OperationRequest) : Prop :=
  ∃ event next,
    matchingOperationEvent? graph request = some event ∧
    commitEvent graph state request.eventId = some next

/-- Rejection retains the exact pre-operation commit state. -/
def applyAdmission
    (graph : EventGraph) (state : CommitState) (request : OperationRequest) : CommitState :=
  match admitOperation graph state request with
  | .accepted next => next
  | .rejected => state

theorem matching_operation_event_is_exact
    {graph : EventGraph} {request : OperationRequest} {event : Event}
    (matched : matchingOperationEvent? graph request = some event) :
    graph.events[request.eventId]? = some event ∧
      event.id = request.eventId ∧ event.action = request.action := by
  unfold matchingOperationEvent? at matched
  cases eventCase : graph.events[request.eventId]? with
  | none => simp [eventCase] at matched
  | some candidate =>
      by_cases exact : candidate.operationRequest = request
      · have candidateEq : candidate = event := by
          exact Option.some.inj (by simpa [eventCase, exact] using matched)
        subst event
        have eventId : candidate.id = request.eventId := by
          exact congrArg OperationRequest.eventId exact
        have action : candidate.action = request.action := by
          exact congrArg OperationRequest.action exact
        exact ⟨rfl, eventId, action⟩
      · simp [eventCase, exact] at matched

theorem admission_rejects_missing_event
    {graph : EventGraph} {state : CommitState} {request : OperationRequest}
    (missing : graph.events[request.eventId]? = none) :
    admitOperation graph state request = .rejected := by
  simp [admitOperation, matchingOperationEvent?, missing]

theorem admission_rejects_descriptor_id_mismatch
    {graph : EventGraph} {state : CommitState} {request : OperationRequest} {event : Event}
    (present : graph.events[request.eventId]? = some event)
    (mismatch : event.id ≠ request.eventId) :
    admitOperation graph state request = .rejected := by
  have requestMismatch : event.operationRequest ≠ request := by
    intro same
    exact mismatch (congrArg OperationRequest.eventId same)
  simp [admitOperation, matchingOperationEvent?, present, requestMismatch]

theorem admission_rejects_action_mismatch
    {graph : EventGraph} {state : CommitState} {request : OperationRequest} {event : Event}
    (present : graph.events[request.eventId]? = some event)
    (mismatch : event.action ≠ request.action) :
    admitOperation graph state request = .rejected := by
  have requestMismatch : event.operationRequest ≠ request := by
    intro same
    exact mismatch (congrArg OperationRequest.action same)
  simp [admitOperation, matchingOperationEvent?, present, requestMismatch]

theorem admission_rejection_preserves_state
    {graph : EventGraph} {state : CommitState} {request : OperationRequest}
    (rejected : admitOperation graph state request = .rejected) :
    applyAdmission graph state request = state := by
  simp [applyAdmission, rejected]

theorem admission_acceptance_is_exact_commit
    {graph : EventGraph} {state next : CommitState} {request : OperationRequest}
    (accepted : admitOperation graph state request = .accepted next) :
    ∃ event,
      matchingOperationEvent? graph request = some event ∧
      graph.events[request.eventId]? = some event ∧
      event.id = request.eventId ∧
      event.action = request.action ∧
      commitEvent graph state request.eventId = some next := by
  unfold admitOperation at accepted
  cases matchCase : matchingOperationEvent? graph request with
  | none => simp [matchCase] at accepted
  | some event =>
      cases commitCase : commitEvent graph state request.eventId with
      | none => simp [matchCase, commitCase] at accepted
      | some candidate =>
          have acceptedStates : AdmissionResult.accepted candidate = .accepted next := by
            simpa [matchCase, commitCase] using accepted
          have candidateEq : candidate = next := AdmissionResult.accepted.inj acceptedStates
          subst next
          obtain ⟨present, eventId, action⟩ := matching_operation_event_is_exact matchCase
          exact ⟨event, rfl, present, eventId, action, rfl⟩

theorem admission_acceptance_iff_admissible
    {graph : EventGraph} {state : CommitState} {request : OperationRequest} :
    (∃ next, admitOperation graph state request = .accepted next) ↔
      OperationAdmissible graph state request := by
  constructor
  · rintro ⟨next, accepted⟩
    obtain ⟨event, matched, present, eventId, action, committed⟩ :=
      admission_acceptance_is_exact_commit accepted
    exact ⟨event, next, matched, committed⟩
  · rintro ⟨event, next, matched, committed⟩
    exact ⟨next, by simp [admitOperation, matched, committed]⟩

theorem admission_rejection_iff_not_admissible
    {graph : EventGraph} {state : CommitState} {request : OperationRequest} :
    admitOperation graph state request = .rejected ↔
      ¬ OperationAdmissible graph state request := by
  constructor
  · intro rejected admissible
    obtain ⟨next, accepted⟩ := admission_acceptance_iff_admissible.mpr admissible
    rw [rejected] at accepted
    contradiction
  · intro inadmissible
    cases result : admitOperation graph state request with
    | rejected => rfl
    | accepted next =>
        exact False.elim
          (inadmissible (admission_acceptance_iff_admissible.mp ⟨next, result⟩))

theorem admission_rejects_send_schema_mismatch
    {graph : EventGraph} {state : CommitState} {event : Event}
    {eventId peer label expectedSchema actualSchema : Nat}
    (present : graph.events[eventId]? = some event)
    (expected : event.action = .send peer label expectedSchema)
    (mismatch : actualSchema ≠ expectedSchema) :
    admitOperation graph state
      { eventId, action := .send peer label actualSchema } = .rejected := by
  apply admission_rejects_action_mismatch present
  rw [expected]
  change LocalAction.send peer label expectedSchema ≠
    LocalAction.send peer label actualSchema
  intro same
  have schemaEq : expectedSchema = actualSchema := by injection same
  exact mismatch schemaEq.symm

theorem admission_rejects_send_peer_mismatch
    {graph : EventGraph} {state : CommitState} {event : Event}
    {eventId expectedPeer actualPeer label schema : Nat}
    (present : graph.events[eventId]? = some event)
    (expected : event.action = .send expectedPeer label schema)
    (mismatch : actualPeer ≠ expectedPeer) :
    admitOperation graph state
      { eventId, action := .send actualPeer label schema } = .rejected := by
  apply admission_rejects_action_mismatch present
  rw [expected]
  change LocalAction.send expectedPeer label schema ≠
    LocalAction.send actualPeer label schema
  intro same
  have peerEq : expectedPeer = actualPeer := by injection same
  exact mismatch peerEq.symm

theorem admission_rejects_wrong_direction
    {graph : EventGraph} {state : CommitState} {event : Event}
    {eventId peer label schema : Nat}
    (present : graph.events[eventId]? = some event)
    (expected : event.action = .recv peer label schema) :
    admitOperation graph state { eventId, action := .send peer label schema } = .rejected := by
  apply admission_rejects_action_mismatch present
  rw [expected]
  simp

/-- The first incomplete local event in one compiled lane. Other lanes do not
    supply an index for this lane's initial ingress dispatch. -/
private def laneIngressHead (events : List (Nat × Nat)) (lane : Nat)
    (completed : Nat → Bool) : Option (Nat × Nat) :=
  events.find? fun event => decide (event.2 = lane) && !completed event.1

/-- Progress outside the observed lane leaves its ingress head unchanged. The
    premise binds the completion view for every event owned by that lane. -/
theorem lane_ingress_head_preserved_by_other_lane_progress
    (events : List (Nat × Nat)) (lane : Nat) (before after : Nat → Bool)
    (sameLane : ∀ event ∈ events, event.2 = lane → before event.1 = after event.1) :
    laneIngressHead events lane before = laneIngressHead events lane after := by
  unfold laneIngressHead
  induction events with
  | nil => rfl
  | cons event rest ih =>
      have owned : event.2 = lane → before event.1 = after event.1 :=
        sameLane event (List.mem_cons_self)
      have tail : ∀ candidate ∈ rest, candidate.2 = lane →
          before candidate.1 = after candidate.1 := by
        intro candidate member
        exact sameLane candidate (List.mem_cons_of_mem event member)
      by_cases inLane : event.2 = lane
      · simp [List.find?, inLane, owned inLane, ih tail]
      · simp [List.find?, inLane, ih tail]

/-- Updating a different, uniquely identified event cannot change this lane's
    head, even if the shared cursor is moved to that event. -/
theorem foreign_event_commit_preserves_lane_ingress_head
    (events : List (Nat × Nat)) (lane committedId : Nat) (before : Nat → Bool)
    (foreign : ∀ event ∈ events, event.2 = lane → event.1 ≠ committedId) :
    laneIngressHead events lane before = laneIngressHead events lane
      (fun eventId => if eventId = committedId then true else before eventId) := by
  apply lane_ingress_head_preserved_by_other_lane_progress
  intro event member owned
  simp [foreign event member owned]

end Hibana
