import Hibana.GlobalSemantics

namespace Hibana

structure TransportChannel where
  session : Nat
  /-- Carrier-instance generation. It is not a core wire field; migration and
  connection-identifier rotation preserve this value. -/
  generation : Nat
  lane : Nat
  sender : Nat
  receiver : Nat
  deriving Repr, DecidableEq

structure TransportFrame where
  channel : TransportChannel
  sequence : Nat
  frameLabel : Fin 256
  deriving Repr, DecidableEq

/-- Strong affine-delivery profile for one session/lane/peer direction. The
runtime monitor does not assume this profile merely to reject a mismatched
observation; global fidelity and progress use it as a separate premise.
`channel` is the carrier-authenticated peer binding, and `delivered` is an
affine cursor into append-only send history. -/
structure TransportState where
  channel : TransportChannel
  frames : List TransportFrame
  delivered : Nat
  peerClosed : Bool
  deriving Repr, DecidableEq

def TransportState.initial (channel : TransportChannel) : TransportState := {
  channel
  frames := []
  delivered := 0
  peerClosed := false
}

def TransportState.send
    (state : TransportState)
    (frameLabel : Fin 256) : Option TransportState :=
  if state.peerClosed then
    none
  else
    let frame : TransportFrame := {
      channel := state.channel
      sequence := state.frames.length
      frameLabel
    }
    some { state with frames := state.frames ++ [frame] }

def TransportState.closePeer (state : TransportState) : TransportState :=
  { state with peerClosed := true }

/-- Abrupt carrier failure retires the whole direction immediately. A substream
reset or connection failure may take this path; no prior or pending frame can be
re-exposed as protocol traffic afterward. -/
def TransportState.abortPeer (state : TransportState) : TransportState := {
  state with
  frames := []
  delivered := 0
  peerClosed := true
}

inductive TransportPoll where
  | pending
  | peerClosed
  | delivered (frame : TransportFrame) (next : TransportState)
  deriving Repr, DecidableEq

def TransportState.pollReceive (state : TransportState) : TransportPoll :=
  match state.frames[state.delivered]? with
  | some frame => .delivered frame { state with delivered := state.delivered + 1 }
  | none => if state.peerClosed then .peerClosed else .pending

def TransportState.WellFormed (state : TransportState) : Prop :=
  state.delivered ≤ state.frames.length ∧
  ∀ (index : Nat) (frame : TransportFrame),
    state.frames[index]? = some frame →
    frame.channel = state.channel ∧ frame.sequence = index

theorem initial_transport_is_well_formed (channel : TransportChannel) :
    (TransportState.initial channel).WellFormed := by
  constructor
  · simp [TransportState.initial]
  · intro index frame present
    simp [TransportState.initial] at present

private theorem get_append_singleton
    {values : List α} {index : Nat} {value : α}
    (present : (values ++ [value])[index]? = some value) :
    index = values.length ∨ values[index]? = some value := by
  by_cases before : index < values.length
  · right
    simpa [List.getElem?_append, before] using present
  · left
    have bound : index < values.length + 1 := by
      have := List.getElem?_eq_some_iff.mp present
      simpa using this.1
    omega

theorem transport_send_preserves_well_formed
    {state next : TransportState} {frameLabel : Fin 256}
    (wellFormed : state.WellFormed)
    (sent : state.send frameLabel = some next) :
    next.WellFormed := by
  unfold TransportState.send at sent
  by_cases closed : state.peerClosed
  · simp [closed] at sent
  · have nextEq :
        { state with
          frames := state.frames ++ [{
            channel := state.channel
            sequence := state.frames.length
            frameLabel
          }] } = next := by
      exact Option.some.inj (by simpa [closed] using sent)
    subst next
    constructor
    · have deliveredBound := wellFormed.1
      simp only [List.length_append, List.length_singleton]
      omega
    · intro index frame present
      by_cases before : index < state.frames.length
      · have oldPresent : state.frames[index]? = some frame := by
          simpa [List.getElem?_append, before] using present
        exact wellFormed.2 index frame oldPresent
      · have indexEq : index = state.frames.length := by
          have bound := (List.getElem?_eq_some_iff.mp present).1
          simp at bound
          omega
        subst index
        simp at present
        subst frame
        exact ⟨rfl, rfl⟩

theorem transport_receive_is_fifo
    {state next : TransportState} {frame : TransportFrame}
    (received : state.pollReceive = .delivered frame next) :
    state.frames[state.delivered]? = some frame ∧
    next.channel = state.channel ∧
    next.frames = state.frames ∧
    next.delivered = state.delivered + 1 := by
  unfold TransportState.pollReceive at received
  cases frameCase : state.frames[state.delivered]? with
  | none =>
      cases closed : state.peerClosed <;> simp [frameCase, closed] at received
  | some candidate =>
      have exact :
          TransportPoll.delivered candidate
              { state with delivered := state.delivered + 1 } =
            .delivered frame next := by
        simpa [frameCase] using received
      cases exact
      exact ⟨rfl, rfl, rfl, rfl⟩

theorem transport_receive_advances_affine_cursor
    {state next : TransportState} {frame : TransportFrame}
    (received : state.pollReceive = .delivered frame next) :
    state.delivered < next.delivered := by
  obtain ⟨_, _, _, advanced⟩ := transport_receive_is_fifo received
  omega

