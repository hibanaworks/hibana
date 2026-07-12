import Hibana.GlobalProgress
import Hibana.Progress
import Hibana.StaticProjectability

namespace Hibana

/-- Finite host-side image of every runtime field observed by `GlobalConfig`.
The Pico runtime remains message-erased; this representation exists only for
proof generation and checking. -/
structure CompactGlobalState where
  epoch : Nat
  phase : List EventPhase
  queue : List (Option AdmittedMessage)
  localState : List CompactCommitState
  routeSelection : List (Option RouteArm)
  resources : ParticipantResourceOwners
  status : GlobalSessionStatus
  deriving DecidableEq

def CompactGlobalState.fromGlobalConfig
    (config : GlobalConfig) : CompactGlobalState := {
  epoch := config.epoch
  phase := (List.range config.choreo.globalEvents.length).map config.phase
  queue := (List.range config.choreo.globalEvents.length).map config.queue
  localState := (List.range config.roleCount).map fun role =>
    CompactCommitState.fromCommitState
      (projectGraph role config.choreo) (config.localState role)
  routeSelection :=
    (List.range config.choreo.routeCount).map config.routeSelection
  resources := config.resources
  status := config.status
}

def checkGlobalProgramWellFormed (roleCount : Nat) (choreo : Choreo) : Bool :=
  choreo.globalEvents.all fun event =>
    decide (event.sender < roleCount) &&
    decide (event.receiver < roleCount) &&
    decide (event.sender ≠ event.receiver \/ event.schema = 0)

/-- Certificate states must carry every finite field consumed by the semantic
decoder. A malformed or partial artifact is rejected instead of being padded. -/
def CompactGlobalState.canonical
    (state : CompactGlobalState)
    (roleCount : Nat)
    (choreo : Choreo) : Bool :=
  state.phase.length == choreo.globalEvents.length &&
  state.queue.length == choreo.globalEvents.length &&
  state.localState.length == roleCount &&
  state.routeSelection.length == choreo.routeCount &&
  (List.range roleCount).all fun role =>
    match state.localState[role]? with
    | some localSnapshot =>
        localSnapshot.canonical (projectGraph role choreo)
    | none => false

/-- Fail-closed decoder from a finite proof artifact to the total semantic
functions used by `GlobalConfig`. Values outside the declared event/role/route
domains are explicit inert extensions; missing values inside those domains make
the whole decode fail. -/
def CompactGlobalState.decode?
    (state : CompactGlobalState)
    (session roleCount : Nat)
    (choreo : Choreo) : Option GlobalConfig :=
  if accepted : state.canonical roleCount choreo = true then
    have fields :
        (((state.phase.length = choreo.globalEvents.length /\
          state.queue.length = choreo.globalEvents.length) /\
          state.localState.length = roleCount) /\
          state.routeSelection.length = choreo.routeCount) /\
          ∀ role, role < roleCount ->
            (match state.localState[role]? with
            | some localSnapshot =>
                localSnapshot.canonical (projectGraph role choreo)
            | none => false) = true := by
      simpa [CompactGlobalState.canonical] using accepted
    if programAccepted : checkGlobalProgramWellFormed roleCount choreo = true then
      some {
        session
        roleCount
        choreo
        epoch := state.epoch
        phase := fun globalId =>
          if bound : globalId < choreo.globalEvents.length then
            state.phase.get ⟨globalId, by simpa [fields.1.1.1.1] using bound⟩
          else
            .consumed state.epoch
        queue := fun globalId =>
          if bound : globalId < choreo.globalEvents.length then
            state.queue.get ⟨globalId, by simpa [fields.1.1.1.2] using bound⟩
          else
            none
        localState := fun role =>
          if bound : role < roleCount then
            (state.localState.get
              ⟨role, by simpa [fields.1.1.2] using bound⟩).toCommitState
          else
            .initial
        routeSelection := fun conflict =>
          if bound : conflict < choreo.routeCount then
            state.routeSelection.get
              ⟨conflict, by simpa [fields.1.2] using bound⟩
          else
            none
        resources := state.resources
        status := state.status
      }
    else
      none
  else
    none

