import Hibana.Cancellation
import Hibana.TransportContract

namespace Hibana

/-- Frames accepted by one exact transport channel but not yet offered to the
receiver's runtime monitor. The list order is the carrier FIFO order. -/
structure AcceptedChannelQueue where
  channel : TransportChannel
  frames : List TransportFrame
  peerClosed : Bool
  deriving Repr, DecidableEq

/-- Cancellation state at the transport/protocol boundary. `protocol.queue`
contains only frames already admitted to protocol execution. `accepted` remains
transport-owned and can only be drained into `quarantined` after cancellation. -/
structure AsyncCancellationConfig where
  protocol : GlobalConfig
  accepted : List AcceptedChannelQueue
  quarantined : List TransportFrame

def TransportState.pendingFrames (state : TransportState) : List TransportFrame :=
  state.frames.drop state.delivered

def AcceptedChannelQueue.fromTransport
    (state : TransportState) : AcceptedChannelQueue := {
  channel := state.channel
  frames := state.pendingFrames
  peerClosed := state.peerClosed
}

def AsyncCancellationConfig.fromTransportSnapshot
    (protocol : GlobalConfig)
    (transports : List TransportState) : AsyncCancellationConfig := {
  protocol
  accepted := transports.map AcceptedChannelQueue.fromTransport
  quarantined := []
}

def AsyncCancellationConfig.beginCancellation
    (config : AsyncCancellationConfig)
    (reporter : Nat)
    (cause : SessionFault) : AsyncCancellationConfig :=
  { config with protocol := config.protocol.beginCancellation reporter cause }

def AsyncCancellationConfig.pendingFrameCount
    (config : AsyncCancellationConfig) : Nat :=
  (config.accepted.map fun queue => queue.frames.length).sum

def AsyncCancellationConfig.openChannelCount
    (config : AsyncCancellationConfig) : Nat :=
  (config.accepted.map fun queue => if queue.peerClosed then 0 else 1).sum

def AsyncCancellationConfig.retirementMeasure
    (config : AsyncCancellationConfig) : Nat :=
  config.pendingFrameCount + config.openChannelCount +
    config.protocol.cancellationMeasure

def AsyncCancellationConfig.transportRetired
    (config : AsyncCancellationConfig) : Prop :=
  ∀ queue, queue ∈ config.accepted ->
    queue.frames = [] /\ queue.peerClosed = true

def AsyncCancellationConfig.roleTransportRetired
    (config : AsyncCancellationConfig) (role : Nat) : Prop :=
  ∀ queue, queue ∈ config.accepted -> queue.channel.receiver = role ->
    queue.frames = [] /\ queue.peerClosed = true

def AsyncCancellationConfig.fullyRetired
    (config : AsyncCancellationConfig) : Prop :=
  config.transportRetired /\ config.protocol.resourcesRetired

def AsyncCancellationConfig.coversAwaitingRoles
    (config : AsyncCancellationConfig) : Prop :=
  match config.protocol.status with
  | .cancelling _ awaiting =>
      ∀ role, role ∈ awaiting ->
        role ∈ config.accepted.map fun queue => queue.channel.receiver
  | .live | .retired _ => True

def AcceptedChannelQueue.check (queue : AcceptedChannelQueue) : Bool :=
  queue.frames.all fun frame => decide (frame.channel = queue.channel)

def AcceptedChannelQueue.WellFormed (queue : AcceptedChannelQueue) : Prop :=
  ∀ frame, frame ∈ queue.frames -> frame.channel = queue.channel

theorem accepted_channel_queue_checker_sound
    {queue : AcceptedChannelQueue}
    (accepted : queue.check = true) : queue.WellFormed := by
  simpa [AcceptedChannelQueue.check, AcceptedChannelQueue.WellFormed] using accepted

theorem accepted_channel_queue_checker_complete
    {queue : AcceptedChannelQueue}
    (wellFormed : queue.WellFormed) : queue.check = true := by
  simpa [AcceptedChannelQueue.check, AcceptedChannelQueue.WellFormed] using wellFormed

