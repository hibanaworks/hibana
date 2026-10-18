import Hibana.ActorCommit
import Hibana.OwnedSourceFacts
import Hibana.EndpointTransfer
import Hibana.SourceResourceContinuation

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
  let source ← image.ownedSourceFacts? choreo sample.pathIndex sample.event
  let afterView ← observedActorView? sample.after source.resource.role
  pure {
    effIndex := source.effIndex
    resource := source.resource
    binding := source.binding
    side := source.side
    lane := source.lane
    frameLabel := source.frameLabel
    afterView
  }

def OwnedCommitFacts.sourceFacts (facts : OwnedCommitFacts) : OwnedSourceFacts := {
  effIndex := facts.effIndex, resource := facts.resource, binding := facts.binding,
  side := facts.side, lane := facts.lane, frameLabel := facts.frameLabel }

theorem decoded_owned_commit_retains_its_source_facts
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {facts : OwnedCommitFacts}
    (decoded : sample.ownedFacts? image choreo = some facts) :
    image.ownedSourceFacts? choreo sample.pathIndex sample.event = some facts.sourceFacts := by
  unfold ActorCommitObservation.ownedFacts? at decoded
  obtain ⟨source, sourceAt, remaining⟩ := Option.bind_eq_some_iff.mp decoded
  obtain ⟨view, _, written⟩ := Option.bind_eq_some_iff.mp remaining
  cases Option.some.inj written
  exact sourceAt

def OwnedCommitFacts.actor? (facts : OwnedCommitFacts) : Option Nat :=
  facts.sourceFacts.actor?

/-- The three-byte envelope matches the production encoder and receive guard.
No source ordinal is truncated into a different accepted event. -/
def OwnedCommitFacts.envelope (facts : OwnedCommitFacts) : List Nat :=
  facts.sourceFacts.envelope

theorem owned_envelope_identifies_resource_and_source_occurrence
    {first second : OwnedCommitFacts} (same : first.envelope = second.envelope) :
    first.resource.role = second.resource.role ∧ first.effIndex = second.effIndex := by
  exact owned_source_envelope_identifies_resource_and_occurrence same

def OwnedSourceFacts.wireMatches
    (facts : OwnedSourceFacts) (actor : Nat)
    (wire : OwnedWireObservation) (session : Nat) : Bool :=
  facts.resource.component == 0 && facts.resource.role < 256 && facts.effIndex < 65536 &&
    facts.actor? == some actor && wire.session == session &&
    wire.lane == facts.lane && wire.frameLabel == facts.frameLabel &&
    wire.sender == facts.binding.message.sender.role &&
    wire.receiver == facts.binding.message.receiver.role &&
    decide (wire.payload = facts.envelope)

def OwnedCommitFacts.wireMatches
    (facts : OwnedCommitFacts) (sample : ActorCommitObservation)
    (wire : OwnedWireObservation) (session : Nat) : Bool :=
  facts.sourceFacts.wireMatches sample.before.actor wire session

def ActorCommitObservation.ownedCheck
    (sample : ActorCommitObservation) (image : RustDescriptorImage) (choreo : Choreo)
    (wire : OwnedWireObservation) (session : Nat) : Bool :=
  sample.check image choreo &&
    match sample.ownedFacts? image choreo with
    | none => false
    | some facts => facts.wireMatches sample wire session

/-- Owned facts retain the same decoded source row and the actual after-view.
This connects a checked wire boundary to its header update without selecting
an unrelated binding or inventing a receiver tag. -/
theorem decoded_owned_facts_retain_the_source_row_and_view
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {facts : OwnedCommitFacts}
    (decoded : sample.ownedFacts? image choreo = some facts) :
    ∃ data path row entry,
      image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[sample.pathIndex]? = some path ∧
      data.rows[sample.event]? = some row ∧
      path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry ∧
      entry.2.message.payload = .endpoint facts.resource ∧ row.actionId.side = facts.side ∧
      facts.binding = entry.2 ∧
      observedActorView? sample.after facts.resource.role = some facts.afterView := by
  unfold ActorCommitObservation.ownedFacts? at decoded
  obtain ⟨source, sourceAt, remaining⟩ := Option.bind_eq_some_iff.mp decoded
  obtain ⟨view, viewAt, written⟩ := Option.bind_eq_some_iff.mp remaining
  cases Option.some.inj written
  obtain ⟨data, path, row, entry, dataAt, pathAt, rowAt, entryAt, payload, side, binding⟩ :=
    decoded_owned_source_facts_retain_the_exact_source_row sourceAt
  exact ⟨data, path, row, entry, dataAt, pathAt, rowAt, entryAt, payload, side, binding, viewAt⟩

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
      simp only [OwnedCommitFacts.wireMatches, OwnedSourceFacts.wireMatches, OwnedCommitFacts.sourceFacts,
        Bool.and_eq_true, beq_iff_eq,
        decide_eq_true_eq] at wireOk
      exact ⟨facts, rfl, by simpa only [and_assoc] using wireOk⟩

