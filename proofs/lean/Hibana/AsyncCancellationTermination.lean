import Hibana.AsyncCancellation

namespace Hibana

private inductive TransportRetirementAction where
  | drain
      (beforeQueues afterQueues : List AcceptedChannelQueue)
      (channel : TransportChannel)
      (frame : TransportFrame)
      (rest : List TransportFrame)
  | close
      (beforeQueues afterQueues : List AcceptedChannelQueue)
      (channel : TransportChannel)
      (frames : List TransportFrame)

private def TransportRetirementAction.prepend
    (queue : AcceptedChannelQueue) :
    TransportRetirementAction -> TransportRetirementAction
  | .drain beforeQueues afterQueues channel frame rest =>
      .drain (queue :: beforeQueues) afterQueues channel frame rest
  | .close beforeQueues afterQueues channel frames =>
      .close (queue :: beforeQueues) afterQueues channel frames

private def TransportRetirementAction.currentQueues :
    TransportRetirementAction -> List AcceptedChannelQueue
  | .drain beforeQueues afterQueues channel frame rest =>
      beforeQueues ++ [{
        channel, frames := frame :: rest, peerClosed := true
      }] ++ afterQueues
  | .close beforeQueues afterQueues channel frames =>
      beforeQueues ++ [{ channel, frames, peerClosed := false }] ++ afterQueues

private def TransportRetirementAction.apply
    (action : TransportRetirementAction)
    (config : AsyncCancellationConfig) : AsyncCancellationConfig :=
  match action with
  | .drain beforeQueues afterQueues channel frame rest => {
      config with
      accepted := beforeQueues ++ [{
        channel, frames := rest, peerClosed := true
      }] ++ afterQueues
      quarantined := config.quarantined ++ [frame]
    }
  | .close beforeQueues afterQueues channel frames => {
      config with
      accepted := beforeQueues ++ [{
        channel, frames, peerClosed := true
      }] ++ afterQueues
    }

private def firstTransportRetirementAction :
    List AcceptedChannelQueue -> Option TransportRetirementAction
  | [] => none
  | { channel, frames, peerClosed } :: rest =>
      if peerClosed then
        match frames with
        | [] => (firstTransportRetirementAction rest).map
            (TransportRetirementAction.prepend {
              channel, frames := [], peerClosed := true
            })
        | frame :: tail => some (.drain [] rest channel frame tail)
      else
        some (.close [] rest channel frames)

private theorem first_transport_action_has_exact_source
    {queues : List AcceptedChannelQueue}
    {action : TransportRetirementAction}
    (found : firstTransportRetirementAction queues = some action) :
    action.currentQueues = queues := by
  induction queues generalizing action with
  | nil => simp [firstTransportRetirementAction] at found
  | cons queue queues inductionHypothesis =>
      cases queue with
      | mk channel frames peerClosed =>
          cases closedCase : peerClosed with
          | false =>
              have actionEq :
                  TransportRetirementAction.close [] queues channel frames = action :=
                Option.some.inj (by
                  simpa [firstTransportRetirementAction, closedCase] using found)
              subst action
              simp [TransportRetirementAction.currentQueues]
          | true =>
              cases frames with
              | cons frame tail =>
                  have actionEq :
                      TransportRetirementAction.drain [] queues channel frame tail = action :=
                    Option.some.inj (by
                      simpa [firstTransportRetirementAction, closedCase] using found)
                  subst action
                  simp [TransportRetirementAction.currentQueues]
              | nil =>
                  cases tailActionCase : firstTransportRetirementAction queues with
                  | none =>
                      simp [firstTransportRetirementAction, closedCase,
                        tailActionCase] at found
                  | some tailAction =>
                      have actionEq :
                          tailAction.prepend {
                            channel, frames := [], peerClosed := true
                          } = action :=
                        Option.some.inj (by
                          simpa [firstTransportRetirementAction, closedCase,
                            tailActionCase] using found)
                      subst action
                      have tailExact := inductionHypothesis tailActionCase
                      have prefixed := congrArg
                        (fun rest => {
                          channel, frames := [], peerClosed := true
                        } :: rest)
                        tailExact
                      cases tailAction <;>
                        simpa [TransportRetirementAction.prepend,
                          TransportRetirementAction.currentQueues] using prefixed

private theorem no_transport_action_means_retired
    {queues : List AcceptedChannelQueue}
    (missing : firstTransportRetirementAction queues = none) :
    ∀ queue, queue ∈ queues ->
      queue.frames = [] /\ queue.peerClosed = true := by
  induction queues with
  | nil => simp
  | cons queue queues inductionHypothesis =>
      cases queue with
      | mk channel frames peerClosed =>
          cases closedCase : peerClosed with
          | false =>
              simp [firstTransportRetirementAction, closedCase] at missing
          | true =>
              cases frames with
              | cons frame tail =>
                  simp [firstTransportRetirementAction, closedCase] at missing
              | nil =>
                  have tailMissing : firstTransportRetirementAction queues = none := by
                    simpa [firstTransportRetirementAction, closedCase] using missing
                  intro candidate member
                  simp only [List.mem_cons] at member
                  rcases member with rfl | member
                  · simp
                  · exact inductionHypothesis tailMissing candidate member