def AsyncCancellationConfig.checkAcceptedChannelAuthority
    (config : AsyncCancellationConfig) : Bool :=
  decide (config.accepted.map AcceptedChannelQueue.channel).Nodup &&
    config.accepted.all AcceptedChannelQueue.check

def AsyncCancellationConfig.AcceptedChannelAuthority
    (config : AsyncCancellationConfig) : Prop :=
  (config.accepted.map AcceptedChannelQueue.channel).Nodup /\
    config.accepted.all AcceptedChannelQueue.check = true

theorem accepted_channel_authority_checker_sound
    {config : AsyncCancellationConfig}
    (accepted : config.checkAcceptedChannelAuthority = true) :
    config.AcceptedChannelAuthority := by
  simpa [AsyncCancellationConfig.checkAcceptedChannelAuthority,
    AsyncCancellationConfig.AcceptedChannelAuthority] using accepted

theorem accepted_channel_authority_frames_match
    {config : AsyncCancellationConfig}
    (authority : config.AcceptedChannelAuthority)
    {queue : AcceptedChannelQueue}
    (queueMember : queue ∈ config.accepted) : queue.WellFormed := by
  exact accepted_channel_queue_checker_sound
    (List.all_eq_true.mp authority.2 queue queueMember)

theorem transport_snapshot_has_accepted_channel_authority
    {protocol : GlobalConfig}
    {transports : List TransportState}
    (channelsUnique : (transports.map TransportState.channel).Nodup)
    (wellFormed : ∀ state, state ∈ transports -> state.WellFormed) :
    (AsyncCancellationConfig.fromTransportSnapshot protocol transports)
      |>.AcceptedChannelAuthority := by
  constructor
  · simpa [AsyncCancellationConfig.fromTransportSnapshot,
      AcceptedChannelQueue.fromTransport] using channelsUnique
  · apply List.all_eq_true.mpr
    intro queue queueMember
    obtain ⟨state, stateMember, queueExact⟩ := List.mem_map.mp queueMember
    subst queue
    apply accepted_channel_queue_checker_complete
    intro frame frameMember
    have sourceMember : frame ∈ state.frames :=
      List.mem_of_mem_drop frameMember
    obtain ⟨index, bound, atIndex⟩ := List.mem_iff_getElem.mp sourceMember
    have present : state.frames[index]? = some frame :=
      List.getElem?_eq_some_iff.mpr ⟨bound, atIndex⟩
    exact (wellFormed state stateMember).2 index frame present |>.1

/-- The carrier retains one unique FIFO owner capable of signalling closure for
every role that still has to observe it, and the protocol cancellation state is
coherent. Individual channels may still require explicit `close` steps. -/
structure AsyncCancellationConfig.RetirementReady
    (config : AsyncCancellationConfig) : Prop where
  protocolWellFormed : config.protocol.CancellationWellFormed
  nonLive : config.protocol.status ≠ .live
  acceptedChannelAuthority : config.AcceptedChannelAuthority
  coversAwaitingRoles : config.coversAwaitingRoles

