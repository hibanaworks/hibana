import Hibana.GlobalCoherence

namespace Hibana

/-- Per-role asynchronous state. `globalSelection` is ghost authority used by
the global abstraction; a role may learn it only through exact local evidence. -/
structure DistributedConfig where
  session : Nat
  roleCount : Nat
  choreo : Choreo
  epoch : Nat
  phase : Nat -> EventPhase
  queue : Nat -> Option AdmittedMessage
  roleState : Nat -> CommitState
  globalSelection : Nat -> Option RouteArm
  resources : ParticipantResourceOwners
  status : GlobalSessionStatus

structure LocalQueuedFrame where
  globalId : Nat
  lane : Nat
  message : AdmittedMessage
  deriving Repr, DecidableEq

/-- One endpoint's independently observable state. The queue is a finite view
of this role's accepted receive occurrences, annotated with the canonical
production lane already carried by the runtime header. -/
structure LocalConfig where
  session : Nat
  role : Nat
  roleCount : Nat
  choreo : Choreo
  epoch : Nat
  commitState : CommitState
  queue : List LocalQueuedFrame
  status : GlobalSessionStatus

/-- The exact local effect whose completion bits are taken from a global
successor. Unlike an optional owner, each variant states how route knowledge
may change for the affected role. -/
inductive DistributedCommitAuthority where
  | event (role globalId : Nat) (memberships : List ConflictArm)
  | resolver (role conflict : Nat) (arm : RouteArm)
  | rejection (role : Nat)
  | roll
  deriving Repr, DecidableEq

def DistributedConfig.fromGlobalConfig
    (config : GlobalConfig) : DistributedConfig := {
  session := config.session
  roleCount := config.roleCount
  choreo := config.choreo
  epoch := config.epoch
  phase := config.phase
  queue := config.queue
  roleState := config.localState
  globalSelection := config.routeSelection
  resources := config.resources
  status := config.status
}

/-- Global abstraction forgets role-local choice-observation lag. Outside the
declared role domain it preserves the total function exactly. -/
def DistributedConfig.abstract
    (config : DistributedConfig) : GlobalConfig := {
  session := config.session
  roleCount := config.roleCount
  choreo := config.choreo
  epoch := config.epoch
  phase := config.phase
  queue := config.queue
  localState := fun role =>
    if role < config.roleCount then
      { config.roleState role with selected := config.globalSelection }
    else
      config.roleState role
  routeSelection := config.globalSelection
  resources := config.resources
  status := config.status
}

private def DistributedConfig.localQueuedFrame?
    (config : DistributedConfig)
    (role globalId : Nat) : Option (Option LocalQueuedFrame) :=
  let global := config.abstract
  match global.queuedMessage? globalId with
  | none => some none
  | some message =>
      if message.receiver ≠ role then
        some none
      else
        match config.choreo.globalEvents[globalId]? with
        | none => none
        | some globalEvent =>
            match (projectGraph role config.choreo).events[
                config.choreo.localEventId role globalId]? with
            | none => none
            | some localEvent =>
                match localEvent.action with
                | .recv peer label schema =>
                    if message.session = config.session /\
                        message.epoch = config.epoch /\
                        message.globalId = globalId /\
                        message.sender = peer /\
                        message.receiver = role /\
                        message.lane = globalEvent.lane /\
                        message.label = label /\
                        message.schema = schema then
                      some (some { globalId, lane := globalEvent.lane, message })
                    else
                      none
                | .send _ _ _ | .local _ _ => none

def DistributedConfig.localQueue?
    (config : DistributedConfig) (role : Nat) : Option (List LocalQueuedFrame) := do
  let frames ← (List.range config.choreo.globalEvents.length).mapM
    (config.localQueuedFrame? role)
  pure (frames.filterMap id)

def DistributedConfig.localConfig?
    (config : DistributedConfig) (role : Nat) : Option LocalConfig :=
  if role < config.roleCount then do
    let queue ← config.localQueue? role
    pure {
      session := config.session
      role
      roleCount := config.roleCount
      choreo := config.choreo
      epoch := config.epoch
      commitState := config.roleState role
      queue
      status := config.status
    }
  else
    none

