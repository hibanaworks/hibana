import Hibana.DistributedSemantics

namespace Hibana

/-- Finite host-only image of a distributed endpoint state. `global` stores the
observable protocol fields; `localSelections` preserves route-knowledge lag
that the global abstraction deliberately erases. -/
structure CompactDistributedState where
  global : CompactGlobalState
  localSelections : List (List (Option RouteArm))
  deriving DecidableEq

def CompactDistributedState.fromDistributedConfig
    (config : DistributedConfig) : CompactDistributedState := {
  global := CompactGlobalState.fromGlobalConfig config.abstract
  localSelections := (List.range config.roleCount).map fun role =>
    (List.range (projectGraph role config.choreo).conflictCount).map
      (config.roleState role).selected
}

def CompactDistributedState.canonical
    (state : CompactDistributedState)
    (roleCount : Nat)
    (choreo : Choreo) : Bool :=
  state.global.canonical roleCount choreo &&
  state.localSelections.length == roleCount &&
  (List.range roleCount).all fun role =>
    match state.localSelections[role]? with
    | none => false
    | some selections =>
        selections.length == (projectGraph role choreo).conflictCount

/-- Fail-closed decoder. Missing role-selection rows reject the whole artifact;
no default route knowledge is synthesized inside the declared role domain. -/
def CompactDistributedState.decode?
    (state : CompactDistributedState)
    (session roleCount : Nat)
    (choreo : Choreo) : Option DistributedConfig :=
  if accepted : state.canonical roleCount choreo = true then
    have fields :
        (state.global.canonical roleCount choreo = true /\
          state.localSelections.length = roleCount) /\
          ∀ role, role < roleCount ->
            (match state.localSelections[role]? with
            | none => false
            | some selections =>
                selections.length ==
                  (projectGraph role choreo).conflictCount) = true := by
      simpa [CompactDistributedState.canonical] using accepted
    match state.global.decode? session roleCount choreo with
    | none => none
    | some global =>
        some {
          session := global.session
          roleCount := global.roleCount
          choreo := global.choreo
          epoch := global.epoch
          phase := global.phase
          queue := global.queue
          roleState := fun role =>
            if bound : role < roleCount then
              have roleIndex : role < state.localSelections.length := by
                simpa [fields.1.2] using bound
              let selections := state.localSelections[role]
              have present :
                  state.localSelections[role]? = some selections :=
                List.getElem?_eq_getElem roleIndex
              have selectionsLength :
                  selections.length =
                    (projectGraph role choreo).conflictCount := by
                have checked := fields.2 role bound
                rw [present] at checked
                simpa using checked
              {
                done := (global.localState role).done
                selected := fun conflict =>
                  if conflictBound :
                      conflict < (projectGraph role choreo).conflictCount then
                    selections.get ⟨conflict, by
                      simpa [selectionsLength] using conflictBound⟩
                  else
                    none
              }
            else
              global.localState role
          globalSelection := global.routeSelection
          resources := global.resources
          status := global.status
        }
  else
    none

def CompactDistributedState.initial
    (session roleCount : Nat) (choreo : Choreo) : CompactDistributedState :=
  CompactDistributedState.fromDistributedConfig <|
    DistributedConfig.fromGlobalConfig <|
      GlobalConfig.initial session roleCount choreo

/-- Finite scheduler vocabulary for the independently advancing role model. -/
inductive DistributedAction where
  | observeRouteEvidence (role globalId : Nat)
  | protocol (role : Nat) (operation : GlobalOperation)
  | rollLinearization (rollId : Nat)
  deriving Repr, DecidableEq

def DistributedConfig.applyAction?
    (config : DistributedConfig) : DistributedAction -> Option DistributedConfig
  | .observeRouteEvidence role globalId =>
      config.observeQueuedRouteEvidence? role globalId
  | .protocol role operation => config.protocolStep? role operation
  | .rollLinearization rollId => config.rollStep? rollId

def DistributedConfig.candidateActions
    (config : DistributedConfig) : List DistributedAction :=
  let observations := (List.range config.roleCount).flatMap fun role =>
    (List.range config.choreo.globalEvents.length).map fun globalId =>
      .observeRouteEvidence role globalId
  let protocol := (List.range config.roleCount).flatMap fun role =>
    config.abstract.candidateOperations.map fun operation =>
      .protocol role operation
  let rolls := (List.range config.choreo.globalRolls.length).map
    DistributedAction.rollLinearization
  observations ++ protocol ++ rolls