/-- The post-fault transition system has no protocol-delivery constructor.
Accepted frames are discarded in FIFO order, and a role may observe peer close
only after its accepted queue is empty. -/
inductive AsyncCancellationStep :
    AsyncCancellationConfig -> AsyncCancellationConfig -> Prop where
  | drain
      {current : AsyncCancellationConfig}
      {beforeQueues afterQueues : List AcceptedChannelQueue}
      {channel : TransportChannel}
      {frame : TransportFrame}
      {rest : List TransportFrame}
      {peerClosed : Bool}
      (nonLive : current.protocol.status ≠ .live)
      (split : current.accepted =
        beforeQueues ++ [{
          channel, frames := frame :: rest, peerClosed
        }] ++ afterQueues) :
      AsyncCancellationStep current {
        current with
        accepted := beforeQueues ++ [{
          channel, frames := rest, peerClosed
        }] ++ afterQueues
        quarantined := current.quarantined ++ [frame]
      }
  | close
      {current : AsyncCancellationConfig}
      {beforeQueues afterQueues : List AcceptedChannelQueue}
      {channel : TransportChannel}
      {frames : List TransportFrame}
      (nonLive : current.protocol.status ≠ .live)
      (split : current.accepted =
        beforeQueues ++ [{
          channel, frames, peerClosed := false
        }] ++ afterQueues) :
      AsyncCancellationStep current {
        current with
        accepted := beforeQueues ++ [{
          channel, frames, peerClosed := true
        }] ++ afterQueues
      }
  | observe
      {current : AsyncCancellationConfig}
      {nextProtocol : GlobalConfig}
      {role : Nat}
      (owned : role ∈ current.accepted.map fun queue => queue.channel.receiver)
      (retired : current.roleTransportRetired role)
      (observed : current.protocol.observeCancellation? role = some nextProtocol) :
      AsyncCancellationStep current { current with protocol := nextProtocol }

inductive AsyncCancellationReachable
    (start : AsyncCancellationConfig) : AsyncCancellationConfig -> Prop where
  | refl : AsyncCancellationReachable start start
  | step
      {current next : AsyncCancellationConfig}
      (path : AsyncCancellationReachable start current)
      (advanced : AsyncCancellationStep current next) :
      AsyncCancellationReachable start next

theorem AsyncCancellationReachable.trans
    {start middle finish : AsyncCancellationConfig}
    (path : AsyncCancellationReachable start middle)
    (suffix : AsyncCancellationReachable middle finish) :
    AsyncCancellationReachable start finish := by
  induction suffix with
  | refl => exact path
  | step suffixPrefix advanced inductionHypothesis =>
      exact .step inductionHypothesis advanced

theorem async_begin_cancellation_preserves_transport_buffers
    (config : AsyncCancellationConfig) (reporter : Nat) (cause : SessionFault) :
    (config.beginCancellation reporter cause).accepted = config.accepted /\
    (config.beginCancellation reporter cause).quarantined = config.quarantined := by
  exact ⟨rfl, rfl⟩

theorem transport_pending_head_poll_is_exact
    {state : TransportState}
    {frame : TransportFrame}
    {rest : List TransportFrame}
    (pending : state.pendingFrames = frame :: rest) :
    ∃ next,
      state.pollReceive = .delivered frame next /\
      next.pendingFrames = rest /\
      next.channel = state.channel /\
      next.peerClosed = state.peerClosed := by
  have present : state.frames[state.delivered]? = some frame := by
    have head := congrArg (fun frames => frames[0]?) pending
    simpa [TransportState.pendingFrames, List.getElem?_drop] using head
  let next := { state with delivered := state.delivered + 1 }
  have polled : state.pollReceive = .delivered frame next := by
    simp [TransportState.pollReceive, present, next]
  have pendingNext : next.pendingFrames = rest := by
    have tails := congrArg List.tail pending
    simpa [TransportState.pendingFrames, List.tail_drop, next] using tails
  exact ⟨next, polled, pendingNext, rfl, rfl⟩

theorem transport_close_and_receive_commute
    {state next : TransportState}
    {frame : TransportFrame}
    (received : state.pollReceive = .delivered frame next) :
    state.closePeer.pollReceive = .delivered frame next.closePeer := by
  have present := (transport_receive_is_fifo received).1
  have nextEq : next = { state with delivered := state.delivered + 1 } := by
    unfold TransportState.pollReceive at received
    rw [present] at received
    cases received
    rfl
  subst next
  simpa [TransportState.closePeer] using
    peer_close_does_not_skip_buffered_frame present

