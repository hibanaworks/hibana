import Hibana.GlobalSemantics

namespace Hibana

theorem send_step_enqueues_exact_message
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.sendStep? globalId = some next) :
    ∃ event,
      next.event? globalId = some event /\
      event.sender ≠ event.receiver /\
      next.phase globalId = .queued next.epoch /\
      next.queuedMessage? globalId = some (next.expectedMessage globalId event) := by
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
                        obtain ⟨_, localState, routeSelection, _, _, _, committedEq⟩ :=
                          commit_local_success_shape commitCase
                        subst committed
                        have nextEq :
                            (((config.withLocalState event.sender localState).withRouteSelection
                                routeSelection).withPhase globalId
                                  (.queued config.epoch)).withQueue globalId
                                    (some (config.expectedMessage globalId event)) = next := by
                          exact Option.some.inj (by
                            simpa [statusCase, eventCase, phaseCase, queueCase, sameRole,
                              commitCase] using stepped)
                        subst next
                        refine ⟨event, ?_, sameRole, ?_, ?_⟩
                        · simpa [GlobalConfig.event?, GlobalConfig.withQueue,
                            GlobalConfig.withPhase, GlobalConfig.withLocalState,
                            GlobalConfig.withRouteSelection] using eventCase
                        · simp [GlobalConfig.withQueue, GlobalConfig.withPhase,
                            GlobalConfig.withLocalState, GlobalConfig.withRouteSelection]
                        · simp [GlobalConfig.queuedMessage?, GlobalConfig.expectedMessage,
                            GlobalConfig.withQueue, GlobalConfig.withPhase,
                            GlobalConfig.withLocalState, GlobalConfig.withRouteSelection,
                            statusCase]

theorem recv_step_consumes_unique_message
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.recvStep? globalId = some next) :
    ∃ event,
      config.event? globalId = some event /\
      config.queuedMessage? globalId = some (config.expectedMessage globalId event) /\
      next.phase globalId = .consumed next.epoch /\
      next.queuedMessage? globalId = none /\
      next.recvStep? globalId = none := by
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
              · subst epoch
                by_cases exactMessage :
                    config.queue globalId = some (config.expectedMessage globalId event)
                · cases commitCase : config.commitLocal? event.receiver globalId
                      (.recv event.sender event.label event.schema) with
                  | none =>
                      simp [statusCase, eventCase, phaseCase, exactMessage,
                        commitCase] at stepped
                  | some committed =>
                      obtain ⟨_, localState, routeSelection, _, _, _, committedEq⟩ :=
                        commit_local_success_shape commitCase
                      subst committed
                      have nextEq :
                          (((config.withLocalState event.receiver localState).withRouteSelection
                              routeSelection).withPhase globalId
                                (.consumed config.epoch)).withQueue globalId none = next := by
                        exact Option.some.inj (by
                          simpa [statusCase, eventCase, phaseCase, exactMessage,
                            commitCase] using stepped)
                      subst next
                      have eventLookup : config.choreo.globalEvents[globalId]? = some event := by
                        simpa [GlobalConfig.event?] using eventCase
                      refine ⟨event, rfl, ?_, ?_, ?_, ?_⟩
                      · simp [GlobalConfig.queuedMessage?, exactMessage, phaseCase, statusCase]
                      · simp [GlobalConfig.withQueue, GlobalConfig.withPhase,
                          GlobalConfig.withLocalState, GlobalConfig.withRouteSelection]
                      · simp [GlobalConfig.queuedMessage?, GlobalConfig.withQueue,
                          GlobalConfig.withPhase, GlobalConfig.withLocalState,
                          GlobalConfig.withRouteSelection]
                      · simp [GlobalConfig.recvStep?, GlobalConfig.event?,
                          GlobalConfig.withQueue, GlobalConfig.withPhase,
                          GlobalConfig.withLocalState, GlobalConfig.withRouteSelection,
                          eventLookup, statusCase]
                · simp [statusCase, eventCase, phaseCase, exactMessage] at stepped
              · simp [statusCase, eventCase, phaseCase, current] at stepped

theorem local_step_consumes_unit_event_without_queue
    {config next : GlobalConfig} {globalId : Nat}
    (stepped : config.localStep? globalId = some next) :
    ∃ event,
      next.event? globalId = some event /\
      event.sender = event.receiver /\
      event.schema = 0 /\
      next.phase globalId = .consumed next.epoch /\
      next.queuedMessage? globalId = none /\
      next.localStep? globalId = none := by
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
                        obtain ⟨_, localState, routeSelection, _, _, _, committedEq⟩ :=
                          commit_local_success_shape commitCase
                        subst committed
                        have nextEq :
                            (((config.withLocalState event.receiver localState).withRouteSelection
                                routeSelection).withPhase globalId
                                  (.consumed config.epoch)).withQueue globalId none = next := by
                          exact Option.some.inj (by
                            simpa [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                              queueEmpty, commitCase] using stepped)
                        subst next
                        have eventLookup : config.choreo.globalEvents[globalId]? = some event := by
                          simpa [GlobalConfig.event?] using eventCase
                        refine ⟨event, ?_, sameRole, unitSchema, ?_, ?_, ?_⟩
                        · simpa [GlobalConfig.event?, GlobalConfig.withQueue,
                            GlobalConfig.withPhase,
                            GlobalConfig.withLocalState,
                            GlobalConfig.withRouteSelection] using eventCase
                        · simp [GlobalConfig.withQueue, GlobalConfig.withPhase,
                            GlobalConfig.withLocalState,
                            GlobalConfig.withRouteSelection]
                        · simp [GlobalConfig.queuedMessage?, GlobalConfig.withQueue,
                            GlobalConfig.withPhase,
                            GlobalConfig.withLocalState, GlobalConfig.withRouteSelection]
                        · simp [GlobalConfig.localStep?, GlobalConfig.event?,
                            GlobalConfig.withQueue, GlobalConfig.withPhase,
                            GlobalConfig.withLocalState,
                            GlobalConfig.withRouteSelection, eventLookup, statusCase]
                  · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema,
                      queueEmpty] at stepped
                · simp [statusCase, eventCase, phaseCase, sameRole, unitSchema] at stepped
              · simp [statusCase, eventCase, phaseCase, sameRole] at stepped

end Hibana
