import Hibana.Generation
import Hibana.GlobalSyntax
import Hibana.OperationAdmission

namespace Hibana

/-- One affine global event is either unstarted, queued for its unique receive,
or consumed. The epoch tags the currently modeled queue token. Roll readiness
and queue clearing prevent two modeled iterations from sharing that token;
delivery outside this queue model remains a transport-contract obligation. -/
inductive EventPhase where
  | ready
  | queued (epoch : Nat)
  | consumed (epoch : Nat)
  deriving Repr, DecidableEq

/-- Global session lifecycle. `awaiting` contains exactly the roles that have
not yet observed cancellation. -/
inductive GlobalSessionStatus where
  | live
  | cancelling (cause : SessionFault) (awaiting : List Nat)
  | retired (cause : SessionFault)
  deriving DecidableEq

/-- A descriptor-admitted protocol occurrence. `globalId`, logical label,
schema, and epoch are protocol facts reconstructed after raw transport evidence
matches the exact resident descriptor; they are not claimed as wire fields. -/
structure AdmittedMessage where
  session : Nat
  epoch : Nat
  globalId : Nat
  sender : Nat
  receiver : Nat
  lane : Nat
  label : Nat
  schema : Nat
  deriving Repr, DecidableEq

/-- Role-indexed runtime ownership that must be retired after cancellation.
Each list is explicit so queue drain alone cannot masquerade as complete
transport, waiter, or lease cleanup. -/
structure ParticipantResourceOwners where
  transports : List Nat
  waiters : List Nat
  leases : List Nat
  deriving Repr, DecidableEq

def ParticipantResourceOwners.forRoles (roles : List Nat) : ParticipantResourceOwners := {
  transports := roles
  waiters := roles
  leases := roles
}

def ParticipantResourceOwners.empty : ParticipantResourceOwners :=
  .forRoles []

/-- Full normalized multiparty runtime state. Local cursor states are indexed
by role; communication state is indexed by global event identity. -/
structure GlobalConfig where
  session : Nat
  roleCount : Nat
  choreo : Choreo
  epoch : Nat
  phase : Nat -> EventPhase
  queue : Nat -> Option AdmittedMessage
  localState : Nat -> CommitState
  routeSelection : Nat -> Option RouteArm
  resources : ParticipantResourceOwners
  status : GlobalSessionStatus

def GlobalConfig.initial (session roleCount : Nat) (choreo : Choreo) : GlobalConfig := {
  session
  roleCount
  choreo
  epoch := 0
  phase := fun _ => .ready
  queue := fun _ => none
  localState := fun _ => .initial
  routeSelection := fun _ => none
  resources := .forRoles (List.range roleCount)
  status := .live
}

def removeAwaitingRole (awaiting : List Nat) (role : Nat) : List Nat :=
  awaiting.filter fun candidate => candidate != role

/-- The first runtime fault atomically clears all message queues and transfers
resource ownership to participants that still need to observe cancellation. -/
def GlobalConfig.beginCancellation
    (config : GlobalConfig) (reporter : Nat) (cause : SessionFault) : GlobalConfig :=
  match config.status with
  | .live =>
      match removeAwaitingRole (List.range config.roleCount) reporter with
      | [] => {
          config with
          status := .retired cause
          queue := fun _ => none
          resources := .empty
        }
      | awaiting => {
          config with
          status := .cancelling cause awaiting
          queue := fun _ => none
          resources := .forRoles awaiting
        }
  | .cancelling _ _ | .retired _ => config

def GlobalConfig.event? (config : GlobalConfig) (globalId : Nat) : Option GlobalEvent :=
  config.choreo.globalEvents[globalId]?

def GlobalConfig.eventConflicts?
    (config : GlobalConfig) (globalId : Nat) : Option (List ConflictArm) :=
  config.choreo.globalEventConflicts[globalId]?

/-- An event in a non-selected route arm is logically absent from the current
iteration. Global route selection is the sole authority for this decision. -/
def GlobalConfig.eventExcluded (config : GlobalConfig) (globalId : Nat) : Bool :=
  match config.eventConflicts? globalId with
  | none => false
  | some memberships => memberships.any fun membership =>
      match config.routeSelection membership.conflict with
      | none => false
      | some selected => selected != membership.arm

/-- Logical completion includes consumed events and route-erased events whose
queue slot is already empty. A queued opposite-arm frame is never hidden. -/
def GlobalConfig.eventSettled (config : GlobalConfig) (globalId : Nat) : Bool :=
  match config.phase globalId with
  | .consumed _ => true
  | .ready | .queued _ =>
      config.eventExcluded globalId && decide (config.queue globalId = none)

def GlobalConfig.eventRetired (config : GlobalConfig) (globalId : Nat) : Bool :=
  config.eventSettled globalId && decide (config.queue globalId = none)

