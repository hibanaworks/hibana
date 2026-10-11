import Hibana.SessionComposition

namespace Hibana

/-- Exhaustive normalized effects used to classify the production endpoint
boundary. This vocabulary is not source-level Rust extraction; concrete Rust
coverage remains with production-generated witnesses, Kani, Miri, and unsafe
contracts. -/
inductive RuntimeEffect where
  | protocol (operation : GlobalOperation)
  | preview
  | dropPending
  | requeue
  | fault (reporter : Nat) (fault : RuntimeFault)
  | ambiguousReceive (reporter : Nat)
  | observeCancellation (role : Nat)
  deriving Repr, DecidableEq

def applyRuntimeEffect?
    (session roleCount : Nat)
    (choreo : Choreo)
    (before : CompactGlobalState)
    (effect : RuntimeEffect) : Option CompactGlobalState :=
  match before.decode? session roleCount choreo with
  | none => none
  | some config =>
      match effect with
      | .protocol operation =>
          match config.step? operation with
          | none => none
          | some next => some (CompactGlobalState.fromGlobalConfig next)

      | .preview | .dropPending | .requeue => some before
      | .fault reporter fault =>
          some (CompactGlobalState.fromGlobalConfig
            (config.beginCancellation reporter fault.sessionCause))
      | .ambiguousReceive reporter =>
          some (CompactGlobalState.fromGlobalConfig
            (config.beginCancellation reporter .protocolViolation))
      | .observeCancellation role =>
          match config.observeCancellation? role with
          | none => none
          | some next => some (CompactGlobalState.fromGlobalConfig next)

theorem runtime_effect_acceptance_requires_canonical
    {session roleCount : Nat}
    {choreo : Choreo}
    {before after : CompactGlobalState}
    {effect : RuntimeEffect}
    (applied : applyRuntimeEffect? session roleCount choreo before effect = some after) :
    before.canonical roleCount choreo = true := by
  unfold applyRuntimeEffect? at applied
  cases decoded : before.decode? session roleCount choreo with
  | none => simp [decoded] at applied
  | some config =>
      exact compact_global_decode_acceptance_implies_canonical decoded

theorem runtime_effect_acceptance_requires_well_formed_program
    {session roleCount : Nat}
    {choreo : Choreo}
    {before after : CompactGlobalState}
    {effect : RuntimeEffect}
    (applied : applyRuntimeEffect? session roleCount choreo before effect = some after) :
    GlobalProgramWellFormed roleCount choreo := by
  unfold applyRuntimeEffect? at applied
  cases decoded : before.decode? session roleCount choreo with
  | none => simp [decoded] at applied
  | some config =>
      exact global_program_well_formed_checker_sound
        (compact_global_decode_acceptance_implies_program_checker decoded)

structure RuntimeTransitionCertificate where
  before : CompactGlobalState
  effect : RuntimeEffect
  after : CompactGlobalState