theorem compact_global_decode_acceptance_implies_canonical
    {state : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    {config : GlobalConfig}
    (decoded : state.decode? session roleCount choreo = some config) :
    state.canonical roleCount choreo = true := by
  unfold CompactGlobalState.decode? at decoded
  split at decoded
  next accepted =>
    split at decoded
    next => exact accepted
    next => simp at decoded
  next rejected => simp at decoded

theorem compact_global_decode_acceptance_implies_program_checker
    {state : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    {config : GlobalConfig}
    (decoded : state.decode? session roleCount choreo = some config) :
    checkGlobalProgramWellFormed roleCount choreo = true := by
  unfold CompactGlobalState.decode? at decoded
  split at decoded
  next =>
    split at decoded
    next accepted => exact accepted
    next => simp at decoded
  next => simp at decoded

theorem compact_global_decode_rejects_noncanonical
    {state : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    (rejected : state.canonical roleCount choreo ≠ true) :
    state.decode? session roleCount choreo = none := by
  simp [CompactGlobalState.decode?, rejected]

theorem compact_global_decode_rejects_invalid_program
    {state : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    (rejected : checkGlobalProgramWellFormed roleCount choreo ≠ true) :
    state.decode? session roleCount choreo = none := by
  simp [CompactGlobalState.decode?, rejected]

def CompactGlobalState.initial
    (session roleCount : Nat) (choreo : Choreo) : CompactGlobalState :=
  CompactGlobalState.fromGlobalConfig
    (GlobalConfig.initial session roleCount choreo)

def CompactGlobalState.step?
    (state : CompactGlobalState)
    (session roleCount : Nat)
    (choreo : Choreo)
    (operation : GlobalOperation) : Option CompactGlobalState :=
  match state.decode? session roleCount choreo with
  | none => none
  | some config =>
      match config.step? operation with
      | none => none
      | some next => some (CompactGlobalState.fromGlobalConfig next)

def compactGlobalSuccessors
    (session roleCount : Nat)
    (choreo : Choreo)
    (state : CompactGlobalState) : List CompactGlobalState :=
  match state.decode? session roleCount choreo with
  | none => []
  | some config =>
      config.candidateOperations.filterMap fun operation =>
        state.step? session roleCount choreo operation

theorem compact_global_step_simulates_global_semantics
    {state next : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    {operation : GlobalOperation}
    (stepped : state.step? session roleCount choreo operation = some next) :
    ∃ config globalNext,
      state.decode? session roleCount choreo = some config /\
      config.step? operation = some globalNext /\
      next = CompactGlobalState.fromGlobalConfig globalNext := by
  unfold CompactGlobalState.step? at stepped
  cases decoded : state.decode? session roleCount choreo with
  | none => simp [decoded] at stepped
  | some config =>
      cases globalStep : config.step? operation with
      | none => simp [decoded, globalStep] at stepped
      | some globalNext =>
          have nextEq : CompactGlobalState.fromGlobalConfig globalNext = next :=
            Option.some.inj (by simpa [decoded, globalStep] using stepped)
          exact ⟨config, globalNext, rfl, globalStep, nextEq.symm⟩

theorem compact_global_successor_is_real_transition
    {state next : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    (successor : next ∈ compactGlobalSuccessors session roleCount choreo state) :
    ∃ config operation globalNext,
      state.decode? session roleCount choreo = some config /\
      config.step? operation = some globalNext /\
      next = CompactGlobalState.fromGlobalConfig globalNext := by
  unfold compactGlobalSuccessors at successor
  cases decoded : state.decode? session roleCount choreo with
  | none => simp [decoded] at successor
  | some config =>
      obtain ⟨operation, operationMember, stepped⟩ :=
        List.mem_filterMap.mp (by simpa [decoded] using successor)
      obtain ⟨decodedConfig, globalNext, decodedAgain, globalStep, nextExact⟩ :=
        compact_global_step_simulates_global_semantics stepped
      have configExact : decodedConfig = config := Option.some.inj
        (decodedAgain.symm.trans decoded)
      subst decodedConfig
      exact ⟨config, operation, globalNext, rfl, globalStep, nextExact⟩

def checkGlobalLogicalProgress
    (session roleCount : Nat)
    (choreo : Choreo)
    (state : CompactGlobalState) : Bool :=
  match state.decode? session roleCount choreo with
  | none => false
  | some config =>
      match config.status with
      | .live => !config.unfinishedEvent?.isSome || config.executableOperation?.isSome
      | .cancelling _ _ | .retired _ => true

theorem global_logical_progress_checker_sound
    {state : CompactGlobalState}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : checkGlobalLogicalProgress session roleCount choreo state = true) :
    ∃ config,
      state.decode? session roleCount choreo = some config /\
      GlobalLogicalProgress config := by
  cases decoded : state.decode? session roleCount choreo with
  | none => simp [checkGlobalLogicalProgress, decoded] at accepted
  | some config =>
      refine ⟨config, rfl, ?_⟩
      cases statusCase : config.status with
      | cancelling cause awaiting =>
          exact Or.inl ⟨cause, awaiting, statusCase⟩
      | retired cause =>
          exact Or.inr <| Or.inl ⟨cause, statusCase⟩
      | live =>
          cases unfinishedCase : config.unfinishedEvent? with
          | none =>
              exact Or.inr <| Or.inr <| Or.inl (by
                simp [GlobalConfig.HasUnfinishedEvent, unfinishedCase])
          | some unfinished =>
              have executable : config.executableOperation?.isSome := by
                simpa [checkGlobalLogicalProgress, decoded, statusCase,
                  unfinishedCase] using accepted
              cases operationCase : config.executableOperation? with
              | none => simp [operationCase] at executable
              | some result =>
                  obtain ⟨operation, next⟩ := result
                  exact Or.inr <| Or.inr <| Or.inr
                    ⟨operation, next, executable_operation_sound operationCase⟩

theorem global_program_well_formed_checker_sound
    {roleCount : Nat} {choreo : Choreo}
    (accepted : checkGlobalProgramWellFormed roleCount choreo = true) :
    GlobalProgramWellFormed roleCount choreo := by
  intro event member
  have checked := List.all_eq_true.mp accepted event member
  simp at checked
  refine ⟨checked.1.1, checked.1.2, ?_⟩
  intro same
  rcases checked.2 with different | unitSchema
  · exact False.elim (different same)
  · exact unitSchema

def checkGuardedRolls : Choreo -> Bool
  | .send _ _ _ _ => true
  | .seq left right | .par left right | .route _ left right =>
      checkGuardedRolls left && checkGuardedRolls right
  | .roll body => !body.globalEvents.isEmpty && checkGuardedRolls body

theorem guarded_rolls_checker_sound
    {choreo : Choreo}
    (accepted : checkGuardedRolls choreo = true) :
    choreo.GuardedRolls := by
  induction choreo with
  | send => trivial
  | seq left right leftIH rightIH =>
      simp [checkGuardedRolls] at accepted
      exact ⟨leftIH accepted.1, rightIH accepted.2⟩
  | par left right leftIH rightIH =>
      simp [checkGuardedRolls] at accepted
      exact ⟨leftIH accepted.1, rightIH accepted.2⟩
  | route authority left right leftIH rightIH =>
      simp [checkGuardedRolls] at accepted
      exact ⟨leftIH accepted.1, rightIH accepted.2⟩
  | roll body bodyIH =>
      simp [checkGuardedRolls] at accepted
      exact ⟨accepted.1, bodyIH accepted.2⟩

structure GlobalProgressCertificate where
  states : List CompactGlobalState

private def globalStateMember
    (target : CompactGlobalState) : List CompactGlobalState -> Bool
  | [] => false
  | state :: rest => decide (state = target) || globalStateMember target rest

private theorem global_state_member_sound
    {target : CompactGlobalState}
    {states : List CompactGlobalState}
    (accepted : globalStateMember target states = true) :
    target ∈ states := by
  induction states with
  | nil => simp [globalStateMember] at accepted
  | cons state rest inductionHypothesis =>
      simp [globalStateMember] at accepted
      rcases accepted with same | tail
      · simpa using Or.inl same.symm
      · simpa using Or.inr (inductionHypothesis tail)

private def enqueueFreshGlobalStates
    (candidates pending visited : List CompactGlobalState) :
    List CompactGlobalState :=
  candidates.foldl (fun queued candidate =>
    if globalStateMember candidate visited || globalStateMember candidate queued then
      queued
    else
      queued ++ [candidate]) pending

def discoverGlobalProgressStates
    (session roleCount : Nat)
    (choreo : Choreo) :
    Nat -> List CompactGlobalState -> List CompactGlobalState ->
      List CompactGlobalState
  | 0, _, visited => visited
  | _ + 1, [], visited => visited
  | fuel + 1, state :: pending, visited =>
      if globalStateMember state visited then
        discoverGlobalProgressStates session roleCount choreo fuel pending visited
      else
        let nextVisited := visited ++ [state]
        let nextPending := enqueueFreshGlobalStates
          (compactGlobalSuccessors session roleCount choreo state)
          pending nextVisited
        discoverGlobalProgressStates session roleCount choreo
          fuel nextPending nextVisited

def buildGlobalProgressCertificate
    (session roleCount : Nat)
    (choreo : Choreo)
    (fuel : Nat) : GlobalProgressCertificate := {
  states := discoverGlobalProgressStates session roleCount choreo fuel
    [CompactGlobalState.initial session roleCount choreo] []
}

/-- Choreography-derived exploration fuel. The checker still requires a closed
successor set, so this bound cannot turn an incomplete search into an accepted
certificate. -/
def globalProgressExplorationFuel (roleCount : Nat) (choreo : Choreo) : Nat :=
  let eventStates := 3 ^ choreo.globalEvents.length
  let queueStates := 2 ^ choreo.globalEvents.length
  let routeStates := 3 ^ choreo.routeCount
  let localStates := (List.range roleCount).map
    (fun role => progressStateBound (projectGraph role choreo)) |>.foldl Nat.mul 1
  (roleCount + 2) * eventStates * queueStates * routeStates * localStates

def GlobalProgressCertificate.Closed
    (certificate : GlobalProgressCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  0 < roleCount /\
  GlobalProgramWellFormed roleCount choreo /\
  choreo.GuardedRolls /\
  CompactGlobalState.initial session roleCount choreo ∈ certificate.states /\
  certificate.states.Nodup /\
  ∀ state, state ∈ certificate.states ->
    state.canonical roleCount choreo = true /\
    (∃ config,
      state.decode? session roleCount choreo = some config /\
      GlobalLogicalProgress config) /\
    ∀ next, next ∈ compactGlobalSuccessors session roleCount choreo state ->
      next ∈ certificate.states

def GlobalProgressCertificate.check
    (certificate : GlobalProgressCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  decide (0 < roleCount) &&
  (checkGlobalProgramWellFormed roleCount choreo &&
  (checkGuardedRolls choreo &&
  (globalStateMember
      (CompactGlobalState.initial session roleCount choreo) certificate.states &&
  (decide certificate.states.Nodup &&
  certificate.states.all fun state =>
    state.canonical roleCount choreo &&
    checkGlobalLogicalProgress session roleCount choreo state &&
    (compactGlobalSuccessors session roleCount choreo state).all fun next =>
      globalStateMember next certificate.states))))

theorem global_progress_certificate_sound
    {certificate : GlobalProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    certificate.Closed session roleCount choreo := by
  unfold GlobalProgressCertificate.check at accepted
  obtain ⟨positive, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨programAccepted, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨guardedAccepted, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨initialAccepted, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨unique, statesAccepted⟩ := Bool.and_eq_true_iff.mp accepted
  refine ⟨of_decide_eq_true positive,
    global_program_well_formed_checker_sound programAccepted,
    guarded_rolls_checker_sound guardedAccepted,
    global_state_member_sound initialAccepted,
    of_decide_eq_true unique, ?_⟩
  intro state member
  have checked := List.all_eq_true.mp statesAccepted state member
  obtain ⟨stateAccepted, successorsAccepted⟩ :=
    Bool.and_eq_true_iff.mp checked
  obtain ⟨canonical, progressAccepted⟩ :=
    Bool.and_eq_true_iff.mp stateAccepted
  refine ⟨canonical, global_logical_progress_checker_sound progressAccepted, ?_⟩
  intro next successor
  exact global_state_member_sound
    (List.all_eq_true.mp successorsAccepted next successor)

inductive CompactGlobalReachable
    (session roleCount : Nat)
    (choreo : Choreo) : CompactGlobalState -> Prop where
  | initial : CompactGlobalReachable session roleCount choreo
      (CompactGlobalState.initial session roleCount choreo)
  | step {state next} :
      CompactGlobalReachable session roleCount choreo state ->
      next ∈ compactGlobalSuccessors session roleCount choreo state ->
      CompactGlobalReachable session roleCount choreo next

theorem global_progress_certificate_covers_reachable
    {certificate : GlobalProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (closed : certificate.Closed session roleCount choreo)
    {state : CompactGlobalState}
    (reachable : CompactGlobalReachable session roleCount choreo state) :
    state ∈ certificate.states := by
  induction reachable with
  | initial => exact closed.2.2.2.1
  | step prior successor inductionHypothesis =>
      exact (closed.2.2.2.2.2 _ inductionHypothesis).2.2 _ successor

/-- An accepted finite closure removes the former hand-written
`GlobalExecutionCoherent` premise for every model-reachable multiparty state. -/
theorem projectable_reachable_state_has_global_logical_progress
    {certificate : GlobalProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {state : CompactGlobalState}
    (reachable : CompactGlobalReachable session roleCount choreo state) :
    ∃ config,
      state.decode? session roleCount choreo = some config /\
      GlobalLogicalProgress config := by
  have closed := global_progress_certificate_sound accepted
  have member := global_progress_certificate_covers_reachable closed reachable
  exact (closed.2.2.2.2.2 state member).2.1

/-- Hibana's semantic counterpart of asynchronous-MPST unstuckness. Because a
normalized descriptor has a finite state image, closure is checked directly:
every reachable live state with unfinished work has a concrete successor. -/
def SemanticallyUnstuck
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  ∀ state config,
    CompactGlobalReachable session roleCount choreo state ->
    state.decode? session roleCount choreo = some config ->
    config.status = .live ->
    config.HasUnfinishedEvent ->
    ∃ operation next, config.step? operation = some next

theorem projectability_certificate_establishes_semantic_unstuckness
    {certificate : GlobalProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    SemanticallyUnstuck session roleCount choreo := by
  intro state config reachable decoded statusLive unfinished
  obtain ⟨verifiedConfig, verifiedDecoded, progress⟩ :=
    projectable_reachable_state_has_global_logical_progress accepted reachable
  have configExact : verifiedConfig = config := Option.some.inj
    (verifiedDecoded.symm.trans decoded)
  subst verifiedConfig
  rcases progress with cancelling | retired | complete | successor
  · obtain ⟨cause, awaiting, cancelling⟩ := cancelling
    rw [statusLive] at cancelling
    contradiction
  · obtain ⟨cause, retired⟩ := retired
    rw [statusLive] at retired
    contradiction
  · exact False.elim (complete unfinished)
  · exact successor

def checkRoleProgressCertificates
    (choreo : Choreo) : Nat -> List ProgressCertificate -> Bool
  | _, [] => true
  | role, certificate :: rest =>
      certificate.check (projectGraph role choreo) &&
        checkRoleProgressCertificates choreo (role + 1) rest

private theorem role_progress_certificates_sound_at
    {choreo : Choreo}
    {start : Nat}
    {certificates : List ProgressCertificate}
    (accepted : checkRoleProgressCertificates choreo start certificates = true) :
    ∀ (offset : Nat) (certificate : ProgressCertificate),
      certificates[offset]? = some certificate ->
      certificate.Closed (projectGraph (start + offset) choreo) := by
  induction certificates generalizing start with
  | nil =>
      intro offset certificate present
      simp at present
  | cons head tail inductionHypothesis =>
      simp [checkRoleProgressCertificates] at accepted
      intro offset certificate present
      cases offset with
      | zero =>
          have exactCertificate : head = certificate := Option.some.inj (by
            simpa using present)
          subst certificate
          simpa using progress_certificate_sound accepted.1
      | succ offset =>
          have tailPresent : tail[offset]? = some certificate := by
            simpa using present
          have closed := inductionHypothesis accepted.2 offset certificate tailPresent
          simpa [Nat.add_assoc, Nat.add_comm 1 offset] using closed

structure ProjectabilityCertificate where
  global : GlobalProgressCertificate
  localCertificates : List ProgressCertificate

def buildProjectabilityCertificate
    (session roleCount : Nat)
    (choreo : Choreo) : ProjectabilityCertificate := {
  global := buildGlobalProgressCertificate session roleCount choreo
    (globalProgressExplorationFuel roleCount choreo)
  localCertificates := (List.range roleCount).map fun role =>
    buildProgressCertificate (projectGraph role choreo)
}

def ProjectabilityCertificate.check
    (certificate : ProjectabilityCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  certificate.global.check session roleCount choreo &&
  (decide (certificate.localCertificates.length = roleCount) &&
    (checkRoleProgressCertificates choreo 0 certificate.localCertificates &&
      checkStaticProjectability roleCount choreo))

def ProjectabilityCertificate.Projectable
    (certificate : ProjectabilityCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  certificate.global.Closed session roleCount choreo /\
  certificate.localCertificates.length = roleCount /\
  (∀ (role : Nat) (localCertificate : ProgressCertificate),
    certificate.localCertificates[role]? = some localCertificate ->
    localCertificate.Closed (projectGraph role choreo)) /\
  choreo.StaticProjectable roleCount

theorem projectability_certificate_sound
    {certificate : ProjectabilityCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    certificate.Projectable session roleCount choreo := by
  unfold ProjectabilityCertificate.check at accepted
  obtain ⟨globalAccepted, localAccepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨lengthAccepted, certificatesAccepted⟩ :=
    Bool.and_eq_true_iff.mp localAccepted
  obtain ⟨certificatesAccepted, staticAccepted⟩ :=
    Bool.and_eq_true_iff.mp certificatesAccepted
  refine ⟨global_progress_certificate_sound globalAccepted,
    of_decide_eq_true lengthAccepted, ?_,
    static_projectability_checker_sound staticAccepted⟩
  intro role localCertificate present
  simpa using role_progress_certificates_sound_at
    certificatesAccepted role localCertificate present

theorem projectability_certificate_establishes_initial_invariant
    {certificate : ProjectabilityCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    GlobalInvariant (GlobalConfig.initial session roleCount choreo) := by
  have projectable := projectability_certificate_sound accepted
  exact initial_global_invariant session roleCount choreo
    projectable.1.1 projectable.1.2.1

theorem projectability_certificate_establishes_static_projectability
    {certificate : ProjectabilityCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    choreo.StaticProjectable roleCount :=
  (projectability_certificate_sound accepted).2.2.2

theorem projectability_certificate_all_roles_have_well_formed_projection
    {certificate : ProjectabilityCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {role : Nat}
    (bound : role < roleCount) :
    (projectGraph role choreo).WellFormed := by
  have projectable := projectability_certificate_sound accepted
  cases certificateCase : certificate.localCertificates[role]? with
  | none =>
      have outOfBounds := List.getElem?_eq_none_iff.mp certificateCase
      have inBounds : role < certificate.localCertificates.length := by
        simpa [projectable.2.1] using bound
      exact False.elim ((Nat.not_lt_of_ge outOfBounds) inBounds)
  | some localCertificate =>
      exact (projectable.2.2.1 role localCertificate certificateCase).1

theorem projectability_certificate_removes_global_progress_premise
    {certificate : ProjectabilityCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true)
    {state : CompactGlobalState}
    (reachable : CompactGlobalReachable session roleCount choreo state) :
    ∃ config,
      state.decode? session roleCount choreo = some config /\
      GlobalLogicalProgress config := by
  have projectable := projectability_certificate_sound accepted
  have member := global_progress_certificate_covers_reachable
    projectable.1 reachable
  exact (projectable.1.2.2.2.2.2 state member).2.1

end Hibana
