import Hibana.PreparedKernelRefinement

namespace Hibana

/-- Runtime-effect classes that every production kernel artifact must exercise.
The protocol class is checked by the full `RuntimeTransitionCertificate`; the
remaining classes distinguish exact stutters from fail-closed cancellation. -/
inductive ProductionEffectClass where
  | protocol
  | preview
  | dropPending
  | requeue
  | fault
  | ambiguousReceive
  | observeCancellation
  deriving Repr, DecidableEq

def RuntimeEffect.productionClass : RuntimeEffect -> ProductionEffectClass
  | .protocol _ => .protocol
  | .preview => .preview
  | .dropPending => .dropPending
  | .requeue => .requeue
  | .fault _ _ => .fault
  | .ambiguousReceive _ => .ambiguousReceive
  | .observeCancellation _ => .observeCancellation

def requiredProductionEffectClasses : List ProductionEffectClass :=
  [.protocol, .preview, .dropPending, .requeue, .fault,
    .ambiguousReceive, .observeCancellation]

/-- Every protocol transition constructor that the compact runtime can expose.
The generated artifact checks one exact accepted transition per constructor;
these cases may use different verified choreographies. -/
inductive ProductionProtocolClass where
  | send
  | recv
  | localAction
  | resolve
  | rejectResolver
  | roll
  deriving Repr, DecidableEq

def GlobalOperation.productionClass : GlobalOperation -> ProductionProtocolClass
  | .send _ => .send
  | .recv _ => .recv
  | .localAction _ => .localAction
  | .resolve _ _ _ _ => .resolve
  | .rejectResolver _ _ _ => .rejectResolver
  | .roll _ => .roll

def requiredProductionProtocolClasses : List ProductionProtocolClass :=
  [.send, .recv, .localAction, .resolve, .rejectResolver, .roll]

structure ProductionProtocolCase where
  session : Nat
  roleCount : Nat
  choreo : Choreo
  transition : RuntimeTransitionCertificate

def ProductionProtocolCase.check (protocolCase : ProductionProtocolCase) : Bool :=
  protocolCase.transition.check protocolCase.session protocolCase.roleCount
    protocolCase.choreo

def ProductionProtocolCase.Refines (protocolCase : ProductionProtocolCase) : Prop :=
  protocolCase.transition.Refines protocolCase.session protocolCase.roleCount
    protocolCase.choreo

def ProductionProtocolCase.operationClass?
    (protocolCase : ProductionProtocolCase) : Option ProductionProtocolClass :=
  match protocolCase.transition.effect with
  | .protocol operation => some operation.productionClass
  | _ => none

/-- Cross-tool owners whose production implementations are checked outside
Lean. The generated artifact names every owner exactly once; CI independently
requires and executes the corresponding Kani or Miri gate. This list is proof
metadata and never enters a runtime image. -/
inductive ProductionKernelOwner where
  | endpoint
  | cursor
  | slab
  | waiter
  | resolver
  | receiveReceipt
  | transport
  | callbackReentry
  deriving Repr, DecidableEq

def requiredProductionKernelOwners : List ProductionKernelOwner :=
  [.endpoint, .cursor, .slab, .waiter, .resolver, .receiveReceipt,
    .transport, .callbackReentry]

/-- Translation-validation artifact emitted by production Rust tests. It
contains exact finite before/effect/after rows plus the complete cross-tool
ownership inventory. It does not add a resident kernel state or source-level
Rust claim. -/
structure ProductionKernelArtifact where
  transitions : List RuntimeTransitionCertificate
  protocolCases : List ProductionProtocolCase
  owners : List ProductionKernelOwner

def ProductionKernelArtifact.check
    (artifact : ProductionKernelArtifact)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  (((artifact.transitions.all fun transition =>
        transition.check session roleCount choreo) &&
      decide (artifact.transitions.map
        (RuntimeEffect.productionClass ∘ RuntimeTransitionCertificate.effect) =
          requiredProductionEffectClasses)) &&
    ((artifact.protocolCases.all ProductionProtocolCase.check) &&
      decide (artifact.protocolCases.map ProductionProtocolCase.operationClass? =
        requiredProductionProtocolClasses.map some))) &&
  decide (artifact.owners = requiredProductionKernelOwners)