def RuntimeTransitionCertificate.check
    (certificate : RuntimeTransitionCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  certificate.before.canonical roleCount choreo &&
  certificate.after.canonical roleCount choreo &&
  decide (applyRuntimeEffect? session roleCount choreo
    certificate.before certificate.effect = some certificate.after)

def RuntimeTransitionCertificate.Refines
    (certificate : RuntimeTransitionCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  certificate.before.canonical roleCount choreo = true /\
  certificate.after.canonical roleCount choreo = true /\
  match certificate.before.decode? session roleCount choreo with
  | none => False
  | some config =>
      match certificate.effect with
      | .protocol operation =>
          ∃ globalNext,
            config.step? operation = some globalNext /\
            certificate.after = CompactGlobalState.fromGlobalConfig globalNext
      | .preview | .dropPending | .requeue =>
          certificate.after = certificate.before
      | .fault reporter fault =>
          certificate.after = CompactGlobalState.fromGlobalConfig
            (config.beginCancellation reporter fault.sessionCause)
      | .ambiguousReceive reporter =>
          certificate.after = CompactGlobalState.fromGlobalConfig
            (config.beginCancellation reporter .protocolViolation)
      | .observeCancellation role =>
          ∃ globalNext,
            config.observeCancellation? role = some globalNext /\
            certificate.after = CompactGlobalState.fromGlobalConfig globalNext

theorem runtime_transition_certificate_sound
    {certificate : RuntimeTransitionCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    certificate.Refines session roleCount choreo := by
  have parsed :
      (certificate.before.canonical roleCount choreo = true /\
        certificate.after.canonical roleCount choreo = true) /\
      decide (applyRuntimeEffect? session roleCount choreo
        certificate.before certificate.effect = some certificate.after) = true := by
    simpa [RuntimeTransitionCertificate.check] using accepted
  have exactEffect := of_decide_eq_true parsed.2
  unfold RuntimeTransitionCertificate.Refines
  refine ⟨parsed.1.1, parsed.1.2, ?_⟩
  cases decoded : certificate.before.decode? session roleCount choreo with
  | none =>
      simp [applyRuntimeEffect?, decoded] at exactEffect
  | some config =>
      simp only
      cases effectCase : certificate.effect with
      | protocol operation =>
          cases globalStep : config.step? operation with
          | none => simp [applyRuntimeEffect?, decoded, effectCase, globalStep] at exactEffect
          | some globalNext =>
              have afterExact :
                  CompactGlobalState.fromGlobalConfig globalNext = certificate.after :=
                Option.some.inj (by
                  simpa [applyRuntimeEffect?, decoded, effectCase, globalStep] using exactEffect)
              exact ⟨globalNext, globalStep, afterExact.symm⟩
      | preview =>
          have same : certificate.before = certificate.after := by
            exact Option.some.inj (by
              simpa [applyRuntimeEffect?, decoded, effectCase] using exactEffect)
          exact same.symm
      | dropPending =>
          have same : certificate.before = certificate.after := by
            exact Option.some.inj (by
              simpa [applyRuntimeEffect?, decoded, effectCase] using exactEffect)
          exact same.symm
      | requeue =>
          have same : certificate.before = certificate.after := by
            exact Option.some.inj (by
              simpa [applyRuntimeEffect?, decoded, effectCase] using exactEffect)
          exact same.symm
      | fault reporter fault =>
          exact (Option.some.inj (by
            simpa [applyRuntimeEffect?, decoded, effectCase] using exactEffect)).symm
      | ambiguousReceive reporter =>
          exact (Option.some.inj (by
            simpa [applyRuntimeEffect?, decoded, effectCase] using exactEffect)).symm
      | observeCancellation role =>
          cases observation : config.observeCancellation? role with
          | none =>
              simp [applyRuntimeEffect?, decoded, effectCase, observation] at exactEffect
          | some globalNext =>
              have afterExact :
                  CompactGlobalState.fromGlobalConfig globalNext = certificate.after :=
                Option.some.inj (by
                  simpa [applyRuntimeEffect?, decoded, effectCase, observation] using exactEffect)
              exact ⟨globalNext, observation, afterExact.symm⟩

theorem runtime_protocol_transition_preserves_global_invariant
    {certificate : RuntimeTransitionCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {operation : GlobalOperation}
    {config : GlobalConfig}
    (accepted : certificate.check session roleCount choreo = true)
    (effectExact : certificate.effect = .protocol operation)
    (decoded : certificate.before.decode? session roleCount choreo = some config)
    (invariant : GlobalInvariant config) :
    ∃ globalNext,
      config.step? operation = some globalNext /\
      GlobalInvariant globalNext /\
      certificate.after = CompactGlobalState.fromGlobalConfig globalNext := by
  have relation := (runtime_transition_certificate_sound accepted).2.2
  rw [decoded, effectExact] at relation
  obtain ⟨globalNext, stepped, afterExact⟩ := relation
  exact ⟨globalNext, stepped,
    global_step_preserves_global_invariant invariant stepped, afterExact⟩

theorem runtime_fault_transition_preserves_global_invariant
    {certificate : RuntimeTransitionCertificate}
    {session roleCount reporter : Nat}
    {choreo : Choreo}
    {fault : RuntimeFault}
    {config : GlobalConfig}
    (accepted : certificate.check session roleCount choreo = true)
    (effectExact : certificate.effect = .fault reporter fault)
    (decoded : certificate.before.decode? session roleCount choreo = some config)
    (invariant : GlobalInvariant config) :
    GlobalInvariant (config.beginCancellation reporter fault.sessionCause) /\
      certificate.after = CompactGlobalState.fromGlobalConfig
        (config.beginCancellation reporter fault.sessionCause) := by
  have relation := (runtime_transition_certificate_sound accepted).2.2
  rw [decoded, effectExact] at relation
  exact ⟨begin_cancellation_preserves_global_invariant invariant, relation⟩

theorem runtime_ambiguous_receive_transition_preserves_global_invariant
    {certificate : RuntimeTransitionCertificate}
    {session roleCount reporter : Nat}
    {choreo : Choreo}
    {config : GlobalConfig}
    (accepted : certificate.check session roleCount choreo = true)
    (effectExact : certificate.effect = .ambiguousReceive reporter)
    (decoded : certificate.before.decode? session roleCount choreo = some config)
    (invariant : GlobalInvariant config) :
    GlobalInvariant (config.beginCancellation reporter .protocolViolation) /\
      certificate.after = CompactGlobalState.fromGlobalConfig
        (config.beginCancellation reporter .protocolViolation) := by
  have relation := (runtime_transition_certificate_sound accepted).2.2
  rw [decoded, effectExact] at relation
  exact ⟨begin_cancellation_preserves_global_invariant invariant, relation⟩

theorem runtime_cancellation_observation_preserves_global_invariant
    {certificate : RuntimeTransitionCertificate}
    {session roleCount role : Nat}
    {choreo : Choreo}
    {config : GlobalConfig}
    (accepted : certificate.check session roleCount choreo = true)
    (effectExact : certificate.effect = .observeCancellation role)
    (decoded : certificate.before.decode? session roleCount choreo = some config)
    (invariant : GlobalInvariant config) :
    ∃ globalNext,
      config.observeCancellation? role = some globalNext /\
      GlobalInvariant globalNext /\
      certificate.after = CompactGlobalState.fromGlobalConfig globalNext := by
  have relation := (runtime_transition_certificate_sound accepted).2.2
  rw [decoded, effectExact] at relation
  obtain ⟨globalNext, observed, afterExact⟩ := relation
  exact ⟨globalNext, observed,
    cancellation_observation_preserves_global_invariant invariant observed,
    afterExact⟩

theorem runtime_stutter_effect_is_zero_transition
    {certificate : RuntimeTransitionCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    {config : GlobalConfig}
    (accepted : certificate.check session roleCount choreo = true)
    (decoded : certificate.before.decode? session roleCount choreo = some config)
    (stutter : certificate.effect = .preview \/
      certificate.effect = .dropPending \/ certificate.effect = .requeue) :
    certificate.after = certificate.before := by
  have relation := (runtime_transition_certificate_sound accepted).2.2
  rw [decoded] at relation
  rcases stutter with preview | dropPending | requeue
  · simpa [preview] using relation
  · simpa [dropPending] using relation
  · simpa [requeue] using relation

theorem ambiguous_receive_fails_closed
    (before : CompactGlobalState)
    (session roleCount reporter : Nat)
    (choreo : Choreo)
    {config : GlobalConfig}
    (decoded : before.decode? session roleCount choreo = some config) :
    applyRuntimeEffect? session roleCount choreo before
        (.ambiguousReceive reporter) =
      some (CompactGlobalState.fromGlobalConfig
        (config.beginCancellation reporter .protocolViolation)) := by
  simp [applyRuntimeEffect?, decoded]

/-- Runtime access is available between operations. Endpoint callbacks execute
under an operation barrier; scratch use is a nested barrier state rather than a
return to registry-visible availability. -/
inductive RuntimeAccessPhase where
  | available
  | registry
  | endpointOperation
  | endpointScratch
  deriving Repr, DecidableEq

def attachMayReadEndpointImage : RuntimeAccessPhase -> Bool
  | .available => true
  | .registry | .endpointOperation | .endpointScratch => false

theorem active_endpoint_access_blocks_reentrant_attach
    (phase : RuntimeAccessPhase)
    (active : phase = .endpointOperation ∨ phase = .endpointScratch) :
    attachMayReadEndpointImage phase = false := by
  rcases active with rfl | rfl <;> rfl

theorem poisoned_callback_reentry_cannot_publish (cause : SessionFault) :
    SessionGenerationState.publicationPermitted (.poisoned cause) = false :=
  poisoned_generation_publish_rejected cause

end Hibana
