import Hibana.GlobalMessageTransitions

namespace Hibana

/-- Static global protocol validity. Self-role actions are unit-only and every
communication endpoint belongs to the declared role domain. -/
def GlobalProgramWellFormed (roleCount : Nat) (choreo : Choreo) : Prop :=
  ∀ event, event ∈ choreo.globalEvents ->
    event.sender < roleCount /\
    event.receiver < roleCount /\
    (event.sender = event.receiver -> event.schema = 0)

def GlobalConfig.WellFormed (config : GlobalConfig) : Prop :=
  0 < config.roleCount /\ GlobalProgramWellFormed config.roleCount config.choreo

/-- Exact queue fidelity classifies every event slot. Ready and consumed slots
own no message; a queued slot is current-epoch and owns exactly the message
derived from its unique global event identity. -/
def SessionFidelity (config : GlobalConfig) : Prop :=
  match config.status with
  | .live =>
      ∀ globalId,
        match config.phase globalId with
        | .ready | .consumed _ => config.queue globalId = none
        | .queued epoch =>
            epoch = config.epoch /\
            ∃ event,
              config.event? globalId = some event /\
              config.queue globalId = some (config.expectedMessage globalId event)
  | .cancelling _ _ | .retired _ =>
      ∀ globalId, config.queue globalId = none

/-- Every declared role observes the one global route-selection function.
Role-local commit state may cache completion bits, but never owns an
independent route decision. -/
def RouteSelectionFidelity (config : GlobalConfig) : Prop :=
  ∀ role, role < config.roleCount ->
    (config.localState role).selected = config.routeSelection

def GlobalInvariant (config : GlobalConfig) : Prop :=
  config.WellFormed /\ SessionFidelity config /\ RouteSelectionFidelity config

theorem initial_session_fidelity (session roleCount : Nat) (choreo : Choreo) :
    SessionFidelity (GlobalConfig.initial session roleCount choreo) := by
  intro globalId
  rfl

theorem initial_route_selection_fidelity (session roleCount : Nat) (choreo : Choreo) :
    RouteSelectionFidelity (GlobalConfig.initial session roleCount choreo) := by
  intro role bound
  rfl

theorem initial_global_invariant
    (session roleCount : Nat) (choreo : Choreo)
    (roleCountPositive : 0 < roleCount)
    (wellFormed : GlobalProgramWellFormed roleCount choreo) :
    GlobalInvariant (GlobalConfig.initial session roleCount choreo) :=
  ⟨⟨roleCountPositive, wellFormed⟩,
    initial_session_fidelity session roleCount choreo,
    initial_route_selection_fidelity session roleCount choreo⟩

theorem session_fidelity_queue_slot_is_exact
    {config : GlobalConfig} (fidelity : SessionFidelity config)
    {globalId : Nat} {message : WireMessage}
    (queued : config.queue globalId = some message) :
    ∃ event,
      config.event? globalId = some event /\
      config.phase globalId = .queued config.epoch /\
      message = config.expectedMessage globalId event := by
  unfold SessionFidelity at fidelity
  cases statusCase : config.status with
  | cancelling cause awaiting | retired cause =>
      rw [statusCase] at fidelity
      have empty := fidelity globalId
      simp [queued] at empty
  | live =>
      rw [statusCase] at fidelity
      have exact := fidelity globalId
      cases phaseCase : config.phase globalId with
      | ready => simp [phaseCase, queued] at exact
      | consumed epoch => simp [phaseCase, queued] at exact
      | queued epoch =>
          rw [phaseCase] at exact
          obtain ⟨current, event, present, queueExact⟩ := exact
          subst epoch
          have messageEq : message = config.expectedMessage globalId event :=
            Option.some.inj (queued.symm.trans queueExact)
          exact ⟨event, present, rfl, messageEq⟩

theorem session_fidelity_ready_slot_is_empty
    {config : GlobalConfig} (fidelity : SessionFidelity config)
    {globalId : Nat} (ready : config.phase globalId = .ready) :
    config.queue globalId = none := by
  unfold SessionFidelity at fidelity
  cases statusCase : config.status with
  | live =>
      rw [statusCase] at fidelity
      simpa [ready] using fidelity globalId
  | cancelling cause awaiting | retired cause =>
      rw [statusCase] at fidelity
      exact fidelity globalId