theorem transport_pending_head_drains_to_quarantine
    {config : AsyncCancellationConfig}
    {state : TransportState}
    {beforeQueues afterQueues : List AcceptedChannelQueue}
    {frame : TransportFrame}
    {rest : List TransportFrame}
    (nonLive : config.protocol.status ≠ .live)
    (pending : state.pendingFrames = frame :: rest)
    (split : config.accepted =
      beforeQueues ++ [AcceptedChannelQueue.fromTransport state] ++ afterQueues) :
    ∃ nextTransport next,
      state.pollReceive = .delivered frame nextTransport /\
      AsyncCancellationStep config next /\
      frame ∈ next.quarantined /\
      next.protocol = config.protocol := by
  obtain ⟨nextTransport, polled, pendingNext, channelEq, closed⟩ :=
    transport_pending_head_poll_is_exact pending
  let next : AsyncCancellationConfig := {
    config with
    accepted := beforeQueues ++ [AcceptedChannelQueue.fromTransport nextTransport] ++
      afterQueues
    quarantined := config.quarantined ++ [frame]
  }
  have exactCurrent : config.accepted = beforeQueues ++ [{
      channel := state.channel
      frames := frame :: rest
      peerClosed := state.peerClosed
    }] ++ afterQueues := by
    simpa [AcceptedChannelQueue.fromTransport, pending] using split
  have exactNext :
      beforeQueues ++ [AcceptedChannelQueue.fromTransport nextTransport] ++ afterQueues =
        beforeQueues ++ [{
          channel := state.channel
          frames := rest
          peerClosed := state.peerClosed
        }] ++ afterQueues := by
    simp [AcceptedChannelQueue.fromTransport, pendingNext, channelEq, closed]
  have advanced : AsyncCancellationStep config next := by
    have canonicalStep : AsyncCancellationStep config {
        config with
        accepted := beforeQueues ++ [{
          channel := state.channel
          frames := rest
          peerClosed := state.peerClosed
        }] ++ afterQueues
        quarantined := config.quarantined ++ [frame]
      } := .drain nonLive exactCurrent
    have nextEq : next = {
        config with
        accepted := beforeQueues ++ [{
          channel := state.channel
          frames := rest
          peerClosed := state.peerClosed
        }] ++ afterQueues
        quarantined := config.quarantined ++ [frame]
      } := by
      simp [next, exactNext]
    rw [nextEq]
    exact canonicalStep
  exact ⟨nextTransport, next, polled, advanced, by simp [next], rfl⟩

theorem transport_peer_close_has_async_close_step
    {config : AsyncCancellationConfig}
    {state : TransportState}
    {beforeQueues afterQueues : List AcceptedChannelQueue}
    (nonLive : config.protocol.status ≠ .live)
    (openChannel : state.peerClosed = false)
    (split : config.accepted =
      beforeQueues ++ [AcceptedChannelQueue.fromTransport state] ++ afterQueues) :
    ∃ next,
      AsyncCancellationStep config next /\
      next.accepted = beforeQueues ++ [
        AcceptedChannelQueue.fromTransport state.closePeer
      ] ++ afterQueues /\
      next.protocol = config.protocol := by
  let next : AsyncCancellationConfig := {
    config with
    accepted := beforeQueues ++ [AcceptedChannelQueue.fromTransport state.closePeer] ++
      afterQueues
  }
  have exactCurrent : config.accepted = beforeQueues ++ [{
      channel := state.channel
      frames := state.pendingFrames
      peerClosed := false
    }] ++ afterQueues := by
    simpa [AcceptedChannelQueue.fromTransport, openChannel] using split
  have exactNext :
      beforeQueues ++ [AcceptedChannelQueue.fromTransport state.closePeer] ++ afterQueues =
        beforeQueues ++ [{
          channel := state.channel
          frames := state.pendingFrames
          peerClosed := true
        }] ++ afterQueues := by
    simp [AcceptedChannelQueue.fromTransport, TransportState.closePeer,
      TransportState.pendingFrames]
  have advanced : AsyncCancellationStep config next := by
    have canonicalStep : AsyncCancellationStep config {
        config with
        accepted := beforeQueues ++ [{
          channel := state.channel
          frames := state.pendingFrames
          peerClosed := true
        }] ++ afterQueues
      } := .close nonLive exactCurrent
    have nextEq : next = {
        config with
        accepted := beforeQueues ++ [{
          channel := state.channel
          frames := state.pendingFrames
          peerClosed := true
        }] ++ afterQueues
      } := by
      simp [next, exactNext]
    rw [nextEq]
    exact canonicalStep
  exact ⟨next, advanced, rfl, rfl⟩

