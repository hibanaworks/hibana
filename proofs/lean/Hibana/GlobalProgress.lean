import Hibana.Cancellation

namespace Hibana

def EventPhase.consumedIn (phase : EventPhase) (epoch : Nat) : Bool :=
  decide (phase = .consumed epoch)

def GlobalConfig.currentIterationComplete (config : GlobalConfig) : Bool :=
  (List.range config.choreo.globalEvents.length).all fun globalId =>
    config.eventSettled globalId

theorem event_excluded_of_selected_opposite
    {config : GlobalConfig} {globalId : Nat}
    {memberships : List ConflictArm} {membership : ConflictArm}
    {selected : RouteArm}
    (membershipsAt : config.eventConflicts? globalId = some memberships)
    (member : membership ∈ memberships)
    (selection : config.routeSelection membership.conflict = some selected)
    (opposite : selected ≠ membership.arm) :
    config.eventExcluded globalId = true := by
  rcases membership with ⟨conflict, arm⟩
  unfold GlobalConfig.eventExcluded
  rw [membershipsAt]
  apply List.any_eq_true.mpr
  refine ⟨⟨conflict, arm⟩, member, ?_⟩
  simp only [selection]
  cases selected <;> cases arm <;> simp_all <;> rfl

theorem consumed_empty_event_is_retired
    {config : GlobalConfig} {globalId epoch : Nat}
    (consumed : config.phase globalId = .consumed epoch)
    (empty : config.queue globalId = none) :
    config.eventRetired globalId = true := by
  simp [GlobalConfig.eventRetired, GlobalConfig.eventSettled, consumed, empty]

theorem excluded_empty_event_is_retired
    {config : GlobalConfig} {globalId : Nat}
    (excluded : config.eventExcluded globalId = true)
    (empty : config.queue globalId = none) :
    config.eventRetired globalId = true := by
  cases phaseCase : config.phase globalId <;>
    simp [GlobalConfig.eventRetired, GlobalConfig.eventSettled,
      phaseCase, excluded, empty]

/-- A roll is complete when every owned event is either consumed or erased by
the unique selected route arm, and every corresponding queue slot is empty. -/
theorem roll_ready_of_consumed_or_excluded
    {config : GlobalConfig} {roll : GlobalRoll}
    (retired : ∀ globalId, globalId ∈ roll.events ->
      ((∃ epoch, config.phase globalId = .consumed epoch) \/
        config.eventExcluded globalId = true) /\
      config.queue globalId = none) :
    config.rollReady roll = true := by
  unfold GlobalConfig.rollReady
  apply List.all_eq_true.mpr
  intro globalId member
  obtain ⟨state, empty⟩ := retired globalId member
  rcases state with consumed | excluded
  · obtain ⟨epoch, phase⟩ := consumed
    exact consumed_empty_event_is_retired phase empty
  · exact excluded_empty_event_is_retired excluded empty

def Choreo.GuardedRolls : Choreo -> Prop
  | .send _ _ _ _ => True
  | .seq left right | .par left right =>
      left.GuardedRolls /\ right.GuardedRolls
  | .route _ left right => left.GuardedRolls /\ right.GuardedRolls
  | .roll body => body.globalEvents ≠ [] /\ body.GuardedRolls

def Choreo.topLevelRollId? (choreo : Choreo) : Option Nat :=
  match choreo with
  | .roll _ => some (choreo.globalRolls.length - 1)
  | _ => none

/-- Top-level roll has no second execution path. It selects the outer syntactic
roll row and delegates to the same atomic transition used by nested rolls. -/
def GlobalConfig.topLevelRollStep? (config : GlobalConfig) : Option GlobalConfig :=
  match config.status, config.choreo.topLevelRollId? with
  | .live, some rollId => config.rollStep? rollId
  | _, _ => none

theorem top_level_roll_step_uses_global_roll_transition
    {config next : GlobalConfig}
    (rolled : config.topLevelRollStep? = some next) :
    ∃ rollId,
      config.choreo.topLevelRollId? = some rollId /\
      config.rollStep? rollId = some next := by
  unfold GlobalConfig.topLevelRollStep? at rolled
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at rolled
  | retired cause => simp [statusCase] at rolled
  | live =>
      cases rollCase : config.choreo.topLevelRollId? with
      | none => simp [statusCase, rollCase] at rolled
      | some rollId => exact ⟨rollId, rfl, by simpa [statusCase, rollCase] using rolled⟩