theorem session_fidelity_consumed_slot_is_empty
    {config : GlobalConfig} (fidelity : SessionFidelity config)
    {globalId epoch : Nat} (consumed : config.phase globalId = .consumed epoch) :
    config.queue globalId = none := by
  unfold SessionFidelity at fidelity
  cases statusCase : config.status with
  | live =>
      rw [statusCase] at fidelity
      simpa [consumed] using fidelity globalId
  | cancelling cause awaiting | retired cause =>
      rw [statusCase] at fidelity
      exact fidelity globalId

private theorem session_fidelity_after_slot_update
    {config : GlobalConfig}
    (fidelity : SessionFidelity config)
    (live : config.status = .live)
    (globalId : Nat) (phase : EventPhase) (message : Option WireMessage)
    (slot :
      match phase with
      | .ready | .consumed _ => message = none
      | .queued epoch =>
          epoch = config.epoch /\
          ∃ event,
            config.event? globalId = some event /\
            message = some (config.expectedMessage globalId event)) :
    SessionFidelity ((config.withPhase globalId phase).withQueue globalId message) := by
  unfold SessionFidelity
  simp [GlobalConfig.withQueue, GlobalConfig.withPhase, live]
  intro candidate
  by_cases same : candidate = globalId
  · subst candidate
    simpa [GlobalConfig.withQueue, GlobalConfig.withPhase] using slot
  · unfold SessionFidelity at fidelity
    rw [live] at fidelity
    simpa [GlobalConfig.withQueue, GlobalConfig.withPhase,
      GlobalConfig.event?, GlobalConfig.expectedMessage, same] using fidelity candidate

private theorem commit_local_preserves_session_fidelity
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (fidelity : SessionFidelity config)
    (committed : config.commitLocal? role globalId action = some next) :
    SessionFidelity next := by
  obtain ⟨_, localState, routeSelection, _, _, _, nextEq⟩ :=
    commit_local_success_shape committed
  subst next
  simpa [SessionFidelity, GlobalConfig.withLocalState,
    GlobalConfig.withRouteSelection] using fidelity

private theorem commit_local_preserves_message_identity
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (committed : config.commitLocal? role globalId action = some next)
    (lookup : Nat) (event : GlobalEvent) :
    next.epoch = config.epoch /\
    next.event? lookup = config.event? lookup /\
    next.expectedMessage lookup event = config.expectedMessage lookup event := by
  have same := (commit_local_preserves_global_frame committed).1
  have sessionEq := congrArg GlobalProtocolIdentity.session same
  have choreoEq := congrArg GlobalProtocolIdentity.choreo same
  have epochEq := congrArg GlobalProtocolIdentity.epoch same
  change next.session = config.session at sessionEq
  change next.choreo = config.choreo at choreoEq
  change next.epoch = config.epoch at epochEq
  exact ⟨epochEq, by simp [GlobalConfig.event?, choreoEq], by
    simp [GlobalConfig.expectedMessage, sessionEq, epochEq]⟩

private theorem commit_local_establishes_route_selection_fidelity
    {config next : GlobalConfig} {role globalId : Nat} {action : LocalAction}
    (committed : config.commitLocal? role globalId action = some next) :
    RouteSelectionFidelity next := by
  obtain ⟨_, localState, routeSelection, _, _, _, nextEq⟩ :=
    commit_local_success_shape committed
  subst next
  intro candidate bound
  rfl

private theorem slot_update_preserves_route_selection_fidelity
    {config : GlobalConfig}
    (fidelity : RouteSelectionFidelity config)
    (globalId : Nat) (phase : EventPhase) (message : Option WireMessage) :
    RouteSelectionFidelity
      ((config.withPhase globalId phase).withQueue globalId message) := by
  intro role bound
  simpa [GlobalConfig.withQueue, GlobalConfig.withPhase] using fidelity role bound

private theorem send_step_establishes_route_selection_fidelity
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.sendStep? globalId = some next) :
    RouteSelectionFidelity next := by
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
                        subst next
                        exact slot_update_preserves_route_selection_fidelity
                          (commit_local_establishes_route_selection_fidelity commitCase)
                          globalId (.queued config.epoch)
                          (some (config.expectedMessage globalId event))

private theorem recv_step_establishes_route_selection_fidelity
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.recvStep? globalId = some next) :
    RouteSelectionFidelity next := by
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
                      subst next
                      exact slot_update_preserves_route_selection_fidelity
                        (commit_local_establishes_route_selection_fidelity commitCase)
                        globalId (.consumed config.epoch) none
                · simp [statusCase, eventCase, phaseCase, current, exactMessage] at stepped
              · simp [statusCase, eventCase, phaseCase, current] at stepped

