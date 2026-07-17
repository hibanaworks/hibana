import Hibana.DescriptorTopology

namespace Hibana

/-- Exact resident bytes and their canonical column counts are the sole
production authority. No independently supplied topology can satisfy this
certificate. -/
structure ExactDescriptorCertificate where
  image : RustDescriptorImage
  choreo : Choreo

private def ExactDescriptorCertificate.ResidentMetadataMatches
    (certificate : ExactDescriptorCertificate) : Prop :=
  certificate.image.roleCount =
      certificate.choreo.canonicalRoleCount certificate.image.role ∧
    certificate.image.activeLaneCount =
      certificate.choreo.canonicalActiveLaneCount certificate.image.role ∧
    certificate.image.endpointLaneSlotCount =
      certificate.choreo.canonicalEndpointLaneSlotCount certificate.image.role ∧
    certificate.image.logicalLaneCount =
      certificate.choreo.canonicalLogicalLaneCount certificate.image.role ∧
    certificate.image.maxRouteCommitCount =
      certificate.choreo.canonicalRoleMaxRouteCommitCount certificate.image.role ∧
    certificate.image.firstActiveLane =
      certificate.choreo.canonicalFirstActiveLane certificate.image.role ∧
    (certificate.image.activeLaneStart, certificate.image.activeLaneLength) =
      certificate.choreo.canonicalActiveLaneRange certificate.image.role

private def ExactDescriptorCertificate.ProgramImageMatches
    (certificate : ExactDescriptorCertificate) : Prop :=
  certificate.image.decodeGlobalEvents? = some certificate.choreo.globalEvents ∧
    certificate.image.decodeAtoms? = some certificate.choreo.canonicalProgramAtoms ∧
    certificate.image.decodeRouteResolvers? =
      some certificate.choreo.canonicalRouteResolvers ∧
    certificate.image.decodeScopeMarkers? =
      some certificate.choreo.canonicalScopeMarkers

private def ExactDescriptorCertificate.RoleEventImageMatches
    (certificate : ExactDescriptorCertificate) : Prop :=
  certificate.image.decodeEventEffIndices? =
      some (certificate.choreo.canonicalRoleEffIndices certificate.image.role) ∧
    certificate.image.decodeEventFrameLabels? =
      some (certificate.choreo.canonicalRoleFrameLabels certificate.image.role) ∧
    certificate.image.decodeEventScopes? =
      some (certificate.choreo.canonicalRoleEventScopes certificate.image.role) ∧
    certificate.image.decodeEventFlags? =
      some (certificate.choreo.canonicalRoleEventFlags certificate.image.role) ∧
    certificate.image.decodeEventDependencyRows? =
      some (certificate.choreo.canonicalRoleEventDependencyRows certificate.image.role) ∧
    certificate.image.decodeDependencyRows? =
      some (certificate.choreo.canonicalRoleDependencies certificate.image.role) ∧
    certificate.image.decodeEventConflictRows? =
      some (certificate.choreo.canonicalRoleEventConflictRows certificate.image.role) ∧
    certificate.image.decodeConflictRows? =
      some (certificate.choreo.canonicalRoleConflicts certificate.image.role)

private def ExactDescriptorCertificate.RoleRouteImageMatches
    (certificate : ExactDescriptorCertificate) : Prop :=
  let graph := projectGraph certificate.image.role certificate.choreo
  certificate.image.decodeRouteScopeRows? =
      some certificate.choreo.canonicalRouteScopes ∧
    certificate.image.decodeRouteScopeConflictRows? =
      some certificate.choreo.canonicalRouteScopeConflicts ∧
    certificate.image.decodeRouteArmRows? =
      some (certificate.choreo.canonicalRoleRouteArms certificate.image.role) ∧
    certificate.image.decodeRouteArmLaneSteps? =
      some (certificate.choreo.canonicalRoleRouteArmLaneSteps certificate.image.role) ∧
    certificate.image.decodeResidentBoundaries? =
      some (certificate.choreo.canonicalRoleResidentBoundaries certificate.image.role) ∧
    certificate.image.decodeLaneBits? =
      some (certificate.choreo.canonicalRoleLaneBits certificate.image.role) ∧
    certificate.image.decodeRouteArmLaneRows? =
      some (certificate.choreo.canonicalRoleRouteArmLaneRows certificate.image.role) ∧
    certificate.image.decodeRouteOfferLaneRows? =
      some (certificate.choreo.canonicalRoleRouteOfferLaneRows certificate.image.role) ∧
    certificate.image.decodeRouteCommitRanges? =
      some certificate.choreo.canonicalRoleRouteCommitRanges ∧
    certificate.image.decodeRouteCommitRows? =
      some certificate.choreo.canonicalRoleRouteCommitRows ∧
    certificate.image.decodeRollRows? =
      some (certificate.choreo.canonicalRoleRollRows certificate.image.role) ∧
    certificate.image.decodeRouteAuthorities? =
      some graph.routeAuthoritiesByConflict