theorem local_queued_frame_has_canonical_lane
    {config : DistributedConfig}
    {role globalId : Nat}
    {frame : LocalQueuedFrame}
    (found : config.localQueuedFrame? role globalId = some (some frame)) :
    ∃ globalEvent,
      config.choreo.globalEvents[globalId]? = some globalEvent /\
      frame.lane = globalEvent.lane /\
      frame.message.lane = globalEvent.lane := by
  unfold DistributedConfig.localQueuedFrame? at found
  cases queuedCase : config.abstract.queuedMessage? globalId with
  | none => simp [queuedCase] at found
  | some message =>
      by_cases receiver : message.receiver ≠ role
      · simp [queuedCase, receiver] at found
      · cases globalCase : config.choreo.globalEvents[globalId]? with
        | none => simp [queuedCase, receiver, globalCase] at found
        | some globalEvent =>
            cases localCase : (projectGraph role config.choreo).events[
                config.choreo.localEventId role globalId]? with
            | none => simp [queuedCase, receiver, globalCase, localCase] at found
            | some localEvent =>
                cases actionCase : localEvent.action with
                | send peer label schema =>
                    simp [queuedCase, receiver, globalCase, localCase, actionCase] at found
                | «local» label schema =>
                    simp [queuedCase, receiver, globalCase, localCase, actionCase] at found
                | recv peer label schema =>
                    by_cases exact :
                        message.session = config.session /\
                        message.epoch = config.epoch /\
                        message.globalId = globalId /\
                        message.sender = peer /\
                        message.receiver = role /\
                        message.lane = globalEvent.lane /\
                        message.label = label /\
                        message.schema = schema
                    · have frameEq :
                          { globalId, lane := globalEvent.lane, message } = frame :=
                        Option.some.inj (by
                          simpa [queuedCase, receiver, globalCase, localCase,
                            actionCase, exact] using found)
                      subst frame
                      exact ⟨globalEvent, rfl, rfl, exact.2.2.2.2.2.1⟩
                    · simp [queuedCase, receiver, globalCase, localCase,
                        actionCase, exact] at found

/-- Preserve non-owner route knowledge while taking protocol-visible fields and
completion bits from the exact global successor. Event and resolver owners
learn only the route facts carried by their own successful operation. -/
def DistributedConfig.nextRoleState
    (current : DistributedConfig)
    (next : GlobalConfig)
    (authority : DistributedCommitAuthority)
    (role : Nat) : CommitState :=
  if role < next.roleCount then
    let selected :=
      match authority with
      | .event actor _ memberships =>
          if role = actor then
            applyConflictArms memberships (current.roleState role).selected
          else
            (current.roleState role).selected
      | .resolver actor conflict arm =>
          if role = actor then
            applyConflictArms [{ conflict, arm }] (current.roleState role).selected
          else
            (current.roleState role).selected
      | .rejection _ => (current.roleState role).selected
      | .roll => (next.localState role).selected
    { next.localState role with selected }
  else
    next.localState role

def DistributedConfig.fromGlobalSuccessor
    (current : DistributedConfig)
    (next : GlobalConfig)
    (authority : DistributedCommitAuthority) : DistributedConfig := {
  session := next.session
  roleCount := next.roleCount
  choreo := next.choreo
  epoch := next.epoch
  phase := next.phase
  queue := next.queue
  roleState := current.nextRoleState next authority
  globalSelection := next.routeSelection
  resources := next.resources
  status := next.status
}

private theorem selected_overwrite_is_identity
    {state : CommitState}
    {selected : Nat -> Option RouteArm}
    (same : state.selected = selected) :
    { state with selected } = state := by
  cases state
  simp_all