private theorem local_step_establishes_route_selection_fidelity
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.localStep? globalId = some next) :
    RouteSelectionFidelity next := by
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
                        subst next
                        exact slot_update_preserves_route_selection_fidelity
                          (commit_local_establishes_route_selection_fidelity commitCase)
                          globalId (.consumed config.epoch) none
                  · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                      queueEmpty] at stepped
                · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema] at stepped
              · simp [statusCase, eventCase, phaseCase, sameRole] at stepped

private theorem resolve_step_establishes_route_selection_fidelity
    {config next : GlobalConfig} {role conflict resolver : Nat} {arm : RouteArm}
    (stepped : config.resolveStep? role conflict resolver arm = some next) :
    RouteSelectionFidelity next := by
  unfold GlobalConfig.resolveStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases resolverCase : applyResolver
              (projectGraph role config.choreo)
              (config.localState role)
              conflict resolver (.select arm) with
          | none => simp [statusCase, selectionCase, resolverCase] at stepped
          | some result =>
              cases result with
              | rejected => simp [statusCase, selectionCase, resolverCase] at stepped
              | selected localState =>
                  have nextEq :
                      (config.withLocalState role localState).withRouteSelection
                        (fun candidate =>
                          if candidate = conflict then some arm
                          else config.routeSelection candidate) = next := by
                    exact Option.some.inj (by
                      simpa [statusCase, selectionCase, resolverCase] using stepped)
                  subst next
                  intro candidate bound
                  rfl

private theorem roll_step_establishes_route_selection_fidelity
    {config next : GlobalConfig} {rollId : Nat}
    (stepped : config.rollStep? rollId = some next) :
    RouteSelectionFidelity next := by
  obtain ⟨globalRoll, _, _, _, _, _, localStates, selections⟩ :=
    roll_step_exact_reset stepped
  intro role bound
  have roleCountEq := congrArg GlobalProtocolIdentity.roleCount
    (roll_step_preserves_protocol_identity stepped)
  change next.roleCount = config.roleCount at roleCountEq
  have sourceBound : role < config.roleCount := by
    simpa [roleCountEq] using bound
  rw [localStates role]
  simp [sourceBound, GlobalConfig.resetLocalRollTo]
  funext conflict
  rw [selections conflict]
  simp

private theorem begin_cancellation_preserves_route_selection_fidelity
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (fidelity : RouteSelectionFidelity config) :
    RouteSelectionFidelity (config.beginCancellation reporter cause) := by
  cases statusCase : config.status with
  | live =>
      cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter <;>
        simpa [GlobalConfig.beginCancellation, statusCase, pendingCase,
          RouteSelectionFidelity] using fidelity
  | cancelling existing awaiting | retired existing =>
      simpa [GlobalConfig.beginCancellation, statusCase] using fidelity

private theorem reject_resolver_step_preserves_route_selection_fidelity
    {config next : GlobalConfig} {role conflict resolver : Nat}
    (fidelity : RouteSelectionFidelity config)
    (stepped : config.rejectResolverStep? role conflict resolver = some next) :
    RouteSelectionFidelity next := by
  unfold GlobalConfig.rejectResolverStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases resolverCase : applyResolver
              (projectGraph role config.choreo)
              (config.localState role)
              conflict resolver .reject with
          | none => simp [statusCase, selectionCase, resolverCase] at stepped
          | some result =>
              cases result with
              | selected localState =>
                  simp [statusCase, selectionCase, resolverCase] at stepped
              | rejected =>
                  have nextEq : config.beginCancellation role .protocolViolation = next := by
                    exact Option.some.inj (by
                      simpa [statusCase, selectionCase, resolverCase] using stepped)
                  subst next
                  exact begin_cancellation_preserves_route_selection_fidelity fidelity

theorem global_step_preserves_route_selection_fidelity
    {config next : GlobalConfig} {operation : GlobalOperation}
    (fidelity : RouteSelectionFidelity config)
    (stepped : config.step? operation = some next) :
    RouteSelectionFidelity next := by
  cases operation with
  | send globalId => exact send_step_establishes_route_selection_fidelity stepped
  | recv globalId => exact recv_step_establishes_route_selection_fidelity stepped
  | localAction globalId => exact local_step_establishes_route_selection_fidelity stepped
  | resolve role conflict resolver arm =>
      exact resolve_step_establishes_route_selection_fidelity stepped
  | rejectResolver role conflict resolver =>
      exact reject_resolver_step_preserves_route_selection_fidelity fidelity stepped
  | roll rollId => exact roll_step_establishes_route_selection_fidelity stepped

