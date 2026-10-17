import Hibana.ActorCommit
import Hibana.EndpointTransfer

namespace Hibana
open EndpointDelegation

/-- Copied wire bytes at the actual accepted send, before receive consumption.
Carrier generation and sequence are deliberately absent: the core header does
not carry them, and they must come from the separate carrier refinement. -/
structure OwnedWireObservation where
  session : Nat
  lane : Nat
  sender : Nat
  receiver : Nat
  frameLabel : Nat
  payload : List Nat
  deriving Repr, DecidableEq

def OwnedWireObservation.identifies
    (wire : OwnedWireObservation) (frame : TransportFrame) : Prop :=
  wire.session = frame.channel.session ∧ wire.lane = frame.channel.lane ∧
    wire.sender = frame.channel.sender ∧ wire.receiver = frame.channel.receiver ∧
    wire.frameLabel = frame.frameLabel.val

/-- Immutable facts decoded from the same source and image as the real actor
commit. A source occurrence resolves its binding; the local row is not the
effect index encoded in the envelope. -/
structure OwnedCommitFacts where
  effIndex : Nat
  resource : EndpointKey
  binding : Binding
  side : Side
  lane : Nat
  frameLabel : Nat
  afterView : ActorViewObservation

def ActorCommitObservation.ownedFacts?
    (sample : ActorCommitObservation) (image : RustDescriptorImage) (choreo : Choreo) :
    Option OwnedCommitFacts := do
  let data ← image.decodeActorOrder? choreo
  let path ← data.normalized.paths[sample.pathIndex]?
  let row ← data.rows[sample.event]?
  let (_, binding) ← path.bindings.find? fun entry => entry.1 == row.actionId.event
  let .endpoint resource := binding.message.payload | none
  let effIndex ← image.eventEffIndex? sample.event
  let lane ← image.eventLane? sample.event
  let frameLabel ← image.eventFrameLabel? sample.event
  let afterView ← observedActorView? sample.after resource.role
  pure {
    effIndex
    resource
    binding
    side := row.actionId.side
    lane
    frameLabel
    afterView
  }

def OwnedCommitFacts.actor? (facts : OwnedCommitFacts) : Option Nat :=
  match facts.side with
  | .send => some facts.binding.senderActor
  | .receive => some facts.binding.receiverActor
  | .local => none

/-- The three-byte envelope matches the production encoder and receive guard.
No source ordinal is truncated into a different accepted event. -/
def OwnedCommitFacts.envelope (facts : OwnedCommitFacts) : List Nat :=
  [facts.resource.role, facts.effIndex / 256, facts.effIndex % 256]

def OwnedCommitFacts.wireMatches
    (facts : OwnedCommitFacts) (sample : ActorCommitObservation)
    (wire : OwnedWireObservation) (session : Nat) : Bool :=
  facts.resource.component == 0 && facts.resource.role < 256 && facts.effIndex < 65536 &&
    facts.actor? == some sample.before.actor && wire.session == session &&
    wire.lane == facts.lane && wire.frameLabel == facts.frameLabel &&
    wire.sender == facts.binding.message.sender.role &&
    wire.receiver == facts.binding.message.receiver.role &&
    decide (wire.payload = facts.envelope)

def ActorCommitObservation.ownedCheck
    (sample : ActorCommitObservation) (image : RustDescriptorImage) (choreo : Choreo)
    (wire : OwnedWireObservation) (session : Nat) : Bool :=
  sample.check image choreo &&
    match sample.ownedFacts? image choreo with
    | none => false
    | some facts => facts.wireMatches sample wire session