theorem distributed_abstraction_round_trip
    {config : GlobalConfig}
    (fidelity : RouteSelectionFidelity config) :
    (DistributedConfig.fromGlobalConfig config).abstract = config := by
  have localStateExact :
      (fun role =>
        if role < config.roleCount then
          { config.localState role with selected := config.routeSelection }
        else
          config.localState role) = config.localState := by
    funext role
    by_cases bound : role < config.roleCount
    · simp [bound, selected_overwrite_is_identity (fidelity role bound)]
    · simp [bound]
  unfold DistributedConfig.fromGlobalConfig DistributedConfig.abstract
  simp only
  rw [localStateExact]

theorem distributed_successor_abstraction
    {current : DistributedConfig}
    {next : GlobalConfig}
    {authority : DistributedCommitAuthority}
    (fidelity : RouteSelectionFidelity next) :
    (DistributedConfig.fromGlobalSuccessor current next authority).abstract = next := by
  have localStateExact :
      (fun role =>
        if role < next.roleCount then
          { current.nextRoleState next authority role with
            selected := next.routeSelection }
        else
          current.nextRoleState next authority role) = next.localState := by
    funext role
    by_cases bound : role < next.roleCount
    · cases authority <;>
        simp only [bound, ↓reduceIte, DistributedConfig.nextRoleState] <;>
        exact selected_overwrite_is_identity (fidelity role bound)
    · simp [DistributedConfig.nextRoleState, bound]
  unfold DistributedConfig.fromGlobalSuccessor DistributedConfig.abstract
  simp only
  rw [localStateExact]

private def DistributedConfig.routeEvidenceCompatible
    (config : DistributedConfig) (role : Nat) (membership : ConflictArm) : Bool :=
  config.globalSelection membership.conflict == some membership.arm &&
    match (config.roleState role).selected membership.conflict with
    | none => true
    | some selected => selected == membership.arm

private def DistributedConfig.routeEvidenceAddsKnowledge
    (config : DistributedConfig) (role : Nat)
    (memberships : List ConflictArm) : Bool :=
  memberships.any fun membership =>
    (config.roleState role).selected membership.conflict |>.isNone

/-- A role may learn route membership only once from an exact queued frame
addressed to that role. Empty membership, contradictory evidence, and replay of
already-known evidence fail closed; there is no global-selection shortcut. -/
def DistributedConfig.observeQueuedRouteEvidence?
    (config : DistributedConfig)
    (role globalId : Nat) : Option DistributedConfig :=
  if role < config.roleCount then
    match config.localQueuedFrame? role globalId,
        config.choreo.globalEventConflicts[globalId]? with
    | some (some _), some memberships =>
        if memberships.isEmpty then
          none
        else if memberships.all (config.routeEvidenceCompatible role) &&
            config.routeEvidenceAddsKnowledge role memberships then
          let selected := applyConflictArms memberships (config.roleState role).selected
          some {
            config with
            roleState := fun candidate =>
              if candidate = role then
                { config.roleState candidate with selected }
              else
                config.roleState candidate
          }
        else
          none
    | _, _ => none
  else
    none

private theorem role_selection_update_is_global_stutter
    (config : DistributedConfig)
    {role : Nat}
    (bound : role < config.roleCount)
    (selected : Nat -> Option RouteArm) :
    ({ config with
        roleState := fun candidate =>
          if candidate = role then
            { config.roleState candidate with selected }
          else
            config.roleState candidate } : DistributedConfig).abstract =
      config.abstract := by
  have localStateExact :
      (fun candidate =>
        if candidate < config.roleCount then
          { (if candidate = role then
              { config.roleState candidate with selected }
            else
              config.roleState candidate) with
            selected := config.globalSelection }
        else if candidate = role then
          { config.roleState candidate with selected }
        else
          config.roleState candidate) =
      (fun candidate =>
        if candidate < config.roleCount then
          { config.roleState candidate with selected := config.globalSelection }
        else
          config.roleState candidate) := by
    funext candidate
    by_cases candidateBound : candidate < config.roleCount
    · by_cases same : candidate = role
      · subst candidate
        simp only [bound, ↓reduceIte]
      · simp [candidateBound, same]
    · have different : candidate ≠ role := by
        intro same
        subst candidate
        exact candidateBound bound
      simp [candidateBound, different]
  unfold DistributedConfig.abstract
  simp only
  rw [localStateExact]