theorem queued_message_has_exact_contract
    {config : GlobalConfig} {globalId : Nat} {message : WireMessage}
    (fidelity : SessionFidelity config)
    (queued : config.queuedMessage? globalId = some message) :
    ∃ event,
      config.event? globalId = some event /\
      message.session = config.session /\
      message.epoch = config.epoch /\
      message.globalId = globalId /\
      message.sender = event.sender /\
      message.receiver = event.receiver /\
      message.label = event.label /\
      message.schema = event.schema := by
  unfold GlobalConfig.queuedMessage? at queued
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at queued
  | retired cause => simp [statusCase] at queued
  | live =>
      cases phaseCase : config.phase globalId with
      | ready => simp [statusCase, phaseCase] at queued
      | consumed epoch => simp [statusCase, phaseCase] at queued
      | queued epoch =>
          by_cases current : epoch = config.epoch
          · have queuedRaw : config.queue globalId = some message := by
              simpa [statusCase, phaseCase, current] using queued
            unfold SessionFidelity at fidelity
            rw [statusCase] at fidelity
            have exact := fidelity globalId
            rw [phaseCase] at exact
            obtain ⟨_, event, present, queueExact⟩ := exact
            have messageEq : config.expectedMessage globalId event = message :=
              Option.some.inj (queueExact.symm.trans queuedRaw)
            subst message
            exact ⟨event, present, rfl, rfl, rfl, rfl, rfl, rfl, rfl⟩
          · simp [statusCase, phaseCase, current] at queued

theorem ready_event_has_no_queued_message
    {config : GlobalConfig} {globalId : Nat}
    (ready : config.phase globalId = .ready) :
    config.queuedMessage? globalId = none := by
  simp [GlobalConfig.queuedMessage?, ready]

theorem consumed_event_has_no_queued_message
    {config : GlobalConfig} {globalId epoch : Nat}
    (consumed : config.phase globalId = .consumed epoch) :
    config.queuedMessage? globalId = none := by
  simp [GlobalConfig.queuedMessage?, consumed]

theorem stale_epoch_message_is_rejected
    {config : GlobalConfig} {globalId epoch : Nat}
    (queued : config.phase globalId = .queued epoch)
  (stale : epoch ≠ config.epoch) :
    config.queuedMessage? globalId = none := by
  cases statusCase : config.status <;>
    simp [GlobalConfig.queuedMessage?, statusCase, queued, stale]

theorem protocol_identity_preserves_well_formed
    {config next : GlobalConfig}
    (same : next.protocolIdentity = config.protocolIdentity)
    (wellFormed : config.WellFormed) :
    next.WellFormed := by
  have roleCountEq := congrArg GlobalProtocolIdentity.roleCount same
  have choreoEq := congrArg GlobalProtocolIdentity.choreo same
  change next.roleCount = config.roleCount at roleCountEq
  change next.choreo = config.choreo at choreoEq
  simpa [GlobalConfig.WellFormed, roleCountEq, choreoEq] using wellFormed

/-- Subject reduction for the normalized global runtime: every accepted public
operation preserves the role domain and exact global choreography. -/
theorem global_step_subject_reduction
    {config next : GlobalConfig} {operation : GlobalOperation}
    (wellFormed : config.WellFormed)
    (stepped : config.step? operation = some next) :
    next.WellFormed := by
  exact protocol_identity_preserves_well_formed
    (global_step_preserves_protocol_identity stepped) wellFormed

private theorem send_step_preserves_session_fidelity
    {config next : GlobalConfig} {globalId : Nat}
    (fidelity : SessionFidelity config)
    (stepped : config.sendStep? globalId = some next) :
    SessionFidelity next := by
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
                        subst next
                        have committedFidelity :=
                          commit_local_preserves_session_fidelity fidelity commitCase
                        have committedLive : committed.status = .live :=
                          (commit_local_preserves_status commitCase).trans statusCase
                        obtain ⟨epochEq, eventEq, messageEq⟩ :=
                          commit_local_preserves_message_identity commitCase globalId event
                        apply session_fidelity_after_slot_update committedFidelity committedLive
                        refine ⟨epochEq.symm, event, ?_, ?_⟩
                        · rw [eventEq]
                          exact eventCase
                        · rw [messageEq]