private def findExecutableDistributedAction
    (config : DistributedConfig) :
    List DistributedAction -> Option (DistributedAction × DistributedConfig)
  | [] => none
  | action :: rest =>
      match config.applyAction? action with
      | some next => some (action, next)
      | none => findExecutableDistributedAction config rest

def DistributedConfig.executableAction?
    (config : DistributedConfig) : Option (DistributedAction × DistributedConfig) :=
  findExecutableDistributedAction config config.candidateActions

private theorem find_executable_distributed_action_sound
    {config next : DistributedConfig}
    {action : DistributedAction}
    {actions : List DistributedAction}
    (found : findExecutableDistributedAction config actions = some (action, next)) :
    config.applyAction? action = some next := by
  induction actions with
  | nil => simp [findExecutableDistributedAction] at found
  | cons candidate rest inductionHypothesis =>
      cases applied : config.applyAction? candidate with
      | none =>
          exact inductionHypothesis (by
            simpa [findExecutableDistributedAction, applied] using found)
      | some candidateNext =>
          have exactPair : (candidate, candidateNext) = (action, next) :=
            Option.some.inj (by
              simpa [findExecutableDistributedAction, applied] using found)
          cases exactPair
          exact applied

theorem distributed_executable_action_sound
    {config next : DistributedConfig}
    {action : DistributedAction}
    (found : config.executableAction? = some (action, next)) :
    config.applyAction? action = some next := by
  unfold DistributedConfig.executableAction? at found
  exact find_executable_distributed_action_sound found

theorem distributed_action_is_model_step
    {config next : DistributedConfig}
    {action : DistributedAction}
    (applied : config.applyAction? action = some next) :
    DistributedStep config next := by
  cases action with
  | observeRouteEvidence role globalId => exact .observeRouteEvidence applied
  | protocol role operation => exact .protocol applied
  | rollLinearization rollId => exact .rollLinearization applied

def compactDistributedSuccessors
    (session roleCount : Nat)
    (choreo : Choreo)
    (state : CompactDistributedState) : List CompactDistributedState :=
  match state.decode? session roleCount choreo with
  | none => []
  | some config =>
      config.candidateActions.filterMap fun action =>
        (config.applyAction? action).map
          CompactDistributedState.fromDistributedConfig

theorem compact_distributed_successor_is_model_step
    {state next : CompactDistributedState}
    {session roleCount : Nat}
    {choreo : Choreo}
    (successor : next ∈
      compactDistributedSuccessors session roleCount choreo state) :
    ∃ config action distributedNext,
      state.decode? session roleCount choreo = some config /\
      config.applyAction? action = some distributedNext /\
      DistributedStep config distributedNext /\
      next = CompactDistributedState.fromDistributedConfig distributedNext := by
  unfold compactDistributedSuccessors at successor
  cases decoded : state.decode? session roleCount choreo with
  | none => simp [decoded] at successor
  | some config =>
      have expanded :
          ∃ action, action ∈ config.candidateActions /\
            ∃ distributedNext,
              config.applyAction? action = some distributedNext /\
              CompactDistributedState.fromDistributedConfig distributedNext = next := by
        simpa [decoded] using successor
      obtain ⟨action, _, distributedNext, applied, nextExact⟩ := expanded
      exact ⟨config, action, distributedNext, rfl, applied,
        distributed_action_is_model_step applied, nextExact.symm⟩

inductive DistributedLogicalProgress (config : DistributedConfig) : Prop where
  | cancelling (cause : SessionFault) (awaiting : List Nat)
      (status : config.status = .cancelling cause awaiting)
  | retired (cause : SessionFault) (status : config.status = .retired cause)
  | complete (complete : ¬config.abstract.HasUnfinishedEvent)
  | successor (action : DistributedAction) (next : DistributedConfig)
      (stepped : config.applyAction? action = some next)

def checkDistributedLogicalProgress
    (session roleCount : Nat)
    (choreo : Choreo)
    (state : CompactDistributedState) : Bool :=
  match state.decode? session roleCount choreo with
  | none => false
  | some config =>
      match config.status with
      | .live =>
          !config.abstract.unfinishedEvent?.isSome ||
            config.executableAction?.isSome
      | .cancelling _ _ | .retired _ => true