structure ProductionKernelArtifact.Refines
    (artifact : ProductionKernelArtifact)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop where
  transitionsRefine : ∀ transition, transition ∈ artifact.transitions ->
    transition.Refines session roleCount choreo
  effectClassesExact : artifact.transitions.map
      (RuntimeEffect.productionClass ∘ RuntimeTransitionCertificate.effect) =
    requiredProductionEffectClasses
  protocolCasesRefine : ∀ protocolCase, protocolCase ∈ artifact.protocolCases ->
    protocolCase.Refines
  protocolClassesExact : artifact.protocolCases.map
      ProductionProtocolCase.operationClass? =
    requiredProductionProtocolClasses.map some
  ownersExact : artifact.owners = requiredProductionKernelOwners

theorem production_kernel_artifact_sound
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true) :
    artifact.Refines session roleCount choreo := by
  unfold ProductionKernelArtifact.check at accepted
  obtain ⟨accepted, ownersAccepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨transitionsAccepted, protocolCasesAccepted⟩ :=
    Bool.and_eq_true_iff.mp accepted
  obtain ⟨transitionsAccepted, classesAccepted⟩ :=
    Bool.and_eq_true_iff.mp transitionsAccepted
  obtain ⟨protocolCasesAccepted, protocolClassesAccepted⟩ :=
    Bool.and_eq_true_iff.mp protocolCasesAccepted
  exact {
    transitionsRefine := fun transition member =>
      runtime_transition_certificate_sound <|
        List.all_eq_true.mp transitionsAccepted transition member
    effectClassesExact := of_decide_eq_true classesAccepted
    protocolCasesRefine := fun protocolCase member =>
      runtime_transition_certificate_sound <|
        List.all_eq_true.mp protocolCasesAccepted protocolCase member
    protocolClassesExact := of_decide_eq_true protocolClassesAccepted
    ownersExact := of_decide_eq_true ownersAccepted
  }

theorem production_kernel_artifact_has_exact_owner_coverage
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true) :
    artifact.owners = requiredProductionKernelOwners :=
  (production_kernel_artifact_sound accepted).ownersExact

theorem production_kernel_artifact_covers_every_runtime_effect_class
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true) :
    artifact.transitions.map
        (RuntimeEffect.productionClass ∘ RuntimeTransitionCertificate.effect) =
      requiredProductionEffectClasses :=
  (production_kernel_artifact_sound accepted).effectClassesExact

theorem production_kernel_artifact_covers_every_protocol_class
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true) :
    artifact.protocolCases.map ProductionProtocolCase.operationClass? =
      requiredProductionProtocolClasses.map some :=
  (production_kernel_artifact_sound accepted).protocolClassesExact

theorem production_kernel_artifact_transition_accepted
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true)
    {transition : RuntimeTransitionCertificate}
    (member : transition ∈ artifact.transitions) :
    transition.check session roleCount choreo = true := by
  unfold ProductionKernelArtifact.check at accepted
  obtain ⟨accepted, _⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨transitionsAccepted, _⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨transitionsAccepted, _⟩ :=
    Bool.and_eq_true_iff.mp transitionsAccepted
  exact List.all_eq_true.mp transitionsAccepted transition member

theorem production_kernel_artifact_protocol_case_accepted
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true)
    {protocolCase : ProductionProtocolCase}
    (member : protocolCase ∈ artifact.protocolCases) :
    protocolCase.check = true := by
  unfold ProductionKernelArtifact.check at accepted
  obtain ⟨accepted, _⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨_, protocolCasesAccepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨protocolCasesAccepted, _⟩ :=
    Bool.and_eq_true_iff.mp protocolCasesAccepted
  exact List.all_eq_true.mp protocolCasesAccepted protocolCase member

inductive ProductionKernelPosition where
  | before
  | after
  deriving Repr, DecidableEq

structure ProductionKernelState
    (transitions : List RuntimeTransitionCertificate) where
  transition : RuntimeTransitionCertificate
  member : transition ∈ transitions
  position : ProductionKernelPosition

structure ProductionKernelPrepared
    (transitions : List RuntimeTransitionCertificate) where
  transition : RuntimeTransitionCertificate
  member : transition ∈ transitions