theorem async_close_and_drain_commute
    {config : AsyncCancellationConfig}
    {beforeQueues afterQueues : List AcceptedChannelQueue}
    {channel : TransportChannel}
    {frame : TransportFrame}
    {rest : List TransportFrame}
    (nonLive : config.protocol.status ≠ .live)
    (split : config.accepted = beforeQueues ++ [{
      channel
      frames := frame :: rest
      peerClosed := false
    }] ++ afterQueues) :
    ∃ final closedFirst drainedFirst,
      AsyncCancellationStep config closedFirst /\
      AsyncCancellationStep closedFirst final /\
      AsyncCancellationStep config drainedFirst /\
      AsyncCancellationStep drainedFirst final := by
  let closedFirst : AsyncCancellationConfig := {
    config with
    accepted := beforeQueues ++ [{
      channel, frames := frame :: rest, peerClosed := true
    }] ++ afterQueues
  }
  let drainedFirst : AsyncCancellationConfig := {
    config with
    accepted := beforeQueues ++ [{
      channel, frames := rest, peerClosed := false
    }] ++ afterQueues
    quarantined := config.quarantined ++ [frame]
  }
  let final : AsyncCancellationConfig := {
    config with
    accepted := beforeQueues ++ [{
      channel, frames := rest, peerClosed := true
    }] ++ afterQueues
    quarantined := config.quarantined ++ [frame]
  }
  have closeFirst : AsyncCancellationStep config closedFirst := by
    exact .close nonLive split
  have drainAfterClose : AsyncCancellationStep closedFirst final := by
    apply AsyncCancellationStep.drain
      (current := closedFirst)
      (beforeQueues := beforeQueues)
      (afterQueues := afterQueues)
      (channel := channel)
      (frame := frame)
      (rest := rest)
      (peerClosed := true)
    · simpa [closedFirst] using nonLive
    · rfl
  have drainFirst : AsyncCancellationStep config drainedFirst := by
    exact .drain nonLive split
  have closeAfterDrain : AsyncCancellationStep drainedFirst final := by
    apply AsyncCancellationStep.close
      (current := drainedFirst)
      (beforeQueues := beforeQueues)
      (afterQueues := afterQueues)
      (channel := channel)
      (frames := rest)
    · simpa [drainedFirst] using nonLive
    · rfl
  exact ⟨final, closedFirst, drainedFirst,
    closeFirst, drainAfterClose, drainFirst, closeAfterDrain⟩

theorem non_live_global_config_has_no_protocol_step
    {config : GlobalConfig}
    (nonLive : config.status ≠ .live)
    (operation : GlobalOperation) :
    config.step? operation = none := by
  cases statusCase : config.status with
  | live => exact False.elim (nonLive statusCase)
  | cancelling cause awaiting =>
      cases operation <;>
        simp [GlobalConfig.step?, GlobalConfig.sendStep?, GlobalConfig.recvStep?,
          GlobalConfig.localStep?, GlobalConfig.resolveStep?,
          GlobalConfig.rejectResolverStep?, GlobalConfig.rollStep?, statusCase]
  | retired cause =>
      cases operation <;>
        simp [GlobalConfig.step?, GlobalConfig.sendStep?, GlobalConfig.recvStep?,
          GlobalConfig.localStep?, GlobalConfig.resolveStep?,
          GlobalConfig.rejectResolverStep?, GlobalConfig.rollStep?, statusCase]