theorem role_selection_observation_is_global_stutter
    {config next : DistributedConfig}
    {role globalId : Nat}
    (observed : config.observeQueuedRouteEvidence? role globalId = some next) :
    next.abstract = config.abstract := by
  unfold DistributedConfig.observeQueuedRouteEvidence? at observed
  by_cases bound : role < config.roleCount
  · cases frameCase : config.localQueuedFrame? role globalId with
    | none => simp [bound, frameCase] at observed
    | some frame =>
        cases frame with
        | none => simp [bound, frameCase] at observed
        | some frame =>
            cases conflictsCase : config.choreo.globalEventConflicts[globalId]? with
            | none => simp [bound, frameCase, conflictsCase] at observed
            | some memberships =>
                by_cases empty : memberships.isEmpty
                · simp [bound, frameCase, conflictsCase, empty] at observed
                · by_cases accepted :
                      memberships.all (config.routeEvidenceCompatible role) &&
                        config.routeEvidenceAddsKnowledge role memberships
                  · let selected :=
                      applyConflictArms memberships (config.roleState role).selected
                    have nextEq :
                        { config with
                          roleState := fun candidate =>
                            if candidate = role then
                              { config.roleState candidate with selected }
                            else
                              config.roleState candidate } = next :=
                      Option.some.inj (by
                        simpa [bound, frameCase, conflictsCase, empty, accepted,
                          selected] using observed)
                    subst next
                    exact role_selection_update_is_global_stutter config bound selected
                  · simp [bound, frameCase, conflictsCase, empty, accepted] at observed
  · simp [bound] at observed

theorem route_selection_observation_has_exact_queued_evidence
    {config next : DistributedConfig}
    {role globalId : Nat}
    (observed : config.observeQueuedRouteEvidence? role globalId = some next) :
    ∃ frame memberships,
      config.localQueuedFrame? role globalId = some (some frame) /\
      config.choreo.globalEventConflicts[globalId]? = some memberships /\
      memberships ≠ [] /\
      memberships.all (config.routeEvidenceCompatible role) = true /\
      config.routeEvidenceAddsKnowledge role memberships = true := by
  unfold DistributedConfig.observeQueuedRouteEvidence? at observed
  by_cases bound : role < config.roleCount
  · cases frameCase : config.localQueuedFrame? role globalId with
    | none => simp [bound, frameCase] at observed
    | some frame =>
        cases frame with
        | none => simp [bound, frameCase] at observed
        | some frame =>
            cases conflictsCase : config.choreo.globalEventConflicts[globalId]? with
            | none => simp [bound, frameCase, conflictsCase] at observed
            | some memberships =>
                by_cases empty : memberships.isEmpty
                · simp [bound, frameCase, conflictsCase, empty] at observed
                · by_cases accepted :
                      memberships.all (config.routeEvidenceCompatible role) &&
                        config.routeEvidenceAddsKnowledge role memberships
                  · have parts := Bool.and_eq_true_iff.mp accepted
                    refine ⟨frame, memberships, rfl, rfl, ?_, parts.1, parts.2⟩
                    simpa using empty
                  · simp [bound, frameCase, conflictsCase, empty, accepted] at observed
  · simp [bound] at observed

def DistributedConfig.operationOwner?
    (config : DistributedConfig) : GlobalOperation -> Option Nat
  | .send globalId =>
      (config.choreo.globalEvents[globalId]?).map GlobalEvent.sender
  | .recv globalId =>
      (config.choreo.globalEvents[globalId]?).map GlobalEvent.receiver
  | .localAction globalId =>
      (config.choreo.globalEvents[globalId]?).map GlobalEvent.sender
  | .resolve role _ _ _ | .rejectResolver role _ _ => some role
  | .roll _ => none