instance (certificate : ExactDescriptorCertificate) :
    Decidable certificate.ResidentMetadataMatches := by
  unfold ExactDescriptorCertificate.ResidentMetadataMatches
  infer_instance

instance (certificate : ExactDescriptorCertificate) :
    Decidable certificate.ProgramImageMatches := by
  unfold ExactDescriptorCertificate.ProgramImageMatches
  infer_instance

instance (certificate : ExactDescriptorCertificate) :
    Decidable certificate.RoleEventImageMatches := by
  unfold ExactDescriptorCertificate.RoleEventImageMatches
  infer_instance

instance (certificate : ExactDescriptorCertificate) :
    Decidable certificate.RoleRouteImageMatches := by
  unfold ExactDescriptorCertificate.RoleRouteImageMatches
  infer_instance

def ExactDescriptorCertificate.Matches
    (certificate : ExactDescriptorCertificate) : Prop :=
  let graph := projectGraph certificate.image.role certificate.choreo
  certificate.image.decodeProjectionShape? = some graph.projectionShape ∧
    certificate.ResidentMetadataMatches ∧
    certificate.ProgramImageMatches ∧
    certificate.RoleEventImageMatches ∧
    certificate.RoleRouteImageMatches

instance (certificate : ExactDescriptorCertificate) :
    Decidable certificate.Matches := by
  unfold ExactDescriptorCertificate.Matches
  infer_instance

def ExactDescriptorCertificate.check
    (certificate : ExactDescriptorCertificate) : Bool :=
  let graph := projectGraph certificate.image.role certificate.choreo
  certificate.image.check && graph.check && decide certificate.Matches

def ExactDescriptorCertificate.Refines
    (certificate : ExactDescriptorCertificate) : Prop :=
  let graph := projectGraph certificate.image.role certificate.choreo
  certificate.image.check = true ∧
    certificate.Matches ∧
    graph.WellFormed

/-- General exact-byte refinement, quantified over every canonical resident
image accepted by the production-layout decoder. -/
theorem exact_descriptor_certificate_sound
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) : certificate.Refines := by
  simp [ExactDescriptorCertificate.check] at accepted
  exact ⟨accepted.1.1, accepted.2, artifact_checker_sound accepted.1.2⟩

/-- An accepted resident image decodes the canonical global occurrence list,
including each production lane. This exposes the program-image component of
the exact descriptor certificate as a reusable refinement theorem. -/
theorem accepted_descriptor_global_events_bind_canonical_lanes
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.decodeGlobalEvents? =
      some certificate.choreo.globalEvents := by
  have refines := exact_descriptor_certificate_sound accepted
  have agreement := refines.2.1
  unfold ExactDescriptorCertificate.Matches at agreement
  exact agreement.2.2.1.1

/-- Every accepted role image carries exactly the frame-label coloring produced
by the host-only compiled occurrence source. Sequential reuse and competing
frontier separation therefore reach the resident descriptor without an
independent label authority. -/
theorem accepted_descriptor_frame_labels_bind_compiled_coloring
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.decodeEventFrameLabels? =
      some (certificate.choreo.canonicalRoleFrameLabels certificate.image.role) := by
  have refines := exact_descriptor_certificate_sound accepted
  have agreement := refines.2.1
  unfold ExactDescriptorCertificate.Matches at agreement
  exact agreement.2.2.2.1.2.1

/-- Every exact descriptor accepted by translation validation also satisfies
the runtime's single-authority rule for nested route membership: the event
conflict key names the innermost scope and arm, while every containing route
range is that scope or an ancestor in the checked parent chain. -/
theorem accepted_descriptor_binds_exact_route_arm_columns
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.routeArmColumnsCoherent = true := by
  exact checked_descriptor_binds_route_arm_columns
    (exact_descriptor_certificate_sound accepted).1

/-- Translation validation binds every passive route-child pointer to the
exact parent route scope and arm consumed by the production cursor. A later
scope slot alone is not sufficient evidence of parentage. -/
theorem accepted_descriptor_binds_exact_passive_child_columns
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.passiveChildColumnsCoherent = true := by
  exact checked_descriptor_binds_passive_child_columns
    (exact_descriptor_certificate_sound accepted).1