theorem async_begin_cancellation_disables_protocol_steps
    {config : AsyncCancellationConfig}
    {reporter : Nat}
    {cause : SessionFault}
    (live : config.protocol.status = .live)
    (operation : GlobalOperation) :
    (config.beginCancellation reporter cause).protocol.step? operation = none := by
  apply non_live_global_config_has_no_protocol_step
  cases pendingCase : removeAwaitingRole
      (List.range config.protocol.roleCount) reporter with
  | nil =>
      simp [AsyncCancellationConfig.beginCancellation,
        GlobalConfig.beginCancellation, live, pendingCase]
  | cons head tail =>
      simp [AsyncCancellationConfig.beginCancellation,
        GlobalConfig.beginCancellation, live, pendingCase]

theorem async_cancellation_step_protocol_effect
    {current next : AsyncCancellationConfig}
    (advanced : AsyncCancellationStep current next) :
    next.protocol = current.protocol \/
      ∃ role, current.protocol.observeCancellation? role = some next.protocol := by
  cases advanced with
  | drain nonLive split => exact Or.inl rfl
  | close nonLive split => exact Or.inl rfl
  | observe owned retired observed => exact Or.inr ⟨_, observed⟩

private theorem accepted_queues_retired_or_actionable
    (queues : List AcceptedChannelQueue) :
    (∀ queue, queue ∈ queues ->
      queue.frames = [] /\ queue.peerClosed = true) \/
      (∃ beforeQueues afterQueues channel frame rest,
        ∃ peerClosed,
          queues = beforeQueues ++ [{
            channel, frames := frame :: rest, peerClosed
          }] ++ afterQueues) \/
      (∃ beforeQueues afterQueues channel frames,
        queues = beforeQueues ++ [{
          channel, frames, peerClosed := false
        }] ++ afterQueues) := by
  induction queues with
  | nil =>
      exact Or.inl (by simp)
  | cons queue queues inductionHypothesis =>
      cases queue with
      | mk channel frames peerClosed =>
          cases closedCase : peerClosed with
          | false =>
              exact Or.inr <| Or.inr
                ⟨[], queues, channel, frames, by simp⟩
          | true =>
              cases frames with
              | cons frame rest =>
                  exact Or.inr <| Or.inl
                    ⟨[], queues, channel, frame, rest, true, by simp⟩
              | nil =>
                  rcases inductionHypothesis with retired | drainable | closeable
                  · left
                    intro candidate member
                    simp only [List.mem_cons] at member
                    rcases member with rfl | member
                    · simp
                    · exact retired candidate member
                  · right
                    left
                    obtain ⟨beforeQueues, afterQueues, targetChannel, frame, rest,
                      targetClosed, split⟩ := drainable
                    refine ⟨{
                      channel, frames := [], peerClosed := true
                    } :: beforeQueues, afterQueues, targetChannel, frame, rest,
                      targetClosed, ?_⟩
                    simp [split]
                  · right
                    right
                    obtain ⟨beforeQueues, afterQueues, targetChannel, targetFrames,
                      split⟩ := closeable
                    refine ⟨{
                      channel, frames := [], peerClosed := true
                    } :: beforeQueues, afterQueues, targetChannel, targetFrames, ?_⟩
                    simp [split]