private def DistributedConfig.eventConflictKnowledge
    (config : DistributedConfig)
    (role : Nat)
    (memberships : List ConflictArm) : Bool :=
  let graph := projectGraph role config.choreo
  memberships.all fun membership =>
    match config.globalSelection membership.conflict with
    | none =>
        match graph.routeForConflict? membership.conflict with
        | some route =>
            match route.authority with
            | .intrinsic => true
            | .dynamic _ => false
        | none => false
    | some selected =>
        selected == membership.arm &&
          (config.roleState role).selected membership.conflict == some selected

/-- An unresolved dynamic route is not local commit authority. Candidate
readiness and global rejection cannot be used as a fallback for missing local
resolver evidence. -/
theorem unresolved_dynamic_conflict_denies_local_knowledge
    {config : DistributedConfig}
    {role : Nat}
    {memberships : List ConflictArm}
    {membership : ConflictArm}
    {route : RouteInfo}
    {resolver : Nat}
    (inside : membership ∈ memberships)
    (routeFound :
      (projectGraph role config.choreo).routeForConflict? membership.conflict =
        some route)
    (dynamic : route.authority = .dynamic resolver)
    (unselected : config.globalSelection membership.conflict = none) :
    config.eventConflictKnowledge role memberships = false := by
  unfold DistributedConfig.eventConflictKnowledge
  cases allCase : memberships.all fun candidate =>
      match config.globalSelection candidate.conflict with
      | none =>
          match (projectGraph role config.choreo).routeForConflict? candidate.conflict with
          | some candidateRoute =>
              match candidateRoute.authority with
              | .intrinsic => true
              | .dynamic _ => false
          | none => false
      | some selected =>
          selected == candidate.arm &&
            (config.roleState role).selected candidate.conflict == some selected with
  | false => rfl
  | true =>
      have allowed := List.all_eq_true.mp allCase membership inside
      simp [unselected, routeFound, dynamic] at allowed

def DistributedConfig.localCommitAuthority?
    (config : DistributedConfig)
    (role : Nat)
    (operation : GlobalOperation) : Option DistributedCommitAuthority :=
  if config.operationOwner? operation = some role /\ role < config.roleCount then
    match operation with
    | .send globalId | .recv globalId | .localAction globalId =>
        match config.choreo.globalEventConflicts[globalId]? with
        | some memberships =>
            if config.eventConflictKnowledge role memberships then
              some (.event role globalId memberships)
            else
              none
        | none => none
    | .resolve _ conflict _ arm => some (.resolver role conflict arm)
    | .rejectResolver _ _ _ => some (.rejection role)
    | .roll _ => none
  else
    none

def DistributedConfig.protocolStep?
    (config : DistributedConfig)
    (role : Nat)
    (operation : GlobalOperation) : Option DistributedConfig :=
  match config.localCommitAuthority? role operation with
  | none => none
  | some authority =>
      match config.abstract.step? operation with
      | none => none
      | some next => some (config.fromGlobalSuccessor next authority)

/-- Abstract linearization point for one completed roll. A source-level runtime
refinement must show how independent endpoint reentry and carrier draining
establish this atomic transition before claiming distributed roll correctness. -/
def DistributedConfig.rollStep?
    (config : DistributedConfig) (rollId : Nat) : Option DistributedConfig :=
  match config.abstract.step? (.roll rollId) with
  | none => none
  | some next => some (config.fromGlobalSuccessor next .roll)

theorem distributed_protocol_step_is_role_owned
    {config next : DistributedConfig}
    {role : Nat}
    {operation : GlobalOperation}
    (stepped : config.protocolStep? role operation = some next) :
    config.operationOwner? operation = some role := by
  unfold DistributedConfig.protocolStep? at stepped
  cases authorityCase : config.localCommitAuthority? role operation with
  | none => simp [authorityCase] at stepped
  | some authority =>
      unfold DistributedConfig.localCommitAuthority? at authorityCase
      by_cases owned :
          config.operationOwner? operation = some role /\ role < config.roleCount
      · exact owned.1
      · simp [owned] at authorityCase