theorem top_level_roll_step_resets_iteration_atomically
    {config next : GlobalConfig}
    (rolled : config.topLevelRollStep? = some next) :
    ∃ rollId globalRoll,
      config.choreo.topLevelRollId? = some rollId /\
      config.choreo.globalRolls[rollId]? = some globalRoll /\
      config.rollReady globalRoll = true /\
      next.epoch = config.epoch /\
      (∀ globalId,
        next.phase globalId =
          if globalRoll.events.contains globalId then .ready else config.phase globalId) /\
      (∀ globalId,
        next.queue globalId =
          if globalRoll.events.contains globalId then none else config.queue globalId) := by
  obtain ⟨rollId, rollIdAt, stepped⟩ :=
    top_level_roll_step_uses_global_roll_transition rolled
  obtain ⟨globalRoll, globalRollAt, ready, epoch, phases, queues, _, _⟩ :=
    roll_step_exact_reset stepped
  exact ⟨rollId, globalRoll, rollIdAt, globalRollAt,
    ready, epoch, phases, queues⟩

theorem top_level_roll_step_establishes_session_fidelity
    {config next : GlobalConfig}
    (fidelity : SessionFidelity config)
    (rolled : config.topLevelRollStep? = some next) :
    SessionFidelity next := by
  obtain ⟨rollId, _, stepped⟩ := top_level_roll_step_uses_global_roll_transition rolled
  exact global_step_preserves_session_fidelity fidelity (operation := .roll rollId) stepped

theorem top_level_roll_step_has_no_owned_stale_queue
    {config next : GlobalConfig}
    (rolled : config.topLevelRollStep? = some next) :
    ∃ rollId globalRoll,
      config.choreo.topLevelRollId? = some rollId /\
      config.choreo.globalRolls[rollId]? = some globalRoll /\
      (∀ globalId, globalId ∈ globalRoll.events ->
        config.queue globalId = none /\ next.queue globalId = none) := by
  obtain ⟨rollId, rollIdAt, stepped⟩ :=
    top_level_roll_step_uses_global_roll_transition rolled
  obtain ⟨globalRoll, globalRollAt, ready, _, _, queues, _, _⟩ :=
    roll_step_exact_reset stepped
  refine ⟨rollId, globalRoll, rollIdAt, globalRollAt, ?_⟩
  intro globalId member
  have allReady := List.all_eq_true.mp ready globalId member
  have retired :
      config.eventSettled globalId = true /\ config.queue globalId = none := by
    simpa [GlobalConfig.eventRetired] using allReady
  constructor
  · exact retired.2
  · rw [queues globalId]
    simp [member]

theorem completed_roll_has_global_successor
    {config : GlobalConfig} {rollId : Nat}
    {globalRoll : GlobalRoll}
    (live : config.status = .live)
    (globalRollAt : config.choreo.globalRolls[rollId]? = some globalRoll)
    (ready : config.rollReady globalRoll = true) :
    ∃ next, config.step? (.roll rollId) = some next := by
  refine ⟨{
    config with
    phase := fun globalId =>
      if globalRoll.events.contains globalId then .ready else config.phase globalId
    queue := fun globalId =>
      if globalRoll.events.contains globalId then none else config.queue globalId
    localState := fun role =>
      if role < config.roleCount then
        config.resetLocalRollTo rollId role fun conflict =>
          if globalRoll.conflicts.contains conflict then none
          else config.routeSelection conflict
      else config.localState role
    routeSelection := fun conflict =>
      if globalRoll.conflicts.contains conflict then none
      else config.routeSelection conflict
  }, ?_⟩
  simp [GlobalConfig.step?, GlobalConfig.rollStep?, live, globalRollAt, ready]

private def findUnfinishedEvent
    (config : GlobalConfig) : List Nat -> Option (Nat × GlobalEvent)
  | [] => none
  | globalId :: rest =>
      match config.event? globalId with
      | none => findUnfinishedEvent config rest
      | some event =>
          if config.eventSettled globalId then
            findUnfinishedEvent config rest
          else
            some (globalId, event)