/-- Preserving the actual accepted payload binds the two checked boundaries
to one resource and source occurrence. Header authentication alone is not a
replacement for this payload correspondence or the ordered carrier contract. -/
theorem checked_owned_frame_has_the_same_source_at_send_and_receive
    {sender receiver : ActorCommitObservation}
    {senderImage receiverImage : RustDescriptorImage} {choreo : Choreo}
    {sent received : OwnedWireObservation} {session : Nat}
    {senderFacts receiverFacts : OwnedCommitFacts}
    (senderChecked : sender.ownedCheck senderImage choreo sent session = true)
    (receiverChecked : receiver.ownedCheck receiverImage choreo received session = true)
    (senderDecoded : sender.ownedFacts? senderImage choreo = some senderFacts)
    (receiverDecoded : receiver.ownedFacts? receiverImage choreo = some receiverFacts)
    (samePayload : sent.payload = received.payload) :
    senderFacts.resource = receiverFacts.resource ∧
      senderFacts.effIndex = receiverFacts.effIndex := by
  obtain ⟨_, actualSender, senderAt, senderFields⟩ :=
    accepted_owned_commit_binds_source_view_and_wire senderChecked
  obtain ⟨_, actualReceiver, receiverAt, receiverFields⟩ :=
    accepted_owned_commit_binds_source_view_and_wire receiverChecked
  have senderSame := Option.some.inj (senderAt.symm.trans senderDecoded)
  have receiverSame := Option.some.inj (receiverAt.symm.trans receiverDecoded)
  subst actualSender
  subst actualReceiver
  obtain ⟨senderComponent, _, _, _, _, _, _, _, _, senderEnvelope⟩ := senderFields
  obtain ⟨receiverComponent, _, _, _, _, _, _, _, _, receiverEnvelope⟩ := receiverFields
  have sameEnvelope : senderFacts.envelope = receiverFacts.envelope :=
    senderEnvelope.symm.trans (samePayload.trans receiverEnvelope)
  obtain ⟨role, event⟩ := owned_envelope_identifies_resource_and_source_occurrence sameEnvelope
  have key : senderFacts.resource = receiverFacts.resource := by
    change EndpointKey.mk senderFacts.resource.component senderFacts.resource.role =
      EndpointKey.mk receiverFacts.resource.component receiverFacts.resource.role
    rw [senderComponent, receiverComponent, role]
  exact ⟨key, event⟩

/-- The source path used by a checked real owned commit has a solved source
elaboration. The continuation/receipt dependencies can be applied to that
same path without postulating a second ownership interpretation. -/
theorem source_checked_owned_commit_has_solved_source_derivation
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat}
    (checked : sample.ownedCheck image choreo wire session = true) :
    ∃ data path issuance run,
      image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[sample.pathIndex]? = some path ∧
      elaborate data.source.resources (initialOwners data.source.resources)
        path.selections data.source 0 = some run ∧
      run.equations.all (equationAccepted issuance) = true ∧
      (run.bindings.mapM fun (event, symbolic) => do
        let binding ← instantiateBinding issuance symbolic
        some (event, binding)) = some path.bindings := by
  obtain ⟨_, facts, factsAt, _⟩ := accepted_owned_commit_binds_source_view_and_wire checked
  obtain ⟨data, path, _, _, decoded, pathAt, _⟩ :=
    decoded_owned_facts_retain_the_source_row_and_view factsAt
  obtain ⟨issuance, run, sourceAt, solved, instantiated⟩ :=
    decoded_actor_path_has_a_solved_source_derivation decoded (List.mem_of_getElem? pathAt)
  exact ⟨data, path, issuance, run, decoded, pathAt, sourceAt, solved, instantiated⟩

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
    {state : EndpointTransfer.Observation (Nat × Nat × Nat × Nat) TransportFrame}
    {rest : List TransportFrame} (initialView : Nat × Nat × Nat × Nat)
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
        next.usable = (sample.after.actor, facts.afterView.header) :: state.usable ∧
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
    simp only [OwnedCommitFacts.actor?, OwnedCommitFacts.sourceFacts, OwnedSourceFacts.actor?, receiving] at actor
    exact (Option.some.inj actor).symm
  obtain ⟨next, after, step, bound, issued, _, committedExact, advances, _⟩ :=
    EndpointTransfer.ordered_carrier_commit_preserves_unique_authority profile
      initialView (sample.after.actor, facts.afterView.header) execution currentExact wellFormed
      received resolved queued
  exact ⟨sourceOrdered, receiverActor, envelope, next, after, step, bound, issued,
    committedExact, advances, identity.1.symm.trans sessionSame,
    identity.2.2.1.symm.trans sender, identity.2.2.2.1.symm.trans receiver⟩

end Hibana
