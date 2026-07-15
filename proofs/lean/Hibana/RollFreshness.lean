import Hibana.GlobalProgress
import Hibana.StaticProjectability

namespace Hibana

def TransportSnapshotDrained (transports : List TransportState) : Prop :=
  ∀ transport, transport ∈ transports ->
    transport.delivered = transport.frames.length

/-- A roll boundary is fresh without a wire epoch field: protocol-owned queue
slots are empty, and the carrier's next accepted frame follows the fully drained
affine sequence prefix. -/
theorem roll_step_with_drained_transport_has_fresh_next_frame
    {config next : GlobalConfig}
    {rollId globalId : Nat}
    {transports : List TransportState}
    {transport sent : TransportState}
    {frameLabel : Fin 256}
    (rolled : config.rollStep? rollId = some next)
    (owned : ∃ roll, config.choreo.globalRolls[rollId]? = some roll /\
      globalId ∈ roll.events)
    (transportMember : transport ∈ transports)
    (snapshotDrained : TransportSnapshotDrained transports)
    (wellFormed : transport.WellFormed)
    (accepted : transport.send frameLabel = some sent) :
    next.phase globalId = .ready /\
    next.queue globalId = none /\
    ∃ frame after,
      sent.pollReceive = .delivered frame after /\
      frame.channel = transport.channel /\
      frame.sequence = transport.frames.length /\
      frame.frameLabel = frameLabel /\
      (∀ old, old ∈ transport.frames -> old.sequence < frame.sequence) := by
  have cleared := roll_step_clears_owned_queue rolled owned
  have drained := snapshotDrained transport transportMember
  obtain ⟨frame, after, polled, channel, sequence, label, fresh⟩ :=
    transport_send_after_drain_is_fresh wellFormed drained accepted
  exact ⟨cleared.1, cleared.2, frame, after, polled,
    channel, sequence, label, fresh⟩

/-- End-to-end roll freshness over the real observable header. The transport
proves sequence freshness; descriptor admission then binds that raw frame to
one canonical event without assuming a wire epoch or global event ID. -/
theorem roll_step_with_admitted_transport_frame_is_fresh_occurrence
    {config next : GlobalConfig}
    {rollId globalId : Nat}
    {event : GlobalEvent}
    {transports : List TransportState}
    {transport sent after : TransportState}
    {frame : TransportFrame}
    {frameLabel : Fin 256}
    (rolled : config.rollStep? rollId = some next)
    (owned : ∃ roll, config.choreo.globalRolls[rollId]? = some roll /\
      globalId ∈ roll.events)
    (transportMember : transport ∈ transports)
    (snapshotDrained : TransportSnapshotDrained transports)
    (wellFormed : transport.WellFormed)
    (accepted : transport.send frameLabel = some sent)
    (polled : sent.pollReceive = .delivered frame after)
    (admitted : next.admitTransportFrame? frame = some (globalId, event)) :
    next.phase globalId = .ready /\
    next.queue globalId = none /\
    frame.channel = transport.channel /\
    frame.sequence = transport.frames.length /\
    frame.frameLabel = frameLabel /\
    frame.channel.session = next.session /\
    TransportAdmission next.choreo frame globalId event /\
    frame.channel.sender = event.sender /\
    frame.channel.receiver = event.receiver /\
    frame.channel.lane = event.lane := by
  obtain ⟨ready, empty, acceptedFrame, acceptedAfter, acceptedPoll,
      channel, sequence, label, _⟩ :=
    roll_step_with_drained_transport_has_fresh_next_frame rolled owned
      transportMember snapshotDrained wellFormed accepted
  have sameDelivery :
      TransportPoll.delivered acceptedFrame acceptedAfter =
        .delivered frame after := acceptedPoll.symm.trans polled
  cases sameDelivery
  have admissionSound := global_transport_admission_checker_sound admitted
  obtain ⟨_, _, _, sender, receiver, lane, _⟩ :=
    transport_admission_binds_exact_descriptor_occurrence admissionSound.2.1
  exact ⟨ready, empty, channel, sequence, label,
    admissionSound.1, admissionSound.2.1, sender, receiver, lane⟩

/-- Session-number reuse is safe only when the transport allocates a distinct
carrier-instance generation. Retired-generation frames cannot be the new iteration's
accepted frame even if every observable core-header field is identical. -/
theorem reused_session_on_new_carrier_generation_excludes_old_frame
    {oldFrame newFrame : TransportFrame}
    (newGeneration :
      oldFrame.channel.generation ≠ newFrame.channel.generation) :
    oldFrame ≠ newFrame :=
  distinct_carrier_generations_cannot_alias newGeneration

end Hibana