private theorem recv_step_preserves_session_fidelity
    {config next : GlobalConfig} {globalId : Nat}
    (fidelity : SessionFidelity config)
    (stepped : config.recvStep? globalId = some next) :
    SessionFidelity next := by
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
                      subst next
                      have committedFidelity :=
                        commit_local_preserves_session_fidelity fidelity commitCase
                      have committedLive : committed.status = .live :=
                        (commit_local_preserves_status commitCase).trans statusCase
                      apply session_fidelity_after_slot_update committedFidelity committedLive
                      rfl
                · simp [statusCase, eventCase, phaseCase, current, exactMessage] at stepped
              · simp [statusCase, eventCase, phaseCase, current] at stepped

private theorem local_step_preserves_session_fidelity
    {config next : GlobalConfig} {globalId : Nat}
    (fidelity : SessionFidelity config)
    (stepped : config.localStep? globalId = some next) :
    SessionFidelity next := by
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
                        subst next
                        have committedFidelity :=
                          commit_local_preserves_session_fidelity fidelity commitCase
                        have committedLive : committed.status = .live :=
                          (commit_local_preserves_status commitCase).trans statusCase
                        apply session_fidelity_after_slot_update committedFidelity committedLive
                        rfl
                  · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                      queueEmpty] at stepped
                · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema] at stepped
              · simp [statusCase, eventCase, phaseCase, sameRole] at stepped

private theorem resolve_step_preserves_session_fidelity
    {config next : GlobalConfig} {role conflict resolver : Nat} {arm : RouteArm}
    (fidelity : SessionFidelity config)
    (stepped : config.resolveStep? role conflict resolver arm = some next) :
    SessionFidelity next := by
  unfold GlobalConfig.resolveStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases resolverCase : applyResolver
              (projectGraph role config.choreo)
              (config.localState role)
              conflict resolver (.select arm) with
          | none => simp [statusCase, selectionCase, resolverCase] at stepped
          | some result =>
              cases result with
              | rejected => simp [statusCase, selectionCase, resolverCase] at stepped
              | selected localState =>
                  have nextEq :
                      (config.withLocalState role localState).withRouteSelection
                        (fun candidate =>
                          if candidate = conflict then some arm
                          else config.routeSelection candidate) = next := by
                    exact Option.some.inj (by
                      simpa [statusCase, selectionCase, resolverCase] using stepped)
                  subst next
                  simpa [SessionFidelity, GlobalConfig.withLocalState,
                    GlobalConfig.withRouteSelection, GlobalConfig.event?,
                    GlobalConfig.expectedMessage] using fidelity

private theorem roll_step_preserves_session_fidelity
    {config next : GlobalConfig} {rollId : Nat}
    (fidelity : SessionFidelity config)
    (stepped : config.rollStep? rollId = some next) :
    SessionFidelity next := by
  unfold GlobalConfig.rollStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      have liveFidelity :
          ∀ globalId,
            match config.phase globalId with
            | .ready | .consumed _ => config.queue globalId = none
            | .queued epoch =>
                epoch = config.epoch /\
                ∃ event,
                  config.event? globalId = some event /\
                  config.queue globalId = some (config.expectedMessage globalId event) := by
        unfold SessionFidelity at fidelity
        simpa [statusCase] using fidelity
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
            unfold SessionFidelity
            simp [statusCase]
            intro globalId
            by_cases member : globalRoll.events.contains globalId
            · have memberProp : globalId ∈ globalRoll.events := by
                simpa using member
              simp [memberProp]
            · have notMember : globalId ∉ globalRoll.events := by
                simpa using member
              simpa [notMember] using liveFidelity globalId
          · simp [statusCase, globalRollCase, ready] at stepped

private theorem begin_cancellation_preserves_session_fidelity
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (fidelity : SessionFidelity config) :
    SessionFidelity (config.beginCancellation reporter cause) := by
  cases statusCase : config.status with
  | live =>
      cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter <;>
        simp [SessionFidelity, GlobalConfig.beginCancellation, statusCase, pendingCase]
  | cancelling existing awaiting | retired existing =>
      simpa [GlobalConfig.beginCancellation, statusCase] using fidelity

private theorem reject_resolver_step_preserves_session_fidelity
    {config next : GlobalConfig} {role conflict resolver : Nat}
    (fidelity : SessionFidelity config)
    (stepped : config.rejectResolverStep? role conflict resolver = some next) :
    SessionFidelity next := by
  unfold GlobalConfig.rejectResolverStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases resolverCase : applyResolver
              (projectGraph role config.choreo)
              (config.localState role)
              conflict resolver .reject with
          | none => simp [statusCase, selectionCase, resolverCase] at stepped
          | some result =>
              cases result with
              | selected localState =>
                  simp [statusCase, selectionCase, resolverCase] at stepped
              | rejected =>
                  have nextEq : config.beginCancellation role .protocolViolation = next := by
                    exact Option.some.inj (by
                      simpa [statusCase, selectionCase, resolverCase] using stepped)
                  subst next
                  exact begin_cancellation_preserves_session_fidelity fidelity