/-- Translation validation also binds the exact root-to-selected route commit
chain consumed by the production cursor. No malformed parent chain can be
accepted merely because its packed ranges are in bounds. -/
theorem accepted_descriptor_binds_exact_route_commit_columns
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.routeCommitColumnsCoherent = true := by
  exact checked_descriptor_binds_route_commit_columns
    (exact_descriptor_certificate_sound accepted).1

theorem accepted_descriptor_binds_event_lane_domain
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.eventLaneColumnCoherent = true := by
  exact checked_descriptor_binds_event_lane_column
    (exact_descriptor_certificate_sound accepted).1

/-- Translation validation binds the complete lane-column fact: active lanes,
both route arms, their exact offer union, and the sparse lane-step rows. -/
theorem accepted_descriptor_binds_exact_lane_columns
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.laneColumnsCoherent = true := by
  exact checked_descriptor_binds_lane_columns
    (exact_descriptor_certificate_sound accepted).1

/-- Every accepted sparse lane relation is the exact first/last occurrence of
its lane in that route arm; it cannot skip, borrow, or extend an arm event. -/
theorem accepted_descriptor_binds_exact_route_arm_lane_relations
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.routeArmLaneRelationsExact = true := by
  exact checked_descriptor_binds_exact_route_arm_lane_relations
    (exact_descriptor_certificate_sound accepted).1

theorem accepted_descriptor_binds_exact_active_lane_metadata
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.activeLaneMetadataCoherent = true := by
  exact checked_descriptor_binds_active_lane_metadata
    (exact_descriptor_certificate_sound accepted).1

theorem accepted_descriptor_binds_exact_route_commit_capacity
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.routeCommitCapacityExact = true := by
  exact checked_descriptor_binds_exact_route_commit_capacity
    (exact_descriptor_certificate_sound accepted).1

theorem accepted_descriptor_binds_roll_columns
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.rollColumnsCoherent = true := by
  exact checked_descriptor_binds_roll_columns
    (exact_descriptor_certificate_sound accepted).1

theorem accepted_descriptor_binds_strict_atom_order
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.atomEffIndicesStrict = true := by
  exact checked_descriptor_binds_strict_atom_order
    (exact_descriptor_certificate_sound accepted).1

/-- Every accepted resident image has exactly the action column projected from
its normalized choreography, for every role and choreography accepted by the
checker rather than only for the generated witness corpus. -/
theorem accepted_descriptor_actions_match_projection
    {certificate : ExactDescriptorCertificate}
    (accepted : certificate.check = true) :
    certificate.image.decodeActions? =
      some ((projectGraph certificate.image.role certificate.choreo).events.map Event.action) := by
  have refines := exact_descriptor_certificate_sound accepted
  have agreement := refines.2.1
  unfold ExactDescriptorCertificate.Matches at agreement
  have actions := decoded_projection_shape_binds_action_column agreement.1
  simpa [EventGraph.projectionShape] using actions

def descriptorCursorStep
    (image : RustDescriptorImage)
    (graph : EventGraph)
    (state : CommitState)
    (eventId : Nat) : AdmissionResult :=
  match image.eventOperation? eventId with
  | none => .rejected
  | some operation => admitOperation graph state operation.request

private def descriptorResolvedOperationStep
    (operation : Option DescriptorOperation)
    (graph : EventGraph)
    (state : CommitState) : AdmissionResult :=
  match operation with
  | none => .rejected
  | some resolved => admitOperation graph state resolved.request

/-- One framed carrier observation selects and admits one exact resident receive
operation, or rejects without manufacturing an event identity. -/
def descriptorInboundStep
    (image : RustDescriptorImage)
    (graph : EventGraph)
    (state : CommitState)
    (key : InboundOperationKey) : AdmissionResult :=
  descriptorResolvedOperationStep
    (image.resolveInboundOperation? graph state key) graph state

/-- Headerless ingress uses the same admission kernel after its reduced key has
identified one enabled resident receive operation. -/
def descriptorDeterministicInboundStep
    (image : RustDescriptorImage)
    (graph : EventGraph)
    (state : CommitState)
    (key : DeterministicInboundKey) : AdmissionResult :=
  descriptorResolvedOperationStep
    (image.resolveDeterministicInboundOperation? graph state key) graph state

def applyDescriptorCursorStep
    (image : RustDescriptorImage)
    (graph : EventGraph)
    (state : CommitState)
    (eventId : Nat) : CommitState :=
  match descriptorCursorStep image graph state eventId with
  | .accepted next => next
  | .rejected => state