theorem transport_receive_preserves_well_formed
    {state next : TransportState} {frame : TransportFrame}
    (wellFormed : state.WellFormed)
    (received : state.pollReceive = .delivered frame next) :
    next.WellFormed := by
  obtain ⟨present, channelEq, framesEq, advanced⟩ :=
    transport_receive_is_fifo received
  constructor
  · have bound := (List.getElem?_eq_some_iff.mp present).1
    rw [framesEq, advanced]
    omega
  · intro index candidate candidateAt
    rw [channelEq]
    apply wellFormed.2 index candidate
    simpa [framesEq] using candidateAt

theorem well_formed_transport_has_no_replay
    {state next : TransportState} {frame : TransportFrame}
    (wellFormed : state.WellFormed)
    (received : state.pollReceive = .delivered frame next) :
    ∀ futureFrame,
      next.frames[next.delivered]? = some futureFrame →
      futureFrame.sequence ≠ frame.sequence := by
  intro futureFrame future
  obtain ⟨current, _, framesEq, advanced⟩ := transport_receive_is_fifo received
  have frameSequence : frame.sequence = state.delivered :=
    (wellFormed.2 state.delivered frame current).2
  have futureInState : state.frames[next.delivered]? = some futureFrame := by
    simpa [framesEq] using future
  have futureSequence : futureFrame.sequence = next.delivered :=
    (wellFormed.2 next.delivered futureFrame futureInState).2
  rw [frameSequence, futureSequence]
  omega

/-- Two consecutive successful receives cannot consume the same accepted
frame. This is the operational exactly-once consequence of FIFO plus the
affine delivery cursor. -/
theorem consecutive_transport_receives_are_exactly_once
    {state next after : TransportState}
    {frame nextFrame : TransportFrame}
    (wellFormed : state.WellFormed)
    (received : state.pollReceive = .delivered frame next)
    (receivedNext : next.pollReceive = .delivered nextFrame after) :
    nextFrame.sequence ≠ frame.sequence := by
  have nextPresent := (transport_receive_is_fifo receivedNext).1
  exact well_formed_transport_has_no_replay wellFormed received nextFrame nextPresent

theorem peer_close_is_observable_after_fifo_drain
    {state : TransportState}
    (drained : state.delivered = state.frames.length) :
    (state.closePeer).pollReceive = .peerClosed := by
  simp [TransportState.closePeer, TransportState.pollReceive, drained]

theorem peer_close_does_not_skip_buffered_frame
    {state : TransportState} {frame : TransportFrame}
    (present : state.frames[state.delivered]? = some frame) :
    (state.closePeer).pollReceive =
      .delivered frame { state.closePeer with delivered := state.delivered + 1 } := by
  simp [TransportState.closePeer, TransportState.pollReceive, present]

theorem abort_peer_is_immediately_observable (state : TransportState) :
    state.abortPeer.pollReceive = .peerClosed := by
  rfl

theorem abort_peer_is_well_formed (state : TransportState) :
    state.abortPeer.WellFormed := by
  constructor
  · exact Nat.le_refl _
  · intro index frame present
    simp [TransportState.abortPeer] at present

theorem closed_transport_rejects_send
    {state : TransportState} {frameLabel : Fin 256}
    (closed : state.peerClosed = true) :
    state.send frameLabel = none := by
  simp [TransportState.send, closed]

/-- Once the accepted prefix is drained, the next accepted send is the unique
next sequence and every frame from the prior iteration has a smaller sequence. -/
theorem transport_send_after_drain_is_fresh
    {state sent : TransportState}
    {frameLabel : Fin 256}
    (wellFormed : state.WellFormed)
    (drained : state.delivered = state.frames.length)
    (accepted : state.send frameLabel = some sent) :
    ∃ frame after,
      sent.pollReceive = .delivered frame after /\
      frame.channel = state.channel /\
      frame.sequence = state.frames.length /\
      frame.frameLabel = frameLabel /\
      (∀ old, old ∈ state.frames -> old.sequence < frame.sequence) := by
  unfold TransportState.send at accepted
  by_cases closed : state.peerClosed
  · simp [closed] at accepted
  · let frame : TransportFrame := {
      channel := state.channel
      sequence := state.frames.length
      frameLabel
    }
    have sentEq :
        { state with frames := state.frames ++ [frame] } = sent :=
      Option.some.inj (by simpa [closed, frame] using accepted)
    subst sent
    let after : TransportState := {
      state with
      frames := state.frames ++ [frame]
      delivered := state.delivered + 1
    }
    have present :
        (state.frames ++ [frame])[state.delivered]? = some frame := by
      simp [drained]
    refine ⟨frame, after, ?_, rfl, rfl, rfl, ?_⟩
    · simp [TransportState.pollReceive, present, after]
    · intro old member
      obtain ⟨index, bound, atIndex⟩ := List.mem_iff_getElem.mp member
      have presentOld : state.frames[index]? = some old :=
        List.getElem?_eq_some_iff.mpr ⟨bound, atIndex⟩
      have sequence := (wellFormed.2 index old presentOld).2
      rw [sequence]
      exact bound

/-- Reusing a session number in a fresh carrier instance cannot alias a frame
from the retired generation, even when all protocol-visible fields coincide. -/
theorem distinct_carrier_generations_cannot_alias
    {left right : TransportFrame}
    (different : left.channel.generation ≠ right.channel.generation) :
    left ≠ right := by
  intro same
  subst right
  exact different rfl

end Hibana