theorem distributed_logical_progress_checker_sound
    {state : CompactDistributedState}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted :
      checkDistributedLogicalProgress session roleCount choreo state = true) :
    ∃ config,
      state.decode? session roleCount choreo = some config /\
      DistributedLogicalProgress config := by
  cases decoded : state.decode? session roleCount choreo with
  | none => simp [checkDistributedLogicalProgress, decoded] at accepted
  | some config =>
      refine ⟨config, rfl, ?_⟩
      cases statusCase : config.status with
      | cancelling cause awaiting => exact .cancelling cause awaiting statusCase
      | retired cause => exact .retired cause statusCase
      | live =>
          cases unfinishedCase : config.abstract.unfinishedEvent? with
          | none =>
              exact .complete (by
                simp [GlobalConfig.HasUnfinishedEvent, unfinishedCase])
          | some unfinished =>
              have executable : config.executableAction?.isSome := by
                simpa [checkDistributedLogicalProgress, decoded, statusCase,
                  unfinishedCase] using accepted
              cases actionCase : config.executableAction? with
              | none => simp [actionCase] at executable
              | some result =>
                  obtain ⟨action, next⟩ := result
                  exact .successor action next
                    (distributed_executable_action_sound actionCase)

structure DistributedProgressCertificate where
  states : List CompactDistributedState

private def distributedStateMember
    (target : CompactDistributedState) : List CompactDistributedState -> Bool
  | [] => false
  | state :: rest =>
      decide (state = target) || distributedStateMember target rest

private theorem distributed_state_member_sound
    {target : CompactDistributedState}
    {states : List CompactDistributedState}
    (accepted : distributedStateMember target states = true) :
    target ∈ states := by
  induction states with
  | nil => simp [distributedStateMember] at accepted
  | cons state rest inductionHypothesis =>
      simp [distributedStateMember] at accepted
      rcases accepted with same | tail
      · simpa using Or.inl same.symm
      · simpa using Or.inr (inductionHypothesis tail)

private def enqueueFreshDistributedStates
    (candidates pending visited : List CompactDistributedState) :
    List CompactDistributedState :=
  candidates.foldl (fun queued candidate =>
    if distributedStateMember candidate visited ||
        distributedStateMember candidate queued then
      queued
    else
      queued ++ [candidate]) pending

def discoverDistributedProgressStates
    (session roleCount : Nat)
    (choreo : Choreo) :
    Nat -> List CompactDistributedState -> List CompactDistributedState ->
      List CompactDistributedState
  | 0, _, visited => visited
  | _ + 1, [], visited => visited
  | fuel + 1, state :: pending, visited =>
      if distributedStateMember state visited then
        discoverDistributedProgressStates session roleCount choreo fuel
          pending visited
      else
        let nextVisited := visited ++ [state]
        let nextPending := enqueueFreshDistributedStates
          (compactDistributedSuccessors session roleCount choreo state)
          pending nextVisited
        discoverDistributedProgressStates session roleCount choreo fuel
          nextPending nextVisited

def distributedProgressExplorationFuel
    (roleCount : Nat) (choreo : Choreo) : Nat :=
  globalProgressExplorationFuel roleCount choreo *
    3 ^ (roleCount * choreo.routeCount)

def buildDistributedProgressCertificate
    (session roleCount : Nat)
    (choreo : Choreo) : DistributedProgressCertificate := {
  states := discoverDistributedProgressStates session roleCount choreo
    (distributedProgressExplorationFuel roleCount choreo)
    [CompactDistributedState.initial session roleCount choreo] []
}