def GlobalConfig.expectedMessage
    (config : GlobalConfig) (globalId : Nat) (event : GlobalEvent) : AdmittedMessage := {
  session := config.session
  epoch := config.epoch
  globalId
  sender := event.sender
  receiver := event.receiver
  lane := event.lane
  label := event.label
  schema := event.schema
}

/-- Event-indexed communication queue. A queue entry cannot exist without its
exact global descriptor occurrence. -/
def GlobalConfig.queuedMessage?
    (config : GlobalConfig) (globalId : Nat) : Option AdmittedMessage :=
  match config.status, config.phase globalId with
  | .live, .queued epoch =>
      if epoch = config.epoch then config.queue globalId else none
  | _, _ => none

def GlobalConfig.queueForChannel
    (config : GlobalConfig) (sender receiver lane : Nat) : List AdmittedMessage :=
  (List.range config.choreo.globalEvents.length).filterMap fun globalId =>
    match config.queuedMessage? globalId with
    | some message =>
        if message.sender = sender /\ message.receiver = receiver /\ message.lane = lane then
          some message
        else
          none
    | none => none

def GlobalConfig.withPhase
    (config : GlobalConfig) (globalId : Nat) (next : EventPhase) : GlobalConfig :=
  { config with
    phase := fun candidate => if candidate = globalId then next else config.phase candidate }

def GlobalConfig.withQueue
    (config : GlobalConfig) (globalId : Nat) (next : Option AdmittedMessage) : GlobalConfig :=
  { config with
    queue := fun candidate => if candidate = globalId then next else config.queue candidate }

def GlobalConfig.withLocalState
    (config : GlobalConfig) (role : Nat) (next : CommitState) : GlobalConfig :=
  { config with
    localState := fun candidate => if candidate = role then next else config.localState candidate }

def GlobalConfig.withRouteSelection
    (config : GlobalConfig) (next : Nat -> Option RouteArm) : GlobalConfig :=
  { config with
    localState := fun role => { config.localState role with selected := next }
    routeSelection := next }

structure GlobalProtocolIdentity where
  session : Nat
  roleCount : Nat
  choreo : Choreo
  epoch : Nat
  deriving DecidableEq

def GlobalConfig.protocolIdentity (config : GlobalConfig) : GlobalProtocolIdentity := {
  session := config.session
  roleCount := config.roleCount
  choreo := config.choreo
  epoch := config.epoch
}

@[simp]
theorem with_phase_preserves_protocol_identity
    (config : GlobalConfig) (globalId : Nat) (phase : EventPhase) :
    (config.withPhase globalId phase).protocolIdentity = config.protocolIdentity := rfl

@[simp]
theorem with_queue_preserves_protocol_identity
    (config : GlobalConfig) (globalId : Nat) (message : Option AdmittedMessage) :
    (config.withQueue globalId message).protocolIdentity = config.protocolIdentity := rfl

@[simp]
theorem with_local_state_preserves_protocol_identity
    (config : GlobalConfig) (role : Nat) (state : CommitState) :
    (config.withLocalState role state).protocolIdentity = config.protocolIdentity := rfl

@[simp]
theorem with_route_selection_preserves_protocol_identity
    (config : GlobalConfig) (selection : Nat -> Option RouteArm) :
    (config.withRouteSelection selection).protocolIdentity = config.protocolIdentity := rfl

def mergeRouteSelection? :
    List ConflictArm -> (Nat -> Option RouteArm) -> Option (Nat -> Option RouteArm)
  | [], selected => some selected
  | membership :: rest, selected =>
      match selected membership.conflict with
      | some arm =>
          if arm = membership.arm then mergeRouteSelection? rest selected else none
      | none =>
          mergeRouteSelection? rest fun conflict =>
            if conflict = membership.conflict then some membership.arm else selected conflict

theorem route_selection_rejects_opposite_arm
    {selected : Nat -> Option RouteArm}
    {conflict : Nat} {selectedArm requestedArm : RouteArm}
    (current : selected conflict = some selectedArm)
    (opposite : selectedArm ≠ requestedArm) :
    mergeRouteSelection? [{ conflict, arm := requestedArm }] selected = none := by
  simp [mergeRouteSelection?, current, opposite]

def GlobalConfig.commitLocal?
    (config : GlobalConfig)
    (role globalId : Nat)
    (action : LocalAction) : Option GlobalConfig :=
  let request : OperationRequest := {
    eventId := config.choreo.localEventId role globalId
    action
  }
  let graph := projectGraph role config.choreo
  match matchingOperationEvent? graph request with
  | none => none
  | some event =>
      match mergeRouteSelection? event.conflicts config.routeSelection with
      | none => none
      | some routeSelection =>
          match admitOperation graph (config.localState role) request with
          | .rejected => none
          | .accepted next =>
              some ((config.withLocalState role next).withRouteSelection routeSelection)

