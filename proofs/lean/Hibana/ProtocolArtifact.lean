import Hibana.DescriptorRefinement
import Hibana.DistributedProgress
import Hibana.DistributedRollRefinement
import Hibana.ElasticAdmissionHistory
import Hibana.GlobalCoherence
import Hibana.IterationErasure
import Hibana.AffineRoutePublication
import Hibana.AsyncCancellationTermination
import Hibana.RuntimeRefinement

namespace Hibana

def checkDescriptorCertificates
    (choreo : Choreo) : Nat -> List ExactDescriptorCertificate -> Bool
  | _, [] => true
  | role, certificate :: rest =>
      certificate.check &&
      decide (certificate.choreo = choreo) &&
      decide (certificate.image.role = role) &&
      checkDescriptorCertificates choreo (role + 1) rest

private theorem descriptor_certificates_sound_at
    {choreo : Choreo}
    {start : Nat}
    {certificates : List ExactDescriptorCertificate}
    (accepted : checkDescriptorCertificates choreo start certificates = true) :
    ∀ (offset : Nat) (certificate : ExactDescriptorCertificate),
      certificates[offset]? = some certificate ->
      certificate.check = true /\
      certificate.Refines /\
      certificate.choreo = choreo /\
      certificate.image.role = start + offset := by
  induction certificates generalizing start with
  | nil =>
      intro offset certificate present
      simp at present
  | cons head tail inductionHypothesis =>
      simp [checkDescriptorCertificates] at accepted
      intro offset certificate present
      cases offset with
      | zero =>
          have exactCertificate : head = certificate := Option.some.inj (by
            simpa using present)
          subst certificate
          exact ⟨accepted.1.1.1,
            exact_descriptor_certificate_sound accepted.1.1.1,
            accepted.1.1.2, by simpa using accepted.1.2⟩
      | succ offset =>
          have tailPresent : tail[offset]? = some certificate := by
            simpa using present
          have verified := inductionHypothesis accepted.2
            offset certificate tailPresent
          refine ⟨verified.1, verified.2.1, verified.2.2.1, ?_⟩
          simpa [Nat.add_assoc, Nat.add_comm 1 offset] using verified.2.2.2

/-- One proof-producing host artifact binds all role descriptor bytes to the
same choreography and to its all-role projectability closure. -/
structure VerifiedProtocolCertificate where
  projectability : ProjectabilityCertificate
  distributedProgress : DistributedProgressCertificate
  descriptors : List ExactDescriptorCertificate

def VerifiedProtocolCertificate.check
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  certificate.projectability.check session roleCount choreo &&
  (certificate.distributedProgress.check session roleCount choreo &&
  (decide (certificate.descriptors.length = roleCount) &&
  checkDescriptorCertificates choreo 0 certificate.descriptors))

def VerifiedProtocolCertificate.AllRolesRefine
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  certificate.projectability.Projectable session roleCount choreo /\
  certificate.descriptors.length = roleCount /\
  ∀ (role : Nat) (descriptor : ExactDescriptorCertificate),
    certificate.descriptors[role]? = some descriptor ->
    descriptor.check = true /\ descriptor.Refines /\
      descriptor.choreo = choreo /\ descriptor.image.role = role

theorem verified_protocol_certificate_establishes_all_role_refinement
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    certificate.AllRolesRefine session roleCount choreo := by
  unfold VerifiedProtocolCertificate.check at accepted
  simp only [Bool.and_eq_true] at accepted
  refine ⟨projectability_certificate_sound accepted.1,
    of_decide_eq_true accepted.2.2.1, ?_⟩
  intro role descriptor present
  simpa using descriptor_certificates_sound_at
    accepted.2.2.2 role descriptor present

theorem verified_protocol_descriptor_actions_match_projection
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (present : certificate.descriptors[role]? = some descriptor) :
    descriptor.image.decodeActions? =
      some ((projectGraph role choreo).events.map Event.action) := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  have descriptorVerified := verified.2.2 role descriptor present
  have actions := accepted_descriptor_actions_match_projection
    (certificate := descriptor)
    descriptorVerified.1
  simpa [descriptorVerified.2.2.1, descriptorVerified.2.2.2] using actions