theorem global_step_preserves_session_fidelity
    {config next : GlobalConfig} {operation : GlobalOperation}
    (fidelity : SessionFidelity config)
    (stepped : config.step? operation = some next) :
    SessionFidelity next := by
  cases operation with
  | send globalId => exact send_step_preserves_session_fidelity fidelity stepped
  | recv globalId => exact recv_step_preserves_session_fidelity fidelity stepped
  | localAction globalId => exact local_step_preserves_session_fidelity fidelity stepped
  | resolve role conflict resolver arm =>
      exact resolve_step_preserves_session_fidelity fidelity stepped
  | rejectResolver role conflict resolver =>
      exact reject_resolver_step_preserves_session_fidelity fidelity stepped
  | roll rollId => exact roll_step_preserves_session_fidelity fidelity stepped

theorem global_step_preserves_global_invariant
    {config next : GlobalConfig} {operation : GlobalOperation}
    (invariant : GlobalInvariant config)
    (stepped : config.step? operation = some next) :
    GlobalInvariant next :=
  ⟨global_step_subject_reduction invariant.1 stepped,
    global_step_preserves_session_fidelity invariant.2.1 stepped,
    global_step_preserves_route_selection_fidelity invariant.2.2 stepped⟩

/-- Reflexive transitive closure of accepted global protocol operations. The
transition witness is retained so reachability cannot be fabricated from an
arbitrary state predicate. -/
inductive GlobalReachable (initial : GlobalConfig) : GlobalConfig -> Prop
  | initial : GlobalReachable initial initial
  | step {current next : GlobalConfig} {operation : GlobalOperation} :
      GlobalReachable initial current ->
      current.step? operation = some next ->
      GlobalReachable initial next

/-- Subject reduction and session fidelity hold at every state reachable from a
well-formed initial all-role configuration, not only for one isolated step. -/
theorem global_reachable_preserves_global_invariant
    {initial current : GlobalConfig}
    (initialInvariant : GlobalInvariant initial)
    (reachable : GlobalReachable initial current) :
    GlobalInvariant current := by
  induction reachable with
  | initial => exact initialInvariant
  | step previous transition inductionHypothesis =>
      exact global_step_preserves_global_invariant inductionHypothesis transition

theorem initial_global_reachable_preserves_global_invariant
    {session roleCount : Nat} {choreo : Choreo} {current : GlobalConfig}
    (roleCountPositive : 0 < roleCount)
    (wellFormed : GlobalProgramWellFormed roleCount choreo)
    (reachable : GlobalReachable
      (GlobalConfig.initial session roleCount choreo) current) :
    GlobalInvariant current :=
  global_reachable_preserves_global_invariant
    (initial_global_invariant session roleCount choreo roleCountPositive wellFormed)
    reachable

/-- A queued token has one receiver action with the exact peer, label, and
schema from its globally unique event identity. -/
theorem queued_message_has_unique_receive
    {config : GlobalConfig} {globalId : Nat} {message : WireMessage}
    (fidelity : SessionFidelity config)
    (queued : config.queuedMessage? globalId = some message)
    (distinct : Nat.beq message.sender message.receiver = false) :
    ∃ event,
      config.event? globalId = some event /\
      event.action? message.receiver =
        some (.recv message.sender message.label message.schema) := by
  obtain ⟨event, present, _, _, _, senderEq, receiverEq, labelEq, schemaEq⟩ :=
    queued_message_has_exact_contract fidelity queued
  rw [senderEq, receiverEq] at distinct ⊢
  rw [labelEq, schemaEq]
  exact ⟨event, present, (global_event_send_recv_duality event distinct).2⟩

/-- Affinity: one successful receive consumes its event token, removes the
queue entry, and makes a duplicate receive impossible. -/
theorem recv_step_is_affine
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.recvStep? globalId = some next) :
    next.queuedMessage? globalId = none /\ next.recvStep? globalId = none := by
  obtain ⟨_, _, _, _, noMessage, duplicateRejected⟩ :=
    recv_step_consumes_unique_message stepped
  exact ⟨noMessage, duplicateRejected⟩


end Hibana
