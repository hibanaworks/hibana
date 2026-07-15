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
    certificate.image.maxRouteStackDepth =
      certificate.choreo.canonicalRoleMaxRouteStackDepth certificate.image.role ∧
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
  match image.eventAction? eventId with
  | none => .rejected
  | some action => admitOperation graph state { eventId, action }

def applyDescriptorCursorStep
    (image : RustDescriptorImage)
    (graph : EventGraph)
    (state : CommitState)
    (eventId : Nat) : CommitState :=
  match descriptorCursorStep image graph state eventId with
  | .accepted next => next
  | .rejected => state

/-- Every accepted byte-decoded cursor transition is exactly one admission
transition carrying event identity, direction, peer, label, and schema. -/
theorem descriptor_cursor_step_simulates_admission
    {image : RustDescriptorImage} {graph : EventGraph}
    {state next : CommitState} {eventId : Nat}
    (stepped : descriptorCursorStep image graph state eventId = .accepted next) :
    ∃ action,
      image.eventAction? eventId = some action ∧
      admitOperation graph state { eventId, action } = .accepted next := by
  unfold descriptorCursorStep at stepped
  cases actionCase : image.eventAction? eventId with
  | none => simp [actionCase] at stepped
  | some action => exact ⟨action, rfl, by simpa [actionCase] using stepped⟩

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
      ∃ action event,
        certificate.image.eventAction? eventId = some action /\
        (projectGraph certificate.image.role certificate.choreo).events[eventId]? = some event /\
        event.id = eventId /\
        event.action = action /\
        commitEvent
          (projectGraph certificate.image.role certificate.choreo)
          state eventId = some next := by
  refine ⟨accepted_descriptor_actions_match_projection certificateAccepted, ?_⟩
  obtain ⟨action, actionAt, admitted⟩ :=
    descriptor_cursor_step_simulates_admission stepped
  obtain ⟨event, _, eventAt, eventIdExact, actionExact, committed⟩ :=
    admission_acceptance_is_exact_commit admitted
  exact ⟨action, event, actionAt, eventAt, eventIdExact, actionExact, committed⟩

theorem descriptor_cursor_rejection_preserves_commit_state
    {image : RustDescriptorImage} {graph : EventGraph}
    {state : CommitState} {eventId : Nat}
    (rejected : descriptorCursorStep image graph state eventId = .rejected) :
    applyDescriptorCursorStep image graph state eventId = state := by
  simp [applyDescriptorCursorStep, rejected]

end Hibana