/-- Every accepted role image carries the canonical per-arm participant masks
derived from the shared choreography. This is the descriptor-to-publication
bridge used by the resident affine route cell. -/
theorem verified_protocol_descriptor_route_participants_match_choreography
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (present : certificate.descriptors[role]? = some descriptor) :
    descriptor.image.decodeRouteResolvers? =
      some choreo.canonicalRouteResolvers := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  have descriptorVerified := verified.2.2 role descriptor present
  have agreement := descriptorVerified.2.1.2.1
  unfold ExactDescriptorCertificate.Matches at agreement
  have resolvers := agreement.2.2.1.2.2.1
  simpa [descriptorVerified.2.2.1] using resolvers

theorem verified_protocol_has_injective_inbound_evidence
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {role : Nat}
    {descriptor : ExactDescriptorCertificate}
    (present : certificate.descriptors[role]? = some descriptor) :
    choreo.InboundOccurrenceIdentity /\
      descriptor.image.decodeEventFrameLabels? =
        some (choreo.canonicalRoleFrameLabels role) := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  have descriptorVerified := verified.2.2 role descriptor present
  have staticIdentity := verified.1.2.2.2.inboundOccurrenceIdentity
  have roleImage := descriptorVerified.2.1.2.1.2.2.2.1.2.1
  simpa [descriptorVerified.2.2.1, descriptorVerified.2.2.2] using
    And.intro staticIdentity roleImage

/-- Every raw header admitted under a verified protocol artifact denotes one
global occurrence. The theorem composes production descriptor bytes, static
evidence injectivity, and transport admission without adding wire fields. -/
theorem verified_protocol_transport_admission_is_unique
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {frame : TransportFrame}
    {leftId rightId : Nat} {leftEvent rightEvent : GlobalEvent}
    (left : TransportAdmission choreo frame leftId leftEvent)
    (right : TransportAdmission choreo frame rightId rightEvent) :
    leftId = rightId /\ leftEvent = rightEvent := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  exact transport_admission_is_unique
    verified.1.2.2.2.inboundOccurrenceIdentity left right

/-- Any accepted descriptor cursor send can append the next ghost occurrence
without waiting for a remote receive. This is the general bridge from exact
resident bytes and local cursor progress to Hibana's elastic rolled FIFO
semantics; it adds no production state. -/
theorem accepted_descriptor_pipelined_send_extends_elastic_history
    {descriptor : ExactDescriptorCertificate}
    {cursor nextCursor : CommitState}
    {event : GlobalEvent}
    {history : ElasticAdmissionHistory}
    {globalId peer label schema : Nat}
    (descriptorAccepted : descriptor.check = true)
    (eventAt : descriptor.choreo.globalEvents[globalId]? = some event)
    (projected : event.action? descriptor.image.role =
      some (.send peer label schema))
    (channelMatches : history.MatchesEventChannel event)
    (historyWellFormed : history.WellFormed)
    (stepped : descriptorCursorStep
      descriptor.image
      (projectGraph descriptor.image.role descriptor.choreo)
      cursor
      (descriptor.choreo.localEventId descriptor.image.role globalId) =
        .accepted nextCursor) :
    let eventId := descriptor.choreo.localEventId descriptor.image.role globalId
    let key := history.nextOccurrence globalId
    let nextHistory := history.appendOccurrence globalId
    descriptor.image.eventAction? eventId = some (.send peer label schema) /\
      nextHistory.WellFormed /\
      nextHistory.channel = history.channel /\
      nextHistory.accepted = history.accepted ++ [key] /\
      key.iteration = history.nextIteration globalId /\
      nextHistory.MatchesEventChannel event := by
  obtain ⟨_, action, localEvent, actionAt, localEventAt, _, actionExact, _⟩ :=
    accepted_descriptor_cursor_step_refines_projection
      descriptorAccepted stepped
  have localActionAt :
      ((projectGraph descriptor.image.role descriptor.choreo).events.map
        Event.action)[descriptor.choreo.localEventId
          descriptor.image.role globalId]? = some action := by
    rw [List.getElem?_map, localEventAt]
    simp [actionExact]
  have projectedAt := project_graph_action_at_global_event_local_id
    eventAt projected
  have actionEq : action = .send peer label schema :=
    Option.some.inj (localActionAt.symm.trans projectedAt)
  have exactActionAt :
      descriptor.image.eventAction?
          (descriptor.choreo.localEventId descriptor.image.role globalId) =
        some (.send peer label schema) := by
    simpa [actionEq] using actionAt
  exact ⟨
    exactActionAt,
    elastic_admission_preserves_well_formed
      historyWellFormed globalId,
    rfl,
    rfl,
    rfl,
    channelMatches
  ⟩