def DistributedProgressCertificate.Closed
    (certificate : DistributedProgressCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  0 < roleCount /\
  GlobalProgramWellFormed roleCount choreo /\
  choreo.GuardedRolls /\
  CompactDistributedState.initial session roleCount choreo ∈ certificate.states /\
  certificate.states.Nodup /\
  ∀ state, state ∈ certificate.states ->
    state.canonical roleCount choreo = true /\
    (∃ config,
      state.decode? session roleCount choreo = some config /\
      DistributedLogicalProgress config) /\
    ∀ next, next ∈ compactDistributedSuccessors
      session roleCount choreo state -> next ∈ certificate.states

def DistributedProgressCertificate.check
    (certificate : DistributedProgressCertificate)
    (session roleCount : Nat)
    (choreo : Choreo) : Bool :=
  decide (0 < roleCount) &&
  (checkGlobalProgramWellFormed roleCount choreo &&
  (checkGuardedRolls choreo &&
  (distributedStateMember
    (CompactDistributedState.initial session roleCount choreo)
    certificate.states &&
  (decide certificate.states.Nodup &&
  certificate.states.all fun state =>
    state.canonical roleCount choreo &&
    (checkDistributedLogicalProgress session roleCount choreo state &&
    (compactDistributedSuccessors session roleCount choreo state).all fun next =>
      distributedStateMember next certificate.states)))))

theorem distributed_progress_certificate_sound
    {certificate : DistributedProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    certificate.Closed session roleCount choreo := by
  unfold DistributedProgressCertificate.check at accepted
  obtain ⟨positive, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨programAccepted, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨guardedAccepted, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨initialAccepted, accepted⟩ := Bool.and_eq_true_iff.mp accepted
  obtain ⟨unique, statesAccepted⟩ := Bool.and_eq_true_iff.mp accepted
  refine ⟨of_decide_eq_true positive,
    global_program_well_formed_checker_sound programAccepted,
    guarded_rolls_checker_sound guardedAccepted,
    distributed_state_member_sound initialAccepted,
    of_decide_eq_true unique, ?_⟩
  intro state member
  have checked := List.all_eq_true.mp statesAccepted state member
  obtain ⟨canonical, stateAccepted⟩ := Bool.and_eq_true_iff.mp checked
  obtain ⟨progressAccepted, successorsAccepted⟩ :=
    Bool.and_eq_true_iff.mp stateAccepted
  refine ⟨canonical,
    distributed_logical_progress_checker_sound progressAccepted, ?_⟩
  intro next successor
  exact distributed_state_member_sound
    (List.all_eq_true.mp successorsAccepted next successor)

inductive CompactDistributedReachable
    (session roleCount : Nat)
    (choreo : Choreo) : CompactDistributedState -> Prop where
  | initial : CompactDistributedReachable session roleCount choreo
      (CompactDistributedState.initial session roleCount choreo)
  | step {state next} :
      CompactDistributedReachable session roleCount choreo state ->
      next ∈ compactDistributedSuccessors session roleCount choreo state ->
      CompactDistributedReachable session roleCount choreo next

theorem distributed_progress_certificate_covers_reachable
    {certificate : DistributedProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (closed : certificate.Closed session roleCount choreo)
    {state : CompactDistributedState}
    (reachable : CompactDistributedReachable session roleCount choreo state) :
    state ∈ certificate.states := by
  induction reachable with
  | initial => exact closed.2.2.2.1
  | step prior successor inductionHypothesis =>
      exact (closed.2.2.2.2.2 _ inductionHypothesis).2.2 _ successor

/-- Distributed asynchronous unstuckness for the independently advancing role
model, including finite choice-evidence delivery lag. This remains a model theorem;
source-level Rust refinement is a separate deployment obligation. -/
def DistributedSemanticallyUnstuck
    (session roleCount : Nat)
    (choreo : Choreo) : Prop :=
  ∀ state config,
    CompactDistributedReachable session roleCount choreo state ->
    state.decode? session roleCount choreo = some config ->
    config.status = .live ->
    config.abstract.HasUnfinishedEvent ->
    ∃ action next, config.applyAction? action = some next

theorem distributed_progress_certificate_establishes_unstuckness
    {certificate : DistributedProgressCertificate}
    {session roleCount : Nat}
    {choreo : Choreo}
    (accepted : certificate.check session roleCount choreo = true) :
    DistributedSemanticallyUnstuck session roleCount choreo := by
  intro state config reachable decoded statusLive unfinished
  have closed := distributed_progress_certificate_sound accepted
  have member := distributed_progress_certificate_covers_reachable closed reachable
  obtain ⟨verifiedConfig, verifiedDecoded, progress⟩ :=
    (closed.2.2.2.2.2 state member).2.1
  have configExact : verifiedConfig = config := Option.some.inj
    (verifiedDecoded.symm.trans decoded)
  subst verifiedConfig
  cases progress with
  | cancelling cause awaiting cancelling =>
      rw [statusLive] at cancelling
      contradiction
  | retired cause retired =>
      rw [statusLive] at retired
      contradiction
  | complete complete => exact False.elim (complete unfinished)
  | successor action next stepped => exact ⟨action, next, stepped⟩

end Hibana