theorem retirement_ready_has_logical_progress
    {config : AsyncCancellationConfig}
    (ready : config.RetirementReady) :
    config.fullyRetired \/
      ∃ next, AsyncCancellationStep config next := by
  rcases accepted_queues_retired_or_actionable config.accepted with
    transportRetired | drainable | closeable
  · cases statusCase : config.protocol.status with
    | live => exact False.elim (ready.nonLive statusCase)
    | retired cause =>
        have cancellationWellFormed :
            config.protocol.resources = .empty /\
              ∀ globalId, config.protocol.queue globalId = none := by
          simpa [GlobalConfig.CancellationWellFormed, statusCase] using
            ready.protocolWellFormed
        exact Or.inl ⟨transportRetired,
          retired_config_retires_resources statusCase
            cancellationWellFormed.2 cancellationWellFormed.1⟩
    | cancelling cause awaiting =>
        have cancellationWellFormed :
            awaiting ≠ [] /\
              config.protocol.resources = .forRoles awaiting /\
              ∀ globalId, config.protocol.queue globalId = none := by
          simpa [GlobalConfig.CancellationWellFormed, statusCase] using
            ready.protocolWellFormed
        cases awaiting with
        | nil => exact False.elim (cancellationWellFormed.1 rfl)
        | cons role remaining =>
            have covered :
                role ∈ config.accepted.map (fun queue => queue.channel.receiver) := by
              have coverage :
                  ∀ candidate, candidate ∈ role :: remaining ->
                    candidate ∈ config.accepted.map
                      (fun queue => queue.channel.receiver) := by
                simpa [AsyncCancellationConfig.coversAwaitingRoles, statusCase] using
                  ready.coversAwaitingRoles
              exact coverage role (by simp)
            have roleRetired : config.roleTransportRetired role := by
              intro queue queueMember queueRole
              exact transportRetired queue queueMember
            cases remainingCase : removeAwaitingRole (role :: remaining) role with
            | nil =>
                let nextProtocol : GlobalConfig := {
                  config.protocol with
                  status := .retired cause
                  resources := .empty
                }
                have observed :
                    config.protocol.observeCancellation? role = some nextProtocol := by
                  simp [GlobalConfig.observeCancellation?, statusCase,
                    remainingCase, nextProtocol]
                exact Or.inr ⟨{ config with protocol := nextProtocol },
                  .observe covered roleRetired observed⟩
            | cons nextRole nextRemaining =>
                let nextProtocol : GlobalConfig := {
                  config.protocol with
                  status := .cancelling cause (nextRole :: nextRemaining)
                  resources := .forRoles (nextRole :: nextRemaining)
                }
                have observed :
                    config.protocol.observeCancellation? role = some nextProtocol := by
                  simp [GlobalConfig.observeCancellation?, statusCase,
                    remainingCase, nextProtocol]
                exact Or.inr ⟨{ config with protocol := nextProtocol },
                  .observe covered roleRetired observed⟩
  · obtain ⟨beforeQueues, afterQueues, channel, frame, rest, peerClosed, split⟩ :=
      drainable
    exact Or.inr ⟨{
      config with
      accepted := beforeQueues ++ [{
        channel, frames := rest, peerClosed
      }] ++ afterQueues
      quarantined := config.quarantined ++ [frame]
    }, .drain ready.nonLive split⟩
  · obtain ⟨beforeQueues, afterQueues, channel, frames, split⟩ := closeable
    exact Or.inr ⟨{
      config with
      accepted := beforeQueues ++ [{
        channel, frames, peerClosed := true
      }] ++ afterQueues
    }, .close ready.nonLive split⟩