def ExactDescriptorPipelinedSendRefinement
    (descriptor : ExactDescriptorCertificate) : Prop :=
  forall
    {cursor nextCursor : CommitState}
    {event : GlobalEvent}
    {history : ElasticAdmissionHistory}
    {globalId peer label schema : Nat},
    descriptor.choreo.globalEvents[globalId]? = some event ->
    event.action? descriptor.image.role = some (.send peer label schema) ->
    history.MatchesEventChannel event ->
    history.WellFormed ->
    descriptorCursorStep
      descriptor.image
      (projectGraph descriptor.image.role descriptor.choreo)
      cursor
      (descriptor.choreo.localEventId descriptor.image.role globalId) =
        .accepted nextCursor ->
    let eventId := descriptor.choreo.localEventId descriptor.image.role globalId
    let key := history.nextOccurrence globalId
    let nextHistory := history.appendOccurrence globalId
    descriptor.image.eventAction? eventId = some (.send peer label schema) /\
      nextHistory.WellFormed /\
      nextHistory.channel = history.channel /\
      nextHistory.accepted = history.accepted ++ [key] /\
      key.iteration = history.nextIteration globalId /\
      nextHistory.MatchesEventChannel event

theorem accepted_descriptor_establishes_pipelined_send_refinement
    {descriptor : ExactDescriptorCertificate}
    (accepted : descriptor.check = true) :
    ExactDescriptorPipelinedSendRefinement descriptor := by
  intro cursor nextCursor event history globalId peer label schema
    eventAt projected channelMatches wellFormed stepped
  exact accepted_descriptor_pipelined_send_extends_elastic_history
    accepted eventAt projected channelMatches wellFormed stepped

/-- Exact Rust descriptor bytes and the all-role projectability certificate
jointly establish Hibana's iteration-erasure criterion for every rolled body in
the verified artifact. -/
theorem verified_protocol_roll_reentry_has_fifo_or_causal_order
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo body : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    (bodyMember : body ∈ choreo.rollBodies)
    {left right : StaticGlobalOccurrence}
    (leftMember : left ∈ body.staticGlobalOccurrences)
    (rightMember : right ∈ body.staticGlobalOccurrences)
    (leftNonlocal : left.event.sender ≠ left.event.receiver)
    (rightNonlocal : right.event.sender ≠ right.event.receiver)
    (sameReceiver : left.event.receiver = right.event.receiver)
    (sameLane : left.event.lane = right.event.lane)
    (rightReceiverBound : right.event.receiver < roleCount) :
    left.event.sender = right.event.sender ∨
      CausalHandoffPath body.rollUnfoldedOccurrences
        (left.inRollIteration body.globalEvents.length .current)
        (right.inRollIteration body.globalEvents.length .next)
        roleCount
        (right.inRollIteration body.globalEvents.length .next) := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  exact roll_reentry_has_fifo_or_causal_order
    (verified.1.2.2.2.rollReceiveLaneCausality body bodyMember)
    leftMember rightMember leftNonlocal rightNonlocal sameReceiver sameLane
    rightReceiverBound

/-- Subject reduction and session fidelity hold for every normalized global
state reachable from an accepted production artifact. -/
theorem verified_protocol_reachable_preserves_global_invariant
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {current : GlobalConfig}
    (reachable : GlobalReachable
      (GlobalConfig.initial session roleCount choreo) current) :
    GlobalInvariant current := by
  unfold VerifiedProtocolCertificate.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact global_reachable_preserves_global_invariant
    (projectability_certificate_establishes_initial_invariant accepted.1)
    reachable

/-- The finite proof artifact removes a hand-written progress premise: every
reachable live state with unfinished work has an accepted successor. -/
theorem verified_protocol_establishes_semantic_unstuckness
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    GlobalSemanticallyUnstuck session roleCount choreo := by
  unfold VerifiedProtocolCertificate.check at accepted
  simp only [Bool.and_eq_true] at accepted
  have projectabilityAccepted := accepted.1
  unfold ProjectabilityCertificate.check at projectabilityAccepted
  simp only [Bool.and_eq_true] at projectabilityAccepted
  exact projectability_certificate_establishes_semantic_unstuckness
    projectabilityAccepted.1

theorem verified_protocol_establishes_distributed_semantic_unstuckness
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    DistributedSemanticallyUnstuck session roleCount choreo := by
  unfold VerifiedProtocolCertificate.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact distributed_progress_certificate_establishes_unstuckness accepted.2.1