def prepareProductionKernel
    (transitions : List RuntimeTransitionCertificate)
    (state : ProductionKernelState transitions)
    (input : RuntimeEffect) : Option (ProductionKernelPrepared transitions) :=
  match state.position with
  | .after => none
  | .before =>
      if input = state.transition.effect then
        some { transition := state.transition, member := state.member }
      else
        none

def commitProductionKernel
    (transitions : List RuntimeTransitionCertificate)
    (_ : ProductionKernelState transitions)
    (prepared : ProductionKernelPrepared transitions) :
    ProductionKernelState transitions := {
  transition := prepared.transition
  member := prepared.member
  position := .after
}

def abstractProductionKernelState
    {transitions : List RuntimeTransitionCertificate}
    (state : ProductionKernelState transitions) : CompactGlobalState :=
  match state.position with
  | .before => state.transition.before
  | .after => state.transition.after

/-- The checked finite relation is represented as a disjoint union of exact
before/after transition pairs. Rejected inputs have no prepared value; accepted
inputs commit to the row's exact `after` image. -/
def productionTransitionKernel
    (transitions : List RuntimeTransitionCertificate) : PreparedKernelSemantics where
  State := ProductionKernelState transitions
  Input := RuntimeEffect
  Prepared := ProductionKernelPrepared transitions
  prepare := prepareProductionKernel transitions
  effect := fun prepared => prepared.transition.effect
  commit := commitProductionKernel transitions
  abstractState := abstractProductionKernelState

def ProductionKernelArtifact.kernel
    (artifact : ProductionKernelArtifact) : PreparedKernelSemantics :=
  productionTransitionKernel artifact.transitions

/-- Any finite relation whose rows pass the exact transition checker refines
the canonical prepare/commit kernel. This single proof is reused by both the
normalized effect relation and each independently checked protocol operation;
there is no weaker operation-only simulation path. -/
theorem production_transition_relation_refines_prepared_kernel
    {transitions : List RuntimeTransitionCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : forall transition, transition ∈ transitions ->
      transition.check session roleCount choreo = true) :
    PreparedKernelRefinement (productionTransitionKernel transitions)
      session roleCount choreo := by
  refine {
    stateCanonical := ?_
    preparedCommitExact := ?_
  }
  · change ∀ state : ProductionKernelState transitions,
      (abstractProductionKernelState state).canonical roleCount choreo = true
    intro state
    rcases state with ⟨transition, member, position⟩
    have refined := runtime_transition_certificate_sound
      (accepted transition member)
    cases position with
    | before =>
        simpa [abstractProductionKernelState] using refined.1
    | after =>
        simpa [abstractProductionKernelState] using refined.2.1
  · change ∀ (state : ProductionKernelState transitions)
        (input : RuntimeEffect) (prepared : ProductionKernelPrepared transitions),
      prepareProductionKernel transitions state input = some prepared ->
        applyRuntimeEffect? session roleCount choreo
            (abstractProductionKernelState state) prepared.transition.effect =
          some (abstractProductionKernelState
            (commitProductionKernel transitions state prepared))
    intro state input prepared preparedExact
    cases state with
    | mk transition member position =>
        cases position with
        | after => simp [prepareProductionKernel] at preparedExact
        | before =>
            by_cases inputExact : input = transition.effect
            · have preparedIdentity :
                  prepared = { transition, member } := by
                simpa [prepareProductionKernel, inputExact] using preparedExact.symm
              subst prepared
              have checked := accepted transition member
              have parsed :
                  ((transition.before.canonical roleCount choreo = true /\
                    transition.after.canonical roleCount choreo = true) /\
                    decide (applyRuntimeEffect? session roleCount choreo
                      transition.before transition.effect =
                        some transition.after) = true) := by
                simpa [RuntimeTransitionCertificate.check] using checked
              exact of_decide_eq_true parsed.2
            · simp [prepareProductionKernel, inputExact] at preparedExact

def ProductionProtocolCase.kernel
    (protocolCase : ProductionProtocolCase) : PreparedKernelSemantics :=
  productionTransitionKernel [protocolCase.transition]

theorem accepted_production_protocol_case_refines_prepared_kernel
    {protocolCase : ProductionProtocolCase}
    (accepted : protocolCase.check = true) :
    PreparedKernelRefinement protocolCase.kernel protocolCase.session
      protocolCase.roleCount protocolCase.choreo := by
  unfold ProductionProtocolCase.kernel
  apply production_transition_relation_refines_prepared_kernel
  intro transition member
  have transitionExact : transition = protocolCase.transition := by
    simpa using member
  subst transition
  simpa [ProductionProtocolCase.check] using accepted