/-- Successful observations connect source ordering and view publication to
the exact accepted wire identity and envelope. Completeness of observations
and physical binding/carrier correctness remain implementation premises. -/
theorem accepted_owned_commit_binds_source_view_and_wire
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat}
    (checked : sample.ownedCheck image choreo wire session = true) :
    sample.SourceOrdered image choreo ∧
      ∃ facts, sample.ownedFacts? image choreo = some facts ∧
        facts.resource.component = 0 ∧ facts.resource.role < 256 ∧ facts.effIndex < 65536 ∧
        facts.actor? = some sample.before.actor ∧ wire.session = session ∧
        wire.lane = facts.lane ∧ wire.frameLabel = facts.frameLabel ∧
        wire.sender = facts.binding.message.sender.role ∧
        wire.receiver = facts.binding.message.receiver.role ∧ wire.payload = facts.envelope := by
  have checked' := Bool.and_eq_true_iff.mp checked
  refine ⟨accepted_actor_commit_matches_source_progress checked'.1, ?_⟩
  cases found : sample.ownedFacts? image choreo with
  | none => simp [found] at checked'
  | some facts =>
      have wireOk : facts.wireMatches sample wire session = true := by
        simpa [found] using checked'.2
      simp only [OwnedCommitFacts.wireMatches, Bool.and_eq_true, beq_iff_eq,
        decide_eq_true_eq] at wireOk
      exact ⟨facts, rfl, by simpa only [and_assoc] using wireOk⟩

/-- Combine an actual source-checked owned receive commit with the existing
ordered-carrier theorem. The fresh value is the observed receiver actor/view,
not an arbitrary new role, and sender publication is not a premise. -/
theorem source_checked_carrier_receive_preserves_unique_authority
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts)
    (receiving : facts.side = .receive)
    {carrier : CarrierSemantics}
    {abstraction : carrier.State → TransportBoundaryState} {Fairness : Prop}
    (profile : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current offered committed : carrier.State} {frame : TransportFrame}
    {before : TransportState}
    {state : EndpointTransfer.Observation (Nat × ActorViewObservation) TransportFrame}
    {rest : List TransportFrame} (initialView : Nat × ActorViewObservation)
    (execution : EndpointTransfer.Execution (EndpointTransfer.initial initialView) state)
    (currentExact : abstraction current = .idle before)
    (wellFormed : before.WellFormed)
    (received : carrier.pollReceive current = .delivered frame offered)
    (resolved : carrier.resolve offered .commit = some committed)
    (identity : wire.identifies frame)
    (queued : state.inFlight = frame :: rest) :
    sample.SourceOrdered image choreo ∧
      sample.before.actor = facts.binding.receiverActor ∧ wire.payload = facts.envelope ∧
      ∃ next after,
        EndpointTransfer.Step .receive state next ∧ next.usable.length ≤ 1 ∧
        next.usable = (sample.after.actor, facts.afterView) :: state.usable ∧
        abstraction committed = .idle after ∧ before.delivered < after.delivered ∧
        frame.channel.session = session ∧
        frame.channel.sender = facts.binding.message.sender.role ∧
        frame.channel.receiver = facts.binding.message.receiver.role := by
  obtain ⟨sourceOrdered, actual, found, fields⟩ :=
    accepted_owned_commit_binds_source_view_and_wire checked
  have same : actual = facts := Option.some.inj (found.symm.trans decoded)
  subst actual
  obtain ⟨_, _, _, actor, sessionSame, _, _, sender, receiver, envelope⟩ := fields
  have receiverActor : sample.before.actor = facts.binding.receiverActor := by
    simp only [OwnedCommitFacts.actor?, receiving] at actor
    exact (Option.some.inj actor).symm
  obtain ⟨next, after, step, bound, issued, _, committedExact, advances, _⟩ :=
    EndpointTransfer.ordered_carrier_commit_preserves_unique_authority profile
      initialView (sample.after.actor, facts.afterView) execution currentExact wellFormed
      received resolved queued
  exact ⟨sourceOrdered, receiverActor, envelope, next, after, step, bound, issued,
    committedExact, advances, identity.1.symm.trans sessionSame,
    identity.2.2.1.symm.trans sender, identity.2.2.2.1.symm.trans receiver⟩

end Hibana