/-- The protocol-local guarantees implemented by Hibana's message-erased
endpoint runtime. Carrier conformance and remote installation are deliberately absent:
they belong to the deployment theorem and cannot be manufactured by a protocol
certificate. A dynamic resolver is controller-local; every non-controller
learns the selected arm only from admitted in-band evidence. -/
structure ProtocolExecutionGuarantees
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop where
  allRolesRefine : certificate.AllRolesRefine session roleCount choreo
  reachableInvariant :
    ∀ {current : GlobalConfig},
      GlobalReachable (GlobalConfig.initial session roleCount choreo) current ->
      GlobalInvariant current
  globalSemanticUnstuck : GlobalSemanticallyUnstuck session roleCount choreo
  distributedSemanticUnstuck :
    DistributedSemanticallyUnstuck session roleCount choreo
  routeControllerAuthority :
    choreo.checkRouteKnowledgeFrom roleCount 0 = true
  descriptorRouteParticipantsExact :
    ∀ {role : Nat} {descriptor : ExactDescriptorCertificate},
      certificate.descriptors[role]? = some descriptor ->
      descriptor.image.decodeRouteResolvers? =
        some choreo.canonicalRouteResolvers
  descriptorPipelinedSendRefinement :
    ∀ {role : Nat} {descriptor : ExactDescriptorCertificate},
      certificate.descriptors[role]? = some descriptor ->
      ExactDescriptorPipelinedSendRefinement descriptor
  inboundIdentity : choreo.InboundOccurrenceIdentity
  transportAdmissionUnique :
    ∀ {frame : TransportFrame}
      {leftId rightId : Nat} {leftEvent rightEvent : GlobalEvent},
      TransportAdmission choreo frame leftId leftEvent ->
      TransportAdmission choreo frame rightId rightEvent ->
      leftId = rightId /\ leftEvent = rightEvent
  rollReentryOrder :
    ∀ {body : Choreo}, body ∈ choreo.rollBodies ->
      ∀ {left right : StaticGlobalOccurrence},
      left ∈ body.staticGlobalOccurrences ->
      right ∈ body.staticGlobalOccurrences ->
      left.event.sender ≠ left.event.receiver ->
      right.event.sender ≠ right.event.receiver ->
      left.event.receiver = right.event.receiver ->
      left.event.lane = right.event.lane ->
      right.event.receiver < roleCount ->
      left.event.sender = right.event.sender ∨
        CausalHandoffPath body.rollUnfoldedOccurrences
          (left.inRollIteration body.globalEvents.length .current)
          (right.inRollIteration body.globalEvents.length .next)
          roleCount
          (right.inRollIteration body.globalEvents.length .next)
  rollPostLinearizationMaterialization :
    ∀ {before after : DistributedConfig}
      {rollId : Nat}
      {transports : List TransportState},
      before.rollStep? rollId = some after ->
      TransportSnapshotDrained transports ->
      ∃ state : DistributedRollMaterialization,
        state.Valid /\
        state.abstract = after.abstract /\
        state.pending.length = after.roleCount
  elasticPipelining : ElasticPipelinedSemantics
  activeRoutePublicationIsAffine :
    ∀ (cell : AffineRouteCell) (arm : RouteArm)
      (participants observed : List Nat),
      beginRouteDecision? (some cell) arm participants observed = none
  begunRoutePublicationTracksExactPending :
    ∀ {arm : RouteArm} {participants observed : List Nat}
      {next : Option AffineRouteCell},
      beginRouteDecision? none arm participants observed = some next ->
      next =
        let pending := pendingRouteParticipants participants observed
        if pending.isEmpty then none else some { arm, pending }
  routeObservationIsAffine :
    ∀ {role : Nat} {cell : AffineRouteCell}
      {next : Option AffineRouteCell},
      observeRouteDecision? role (some cell) = some next ->
      routeRolePending role next = false
  routeObservationRejectsDuplicate :
    ∀ {role : Nat} {cell : AffineRouteCell}
      {next : Option AffineRouteCell},
      observeRouteDecision? role (some cell) = some next ->
      observeRouteDecision? role next = none
  routeObservationPreservesArm :
    ∀ {role : Nat} {cell next : AffineRouteCell},
      observeRouteDecision? role (some cell) = some (some next) ->
      next.arm = cell.arm
  localRouteParticipantsAreExact :
    ∀ (role : Nat) (selectedParticipants attachedRoles : List Nat),
      role ∈ selectedLocalRouteParticipants selectedParticipants attachedRoles ↔
        role ∈ selectedParticipants ∧ role ∈ attachedRoles
  dynamicResolutionSealsLocalMembership :
    ∀ (membership : LocalMembership) (role : Nat),
      attachLocalRole? (sealLocalMembership membership) role = none
  callbackAttachCannotReadBorrowedEndpoint :
    ∀ (phase : RuntimeAccessPhase),
      phase = .endpointOperation ∨ phase = .endpointScratch ->
      attachMayReadEndpointImage phase = false
  abstractRetirementAfterLiveFault :
    ∀ {protocol : GlobalConfig}
      {reporter : Nat}
      {cause : SessionFault}
      {transports : List TransportState},
      protocol.status = .live ->
      (transports.map TransportState.channel).Nodup ->
      (∀ state, state ∈ transports -> state.WellFormed) ->
      (∀ role, role < protocol.roleCount -> role ≠ reporter ->
        role ∈ transports.map fun state => state.channel.receiver) ->
      ∃ retired,
        AsyncCancellationReachable
          (AsyncCancellationConfig.fromTransportSnapshot
            (protocol.beginCancellation reporter cause) transports) retired /\
        retired.fullyRetired