theorem async_cancellation_step_preserves_retirement_ready
    {current next : AsyncCancellationConfig}
    (ready : current.RetirementReady)
    (advanced : AsyncCancellationStep current next) :
    next.RetirementReady := by
  cases advanced with
  | @drain beforeQueues afterQueues channel frame rest peerClosed nonLive split =>
      refine ⟨ready.protocolWellFormed, ready.nonLive, ?_, ?_⟩
      · have authority := ready.acceptedChannelAuthority
        simp [AsyncCancellationConfig.AcceptedChannelAuthority,
          AcceptedChannelQueue.check, split] at authority ⊢
        exact ⟨authority.1, authority.2.1,
          authority.2.2.1.2, authority.2.2.2⟩
      cases statusCase : current.protocol.status with
      | live => exact False.elim (ready.nonLive statusCase)
      | retired cause =>
          simp [AsyncCancellationConfig.coversAwaitingRoles, statusCase]
      | cancelling cause awaiting =>
          have coverage :
              ∀ candidate, candidate ∈ awaiting ->
                candidate ∈ current.accepted.map
                  (fun queue => queue.channel.receiver) := by
            simpa [AsyncCancellationConfig.coversAwaitingRoles, statusCase] using
              ready.coversAwaitingRoles
          simp only [AsyncCancellationConfig.coversAwaitingRoles, statusCase]
          intro candidate candidateMember
          have covered := coverage candidate candidateMember
          simpa [split] using covered
  | @close beforeQueues afterQueues channel frames nonLive split =>
      refine ⟨ready.protocolWellFormed, ready.nonLive, ?_, ?_⟩
      · simpa [AsyncCancellationConfig.AcceptedChannelAuthority,
          AcceptedChannelQueue.check, split] using ready.acceptedChannelAuthority
      cases statusCase : current.protocol.status with
      | live => exact False.elim (ready.nonLive statusCase)
      | retired cause =>
          simp [AsyncCancellationConfig.coversAwaitingRoles, statusCase]
      | cancelling cause awaiting =>
          have coverage :
              ∀ candidate, candidate ∈ awaiting ->
                candidate ∈ current.accepted.map
                  (fun queue => queue.channel.receiver) := by
            simpa [AsyncCancellationConfig.coversAwaitingRoles, statusCase] using
              ready.coversAwaitingRoles
          simp only [AsyncCancellationConfig.coversAwaitingRoles, statusCase]
          intro candidate candidateMember
          have covered := coverage candidate candidateMember
          simpa [split] using covered
  | @observe nextProtocol role owned drained observed =>
      refine ⟨
        cancellation_observation_preserves_well_formed
          ready.protocolWellFormed observed,
        cancellation_observation_cannot_restore_live observed,
        ready.acceptedChannelAuthority,
        ?_⟩
      unfold GlobalConfig.observeCancellation? at observed
      cases statusCase : current.protocol.status with
      | live => simp [statusCase] at observed
      | retired cause => simp [statusCase] at observed
      | cancelling cause awaiting =>
          have coverage :
              ∀ candidate, candidate ∈ awaiting ->
                candidate ∈ current.accepted.map
                  (fun queue => queue.channel.receiver) := by
            simpa [AsyncCancellationConfig.coversAwaitingRoles, statusCase] using
              ready.coversAwaitingRoles
          by_cases member : role ∈ awaiting
          · cases remainingCase : removeAwaitingRole awaiting role with
            | nil =>
                have nextEq :
                    { current.protocol with
                      status := .retired cause
                      resources := .empty } = nextProtocol := by
                  exact Option.some.inj (by
                    simpa [statusCase, member, remainingCase] using observed)
                subst nextProtocol
                simp [AsyncCancellationConfig.coversAwaitingRoles]
            | cons nextRole nextRemaining =>
                have nextEq :
                    { current.protocol with
                      status := .cancelling cause (nextRole :: nextRemaining)
                      resources := .forRoles (nextRole :: nextRemaining) } =
                      nextProtocol := by
                  exact Option.some.inj (by
                    simpa [statusCase, member, remainingCase] using observed)
                subst nextProtocol
                intro candidate candidateMember
                apply coverage candidate
                have filtered :
                    candidate ∈ removeAwaitingRole awaiting role := by
                  simpa [remainingCase] using candidateMember
                have filteredShape : candidate ∈ awaiting /\ candidate ≠ role := by
                  simpa [removeAwaitingRole] using filtered
                exact filteredShape.1
          · simp [statusCase, member] at observed

theorem async_cancellation_step_strictly_decreases
    {current next : AsyncCancellationConfig}
    (advanced : AsyncCancellationStep current next) :
    next.retirementMeasure < current.retirementMeasure := by
  cases advanced with
  | drain nonLive split =>
      simp [AsyncCancellationConfig.retirementMeasure,
        AsyncCancellationConfig.pendingFrameCount,
        AsyncCancellationConfig.openChannelCount, split]
  | close nonLive split =>
      simp [AsyncCancellationConfig.retirementMeasure,
        AsyncCancellationConfig.pendingFrameCount,
        AsyncCancellationConfig.openChannelCount, split]
  | @observe nextProtocol role owned drained observed =>
      have decreased := cancellation_observation_strictly_decreases observed
      change current.pendingFrameCount + current.openChannelCount +
          nextProtocol.cancellationMeasure <
        current.pendingFrameCount + current.openChannelCount +
          current.protocol.cancellationMeasure
      exact Nat.add_lt_add_left decreased _

end Hibana