/-- Every accepted byte-decoded cursor transition is exactly one admission
transition carrying resident event identity, lane, frame label, direction,
peer, logical label, and schema. -/
theorem descriptor_cursor_step_simulates_admission
    {image : RustDescriptorImage} {graph : EventGraph}
    {state next : CommitState} {eventId : Nat}
    (stepped : descriptorCursorStep image graph state eventId = .accepted next) :
    ∃ operation,
      image.eventOperation? eventId = some operation ∧
      operation.eventId = eventId ∧
      admitOperation graph state operation.request = .accepted next := by
  unfold descriptorCursorStep at stepped
  cases operationCase : image.eventOperation? eventId with
  | none => simp [operationCase] at stepped
  | some operation =>
      have eventExact := (decoded_event_operation_binds_exact_columns operationCase).1
      exact ⟨operation, rfl, eventExact, by simpa [operationCase] using stepped⟩

/-- Framed ingress acceptance carries one complete observed key, one exact
resident row, and one admission transition. -/
theorem descriptor_inbound_step_simulates_exact_admission
    {image : RustDescriptorImage} {graph : EventGraph}
    {state next : CommitState} {key : InboundOperationKey}
    (stepped : descriptorInboundStep image graph state key = .accepted next) :
    ∃ operation,
      image.resolveInboundOperation? graph state key = some operation ∧
      image.eventOperation? operation.eventId = some operation ∧
      operation.matchesInbound key = true ∧
      operation.enabledCandidate graph state = true ∧
      admitOperation graph state operation.request = .accepted next := by
  unfold descriptorInboundStep descriptorResolvedOperationStep at stepped
  cases operationCase : image.resolveInboundOperation? graph state key with
  | none => simp [operationCase] at stepped
  | some operation =>
      have exactColumns :=
        resolved_inbound_operation_binds_exact_event_columns operationCase
      have keyExact :=
        resolved_inbound_operation_matches_complete_enabled_key operationCase
      exact ⟨operation, rfl, exactColumns, keyExact.1, keyExact.2,
        by simpa [operationCase] using stepped⟩

/-- Deterministic ingress acceptance carries one reduced key, one exact resident
row, and one admission transition. -/
theorem descriptor_deterministic_inbound_step_simulates_exact_admission
    {image : RustDescriptorImage} {graph : EventGraph}
    {state next : CommitState} {key : DeterministicInboundKey}
    (stepped : descriptorDeterministicInboundStep image graph state key = .accepted next) :
    ∃ operation,
      image.resolveDeterministicInboundOperation? graph state key = some operation ∧
      image.eventOperation? operation.eventId = some operation ∧
      operation.matchesDeterministicInbound key = true ∧
      operation.enabledCandidate graph state = true ∧
      admitOperation graph state operation.request = .accepted next := by
  unfold descriptorDeterministicInboundStep descriptorResolvedOperationStep at stepped
  cases operationCase : image.resolveDeterministicInboundOperation? graph state key with
  | none => simp [operationCase] at stepped
  | some operation =>
      have exactColumns :=
        resolved_deterministic_inbound_operation_binds_exact_event_columns operationCase
      have keyExact :=
        resolved_deterministic_inbound_operation_matches_complete_enabled_key operationCase
      exact ⟨operation, rfl, exactColumns, keyExact.1, keyExact.2,
        by simpa [operationCase] using stepped⟩

/-- An accepted step of an accepted resident image commits the exact projected
event. This closes the byte-decoder, choreography projection, operation-key,
and commit chain in one theorem. -/
theorem accepted_descriptor_cursor_step_refines_projection
    {certificate : ExactDescriptorCertificate}
    {state next : CommitState} {eventId : Nat}
    (certificateAccepted : certificate.check = true)
    (stepped : descriptorCursorStep
      certificate.image
      (projectGraph certificate.image.role certificate.choreo)
      state eventId = .accepted next) :
    certificate.image.decodeActions? =
        some ((projectGraph certificate.image.role certificate.choreo).events.map Event.action) /\
      ∃ operation event,
        certificate.image.eventOperation? eventId = some operation /\
        operation.eventId = eventId /\
        certificate.image.eventLane? eventId = some operation.lane /\
        certificate.image.eventFrameLabel? eventId = some operation.frameLabel /\
        (projectGraph certificate.image.role certificate.choreo).events[eventId]? = some event /\
        event.id = eventId /\
        event.action = operation.action /\
        commitEvent
          (projectGraph certificate.image.role certificate.choreo)
          state eventId = some next := by
  refine ⟨accepted_descriptor_actions_match_projection certificateAccepted, ?_⟩
  obtain ⟨operation, operationAt, operationEventId, admitted⟩ :=
    descriptor_cursor_step_simulates_admission stepped
  have operationColumns := decoded_event_operation_binds_exact_columns operationAt
  obtain ⟨event, _, eventAt, eventIdExact, actionExact, committed⟩ :=
    admission_acceptance_is_exact_commit admitted
  change
    (projectGraph certificate.image.role certificate.choreo).events[operation.eventId]? =
      some event at eventAt
  change event.id = operation.eventId at eventIdExact
  change event.action = operation.action at actionExact
  change commitEvent
    (projectGraph certificate.image.role certificate.choreo)
    state operation.eventId = some next at committed
  rw [operationEventId] at eventAt eventIdExact committed
  exact ⟨operation, event, operationAt, operationEventId,
    operationColumns.2.2.1, operationColumns.2.2.2,
    eventAt, eventIdExact, actionExact, committed⟩