def GlobalConfig.unfinishedEvent? (config : GlobalConfig) : Option (Nat × GlobalEvent) :=
  findUnfinishedEvent config (List.range config.choreo.globalEvents.length)

def GlobalConfig.HasUnfinishedEvent (config : GlobalConfig) : Prop :=
  config.unfinishedEvent?.isSome

private theorem find_unfinished_event_sound
    (config : GlobalConfig) {ids : List Nat} {globalId : Nat} {event : GlobalEvent}
    (found : findUnfinishedEvent config ids = some (globalId, event)) :
    config.event? globalId = some event /\
      config.eventSettled globalId = false := by
  induction ids with
  | nil => simp [findUnfinishedEvent] at found
  | cons head tail inductionHypothesis =>
      unfold findUnfinishedEvent at found
      cases eventCase : config.event? head with
      | none =>
          rw [eventCase] at found
          exact inductionHypothesis found
      | some candidate =>
          rw [eventCase] at found
          cases settledCase : config.eventSettled head with
          | true =>
              rw [settledCase] at found
              exact inductionHypothesis found
          | false =>
              rw [settledCase] at found
              have pairExact := Option.some.inj found
              cases pairExact
              exact ⟨eventCase, settledCase⟩

private def GlobalConfig.resolverOperationsForRole
    (config : GlobalConfig) (role : Nat) : List GlobalOperation :=
  (projectGraph role config.choreo).routes.flatMap fun route =>
    match route.authority with
    | .intrinsic => []
    | .dynamic resolver => [
        .resolve role route.conflict resolver .left,
        .resolve role route.conflict resolver .right,
        .rejectResolver role route.conflict resolver
      ]

/-- Finite operation universe for one descriptor state. Choreography size and
declared role count determine the scan; no Rust generic or fixed proof capacity
is introduced. -/
def GlobalConfig.candidateOperations (config : GlobalConfig) : List GlobalOperation :=
  let eventOperations :=
    (List.range config.choreo.globalEvents.length).flatMap fun globalId =>
      [.send globalId, .recv globalId, .localAction globalId]
  let resolverOperations :=
    (List.range config.roleCount).flatMap config.resolverOperationsForRole
  let rollOperations :=
    (List.range config.choreo.globalRolls.length).map GlobalOperation.roll
  eventOperations ++ resolverOperations ++ rollOperations

private def findExecutableOperation
    (config : GlobalConfig) : List GlobalOperation -> Option (GlobalOperation × GlobalConfig)
  | [] => none
  | operation :: rest =>
      match config.step? operation with
      | some next => some (operation, next)
      | none => findExecutableOperation config rest

def GlobalConfig.executableOperation?
    (config : GlobalConfig) : Option (GlobalOperation × GlobalConfig) :=
  findExecutableOperation config config.candidateOperations

private theorem find_executable_operation_sound
    (config : GlobalConfig) {operations : List GlobalOperation}
    {operation : GlobalOperation} {next : GlobalConfig}
    (found : findExecutableOperation config operations = some (operation, next)) :
    config.step? operation = some next := by
  induction operations with
  | nil => simp [findExecutableOperation] at found
  | cons candidate rest inductionHypothesis =>
      unfold findExecutableOperation at found
      cases stepped : config.step? candidate with
      | none =>
          rw [stepped] at found
          exact inductionHypothesis found
      | some successor =>
          rw [stepped] at found
          have exactPair := Option.some.inj found
          cases exactPair
          exact stepped

theorem executable_operation_sound
    {config : GlobalConfig} {operation : GlobalOperation} {next : GlobalConfig}
    (found : config.executableOperation? = some (operation, next)) :
    config.step? operation = some next :=
  find_executable_operation_sound config found

/-- The remaining cross-role obligation is executable and finite: every live,
unfinished state must make the scheduler scan produce a real successor. -/
def GlobalExecutionCoherent (config : GlobalConfig) : Prop :=
  config.status = .live ->
  config.HasUnfinishedEvent ->
  config.executableOperation?.isSome