private theorem commit_local_preserves_protocol_identity
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (committed : config.commitLocal? role globalId action = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  unfold GlobalConfig.commitLocal? at committed
  cases matchCase : matchingOperationEvent?
      (projectGraph role config.choreo)
      { eventId := config.choreo.localEventId role globalId, action } with
  | none => simp [matchCase] at committed
  | some event =>
      cases mergeCase : mergeRouteSelection? event.conflicts config.routeSelection with
      | none => simp [matchCase, mergeCase] at committed
      | some routeSelection =>
          cases admissionCase : admitOperation
              (projectGraph role config.choreo)
              (config.localState role)
              { eventId := config.choreo.localEventId role globalId, action } with
          | rejected => simp [matchCase, mergeCase, admissionCase] at committed
          | accepted localState =>
              have nextEq :
                  (config.withLocalState role localState).withRouteSelection
                    routeSelection = next := by
                exact Option.some.inj (by
                  simpa [matchCase, mergeCase, admissionCase] using committed)
              subst next
              rfl

theorem commit_local_success_shape
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (committed : config.commitLocal? role globalId action = some next) :
    ∃ event localState routeSelection,
      matchingOperationEvent?
          (projectGraph role config.choreo)
          { eventId := config.choreo.localEventId role globalId, action } = some event /\
      mergeRouteSelection? event.conflicts config.routeSelection = some routeSelection /\
      admitOperation
          (projectGraph role config.choreo)
          (config.localState role)
          { eventId := config.choreo.localEventId role globalId, action } =
        .accepted localState /\
      next =
        (config.withLocalState role localState).withRouteSelection routeSelection := by
  unfold GlobalConfig.commitLocal? at committed
  cases matchCase : matchingOperationEvent?
      (projectGraph role config.choreo)
      { eventId := config.choreo.localEventId role globalId, action } with
  | none => simp [matchCase] at committed
  | some event =>
      cases mergeCase : mergeRouteSelection? event.conflicts config.routeSelection with
      | none => simp [matchCase, mergeCase] at committed
      | some routeSelection =>
          cases admissionCase : admitOperation
              (projectGraph role config.choreo)
              (config.localState role)
              { eventId := config.choreo.localEventId role globalId, action } with
          | rejected => simp [matchCase, mergeCase, admissionCase] at committed
          | accepted localState =>
              have nextEq :
                  (config.withLocalState role localState).withRouteSelection
                    routeSelection = next := by
                exact Option.some.inj (by
                  simpa [matchCase, mergeCase, admissionCase] using committed)
              exact ⟨event, localState, routeSelection, rfl, mergeCase,
                rfl, nextEq.symm⟩

/-- A local endpoint commit may update only local commit and route-selection
state. It cannot mutate session identity, event phases, wire queue, or resource
ownership. -/
theorem commit_local_preserves_global_frame
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (committed : config.commitLocal? role globalId action = some next) :
    next.protocolIdentity = config.protocolIdentity /\
      next.phase = config.phase /\
      next.queue = config.queue /\
      next.resources = config.resources := by
  refine ⟨commit_local_preserves_protocol_identity committed, ?_⟩
  obtain ⟨_, localState, routeSelection, _, _, _, nextEq⟩ :=
    commit_local_success_shape committed
  subst next
  exact ⟨rfl, rfl, rfl⟩

theorem commit_local_preserves_status
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (committed : config.commitLocal? role globalId action = some next) :
    next.status = config.status := by
  obtain ⟨_, localState, routeSelection, _, _, _, nextEq⟩ :=
    commit_local_success_shape committed
  subst next
  rfl

def GlobalConfig.sendStep? (config : GlobalConfig) (globalId : Nat) : Option GlobalConfig :=
  match config.status, config.event? globalId, config.phase globalId,
      config.queue globalId with
  | .live, some event, .ready, none =>
      if event.sender = event.receiver then
        none
      else
        match config.commitLocal? event.sender globalId
            (.send event.receiver event.label event.schema) with
        | none => none
        | some committed =>
            some ((committed.withPhase globalId (.queued config.epoch)).withQueue
              globalId (some (config.expectedMessage globalId event)))
  | _, _, _, _ => none

def GlobalConfig.recvStep? (config : GlobalConfig) (globalId : Nat) : Option GlobalConfig :=
  match config.status, config.event? globalId, config.phase globalId with
  | .live, some event, .queued epoch =>
      if epoch = config.epoch then
        if config.queue globalId = some (config.expectedMessage globalId event) then
          match config.commitLocal? event.receiver globalId
              (.recv event.sender event.label event.schema) with
          | none => none
          | some committed =>
              some ((committed.withPhase globalId (.consumed config.epoch)).withQueue
                globalId none)
        else
          none
      else
        none
  | _, _, _ => none

def GlobalConfig.localStep? (config : GlobalConfig) (globalId : Nat) : Option GlobalConfig :=
  match config.status, config.event? globalId, config.phase globalId with
  | .live, some event, .ready =>
      if event.sender = event.receiver then
        if event.schema = 0 then
          if config.queue globalId = none then
            match config.commitLocal? event.sender globalId (.local event.label event.schema) with
            | none => none
            | some committed =>
                some ((committed.withPhase globalId (.consumed config.epoch)).withQueue
                  globalId none)
          else
            none
        else
          none
      else
        none
  | _, _, _ => none

theorem send_step_rejects_occupied_queue
    {config : GlobalConfig} {globalId : Nat} {event : GlobalEvent}
    {message : AdmittedMessage}
    (live : config.status = .live)
    (present : config.event? globalId = some event)
    (ready : config.phase globalId = .ready)
    (occupied : config.queue globalId = some message) :
    config.sendStep? globalId = none := by
  simp [GlobalConfig.sendStep?, live, present, ready, occupied]

theorem recv_step_rejects_wrong_message
    {config : GlobalConfig} {globalId epoch : Nat} {event : GlobalEvent}
    {message : AdmittedMessage}
    (live : config.status = .live)
    (present : config.event? globalId = some event)
    (queued : config.phase globalId = .queued epoch)
    (current : epoch = config.epoch)
    (messageAt : config.queue globalId = some message)
    (wrong : message ≠ config.expectedMessage globalId event) :
    config.recvStep? globalId = none := by
  subst epoch
  simp [GlobalConfig.recvStep?, live, present, queued, messageAt, wrong]

def GlobalConfig.resolveStep?
    (config : GlobalConfig)
    (role conflict resolver : Nat)
    (arm : RouteArm) : Option GlobalConfig :=
  match config.status, config.routeSelection conflict,
      config.choreo.controllerForConflict? conflict with
  | .live, none, some controller =>
      if role = controller then
        match applyResolver
            (projectGraph role config.choreo)
            (config.localState role)
            conflict
            resolver
            (.select arm) with
        | some (.selected localState) =>
            let routeSelection := fun candidate =>
              if candidate = conflict then some arm else config.routeSelection candidate
            some ((config.withLocalState role localState).withRouteSelection routeSelection)
        | none | some .rejected => none
      else
        none
  | _, _, _ => none

/-- Resolver rejection is a protocol fault, not scheduler stutter. The same
operation that observes rejection begins distributed cancellation. -/
def GlobalConfig.rejectResolverStep?
    (config : GlobalConfig)
    (role conflict resolver : Nat) : Option GlobalConfig :=
  match config.status, config.routeSelection conflict,
      config.choreo.controllerForConflict? conflict with
  | .live, none, some controller =>
      if role = controller then
        match applyResolver
            (projectGraph role config.choreo)
            (config.localState role)
            conflict
            resolver
            .reject with
        | some .rejected =>
            some (config.beginCancellation role .protocolViolation)
        | none | some (.selected _) => none
      else
        none
  | _, _, _ => none

theorem resolve_step_has_exact_controller_successor
    {config next : GlobalConfig}
    {role conflict resolver : Nat}
    {arm : RouteArm}
    (stepped : config.resolveStep? role conflict resolver arm = some next) :
    ∃ localState,
      config.status = .live /\
      config.routeSelection conflict = none /\
      config.choreo.controllerForConflict? conflict = some role /\
      applyResolver
          (projectGraph role config.choreo)
          (config.localState role)
          conflict resolver (.select arm) = some (.selected localState) /\
      (config.withLocalState role localState).withRouteSelection
        (fun candidate =>
          if candidate = conflict then some arm
          else config.routeSelection candidate) = next := by
  unfold GlobalConfig.resolveStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases controllerCase : config.choreo.controllerForConflict? conflict with
          | none => simp [statusCase, selectionCase, controllerCase] at stepped
          | some controller =>
              by_cases controllerExact : role = controller
              · subst controller
                cases resolverCase : applyResolver
                    (projectGraph role config.choreo)
                    (config.localState role)
                    conflict resolver (.select arm) with
                | none =>
                    simp [statusCase, selectionCase, controllerCase,
                      resolverCase] at stepped
                | some result =>
                    cases result with
                    | rejected =>
                        simp [statusCase, selectionCase, controllerCase,
                          resolverCase] at stepped
                    | selected localState =>
                        have nextExact :
                            (config.withLocalState role localState).withRouteSelection
                                (fun candidate =>
                                  if candidate = conflict then some arm
                                  else config.routeSelection candidate) = next :=
                          Option.some.inj (by
                            simpa [statusCase, selectionCase, controllerCase,
                              resolverCase] using stepped)
                        exact ⟨localState, rfl, rfl, rfl, rfl, nextExact⟩
              · simp [statusCase, selectionCase, controllerCase,
                  controllerExact] at stepped

theorem reject_resolver_step_has_exact_controller_successor
    {config next : GlobalConfig}
    {role conflict resolver : Nat}
    (stepped : config.rejectResolverStep? role conflict resolver = some next) :
    config.status = .live /\
    config.routeSelection conflict = none /\
    config.choreo.controllerForConflict? conflict = some role /\
    applyResolver
        (projectGraph role config.choreo)
        (config.localState role)
        conflict resolver .reject = some .rejected /\
    config.beginCancellation role .protocolViolation = next := by
  unfold GlobalConfig.rejectResolverStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases controllerCase : config.choreo.controllerForConflict? conflict with
          | none => simp [statusCase, selectionCase, controllerCase] at stepped
          | some controller =>
              by_cases controllerExact : role = controller
              · subst controller
                cases resolverCase : applyResolver
                    (projectGraph role config.choreo)
                    (config.localState role)
                    conflict resolver .reject with
                | none =>
                    simp [statusCase, selectionCase, controllerCase,
                      resolverCase] at stepped
                | some result =>
                    cases result with
                    | selected localState =>
                        simp [statusCase, selectionCase, controllerCase,
                          resolverCase] at stepped
                    | rejected =>
                        have nextExact :
                            config.beginCancellation role .protocolViolation = next :=
                          Option.some.inj (by
                            simpa [statusCase, selectionCase, controllerCase,
                              resolverCase] using stepped)
                        exact ⟨rfl, rfl, rfl, rfl, nextExact⟩
              · simp [statusCase, selectionCase, controllerCase,
                  controllerExact] at stepped

theorem non_controller_resolve_is_rejected
    (config : GlobalConfig)
    {role controller conflict resolver : Nat}
    {arm : RouteArm}
    (controllerExact :
      config.choreo.controllerForConflict? conflict = some controller)
    (wrongRole : role ≠ controller) :
    config.resolveStep? role conflict resolver arm = none := by
  unfold GlobalConfig.resolveStep?
  rw [controllerExact]
  cases config.status <;> cases config.routeSelection conflict <;>
    simp [wrongRole]

theorem non_controller_rejection_is_rejected
    (config : GlobalConfig)
    {role controller conflict resolver : Nat}
    (controllerExact :
      config.choreo.controllerForConflict? conflict = some controller)
    (wrongRole : role ≠ controller) :
    config.rejectResolverStep? role conflict resolver = none := by
  unfold GlobalConfig.rejectResolverStep?
  rw [controllerExact]
  cases config.status <;> cases config.routeSelection conflict <;>
    simp [wrongRole]

def GlobalConfig.rollReady
    (config : GlobalConfig) (roll : GlobalRoll) : Bool :=
  roll.events.all config.eventRetired

def GlobalConfig.resetLocalRoll
    (config : GlobalConfig) (rollId role : Nat) : CommitState :=
  match (projectGraph role config.choreo).rolls[rollId]? with
  | some roll => resetRoll (config.localState role) roll
  | none => config.localState role

def GlobalConfig.resetLocalRollTo
    (config : GlobalConfig)
    (rollId role : Nat)
    (selected : Nat -> Option RouteArm) : CommitState :=
  { config.resetLocalRoll rollId role with selected }

theorem reset_local_roll_has_projected_row
    {config : GlobalConfig} {rollId role : Nat} {globalRoll : GlobalRoll}
    (present : config.choreo.globalRolls[rollId]? = some globalRoll) :
    ∃ projectedRoll,
      (projectGraph role config.choreo).rolls[rollId]? = some projectedRoll /\
      projectedRoll.conflicts = globalRoll.conflicts /\
      config.resetLocalRoll rollId role =
        resetRoll (config.localState role) projectedRoll := by
  have globalBound : rollId < config.choreo.globalRolls.length := by
    by_cases bound : rollId < config.choreo.globalRolls.length
    · exact bound
    · have missing : config.choreo.globalRolls[rollId]? = none :=
        List.getElem?_eq_none (Nat.le_of_not_gt bound)
      rw [missing] at present
      simp at present
  have projectedBound : rollId < (projectGraph role config.choreo).rolls.length := by
    rw [project_graph_roll_count_matches_global]
    exact globalBound
  cases projectedCase : (projectGraph role config.choreo).rolls[rollId]? with
  | none =>
      have outOfBounds : (projectGraph role config.choreo).rolls.length ≤ rollId :=
        List.getElem?_eq_none_iff.mp projectedCase
      exact False.elim ((Nat.not_lt_of_ge outOfBounds) projectedBound)
  | some projectedRoll =>
      have conflictLists := project_graph_roll_conflicts_match_global role config.choreo
      have conflictAt := congrArg (fun rows => rows[rollId]?) conflictLists
      have conflictsEqual : projectedRoll.conflicts = globalRoll.conflicts := by
        have optionsEqual : some projectedRoll.conflicts = some globalRoll.conflicts := by
          simpa [projectedCase, present] using conflictAt
        exact Option.some.inj optionsEqual
      exact ⟨projectedRoll, rfl, conflictsEqual, by
        simp [GlobalConfig.resetLocalRoll, projectedCase]⟩

/-- Reset one completed syntactic roll occurrence across every role. The reset
touches only the roll's global event range and route conflicts; unrelated
parallel work, queues, and local completion bits remain intact. -/
def GlobalConfig.rollStep?
    (config : GlobalConfig) (rollId : Nat) : Option GlobalConfig :=
  match config.status, config.choreo.globalRolls[rollId]? with
  | .live, some globalRoll =>
      if config.rollReady globalRoll then
        let selected := fun conflict =>
          if globalRoll.conflicts.contains conflict then none
          else config.routeSelection conflict
        some {
          config with
          phase := fun globalId =>
            if globalRoll.events.contains globalId then .ready else config.phase globalId
          queue := fun globalId =>
            if globalRoll.events.contains globalId then none else config.queue globalId
          localState := fun role =>
            if role < config.roleCount then config.resetLocalRollTo rollId role selected
            else config.localState role
          routeSelection := selected
        }
      else
        none
  | _, _ => none

inductive GlobalOperation where
  | send (globalId : Nat)
  | recv (globalId : Nat)
  | localAction (globalId : Nat)
  | resolve (role conflict resolver : Nat) (arm : RouteArm)
  | rejectResolver (role conflict resolver : Nat)
  | roll (rollId : Nat)
  deriving Repr, DecidableEq

def GlobalConfig.step? (config : GlobalConfig) : GlobalOperation -> Option GlobalConfig
  | .send globalId => config.sendStep? globalId
  | .recv globalId => config.recvStep? globalId
  | .localAction globalId => config.localStep? globalId
  | .resolve role conflict resolver arm =>
      config.resolveStep? role conflict resolver arm
  | .rejectResolver role conflict resolver =>
      config.rejectResolverStep? role conflict resolver
  | .roll rollId => config.rollStep? rollId

theorem send_step_preserves_protocol_identity
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.sendStep? globalId = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  unfold GlobalConfig.sendStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases eventCase : config.event? globalId with
      | none => simp [statusCase, eventCase] at stepped
      | some event =>
          cases phaseCase : config.phase globalId with
          | queued epoch => simp [statusCase, eventCase, phaseCase] at stepped
          | consumed epoch => simp [statusCase, eventCase, phaseCase] at stepped
          | ready =>
              cases queueCase : config.queue globalId with
              | some message => simp [statusCase, eventCase, phaseCase, queueCase] at stepped
              | none =>
                  by_cases sameRole : event.sender = event.receiver
                  · simp [statusCase, eventCase, phaseCase, queueCase, sameRole] at stepped
                  · cases commitCase : config.commitLocal? event.sender globalId
                        (.send event.receiver event.label event.schema) with
                    | none =>
                        simp [statusCase, eventCase, phaseCase, queueCase, sameRole,
                          commitCase] at stepped
                    | some committed =>
                        have nextEq :
                            (committed.withPhase globalId (.queued config.epoch)).withQueue
                              globalId (some (config.expectedMessage globalId event)) = next := by
                          exact Option.some.inj (by
                            simpa [statusCase, eventCase, phaseCase, queueCase, sameRole,
                              commitCase] using stepped)
                        have same := commit_local_preserves_protocol_identity commitCase
                        subst next
                        simpa using same

theorem recv_step_preserves_protocol_identity
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.recvStep? globalId = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  unfold GlobalConfig.recvStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases eventCase : config.event? globalId with
      | none => simp [statusCase, eventCase] at stepped
      | some event =>
          cases phaseCase : config.phase globalId with
          | ready => simp [statusCase, eventCase, phaseCase] at stepped
          | consumed epoch => simp [statusCase, eventCase, phaseCase] at stepped
          | queued epoch =>
              by_cases current : epoch = config.epoch
              · by_cases exactMessage :
                    config.queue globalId = some (config.expectedMessage globalId event)
                · cases commitCase : config.commitLocal? event.receiver globalId
                      (.recv event.sender event.label event.schema) with
                  | none =>
                      simp [statusCase, eventCase, phaseCase, current, exactMessage,
                        commitCase] at stepped
                  | some committed =>
                      have nextEq :
                          (committed.withPhase globalId (.consumed config.epoch)).withQueue
                            globalId none = next := by
                        exact Option.some.inj (by
                          simpa [statusCase, eventCase, phaseCase, current, exactMessage,
                            commitCase] using stepped)
                      have same := commit_local_preserves_protocol_identity commitCase
                      subst next
                      simpa using same
                · simp [statusCase, eventCase, phaseCase, current, exactMessage] at stepped
              · simp [statusCase, eventCase, phaseCase, current] at stepped

theorem local_step_preserves_protocol_identity
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.localStep? globalId = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  unfold GlobalConfig.localStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases eventCase : config.event? globalId with
      | none => simp [statusCase, eventCase] at stepped
      | some event =>
          cases phaseCase : config.phase globalId with
          | queued epoch => simp [statusCase, eventCase, phaseCase] at stepped
          | consumed epoch => simp [statusCase, eventCase, phaseCase] at stepped
          | ready =>
              by_cases sameRole : event.sender = event.receiver
              · by_cases unitSchema : event.schema = 0
                · by_cases queueEmpty : config.queue globalId = none
                  · cases commitCase : config.commitLocal? event.receiver globalId
                        (.local event.label 0) with
                    | none =>
                        simp [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                          queueEmpty, commitCase] at stepped
                    | some committed =>
                        have nextEq :
                            (committed.withPhase globalId (.consumed config.epoch)).withQueue
                              globalId none = next := by
                          exact Option.some.inj (by
                            simpa [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                              queueEmpty, commitCase] using stepped)
                        have same := commit_local_preserves_protocol_identity commitCase
                        subst next
                        simpa using same
                  · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                      queueEmpty] at stepped
                · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema] at stepped
              · simp [statusCase, eventCase, phaseCase, sameRole] at stepped