/-- A framed carrier observation accepted against an exact descriptor commits
the unique projected receive event selected by its complete wire key. -/
theorem accepted_descriptor_inbound_step_refines_projection
    {certificate : ExactDescriptorCertificate}
    {state next : CommitState} {key : InboundOperationKey}
    (certificateAccepted : certificate.check = true)
    (stepped : descriptorInboundStep
      certificate.image
      (projectGraph certificate.image.role certificate.choreo)
      state key = .accepted next) :
    certificate.image.decodeActions? =
        some ((projectGraph certificate.image.role certificate.choreo).events.map Event.action) ∧
      ∃ operation event,
        certificate.image.resolveInboundOperation?
          (projectGraph certificate.image.role certificate.choreo)
          state key = some operation ∧
        certificate.image.eventOperation? operation.eventId = some operation ∧
        operation.matchesInbound key = true ∧
        (projectGraph certificate.image.role certificate.choreo).events[operation.eventId]? =
          some event ∧
        event.id = operation.eventId ∧
        event.action = operation.action ∧
        commitEvent
          (projectGraph certificate.image.role certificate.choreo)
          state operation.eventId = some next := by
  refine ⟨accepted_descriptor_actions_match_projection certificateAccepted, ?_⟩
  obtain ⟨operation, resolved, operationAt, keyExact, _, admitted⟩ :=
    descriptor_inbound_step_simulates_exact_admission stepped
  obtain ⟨event, _, eventAt, eventIdExact, actionExact, committed⟩ :=
    admission_acceptance_is_exact_commit admitted
  exact ⟨operation, event, resolved, operationAt, keyExact,
    eventAt, eventIdExact, actionExact, committed⟩

/-- A headerless observation accepted against an exact descriptor commits the
unique projected receive event selected by lane, logical label, and schema. -/
theorem accepted_descriptor_deterministic_inbound_step_refines_projection
    {certificate : ExactDescriptorCertificate}
    {state next : CommitState} {key : DeterministicInboundKey}
    (certificateAccepted : certificate.check = true)
    (stepped : descriptorDeterministicInboundStep
      certificate.image
      (projectGraph certificate.image.role certificate.choreo)
      state key = .accepted next) :
    certificate.image.decodeActions? =
        some ((projectGraph certificate.image.role certificate.choreo).events.map Event.action) ∧
      ∃ operation event,
        certificate.image.resolveDeterministicInboundOperation?
          (projectGraph certificate.image.role certificate.choreo)
          state key = some operation ∧
        certificate.image.eventOperation? operation.eventId = some operation ∧
        operation.matchesDeterministicInbound key = true ∧
        (projectGraph certificate.image.role certificate.choreo).events[operation.eventId]? =
          some event ∧
        event.id = operation.eventId ∧
        event.action = operation.action ∧
        commitEvent
          (projectGraph certificate.image.role certificate.choreo)
          state operation.eventId = some next := by
  refine ⟨accepted_descriptor_actions_match_projection certificateAccepted, ?_⟩
  obtain ⟨operation, resolved, operationAt, keyExact, _, admitted⟩ :=
    descriptor_deterministic_inbound_step_simulates_exact_admission stepped
  obtain ⟨event, _, eventAt, eventIdExact, actionExact, committed⟩ :=
    admission_acceptance_is_exact_commit admitted
  exact ⟨operation, event, resolved, operationAt, keyExact,
    eventAt, eventIdExact, actionExact, committed⟩

theorem descriptor_cursor_rejection_preserves_commit_state
    {image : RustDescriptorImage} {graph : EventGraph}
    {state : CommitState} {eventId : Nat}
    (rejected : descriptorCursorStep image graph state eventId = .rejected) :
    applyDescriptorCursorStep image graph state eventId = state := by
  simp [applyDescriptorCursorStep, rejected]

end Hibana