theorem distributed_protocol_step_has_local_authority
    {config next : DistributedConfig}
    {role : Nat}
    {operation : GlobalOperation}
    (stepped : config.protocolStep? role operation = some next) :
    ∃ authority,
      config.localCommitAuthority? role operation = some authority := by
  unfold DistributedConfig.protocolStep? at stepped
  cases authorityCase : config.localCommitAuthority? role operation with
  | none => simp [authorityCase] at stepped
  | some authority => exact ⟨authority, rfl⟩

theorem distributed_protocol_step_simulates_global
    {config next : DistributedConfig}
    {role : Nat}
    {operation : GlobalOperation}
    (fidelity : RouteSelectionFidelity config.abstract)
    (stepped : config.protocolStep? role operation = some next) :
    config.abstract.step? operation = some next.abstract := by
  unfold DistributedConfig.protocolStep? at stepped
  cases authorityCase : config.localCommitAuthority? role operation with
  | none => simp [authorityCase] at stepped
  | some authority =>
      cases globalStep : config.abstract.step? operation with
      | none => simp [authorityCase, globalStep] at stepped
      | some globalNext =>
          have nextEq : config.fromGlobalSuccessor globalNext authority = next :=
            Option.some.inj (by simpa [authorityCase, globalStep] using stepped)
          subst next
          have nextFidelity :=
            global_step_preserves_route_selection_fidelity fidelity globalStep
          rw [distributed_successor_abstraction nextFidelity]

theorem distributed_roll_step_simulates_global
    {config next : DistributedConfig}
    {rollId : Nat}
    (fidelity : RouteSelectionFidelity config.abstract)
    (stepped : config.rollStep? rollId = some next) :
    config.abstract.step? (.roll rollId) = some next.abstract := by
  unfold DistributedConfig.rollStep? at stepped
  cases globalStep : config.abstract.step? (.roll rollId) with
  | none => simp [globalStep] at stepped
  | some globalNext =>
      have nextEq : config.fromGlobalSuccessor globalNext .roll = next :=
        Option.some.inj (by simpa [globalStep] using stepped)
      subst next
      have nextFidelity :=
        global_step_preserves_route_selection_fidelity fidelity globalStep
      rw [distributed_successor_abstraction nextFidelity]

inductive DistributedStep : DistributedConfig -> DistributedConfig -> Prop where
  | observeRouteEvidence
      {current next : DistributedConfig}
      {role globalId : Nat}
      (observed : current.observeQueuedRouteEvidence? role globalId = some next) :
      DistributedStep current next
  | protocol
      {current next : DistributedConfig}
      {role : Nat}
      {operation : GlobalOperation}
      (stepped : current.protocolStep? role operation = some next) :
      DistributedStep current next
  | rollLinearization
      {current next : DistributedConfig}
      {rollId : Nat}
      (stepped : current.rollStep? rollId = some next) :
      DistributedStep current next

/-- Exact queued route evidence is a weak stutter; every externally visible
operation is exactly one transition of the existing global semantics. -/
theorem distributed_step_weakly_simulates_global
    {current next : DistributedConfig}
    (fidelity : RouteSelectionFidelity current.abstract)
    (advanced : DistributedStep current next) :
    next.abstract = current.abstract \/
      ∃ operation, current.abstract.step? operation = some next.abstract := by
  cases advanced with
  | observeRouteEvidence observed =>
      exact Or.inl (role_selection_observation_is_global_stutter observed)
  | @protocol role operation stepped =>
      exact Or.inr ⟨operation,
        distributed_protocol_step_simulates_global fidelity stepped⟩
  | @rollLinearization rollId stepped =>
      exact Or.inr ⟨.roll rollId,
        distributed_roll_step_simulates_global fidelity stepped⟩

theorem distributed_step_preserves_abstract_route_fidelity
    {current next : DistributedConfig}
    (fidelity : RouteSelectionFidelity current.abstract)
    (advanced : DistributedStep current next) :
    RouteSelectionFidelity next.abstract := by
  rcases distributed_step_weakly_simulates_global fidelity advanced with
    stutter | ⟨operation, stepped⟩
  · rw [stutter]
    exact fidelity
  · exact global_step_preserves_route_selection_fidelity fidelity stepped

end Hibana