theorem resolve_step_preserves_protocol_identity
    {config next : GlobalConfig} {role conflict resolver : Nat} {arm : RouteArm}
    (stepped : config.resolveStep? role conflict resolver arm = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  obtain ⟨localState, _, _, _, _, nextExact⟩ :=
    resolve_step_has_exact_controller_successor stepped
  rw [← nextExact]
  rfl

theorem reject_resolver_step_preserves_protocol_identity
    {config next : GlobalConfig} {role conflict resolver : Nat}
    (stepped : config.rejectResolverStep? role conflict resolver = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  obtain ⟨statusLive, _, _, _, nextExact⟩ :=
    reject_resolver_step_has_exact_controller_successor stepped
  rw [← nextExact]
  cases pendingCase : removeAwaitingRole
      (List.range config.roleCount) role <;>
    simp [GlobalConfig.beginCancellation, statusLive, pendingCase,
      GlobalConfig.protocolIdentity]

theorem resolve_step_publishes_one_arm_and_rejects_reselection
    {config next : GlobalConfig} {role conflict resolver : Nat} {arm : RouteArm}
    (stepped : config.resolveStep? role conflict resolver arm = some next) :
    next.routeSelection conflict = some arm /\
    ∀ requested, next.resolveStep? role conflict resolver requested = none := by
  obtain ⟨localState, statusLive, _, controller, _, nextExact⟩ :=
    resolve_step_has_exact_controller_successor stepped
  rw [← nextExact]
  constructor
  · simp [GlobalConfig.withLocalState, GlobalConfig.withRouteSelection]
  · intro requested
    simp [GlobalConfig.resolveStep?, GlobalConfig.withLocalState,
      GlobalConfig.withRouteSelection, statusLive, controller]

theorem roll_step_preserves_protocol_identity
    {config next : GlobalConfig} {rollId : Nat}
    (stepped : config.rollStep? rollId = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  unfold GlobalConfig.rollStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases globalRollCase : config.choreo.globalRolls[rollId]? with
      | none => simp [statusCase, globalRollCase] at stepped
      | some globalRoll =>
          by_cases ready : config.rollReady globalRoll
          · have nextEq :
                { config with
                  phase := fun globalId =>
                    if globalRoll.events.contains globalId then .ready
                    else config.phase globalId
                  queue := fun globalId =>
                    if globalRoll.events.contains globalId then none
                    else config.queue globalId
                  localState := fun role =>
                    if role < config.roleCount then
                      config.resetLocalRollTo rollId role fun conflict =>
                        if globalRoll.conflicts.contains conflict then none
                        else config.routeSelection conflict
                    else config.localState role
                  routeSelection := fun conflict =>
                    if globalRoll.conflicts.contains conflict then none
                    else config.routeSelection conflict } = next := by
                exact Option.some.inj (by
                  simpa [statusCase, globalRollCase, ready] using stepped)
            subst next
            rfl
          · simp [statusCase, globalRollCase, ready] at stepped

/-- A successful nested or top-level roll has one exact atomic reset delta.
Every declared role resets the corresponding projected roll; unrelated global
events and route decisions retain their previous values. -/
theorem roll_step_exact_reset
    {config next : GlobalConfig} {rollId : Nat}
    (stepped : config.rollStep? rollId = some next) :
    ∃ globalRoll,
      config.choreo.globalRolls[rollId]? = some globalRoll /\
      config.rollReady globalRoll = true /\
      next.epoch = config.epoch /\
      (∀ globalId,
        next.phase globalId =
          if globalRoll.events.contains globalId then .ready else config.phase globalId) /\
      (∀ globalId,
        next.queue globalId =
          if globalRoll.events.contains globalId then none else config.queue globalId) /\
      (∀ role,
        next.localState role =
          if role < config.roleCount then
            config.resetLocalRollTo rollId role fun conflict =>
              if globalRoll.conflicts.contains conflict then none
              else config.routeSelection conflict
          else config.localState role) /\
      (∀ conflict,
        next.routeSelection conflict =
          if globalRoll.conflicts.contains conflict then none
          else config.routeSelection conflict) := by
  unfold GlobalConfig.rollStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases globalRollCase : config.choreo.globalRolls[rollId]? with
      | none => simp [statusCase, globalRollCase] at stepped
      | some globalRoll =>
          cases readyCase : config.rollReady globalRoll with
          | false => simp [statusCase, globalRollCase, readyCase] at stepped
          | true =>
              have nextEq :
                  { config with
                    phase := fun globalId =>
                      if globalRoll.events.contains globalId then .ready
                      else config.phase globalId
                    queue := fun globalId =>
                      if globalRoll.events.contains globalId then none
                      else config.queue globalId
                    localState := fun role =>
                      if role < config.roleCount then
                        config.resetLocalRollTo rollId role fun conflict =>
                          if globalRoll.conflicts.contains conflict then none
                          else config.routeSelection conflict
                      else config.localState role
                    routeSelection := fun conflict =>
                      if globalRoll.conflicts.contains conflict then none
                      else config.routeSelection conflict } = next := by
                exact Option.some.inj (by
                  simpa [statusCase, globalRollCase, readyCase] using stepped)
              subst next
              exact ⟨globalRoll, rfl, readyCase, rfl,
                fun _ => rfl, fun _ => rfl, fun _ => rfl, fun _ => rfl⟩

theorem roll_step_clears_owned_queue
    {config next : GlobalConfig} {rollId globalId : Nat}
    (stepped : config.rollStep? rollId = some next)
    (owned : ∃ roll, config.choreo.globalRolls[rollId]? = some roll /\
      globalId ∈ roll.events) :
    next.phase globalId = .ready /\ next.queue globalId = none := by
  obtain ⟨globalRoll, globalRollAt, _, _, phases, queues, _, _⟩ :=
    roll_step_exact_reset stepped
  obtain ⟨ownedRoll, ownedAt, member⟩ := owned
  have sameRoll : ownedRoll = globalRoll := by
    rw [globalRollAt] at ownedAt
    exact Option.some.inj ownedAt.symm
  subst ownedRoll
  constructor
  · rw [phases globalId]
    simp [member]
  · rw [queues globalId]
    simp [member]

theorem roll_step_preserves_outside_event
    {config next : GlobalConfig} {rollId globalId : Nat}
    (stepped : config.rollStep? rollId = some next)
    (outside : ∀ roll, config.choreo.globalRolls[rollId]? = some roll ->
      globalId ∉ roll.events) :
    next.phase globalId = config.phase globalId /\
      next.queue globalId = config.queue globalId := by
  obtain ⟨globalRoll, globalRollAt, _, _, phases, queues, _, _⟩ :=
    roll_step_exact_reset stepped
  have notMember := outside globalRoll globalRollAt
  constructor
  · rw [phases globalId]
    simp [notMember]
  · rw [queues globalId]
    simp [notMember]

theorem global_step_preserves_protocol_identity
    {config next : GlobalConfig} {operation : GlobalOperation}
    (stepped : config.step? operation = some next) :
    next.protocolIdentity = config.protocolIdentity := by
  cases operation with
  | send globalId => exact send_step_preserves_protocol_identity stepped
  | recv globalId => exact recv_step_preserves_protocol_identity stepped
  | localAction globalId => exact local_step_preserves_protocol_identity stepped
  | resolve role conflict resolver arm =>
      exact resolve_step_preserves_protocol_identity stepped
  | rejectResolver role conflict resolver =>
      exact reject_resolver_step_preserves_protocol_identity stepped
  | roll rollId => exact roll_step_preserves_protocol_identity stepped

end Hibana