private theorem transport_retirement_action_is_step
    {config : AsyncCancellationConfig}
    {action : TransportRetirementAction}
    (nonLive : config.protocol.status ≠ .live)
    (found : firstTransportRetirementAction config.accepted = some action) :
    AsyncCancellationStep config (action.apply config) := by
  have source := first_transport_action_has_exact_source found
  cases action with
  | drain beforeQueues afterQueues channel frame rest =>
      exact .drain nonLive source.symm
  | close beforeQueues afterQueues channel frames =>
      exact .close nonLive source.symm

private def AsyncCancellationConfig.retirementStep?
    (config : AsyncCancellationConfig) : Option AsyncCancellationConfig :=
  match firstTransportRetirementAction config.accepted with
  | some action => some (action.apply config)
  | none =>
      match config.protocol.status with
      | .cancelling _ (role :: _) =>
          if role ∈ config.accepted.map (fun queue => queue.channel.receiver) then
            match config.protocol.observeCancellation? role with
            | some nextProtocol => some { config with protocol := nextProtocol }
            | none => none
          else
            none
      | .live | .cancelling _ [] | .retired _ => none

private theorem retirement_step_is_sound
    {config next : AsyncCancellationConfig}
    (ready : config.RetirementReady)
    (stepped : config.retirementStep? = some next) :
    AsyncCancellationStep config next := by
  unfold AsyncCancellationConfig.retirementStep? at stepped
  cases actionCase : firstTransportRetirementAction config.accepted with
  | some action =>
      have nextEq : action.apply config = next :=
        Option.some.inj (by simpa [actionCase] using stepped)
      subst next
      exact transport_retirement_action_is_step ready.nonLive actionCase
  | none =>
      cases statusCase : config.protocol.status with
      | live => simp [actionCase, statusCase] at stepped
      | retired cause => simp [actionCase, statusCase] at stepped
      | cancelling cause awaiting =>
          cases awaiting with
          | nil => simp [actionCase, statusCase] at stepped
          | cons role remaining =>
              have coverage :
                  ∀ candidate, candidate ∈ role :: remaining ->
                    candidate ∈ config.accepted.map
                      (fun queue => queue.channel.receiver) := by
                simpa [AsyncCancellationConfig.coversAwaitingRoles, statusCase] using
                  ready.coversAwaitingRoles
              have owned :
                  role ∈ config.accepted.map
                    (fun queue => queue.channel.receiver) :=
                coverage role (by simp)
              cases observationCase : config.protocol.observeCancellation? role with
              | none =>
                  simp [actionCase, statusCase, owned, observationCase] at stepped
              | some nextProtocol =>
                  have nextEq :
                      { config with protocol := nextProtocol } = next :=
                    Option.some.inj (by
                      simpa [actionCase, statusCase, owned, observationCase] using stepped)
                  subst next
                  have transportRetired := no_transport_action_means_retired actionCase
                  have roleRetired : config.roleTransportRetired role := by
                    intro queue member queueRole
                    exact transportRetired queue member
                  exact .observe owned roleRetired observationCase

private theorem retirement_step_none_is_fully_retired
    {config : AsyncCancellationConfig}
    (ready : config.RetirementReady)
    (stopped : config.retirementStep? = none) :
    config.fullyRetired := by
  unfold AsyncCancellationConfig.retirementStep? at stopped
  cases actionCase : firstTransportRetirementAction config.accepted with
  | some action => simp [actionCase] at stopped
  | none =>
      have transportRetired := no_transport_action_means_retired actionCase
      cases statusCase : config.protocol.status with
      | live => exact False.elim (ready.nonLive statusCase)
      | retired cause =>
          have cancellationWellFormed :
              config.protocol.resources = .empty /\
                ∀ globalId, config.protocol.queue globalId = none := by
            simpa [GlobalConfig.CancellationWellFormed, statusCase] using
              ready.protocolWellFormed
          exact ⟨transportRetired,
            retired_config_retires_resources statusCase
              cancellationWellFormed.2 cancellationWellFormed.1⟩
      | cancelling cause awaiting =>
          have cancellationWellFormed : awaiting ≠ [] := by
            have full :
                awaiting ≠ [] /\
                  config.protocol.resources = .forRoles awaiting /\
                  ∀ globalId, config.protocol.queue globalId = none := by
              simpa [GlobalConfig.CancellationWellFormed, statusCase] using
                ready.protocolWellFormed
            exact full.1
          cases awaiting with
          | nil => exact False.elim (cancellationWellFormed rfl)
          | cons role remaining =>
              have coverage :
                  ∀ candidate, candidate ∈ role :: remaining ->
                    candidate ∈ config.accepted.map
                      (fun queue => queue.channel.receiver) := by
                simpa [AsyncCancellationConfig.coversAwaitingRoles, statusCase] using
                  ready.coversAwaitingRoles
              have owned :
                  role ∈ config.accepted.map
                    (fun queue => queue.channel.receiver) :=
                coverage role (by simp)
              cases remainingCase : removeAwaitingRole (role :: remaining) role with
              | nil =>
                  simp [actionCase, statusCase, owned,
                    GlobalConfig.observeCancellation?, remainingCase] at stopped
              | cons nextRole nextRemaining =>
                  simp [actionCase, statusCase, owned,
                    GlobalConfig.observeCancellation?, remainingCase] at stopped