/-- One accepted production artifact establishes the exact descriptor,
asynchronous admission, iteration-erasure, and affine route-publication
properties together. -/
theorem verified_protocol_establishes_execution_guarantees
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    ProtocolExecutionGuarantees certificate session roleCount choreo := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  refine {
    allRolesRefine := verified
    reachableInvariant := ?_
    globalSemanticUnstuck := verified_protocol_establishes_semantic_unstuckness accepted
    distributedSemanticUnstuck :=
      verified_protocol_establishes_distributed_semantic_unstuckness accepted
    routeControllerAuthority := verified.1.2.2.2.routeKnowledge
    descriptorRouteParticipantsExact := ?_
    descriptorPipelinedSendRefinement := ?_
    inboundIdentity := verified.1.2.2.2.inboundOccurrenceIdentity
    transportAdmissionUnique := ?_
    rollReentryOrder := ?_
    rollPostLinearizationMaterialization := ?_
    elasticPipelining := elastic_pipelined_semantics_holds
    activeRoutePublicationIsAffine := ?_
    begunRoutePublicationTracksExactPending := ?_
    routeObservationIsAffine := ?_
    routeObservationRejectsDuplicate := ?_
    routeObservationPreservesArm := ?_
    localRouteParticipantsAreExact := ?_
    dynamicResolutionSealsLocalMembership := sealed_local_membership_rejects_late_attach
    callbackAttachCannotReadBorrowedEndpoint := active_endpoint_access_blocks_reentrant_attach
    abstractRetirementAfterLiveFault := ?_
  }
  · intro current reachable
    exact verified_protocol_reachable_preserves_global_invariant accepted reachable
  · intro role descriptor present
    exact verified_protocol_descriptor_route_participants_match_choreography
      accepted present
  · intro role descriptor present
    exact accepted_descriptor_establishes_pipelined_send_refinement
      (verified.2.2 role descriptor present).1
  · intro frame leftId rightId leftEvent rightEvent left right
    exact transport_admission_is_unique
      verified.1.2.2.2.inboundOccurrenceIdentity left right
  · intro body bodyMember left right leftMember rightMember leftNonlocal
      rightNonlocal sameReceiver sameLane rightReceiverBound
    exact verified_protocol_roll_reentry_has_fifo_or_causal_order
      accepted bodyMember leftMember rightMember leftNonlocal rightNonlocal
      sameReceiver sameLane rightReceiverBound
  · intro before after rollId transports linearized drained
    exact distributed_roll_linearization_has_materialization_refinement
      linearized drained
  · intro cell arm participants observed
    exact active_route_decision_cannot_be_overwritten cell arm participants observed
  · intro arm participants observed next begun
    exact begun_route_decision_tracks_exact_unobserved_participants begun
  · intro role cell next observed
    exact route_observation_is_affine observed
  · intro role cell next observed
    exact route_observation_rejects_duplicate observed
  · intro role cell next observed
    exact route_observation_preserves_selected_arm observed
  · intro role selectedParticipants attachedRoles
    exact selected_local_route_participants_are_exact
  · intro protocol reporter cause transports live unique wellFormed covers
    exact live_fault_snapshot_reaches_full_retirement
      live unique wellFormed covers

end Hibana
