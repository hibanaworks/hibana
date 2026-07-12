import Hibana.DescriptorRefinement
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
  descriptors : List ExactDescriptorCertificate

def VerifiedProtocolCertificate.check
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  certificate.projectability.check session roleCount choreo &&
  decide (certificate.descriptors.length = roleCount) &&
  checkDescriptorCertificates choreo 0 certificate.descriptors

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
  refine ⟨projectability_certificate_sound accepted.1.1,
    of_decide_eq_true accepted.1.2, ?_⟩
  intro role descriptor present
  simpa using descriptor_certificates_sound_at
    accepted.2 role descriptor present

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
    (projectability_certificate_establishes_initial_invariant accepted.1.1)
    reachable

/-- The finite proof artifact removes a hand-written progress premise: every
reachable live state with unfinished work has an accepted successor. -/
theorem verified_protocol_establishes_semantic_unstuckness
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    SemanticallyUnstuck session roleCount choreo := by
  unfold VerifiedProtocolCertificate.check at accepted
  simp only [Bool.and_eq_true] at accepted
  have projectabilityAccepted := accepted.1.1
  unfold ProjectabilityCertificate.check at projectabilityAccepted
  simp only [Bool.and_eq_true] at projectabilityAccepted
  exact projectability_certificate_establishes_semantic_unstuckness
    projectabilityAccepted.1

/-- The combined criterion implemented by Hibana's message-erased monitor. It
does not assert priority over other systems; it packages the exact properties
that distinguish this runtime boundary into one kernel-checked proposition. -/
structure MessageErasedAffineAsyncMonitor
    (certificate : VerifiedProtocolCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop where
  allRolesRefine : certificate.AllRolesRefine session roleCount choreo
  reachableInvariant :
    ∀ {current : GlobalConfig},
      GlobalReachable (GlobalConfig.initial session roleCount choreo) current ->
      GlobalInvariant current
  semanticUnstuck : SemanticallyUnstuck session roleCount choreo
  routeControllerAuthority :
    choreo.checkRouteKnowledgeFrom roleCount 0 = true
  affineTransport : AffineTransportProfile
  descriptorRouteParticipantsExact :
    ∀ {role : Nat} {descriptor : ExactDescriptorCertificate},
      certificate.descriptors[role]? = some descriptor ->
      descriptor.image.decodeRouteResolvers? =
        some choreo.canonicalRouteResolvers
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
  readyCancellationRetires :
    ∀ {config : AsyncCancellationConfig}, config.RetirementReady ->
      ∃ retired,
        AsyncCancellationReachable config retired /\ retired.fullyRetired

/-- One accepted production artifact establishes the exact descriptor,
asynchronous admission, iteration-erasure, and affine route-publication
properties together. -/
theorem verified_protocol_establishes_message_erased_affine_async_monitor
    {certificate : VerifiedProtocolCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    MessageErasedAffineAsyncMonitor certificate session roleCount choreo := by
  have verified := verified_protocol_certificate_establishes_all_role_refinement accepted
  refine {
    allRolesRefine := verified
    reachableInvariant := ?_
    semanticUnstuck := verified_protocol_establishes_semantic_unstuckness accepted
    routeControllerAuthority := verified.1.2.2.2.routeKnowledge
    affineTransport := transport_state_establishes_affine_profile
    descriptorRouteParticipantsExact := ?_
    inboundIdentity := verified.1.2.2.2.inboundOccurrenceIdentity
    transportAdmissionUnique := ?_
    rollReentryOrder := ?_
    activeRoutePublicationIsAffine := ?_
    begunRoutePublicationTracksExactPending := ?_
    routeObservationIsAffine := ?_
    routeObservationRejectsDuplicate := ?_
    routeObservationPreservesArm := ?_
    localRouteParticipantsAreExact := ?_
    dynamicResolutionSealsLocalMembership := sealed_local_membership_rejects_late_attach
    callbackAttachCannotReadBorrowedEndpoint := active_endpoint_access_blocks_reentrant_attach
    readyCancellationRetires := ?_
  }
  · intro current reachable
    exact verified_protocol_reachable_preserves_global_invariant accepted reachable
  · intro role descriptor present
    exact verified_protocol_descriptor_route_participants_match_choreography
      accepted present
  · intro frame leftId rightId leftEvent rightEvent left right
    exact transport_admission_is_unique
      verified.1.2.2.2.inboundOccurrenceIdentity left right
  · intro body bodyMember left right leftMember rightMember leftNonlocal
      rightNonlocal sameReceiver sameLane rightReceiverBound
    exact verified_protocol_roll_reentry_has_fifo_or_causal_order
      accepted bodyMember leftMember rightMember leftNonlocal rightNonlocal
      sameReceiver sameLane rightReceiverBound
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
  · intro config ready
    exact retirement_ready_reaches_full_retirement ready

end Hibana