/-- A checked artifact constructs a canonical finite prepare/commit model for
its accepted rows. This theorem is translation validation of the artifact
kernel; it does not identify that model with the production Rust kernel. -/
theorem accepted_production_kernel_artifact_refines_artifact_kernel
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true) :
    PreparedKernelRefinement artifact.kernel session roleCount choreo := by
  unfold ProductionKernelArtifact.kernel
  exact production_transition_relation_refines_prepared_kernel fun _ member =>
    production_kernel_artifact_transition_accepted accepted member

theorem accepted_production_kernel_artifact_refines_protocol_case_kernels
    {artifact : ProductionKernelArtifact}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true) :
    forall protocolCase, protocolCase ∈ artifact.protocolCases ->
      PreparedKernelRefinement protocolCase.kernel protocolCase.session
        protocolCase.roleCount protocolCase.choreo := by
  intro protocolCase member
  exact accepted_production_protocol_case_refines_prepared_kernel
    (production_kernel_artifact_protocol_case_accepted accepted member)

/-- Evidence supplied outside Lean for every owner named by the exact artifact.
`OwnerVerified` is deliberately abstract: a deployment may bind it to pinned
Kani/Miri results, while Lean never turns an owner name into a proof. -/
structure ProductionOwnerEvidence
    (OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop)
    (artifact : ProductionKernelArtifact)
    (kernel : PreparedKernelSemantics) : Prop where
  verified : forall owner, owner ∈ artifact.owners -> OwnerVerified owner kernel

/-- One explicit cross-tool boundary for the production prepare/commit kernel.
Lean proves the exact finite transition relation and complete owner inventory;
the named Rust owners remain an explicit external premise discharged by the
separately gated Kani and Miri checks. This structure does not claim that Lean
parsed or verified Rust source. -/
structure CrossToolProductionRefinement
    (OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop)
    (artifact : ProductionKernelArtifact)
    (kernel : PreparedKernelSemantics)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop where
  artifactExact : artifact.Refines session roleCount choreo
  artifactKernel :
    PreparedKernelRefinement artifact.kernel session roleCount choreo
  protocolArtifactKernels : forall protocolCase,
    protocolCase ∈ artifact.protocolCases ->
      PreparedKernelRefinement protocolCase.kernel protocolCase.session
        protocolCase.roleCount protocolCase.choreo
  productionKernel : PreparedKernelRefinement kernel session roleCount choreo
  ownerEvidence : ProductionOwnerEvidence OwnerVerified artifact kernel

theorem accepted_production_kernel_artifact_combines_with_external_kernel_refinement
    {OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop}
    {artifact : ProductionKernelArtifact}
    {kernel : PreparedKernelSemantics}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : artifact.check session roleCount choreo = true)
    (kernelRefinement : PreparedKernelRefinement kernel session roleCount choreo)
    (ownerEvidence : ProductionOwnerEvidence OwnerVerified artifact kernel) :
    CrossToolProductionRefinement OwnerVerified artifact kernel session roleCount choreo := {
  artifactExact := production_kernel_artifact_sound accepted
  artifactKernel :=
    accepted_production_kernel_artifact_refines_artifact_kernel accepted
  protocolArtifactKernels :=
    accepted_production_kernel_artifact_refines_protocol_case_kernels accepted
  productionKernel := kernelRefinement
  ownerEvidence
}

theorem cross_tool_refinement_verifies_every_required_owner
    {OwnerVerified : ProductionKernelOwner -> PreparedKernelSemantics -> Prop}
    {artifact : ProductionKernelArtifact}
    {kernel : PreparedKernelSemantics}
    {session roleCount : Nat}
    {choreo : Choreo}
    (refinement : CrossToolProductionRefinement OwnerVerified artifact kernel
      session roleCount choreo)
    {owner : ProductionKernelOwner}
    (required : owner ∈ requiredProductionKernelOwners) :
    OwnerVerified owner kernel := by
  apply refinement.ownerEvidence.verified owner
  rw [refinement.artifactExact.ownersExact]
  exact required

end Hibana