private def runRetirement :
    Nat -> AsyncCancellationConfig -> AsyncCancellationConfig
  | 0, config => config
  | fuel + 1, config =>
      match config.retirementStep? with
      | none => config
      | some next => runRetirement fuel next

private theorem run_retirement_reaches_fully_retired_with_fuel
    (fuel : Nat)
    {config : AsyncCancellationConfig}
    (ready : config.RetirementReady)
    (bounded : config.retirementMeasure ≤ fuel) :
    AsyncCancellationReachable config (runRetirement fuel config) /\
    (runRetirement fuel config).fullyRetired := by
  induction fuel generalizing config with
  | zero =>
      cases stepCase : config.retirementStep? with
      | none =>
          exact ⟨.refl, retirement_step_none_is_fully_retired ready stepCase⟩
      | some next =>
          have advanced := retirement_step_is_sound ready stepCase
          have decreased := async_cancellation_step_strictly_decreases advanced
          have measureZero : config.retirementMeasure = 0 :=
            Nat.eq_zero_of_le_zero bounded
          rw [measureZero] at decreased
          exact False.elim (Nat.not_lt_zero _ decreased)
  | succ fuel inductionHypothesis =>
      cases stepCase : config.retirementStep? with
      | none =>
          simpa [runRetirement, stepCase] using
            And.intro (AsyncCancellationReachable.refl (start := config))
              (retirement_step_none_is_fully_retired ready stepCase)
      | some next =>
          have advanced := retirement_step_is_sound ready stepCase
          have decreased := async_cancellation_step_strictly_decreases advanced
          have nextBound : next.retirementMeasure ≤ fuel := by
            exact Nat.le_of_lt_succ (Nat.lt_of_lt_of_le decreased bounded)
          have nextReady :=
            async_cancellation_step_preserves_retirement_ready ready advanced
          have result := inductionHypothesis nextReady nextBound
          have reachable := AsyncCancellationReachable.trans
            (.step .refl advanced) result.1
          simpa [runRetirement, stepCase] using
            And.intro reachable result.2

theorem retirement_ready_reaches_full_retirement
    {config : AsyncCancellationConfig}
    (ready : config.RetirementReady) :
    ∃ retired,
      AsyncCancellationReachable config retired /\
      retired.fullyRetired := by
  let retired := runRetirement config.retirementMeasure config
  have result := run_retirement_reaches_fully_retired_with_fuel
    config.retirementMeasure ready (Nat.le_refl _)
  exact ⟨retired, result.1, result.2⟩

theorem async_cancellation_step_cannot_enable_protocol
    {current next : AsyncCancellationConfig}
    (nonLive : current.protocol.status ≠ .live)
    (advanced : AsyncCancellationStep current next)
    (operation : GlobalOperation) :
    next.protocol.step? operation = none := by
  apply non_live_global_config_has_no_protocol_step
  cases advanced with
  | drain drainNonLive split => exact nonLive
  | close closeNonLive split => exact nonLive
  | observe owned drained observed =>
      exact cancellation_observation_cannot_restore_live observed

theorem async_cancellation_step_is_irreflexive
    {config : AsyncCancellationConfig} :
    ¬AsyncCancellationStep config config := by
  intro advanced
  have decreased := async_cancellation_step_strictly_decreases advanced
  exact Nat.lt_irrefl _ decreased

theorem async_cancellation_reachable_measure_never_increases
    {start finish : AsyncCancellationConfig}
    (reachable : AsyncCancellationReachable start finish) :
    finish.retirementMeasure ≤ start.retirementMeasure := by
  induction reachable with
  | refl => exact Nat.le_refl _
  | step path advanced inductionHypothesis =>
      exact Nat.le_trans
        (Nat.le_of_lt (async_cancellation_step_strictly_decreases advanced))
        inductionHypothesis

theorem nontrivial_async_cancellation_reach_strictly_decreases
    {start finish : AsyncCancellationConfig}
    (reachable : AsyncCancellationReachable start finish)
    (different : start ≠ finish) :
    finish.retirementMeasure < start.retirementMeasure := by
  cases reachable with
  | refl => exact False.elim (different rfl)
  | step path advanced =>
      have finalDecrease := async_cancellation_step_strictly_decreases advanced
      have prefixBound := async_cancellation_reachable_measure_never_increases path
      exact Nat.lt_of_lt_of_le finalDecrease prefixBound

end Hibana