def GlobalLogicalProgress (config : GlobalConfig) : Prop :=
  (∃ cause awaiting, config.status = .cancelling cause awaiting) \/
  (∃ cause, config.status = .retired cause) \/
  ¬config.HasUnfinishedEvent \/
  ∃ operation next, config.step? operation = some next

theorem completed_roll_has_global_logical_progress
    {config : GlobalConfig} {rollId : Nat}
    {globalRoll : GlobalRoll}
    (live : config.status = .live)
    (globalRollAt : config.choreo.globalRolls[rollId]? = some globalRoll)
    (ready : config.rollReady globalRoll = true) :
    GlobalLogicalProgress config := by
  obtain ⟨next, stepped⟩ := completed_roll_has_global_successor
    live globalRollAt ready
  exact Or.inr <| Or.inr <| Or.inr ⟨.roll rollId, next, stepped⟩

theorem coherent_global_config_has_logical_progress
    {config : GlobalConfig}
    (coherent : GlobalExecutionCoherent config) :
    GlobalLogicalProgress config := by
  cases statusCase : config.status with
  | cancelling cause awaiting => exact Or.inl ⟨cause, awaiting, statusCase⟩
  | retired cause => exact Or.inr <| Or.inl ⟨cause, statusCase⟩
  | live =>
      cases unfinishedCase : config.unfinishedEvent? with
      | none =>
          exact Or.inr <| Or.inr <| Or.inl (by
            simp [GlobalConfig.HasUnfinishedEvent, unfinishedCase])
      | some unfinished =>
          have unfinished : config.HasUnfinishedEvent := by
            simp [GlobalConfig.HasUnfinishedEvent, unfinishedCase]
          have executable := coherent statusCase unfinished
          cases operationCase : config.executableOperation? with
          | none => simp [operationCase] at executable
          | some result =>
              obtain ⟨operation, next⟩ := result
              exact Or.inr <| Or.inr <| Or.inr
                ⟨operation, next, executable_operation_sound operationCase⟩

/-- Parallel branch occurrence IDs occupy disjoint intervals in the canonical
global preorder; identical message contracts cannot alias one event identity. -/
theorem parallel_global_event_id_ranges_are_disjoint
    (left right : Choreo)
    {leftId rightId : Nat}
    (leftBound : leftId < left.globalEvents.length)
    (rightBound : rightId < right.globalEvents.length) :
    leftId ≠ left.globalEvents.length + rightId /\
    left.globalEvents.length + rightId <
      (Choreo.par left right).globalEvents.length := by
  simp [Choreo.globalEvents]
  constructor <;> omega

/-- One checked protocol run. `none` is an explicit scheduler stutter; every
scheduled operation must be a real global transition. -/
structure GlobalRun (initial : GlobalConfig) where
  state : Nat -> GlobalConfig
  operation : Nat -> Option GlobalOperation
  startsAt : state 0 = initial
  advances : ∀ time,
    match operation time with
    | none => state (time + 1) = state time
    | some scheduled => (state time).step? scheduled = some (state (time + 1))

/-- Temporal scheduling is the sole liveness assumption. It ranges over the
actual finite operation type, so send, receive, local, resolver selection,
resolver rejection, and roll cannot fall through an incomplete case list.
Strong fairness applies only to operations enabled arbitrarily late; choosing
one route arm does not create an impossible obligation to run the losing arm. -/
structure GlobalFairnessAssumptions
    {initial : GlobalConfig} (run : GlobalRun initial) : Prop where
  recurrentlyEnabledOperationRuns :
    ∀ operation,
      (∀ time,
        ∃ later next,
          time ≤ later /\
          (run.state later).step? operation = some next) ->
      ∀ time,
        ∃ later,
          time ≤ later /\
          run.operation later = some operation

theorem fairness_schedules_recurrently_enabled_operation
    {initial : GlobalConfig} {run : GlobalRun initial}
    (fair : GlobalFairnessAssumptions run)
    {operation : GlobalOperation}
    (recurrentlyEnabled :
      ∀ time,
        ∃ later next,
          time ≤ later /\
          (run.state later).step? operation = some next) :
    ∀ time,
      ∃ later,
        time ≤ later /\
        run.operation later = some operation :=
  fair.recurrentlyEnabledOperationRuns operation recurrentlyEnabled

end Hibana
