import Hibana.OccurrenceChoiceKnowledge

namespace Hibana
open EndpointDelegation

/-- The immutable source identity of either half of an operation. In
particular ordinary payloads do not get their source identity from a reused
frame label. Physical actors are retained alongside the logical message. -/
def sourceOperationIdentity? (image : RustDescriptorImage) (choreo : Choreo)
    (pathIndex event : Nat) : Option (Nat × Binding × Side) := do
  let data ← image.decodeActorOrder? choreo
  let path ← data.normalized.paths[pathIndex]?
  let row ← data.rows[event]?
  let (_, binding) ← path.bindings.find? (fun entry => entry.1 == row.actionId.event)
  let eff ← image.eventEffIndex? event
  pure (eff, binding, row.actionId.side)

/-- Source position and direction come from the immutable row, not its path
completion. Peer holders may differ between the two valid identities. -/
theorem source_identity_paths_keep_source_and_side
    {image : RustDescriptorImage} {choreo : Choreo} {leftPath rightPath event leftEff rightEff : Nat}
    {leftBinding rightBinding : Binding} {leftSide rightSide : Side}
    (left : sourceOperationIdentity? image choreo leftPath event = some (leftEff, leftBinding, leftSide))
    (right : sourceOperationIdentity? image choreo rightPath event = some (rightEff, rightBinding, rightSide)) :
    leftEff = rightEff ∧ leftSide = rightSide := by
  unfold sourceOperationIdentity? at left right
  simp only [Option.bind_eq_bind, Option.bind_eq_some_iff, Option.pure_def, Option.some.injEq] at left right
  obtain ⟨leftData, leftAt, _, _, leftRow, leftRowAt, _, _, actualLeft, leftEffAt, leftSame⟩ := left
  obtain ⟨rightData, rightAt, _, _, rightRow, rightRowAt, _, _, actualRight, rightEffAt, rightSame⟩ := right
  have sameData := Option.some.inj (leftAt.symm.trans rightAt)
  subst rightData
  have sameRow := Option.some.inj (leftRowAt.symm.trans rightRowAt)
  subst rightRow
  exact ⟨(congrArg Prod.fst leftSame).symm.trans
      ((Option.some.inj (leftEffAt.symm.trans rightEffAt)).trans (congrArg Prod.fst rightSame)),
    (congrArg (fun triple : Nat × Binding × Side => triple.2.2) leftSame).symm.trans
      (congrArg (fun triple : Nat × Binding × Side => triple.2.2) rightSame)⟩

/-- The accepted bytes retain their exact source binding and static wire
columns. This extracts existing checks; it is not an additional admission
condition or a comparison with a prospective receive. -/
theorem checked_source_send_has_its_bound_wire_columns
    {sample : SourceSendAdmission} {choreo : Choreo} {session : Nat}
    (checked : sample.check choreo session = true) :
    ∃ eff binding,
      sourceOperationIdentity? sample.image choreo sample.pathIndex sample.event =
        some (eff, binding, .send) ∧
      sample.effIndex? = some eff ∧ sample.wire.session = session ∧
      sample.base.actor = binding.senderActor ∧
      sample.wire.sender = binding.message.sender.role ∧
      sample.wire.receiver = binding.message.receiver.role ∧
      sample.image.eventLane? sample.event = some sample.wire.lane ∧
      sample.image.eventFrameLabel? sample.event = some sample.wire.frameLabel := by
  obtain ⟨data, path, row, entry, decoded, pathAt, rowAt, entryAt, _, side, _, _⟩ :=
    checked_source_send_has_its_ready_source_row checked
  unfold SourceSendAdmission.check at checked
  have rest := (Bool.and_eq_true_iff.mp checked).2
  simp only [decoded, pathAt, rowAt, entryAt, Bool.and_eq_true, and_assoc] at rest
  obtain ⟨_, _, _, _, _, _, _, someEff, sessionAt, actorAt, senderAt, receiverAt,
    laneAt, colorAt, _⟩ := rest
  obtain ⟨eff, effectAt⟩ := Option.isSome_iff_exists.mp someEff
  refine ⟨eff, entry.2, ?_, effectAt, beq_iff_eq.mp sessionAt, beq_iff_eq.mp actorAt,
    beq_iff_eq.mp senderAt, beq_iff_eq.mp receiverAt, beq_iff_eq.mp laneAt, beq_iff_eq.mp colorAt⟩
  change sample.image.eventEffIndex? sample.event = some eff at effectAt
  simp only [sourceOperationIdentity?, Option.bind_eq_bind, decoded, pathAt, rowAt, entryAt,
    effectAt, side, Option.bind_some, Option.pure_def]

theorem checked_record_has_its_source_identity
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat} {record : ActorCommitRecord}
    (checked : record.check image choreo session = true) :
    ∃ eff binding side, sourceOperationIdentity? image choreo record.step.pathIndex record.step.event =
      some (eff, binding, side) := by
  obtain ⟨_, data, path, row, entry, decoded, pathAt, rowAt, _, _, _, entryAt,
    _, _, _, _, _, _, _, publication⟩ :=
    accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
  obtain ⟨eff, effectAt, _⟩ := Option.bind_eq_some_iff.mp publication
  refine ⟨eff, entry.2, row.actionId.side, ?_⟩
  simp only [sourceOperationIdentity?, Option.bind_eq_bind, decoded, pathAt, rowAt, entryAt,
    effectAt, Option.bind_some, Option.pure_def]

/-- Check one supplied pair of source-path witnesses for the actual FIFO
receive. Source identity and the full physical binding must match, including
for ordinary bytes. `SourceReceivePair` below retains the original local
checks while allowing a different compatible completion of unknown choices.
Neither this checker nor that relation is a Rust admission guard. -/
def sourceReceivePairCheck (sent : SourceSendAdmission) (received : ActorCommitRecord)
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) : Bool :=
  sent.check choreo session && received.check image choreo session &&
    decide (sent.wire = received.wire) &&
    match sourceOperationIdentity? sent.image choreo sent.pathIndex sent.event,
        sourceOperationIdentity? image choreo received.step.pathIndex received.step.event with
    | some (sendEff, sendBinding, .send), some (recvEff, recvBinding, .receive) =>
        sendEff == recvEff && decide (sendBinding = recvBinding) &&
          image.role == sendBinding.receiverActor
    | _, _ => false

/-- Correspondence of the same actual boundaries. Local observations retain
their original witnesses. Completing only unknown choices must also admit
one pair of locally checked witnesses with the exact same physical binding.
Only proof witnesses change: images, events, bytes and both runtime cuts stay
fixed. No search, notification or choice publication is added to the runtime. -/
structure SourceReceivePair (sent : SourceSendAdmission) (received : ActorCommitRecord)
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) : Prop where
  accepted : sent.check choreo session = true
  committed : received.check image choreo session = true
  coherent : ∃ sendPath receivePath,
    sourceReceivePairCheck { sent with pathIndex := sendPath }
      { received with step := { received.step with pathIndex := receivePath } }
      image choreo session = true

theorem a_concrete_receive_pair_has_coherent_local_witnesses
    {sent : SourceSendAdmission} {received : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (checked : sourceReceivePairCheck sent received image choreo session = true) :
    SourceReceivePair sent received image choreo session := by
  have header := (Bool.and_eq_true_iff.mp checked).1
  have localChecks := (Bool.and_eq_true_iff.mp header).1
  exact ⟨(Bool.and_eq_true_iff.mp localChecks).1,
    (Bool.and_eq_true_iff.mp localChecks).2,
    sent.pathIndex, received.step.pathIndex, checked⟩

/-- Reindexing a local proof does not discard physical coherence. The new
local observations must still check, including their exact bytes and source
columns; the original coherent completion remains available unchanged. -/
theorem a_receive_pair_keeps_its_coherence_under_local_witness_changes
    {sent : SourceSendAdmission} {received : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session sendPath receivePath : Nat}
    (paired : SourceReceivePair sent received image choreo session)
    (accepted : ({ sent with pathIndex := sendPath } : SourceSendAdmission).check choreo session = true)
    (committed : ({ received with step := { received.step with pathIndex := receivePath } } :
      ActorCommitRecord).check image choreo session = true) :
    SourceReceivePair { sent with pathIndex := sendPath }
      { received with step := { received.step with pathIndex := receivePath } }
      image choreo session := by
  refine ⟨accepted, committed, ?_⟩
  obtain ⟨left, right, coherent⟩ := paired.coherent
  exact ⟨left, right, coherent⟩

theorem a_corresponding_receive_pair_preserves_its_observed_boundaries
    {sent : SourceSendAdmission} {received : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : SourceReceivePair sent received image choreo session) :
    sent.check choreo session = true ∧ received.check image choreo session = true ∧
      sent.wire = received.wire := by
  obtain ⟨_, _, coherent⟩ := paired.coherent
  have header := (Bool.and_eq_true_iff.mp coherent).1
  exact ⟨paired.accepted, paired.committed,
    of_decide_eq_true (Bool.and_eq_true_iff.mp header).2⟩

/-- The relational boundary still requires one exact physical binding, not
merely equal logical message labels. Both witnesses check the same original
runtime snapshots, events and bytes. Only their path indices may differ. -/
theorem a_corresponding_receive_pair_has_one_physical_binding
    {sent : SourceSendAdmission} {received : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : SourceReceivePair sent received image choreo session) :
    ∃ sendPath receivePath eff binding,
      ({ sent with pathIndex := sendPath } : SourceSendAdmission).check choreo session = true ∧
      ({ received with step := { received.step with pathIndex := receivePath } } :
        ActorCommitRecord).check image choreo session = true ∧
      sourceOperationIdentity? sent.image choreo sendPath sent.event = some (eff, binding, .send) ∧
      sourceOperationIdentity? image choreo receivePath received.step.event = some (eff, binding, .receive) ∧
      image.role = binding.receiverActor := by
  obtain ⟨sendPath, receivePath, checked⟩ := paired.coherent
  unfold sourceReceivePairCheck at checked
  have header := (Bool.and_eq_true_iff.mp checked).1
  have localChecks := (Bool.and_eq_true_iff.mp header).1
  have matched := (Bool.and_eq_true_iff.mp checked).2
  cases leftAt : sourceOperationIdentity? sent.image choreo sendPath sent.event with
  | none => simp [leftAt] at matched
  | some left =>
    rcases left with ⟨eff, binding, side⟩
    cases side with
    | receive | «local» => simp [leftAt] at matched
    | send =>
      cases rightAt : sourceOperationIdentity? image choreo receivePath received.step.event with
      | none => simp [leftAt, rightAt] at matched
      | some right =>
        rcases right with ⟨otherEff, otherBinding, otherSide⟩
        cases otherSide with
        | send | «local» => simp [leftAt, rightAt] at matched
        | receive =>
          have fields : eff = otherEff ∧ binding = otherBinding ∧ image.role = binding.receiverActor := by
            simpa only [leftAt, rightAt, Bool.and_eq_true, beq_iff_eq, decide_eq_true_eq, and_assoc] using matched
          exact ⟨sendPath, receivePath, eff, binding,
            (Bool.and_eq_true_iff.mp localChecks).1, (Bool.and_eq_true_iff.mp localChecks).2,
            leftAt, by simpa only [← fields.1, ← fields.2.1] using rightAt, fields.2.2⟩

/-- Reindexing only unknown choices cannot change either actual operation's
source position or direction. Their original source correspondence is derived
from checked rows and the coherent witness, rather than supplied again. -/
theorem a_corresponding_receive_pair_has_its_original_source_halves
    {sent : SourceSendAdmission} {received : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : SourceReceivePair sent received image choreo session) :
    ∃ eff sentBinding receivedBinding,
      sourceOperationIdentity? sent.image choreo sent.pathIndex sent.event = some (eff, sentBinding, .send) ∧
      sourceOperationIdentity? image choreo received.step.pathIndex received.step.event =
        some (eff, receivedBinding, .receive) := by
  obtain ⟨eff, sentBinding, sendAt, _⟩ := checked_source_send_has_its_bound_wire_columns paired.accepted
  obtain ⟨receivedEff, receivedBinding, side, receiveAt⟩ := checked_record_has_its_source_identity paired.committed
  obtain ⟨sendPath, receivePath, coherentEff, coherentBinding, _, _, coherentSend, coherentReceive, _⟩ :=
    a_corresponding_receive_pair_has_one_physical_binding paired
  have sentColumns := source_identity_paths_keep_source_and_side sendAt coherentSend
  have receivedColumns := source_identity_paths_keep_source_and_side receiveAt coherentReceive
  have sameEff : receivedEff = eff := receivedColumns.1.trans sentColumns.1.symm
  exact ⟨eff, sentBinding, receivedBinding, sendAt,
    by simpa only [sameEff, receivedColumns.2] using receiveAt⟩

def sourcePublicationPairCheck (sent : SourceSendAdmission) (published : ActorCommitRecord)
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) : Bool :=
  sent.check choreo session && published.check image choreo session &&
    decide (sent.wire = published.wire) &&
    decide (sent.base.progress = published.step.before.progress) &&
    decide (sent.base.choices = published.step.before.choices) &&
    match sourceOperationIdentity? sent.image choreo sent.pathIndex sent.event,
        sourceOperationIdentity? image choreo published.step.pathIndex published.step.event with
    | some (sendEff, sendBinding, .send), some (pubEff, pubBinding, .send) =>
        sendEff == pubEff && decide (sendBinding = pubBinding) &&
          image.role == sendBinding.senderActor
    | _, _ => false

/-- Publication consumes the same prepared cursor used at acceptance. Header
retirement and transient operation phases are separate from this source base;
they cannot justify a different reset or a different committed choice. -/
theorem a_source_publication_retains_its_exact_prepared_cursor
    {sent : SourceSendAdmission} {published : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : sourcePublicationPairCheck sent published image choreo session = true) :
    sent.base.progress = published.step.before.progress ∧
      sent.base.choices = published.step.before.choices := by
  have beforeMatch := (Bool.and_eq_true_iff.mp paired).1
  have choices := (Bool.and_eq_true_iff.mp beforeMatch).2
  have progress := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp beforeMatch).1).2
  exact ⟨of_decide_eq_true progress, of_decide_eq_true choices⟩

/-- One actual transport acceptance, stamped by its actor's existing measured
completion-write count. The stamp and append-only list are erased observations,
not a runtime call identifier, pending flag, owner table or new wire field. -/
structure SourceAcceptance where
  sample : SourceSendAdmission
  channel : TransportChannel
  event : Nat
  mutation : Nat
  carrierWitness : Nat
  deriving DecidableEq

def SourceAcceptance.stamps (entry : SourceAcceptance) (actor mutation : Nat) : Prop :=
  entry.sample.image.role = actor ∧ entry.mutation = mutation

instance (entry : SourceAcceptance) (actor mutation : Nat) :
    Decidable (entry.stamps actor mutation) :=
  inferInstanceAs (Decidable (entry.sample.image.role = actor ∧ entry.mutation = mutation))

/-- Finite scalar checks are exactly the original membership predicates.
They avoid asking a decision procedure to compare complete source images. -/
theorem source_acceptance_observation_checks_are_exact
    (entries : List SourceAcceptance) (actor mutation witness : Nat) (channel : TransportChannel) :
    ((entries.all (fun entry => !(entry.sample.image.role == actor && entry.mutation == mutation)) = true) ↔
      ∀ entry ∈ entries, ¬ entry.stamps actor mutation) ∧
    ((entries.all (fun entry => entry.channel != channel || entry.carrierWitness != witness) = true) ↔
      ∀ entry ∈ entries, entry.channel = channel → entry.carrierWitness ≠ witness) := by
  constructor
  · constructor
    · intro checked entry member matched
      have one := List.all_eq_true.mp checked entry member
      have both : (entry.sample.image.role == actor && entry.mutation == mutation) = true :=
        Bool.and_eq_true_iff.mpr ⟨beq_iff_eq.mpr matched.1, beq_iff_eq.mpr matched.2⟩
      rw [both] at one
      cases one
    · intro checked
      apply List.all_eq_true.mpr
      intro entry member
      cases matched : (entry.sample.image.role == actor && entry.mutation == mutation) with
      | false => rfl
      | true =>
        have fields := Bool.and_eq_true_iff.mp matched
        exact False.elim (checked entry member ⟨beq_iff_eq.mp fields.1, beq_iff_eq.mp fields.2⟩)
  · constructor
    · intro checked entry member same equal
      have one := List.all_eq_true.mp checked entry member
      have channelAt : (entry.channel == channel) = true := beq_iff_eq.mpr same
      have witnessAt : (entry.carrierWitness == witness) = true := beq_iff_eq.mpr equal
      change (!(entry.channel == channel) || !(entry.carrierWitness == witness)) = true at one
      rw [channelAt, witnessAt] at one
      cases one
    · intro checked
      apply List.all_eq_true.mpr
      intro entry member
      change (!(entry.channel == channel) || !(entry.carrierWitness == witness)) = true
      cases channelAt : entry.channel == channel with
      | false => rfl
      | true =>
        cases witnessAt : entry.carrierWitness == witness with
        | false => rfl
        | true =>
          exact False.elim (checked entry member (beq_iff_eq.mp channelAt) (beq_iff_eq.mp witnessAt))

def sourceChannelEntries (entries : List SourceAcceptance) (channel : TransportChannel) :
    List SourceAcceptance := entries.filter fun entry => decide (entry.channel = channel)

def sourceChannelRecords (entries : List SourceAcceptance) (channel : TransportChannel) :
    List (SourceSendAdmission × Nat) :=
  (sourceChannelEntries entries channel).map fun entry => (entry.sample, entry.event)

/-- A product of copied actor cuts and physical carrier cuts. Neither is a
new executor: the successful boundary history below relates their actual
measurements. Actor images stay fixed, but their choices and roll histories
are independent. There is no shared normalized path or session iteration. -/
structure DistributedSourceCut where
  actors : Nat → ActorProgressObservation × Nat
  carriers : TransportChannel → TransportState

def DistributedSourceCut.initial
    (actors : Nat → ActorProgressObservation × Nat) : DistributedSourceCut :=
  ⟨actors, TransportState.initial⟩

def DistributedSourceCut.actorAt (cut : DistributedSourceCut) (actor : Nat)
    (after : ActorProgressObservation × Nat) : DistributedSourceCut :=
  { cut with actors := fun query => if query = actor then after else cut.actors query }

def DistributedSourceCut.carrierAt (cut : DistributedSourceCut) (channel : TransportChannel)
    (after : TransportState) : DistributedSourceCut :=
  { cut with carriers := fun query => if query = channel then after else cut.carriers query }

/-- Successful source/transport boundaries in their observed order. Acceptance
and local publication are different transitions; a remote receive may occur
between them. The actor's short mutation lease prevents another acceptance
or commit at its unconsumed acceptance stamp. Quiet observations do not
advance its protocol. This does not
delay any other actor or require a receiver acknowledgement.

Receive-pair validation is an explicit implementation-correspondence obligation.
The relation does not yet prove that all Rust histories pass it, or global
progress/custody/fault composition. It supplies one common history instead of
assuming unrelated actor and FIFO histories happen to describe the same cut. -/
inductive DistributedSourceStep
    (images : Nat → RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    DistributedSourceCut × List SourceAcceptance →
      DistributedSourceCut × List SourceAcceptance → Option (Nat × ActorCommitRecord) → Prop where
  | quiet {cut : DistributedSourceCut} {entries : List SourceAcceptance}
      {actor : Nat} {after : ActorProgressObservation}
      (checked : actorCommitMutationHistoryCheck (images actor) choreo session
        (cut.actors actor).1 (cut.actors actor).2 after (cut.actors actor).2 [] = true) :
      DistributedSourceStep images choreo session (cut, entries)
        (cut.actorAt actor (after, (cut.actors actor).2), entries) none
  | accept {cut : DistributedSourceCut} {entries : List SourceAcceptance}
      {sample : SourceSendAdmission} {channel : TransportChannel} {sent : TransportState}
      {event previousPath witness : Nat} {label : Fin 256}
      {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
      (imageAt : sample.image = images sample.image.role)
      (checked : sample.check choreo session = true)
      (decoded : sample.image.decodeActorOrder? choreo = some data)
      (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
      (rowAt : data.rows[sample.event]? = some row)
      (prepared : ActorSourceCandidateBase
        { snapshot := (cut.actors sample.image.role).1, pathIndex := sample.pathIndex, admitted := [] }
        sample.image choreo previousPath path row sample.base)
      (available : entries.all (fun entry => !(entry.sample.image.role == sample.image.role &&
        entry.mutation == (cut.actors sample.image.role).2)) = true)
      (freshWitness : entries.all (fun entry => entry.channel != channel || entry.carrierWitness != witness) = true)
      (sourceAt : sample.effIndex? = some event)
      (accepted : (cut.carriers channel).send label = some sent)
      (identity : sample.wire.identifies {
        channel := (cut.carriers channel).channel,
        sequence := (cut.carriers channel).frames.length, frameLabel := label }) :
      DistributedSourceStep images choreo session (cut, entries)
        (cut.carrierAt channel sent,
          entries ++ [{ sample, channel, event, mutation := (cut.actors sample.image.role).2, carrierWitness := witness }]) none
  | publish {cut : DistributedSourceCut} {entries : List SourceAcceptance}
      {actor : Nat} {after : ActorProgressObservation × Nat} {cleared : List Nat}
      {record : ActorCommitRecord} {entry : SourceAcceptance}
      (member : entry ∈ entries)
      (stamp : entry.stamps actor (cut.actors actor).2)
      (paired : sourcePublicationPairCheck entry.sample record (images actor) choreo session = true)
      (committed : ActorMeasuredCommit (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedSourceStep images choreo session (cut, entries)
        (cut.actorAt actor after, entries) (some (actor, record))
  | localCommit {cut : DistributedSourceCut} {entries : List SourceAcceptance}
      {actor : Nat} {after : ActorProgressObservation × Nat} {cleared : List Nat}
      {record : ActorCommitRecord} {eff : Nat} {binding : Binding}
      (actorRole : (images actor).role = actor)
      (localAt : sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event =
        some (eff, binding, .local))
      (available : entries.all (fun entry => !(entry.sample.image.role == actor &&
        entry.mutation == (cut.actors actor).2)) = true)
      (committed : ActorMeasuredCommit (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedSourceStep images choreo session (cut, entries)
        (cut.actorAt actor after, entries) (some (actor, record))
  | receive {cut : DistributedSourceCut} {entries : List SourceAcceptance}
      {actor : Nat} {channel : TransportChannel} {next : TransportState} {frame : TransportFrame}
      {after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
      {entry : SourceAcceptance} {witness : Nat}
      (actorRole : (images actor).role = actor)
      (received : (cut.carriers channel).pollReceive = .delivered frame next)
      (entryAt : (sourceChannelEntries entries channel)[(cut.carriers channel).delivered]? = some entry)
      (identity : entry.carrierWitness = witness)
      (paired : SourceReceivePair entry.sample record (images actor) choreo session)
      (available : entries.all (fun entry => !(entry.sample.image.role == actor &&
        entry.mutation == (cut.actors actor).2)) = true)
      (committed : ActorMeasuredCommit (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedSourceStep images choreo session (cut, entries)
        ((cut.actorAt actor after).carrierAt channel next, entries) (some (actor, record))

/-- A finite boundary history, with its actual commits retained as one erased
observation log. The accepted-frame subsequence is the source of all per-channel
records; it never authorizes a Rust operation. Prefixes and suffixes can share
an observed cut without a global iteration or barrier. -/
inductive DistributedSourceExecution
    (images : Nat → RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    DistributedSourceCut × List SourceAcceptance →
      DistributedSourceCut × List SourceAcceptance → List (Nat × ActorCommitRecord) → Prop where
  | refl {state} : DistributedSourceExecution images choreo session state state []
  | tail {first middle final} {records : List (Nat × ActorCommitRecord)}
      {record : Option (Nat × ActorCommitRecord)}
      (history : DistributedSourceExecution images choreo session first middle records)
      (step : DistributedSourceStep images choreo session middle final record) :
      DistributedSourceExecution images choreo session first final (records ++ record.toList)

private theorem a_measured_commit_is_its_actual_step
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
    (committed : ActorMeasuredCommit image choreo session before after cleared record) :
    ActorMeasuredStep image choreo session before after cleared 1 := by
  cases committed with
  | ordinary checked written => exact .commit checked written
  | fresh checked prepared => exact .reentry checked prepared

private theorem source_channel_records_append
    (entries : List SourceAcceptance) (entry : SourceAcceptance) (channel : TransportChannel) :
    sourceChannelRecords (entries ++ [entry]) channel = sourceChannelRecords entries channel ++
      (if entry.channel = channel then [(entry.sample, entry.event)] else []) := by
  simp only [sourceChannelRecords, sourceChannelEntries, List.filter_append, List.filter_cons,
    List.filter_nil, List.map_append]
  split <;> simp_all

/-- Project the actual record of this boundary at one actor. Carrier changes
and foreign actor commits preserve its state. Zero-write observations are
checked separately; a real local commit retains its exact global log record. -/
theorem a_distributed_step_preserves_or_measures_its_actor
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged) (actor : Nat) :
    first.1.actors actor = last.1.actors actor ∨
    actorCommitMutationHistoryCheck (images actor) choreo session
      (first.1.actors actor).1 (first.1.actors actor).2
      (last.1.actors actor).1 (last.1.actors actor).2 [] = true ∨
    ∃ record cleared, logged = some (actor, record) ∧
      ActorMeasuredCommit (images actor) choreo session
        (first.1.actors actor) (last.1.actors actor) cleared record := by
  cases step with
  | @quiet cut entries changed after checked =>
    by_cases same : actor = changed
    · subst changed
      right; left
      simpa only [DistributedSourceCut.actorAt, if_pos rfl] using checked
    · left
      simp only [DistributedSourceCut.actorAt, if_neg same]
  | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    exact .inl rfl
  | @publish cut entries changed after cleared record entry member stamp paired committed =>
    by_cases same : actor = changed
    · subst changed
      right; right
      exact ⟨record, cleared, rfl, by simpa only [DistributedSourceCut.actorAt, if_pos rfl] using committed⟩
    · left
      simp only [DistributedSourceCut.actorAt, if_neg same]
  | @localCommit cut entries changed after cleared record eff binding actorRole localAt available committed =>
    by_cases same : actor = changed
    · subst changed
      right; right
      exact ⟨record, cleared, rfl, by simpa only [DistributedSourceCut.actorAt, if_pos rfl] using committed⟩
    · left
      simp only [DistributedSourceCut.actorAt, if_neg same]
  | @receive cut entries changed channel next frame after cleared record entry witness
      actorRole received entryAt identity paired available committed =>
    by_cases same : actor = changed
    · subst changed
      right; right
      exact ⟨record, cleared, rfl, by simpa only [DistributedSourceCut.carrierAt,
        DistributedSourceCut.actorAt, if_pos rfl] using committed⟩
    · left
      simp only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg same]

private theorem a_quiet_actor_interval_is_its_measured_step
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat}
    (checked : actorCommitMutationHistoryCheck image choreo session
      before.1 before.2 after.1 after.2 [] = true) :
    ActorMeasuredStep image choreo session before after [] 0 := by
  rcases before with ⟨before, count⟩
  rcases after with ⟨after, next⟩
  have same : next = count := by
    simpa only [List.length_nil, Nat.add_zero] using
      checked_mutation_history_accounts_for_every_observed_write checked
  subst next
  exact .quiet checked

private theorem a_distributed_source_step_projects_each_actor
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {record : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last record) (actor : Nat) :
    ∃ cleared commits, ActorMeasuredHistory (images actor) choreo session
      (first.1.actors actor) (last.1.actors actor) cleared commits := by
  rcases a_distributed_step_preserves_or_measures_its_actor step actor with
    same | quiet | ⟨record, cleared, logged, committed⟩
  · exact ⟨[], 0, by rw [← same]; exact .refl⟩
  · exact ⟨[], 0, .tail .refl (a_quiet_actor_interval_is_its_measured_step quiet)⟩
  · exact ⟨cleared, 1, .tail .refl (a_measured_commit_is_its_actual_step committed)⟩

/-- Every actor's complete measured history is derived from the same global
boundary history. Foreign actors and carrier acceptance are genuine stutters;
no common roll reset or branch path is inserted by the projection. -/
theorem a_distributed_source_execution_projects_each_actual_actor_history
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance} {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records) (actor : Nat) :
    ∃ cleared commits, ActorMeasuredHistory (images actor) choreo session
      (first.1.actors actor) (last.1.actors actor) cleared commits := by
  induction execution with
  | refl => exact ⟨[], 0, .refl⟩
  | tail history step ih =>
    obtain ⟨earlier, commits, prior⟩ := ih
    obtain ⟨later, added, current⟩ := a_distributed_source_step_projects_each_actor step actor
    exact ⟨_, _, measured_actor_histories_compose_at_the_same_boundary prior current⟩

private theorem a_distributed_source_step_projects_each_carrier
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {record : Option (Nat × ActorCommitRecord)} {history : ElasticAdmissionHistory}
    (step : DistributedSourceStep images choreo session first last record) (channel : TransportChannel)
    (trace : SendAdmissionTrace choreo session (first.1.carriers channel) history
      (sourceChannelRecords first.2 channel)) :
    ∃ next, SendAdmissionTrace choreo session (last.1.carriers channel) next
      (sourceChannelRecords last.2 channel) := by
  cases step with
  | quiet checked => exact ⟨history, trace⟩
  | @accept cut entries sample changed sent event previousPath witness label data path row
      imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    by_cases same : channel = changed
    · subst changed
      refine ⟨history.appendOccurrence event, ?_⟩
      simpa only [DistributedSourceCut.carrierAt, if_pos rfl,
        source_channel_records_append, if_pos rfl] using
        SendAdmissionTrace.accept trace checked sourceAt accepted identity
    · refine ⟨history, ?_⟩
      simpa only [DistributedSourceCut.carrierAt, if_neg same,
        source_channel_records_append, if_neg (Ne.symm same), List.append_nil] using trace
  | publish member stamp paired committed => exact ⟨history, trace⟩
  | localCommit actorRole localAt available committed => exact ⟨history, trace⟩
  | @receive cut entries actor changed next frame after cleared record entry witness
      actorRole received entryAt identity paired available committed =>
    by_cases same : channel = changed
    · subst changed
      obtain ⟨_, _, _, following, _, _, _, _, _, _, _, _, _, advanced⟩ :=
        a_fifo_receive_has_its_actual_accepted_source_occurrence trace received
      exact ⟨following, by simpa only [DistributedSourceCut.carrierAt,
        DistributedSourceCut.actorAt, if_pos rfl] using advanced⟩
    · exact ⟨history, by simpa only [DistributedSourceCut.carrierAt,
        DistributedSourceCut.actorAt, if_neg same] using trace⟩

/-- Every channel's full admission/receive history is projected from that
same global execution, including ordinary frames between owned transfers.
The result permits arbitrary independently advancing actors and rolled bodies. -/
theorem a_distributed_source_execution_projects_its_complete_fifo_histories
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records) (channel : TransportChannel) :
    ∃ history, SendAdmissionTrace choreo session (last.1.carriers channel) history
      (sourceChannelRecords last.2 channel) := by
  have lift : ∀ {first last : DistributedSourceCut × List SourceAcceptance}
      {records : List (Nat × ActorCommitRecord)}
      {history : ElasticAdmissionHistory},
      DistributedSourceExecution images choreo session first last records →
      SendAdmissionTrace choreo session (first.1.carriers channel) history
        (sourceChannelRecords first.2 channel) →
      ∃ next, SendAdmissionTrace choreo session (last.1.carriers channel) next
        (sourceChannelRecords last.2 channel) := by
    intro first last records history trace
    induction trace generalizing history with
    | refl => intro current; exact ⟨history, current⟩
    | tail prior step ih =>
      intro current
      obtain ⟨middle, atMiddle⟩ := ih current
      exact a_distributed_source_step_projects_each_carrier step channel atMiddle
  exact lift execution (SendAdmissionTrace.initial channel)

theorem distributed_source_executions_compose_at_the_same_observed_cut
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first middle last : DistributedSourceCut × List SourceAcceptance}
    {prior later : List (Nat × ActorCommitRecord)}
    (earlier : DistributedSourceExecution images choreo session first middle prior)
    (suffix : DistributedSourceExecution images choreo session middle last later) :
    DistributedSourceExecution images choreo session first last (prior ++ later) := by
  induction suffix with
  | refl => simpa only [List.append_nil] using earlier
  | tail history step ih =>
    simpa only [List.append_assoc] using DistributedSourceExecution.tail ih step

/-- A retained commit in the one global log names its exact actual boundary,
not merely some compatible per-actor history. The global suffix is preserved
even if other actors have entered different visits in the meantime. -/
theorem a_logged_distributed_source_commit_has_its_exact_boundary
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {record : ActorCommitRecord}
    (execution : DistributedSourceExecution images choreo session first last records)
    (member : (actor, record) ∈ records) :
    ∃ before after prior later,
      DistributedSourceExecution images choreo session first before prior ∧
      DistributedSourceStep images choreo session before after (some (actor, record)) ∧
      DistributedSourceExecution images choreo session after last later := by
  induction execution with
  | refl => simp at member
  | @tail middle last records logged history step ih =>
    rcases List.mem_append.mp member with previous | current
    · obtain ⟨before, after, prior, later, prefixHistory, boundary, suffix⟩ := ih previous
      exact ⟨before, after, prior, _, prefixHistory, boundary, .tail suffix step⟩
    · cases logged with
      | none => simp at current
      | some actual =>
        have same : (actor, record) = actual := by simpa only [Option.toList_some, List.mem_singleton] using current
        subst actual
        exact ⟨middle, last, records, [], history, step, .refl⟩

private theorem a_distributed_step_marks_or_preserves_the_completed_actor_row
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged)
    (completed : (last.1.actors actor).1.doneState.done event = true) :
    (∃ record cleared, logged = some (actor, record) ∧
      ActorMeasuredCommit (images actor) choreo session
        (first.1.actors actor) (last.1.actors actor) cleared record ∧ record.step.event = event) ∨
    (∃ cleared commits, ActorMeasuredHistory (images actor) choreo session
      (first.1.actors actor) (last.1.actors actor) cleared commits ∧
      event ∉ cleared ∧ (first.1.actors actor).1.doneState.done event = true) := by
  rcases a_distributed_step_preserves_or_measures_its_actor step actor with
    same | quiet | ⟨record, cleared, loggedAt, committed⟩
  · exact .inr ⟨[], 0, by rw [← same]; exact .refl, by simp, by rw [same]; exact completed⟩
  · have progressAt := beq_iff_eq.mp
      (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp quiet).2).1
    exact .inr ⟨[], 0, .tail .refl (a_quiet_actor_interval_is_its_measured_step quiet), by simp,
      by simpa only [ActorProgressObservation.doneState, progressAt] using completed⟩
  · rcases a_measured_commit_marks_or_preserves_a_completed_row committed completed with marked | preserved
    · exact .inl ⟨record, cleared, loggedAt, committed, marked⟩
    · exact .inr ⟨cleared, 1, .tail .refl (a_measured_commit_is_its_actual_step committed),
        preserved.2, preserved.1⟩

/-- Recover a current row's origin in the one distributed execution itself.
The suffix retains that row's completion, while other actors and unrelated
rolled bodies may progress or restart. The record is the actual global log
entry, not a separately chosen local history or a shared session iteration. -/
theorem a_current_distributed_row_has_its_unreset_actual_boundary
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor event : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : (first.1.actors actor).1.doneState.done event = false)
    (completed : (last.1.actors actor).1.doneState.done event = true) :
    ∃ before after prior later record marked untouched commits,
      DistributedSourceExecution images choreo session first before prior ∧
      DistributedSourceStep images choreo session before after (some (actor, record)) ∧
      ActorMeasuredCommit (images actor) choreo session
        (before.1.actors actor) (after.1.actors actor) marked record ∧
      DistributedSourceExecution images choreo session after last later ∧
      ActorMeasuredHistory (images actor) choreo session
        (after.1.actors actor) (last.1.actors actor) untouched commits ∧
      event ∉ untouched ∧ record.step.event = event := by
  induction execution with
  | refl => rw [initial] at completed; contradiction
  | @tail middle last earlier logged history step ih =>
    rcases a_distributed_step_marks_or_preserves_the_completed_actor_row step completed with
      ⟨record, marked, loggedAt, actual, same⟩ |
      ⟨cleared, commits, localSuffix, untouched, old⟩
    · exact ⟨middle, last, earlier, [], record, marked, [], 0,
        history, loggedAt ▸ step, actual, .refl, .refl, by simp, same⟩
    · obtain ⟨before, after, prior, later, record, marked, earlierClear, earlierCommits,
        prefixHistory, boundary, actual, suffixHistory, localHistory, unreset, same⟩ := ih old
      exact ⟨before, after, prior, _, record, marked, _, _, prefixHistory, boundary, actual,
        .tail suffixHistory step,
        measured_actor_histories_compose_at_the_same_boundary localHistory localSuffix,
        by simpa only [List.mem_append, not_or] using And.intro unreset untouched, same⟩

private theorem a_source_step_only_appends_its_actual_acceptance
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged) :
    last.2 = first.2 ∨ ∃ entry, logged = none ∧ last.2 = first.2 ++ [entry] := by
  cases step with
  | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    exact .inr ⟨_, rfl, rfl⟩
  | quiet checked => exact .inl rfl
  | publish member stamp paired committed => exact .inl rfl
  | localCommit actorRole localAt available committed => exact .inl rfl
  | receive actorRole received entryAt identity paired available committed => exact .inl rfl

/-- An acceptance in the common erased log comes from an actual prior
transport boundary in that same execution. This does not require sender
publication, and remains valid when the receiver runs first. -/
theorem an_accepted_distributed_entry_has_its_actual_transport_boundary
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {entry : SourceAcceptance}
    (execution : DistributedSourceExecution images choreo session first last records)
    (absent : entry ∉ first.2) (present : entry ∈ last.2) :
    ∃ before after prior later,
      DistributedSourceExecution images choreo session first before prior ∧
      DistributedSourceStep images choreo session before after none ∧
      after.2 = before.2 ++ [entry] ∧
      DistributedSourceExecution images choreo session after last later := by
  induction execution with
  | refl => exact False.elim (absent present)
  | @tail middle last records logged history step ih =>
    rcases a_source_step_only_appends_its_actual_acceptance step with same | ⟨current, noCommit, appended⟩
    · obtain ⟨before, after, prior, later, prefixHistory, boundary, appended, suffixHistory⟩ :=
        ih (same ▸ present)
      exact ⟨before, after, prior, _, prefixHistory, boundary, appended, .tail suffixHistory step⟩
    · rw [appended] at present
      rcases List.mem_append.mp present with previous | currentMember
      · obtain ⟨before, after, prior, later, prefixHistory, boundary, appended, suffixHistory⟩ := ih previous
        exact ⟨before, after, prior, _, prefixHistory, boundary, appended, .tail suffixHistory step⟩
      · have identical : entry = current := by simpa only [List.mem_singleton] using currentMember
        subst current
        exact ⟨middle, last, records, [], history, noCommit ▸ step, appended, .refl⟩

private theorem a_source_publication_is_a_send
    {sent : SourceSendAdmission} {published : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : sourcePublicationPairCheck sent published image choreo session = true) :
    ∃ eff binding, sourceOperationIdentity? image choreo published.step.pathIndex published.step.event =
      some (eff, binding, .send) := by
  unfold sourcePublicationPairCheck at paired
  have matched := (Bool.and_eq_true_iff.mp paired).2
  cases firstAt : sourceOperationIdentity? sent.image choreo sent.pathIndex sent.event with
  | none => simp [firstAt] at matched
  | some first =>
    rcases first with ⟨eff, binding, side⟩
    cases side <;> simp only [firstAt] at matched
    all_goals
      cases secondAt : sourceOperationIdentity? image choreo published.step.pathIndex published.step.event with
      | none => simp [secondAt] at matched
      | some second =>
        rcases second with ⟨eff', binding', side'⟩
        cases side' <;> simp_all

/-- A checked receive in the retained log is tied to its physical FIFO head,
its actual carrier witness, and its matching source send. Different actors
may use different complete path witnesses; only this communication must agree. -/
theorem a_logged_receive_has_its_exact_fifo_acceptance
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor eff : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {record : ActorCommitRecord} {binding : Binding}
    (execution : DistributedSourceExecution images choreo session first last records)
    (member : (actor, record) ∈ records)
    (receiveAt : sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event =
      some (eff, binding, .receive)) :
    ∃ before after prior later channel entry witness next frame,
      DistributedSourceExecution images choreo session first before prior ∧
      DistributedSourceStep images choreo session before after (some (actor, record)) ∧
      (before.1.carriers channel).pollReceive = .delivered frame next ∧
      (sourceChannelEntries before.2 channel)[(before.1.carriers channel).delivered]? = some entry ∧
      entry.carrierWitness = witness ∧
      SourceReceivePair entry.sample record (images actor) choreo session ∧
      DistributedSourceExecution images choreo session after last later := by
  obtain ⟨before, after, prior, later, prefixHistory, boundary, suffixHistory⟩ :=
    a_logged_distributed_source_commit_has_its_exact_boundary execution member
  have atBoundary := boundary
  cases boundary with
  | publish present stamp paired committed =>
    obtain ⟨actual, actualBinding, sendAt⟩ := a_source_publication_is_a_send paired
    rw [receiveAt] at sendAt
    have impossible := congrArg (fun triple : Nat × Binding × Side => triple.2.2) (Option.some.inj sendAt)
    cases impossible
  | localCommit actorRole localAt available committed =>
    rw [receiveAt] at localAt
    have impossible := congrArg (fun triple : Nat × Binding × Side => triple.2.2) (Option.some.inj localAt)
    cases impossible
  | @receive cut entries actor channel next frame after cleared record entry witness
      actorRole received entryAt identity paired available committed =>
    exact ⟨_, _, prior, later, channel, entry, witness, next, frame,
      prefixHistory, atBoundary, received, entryAt, identity, paired, suffixHistory⟩

/-- Tie both ends of a receive to the same chronological distributed history.
The matching transport acceptance is in the prefix before the receipt, even
if its sender has not yet published. There is no common actor visit, whole
path agreement, sender-publication edge or live-value oracle in the premises. -/
theorem a_logged_receive_has_its_prior_actual_transport_acceptance
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor eff : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {record : ActorCommitRecord} {binding : Binding}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : first.2 = [])
    (member : (actor, record) ∈ records)
    (receiveAt : sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event =
      some (eff, binding, .receive)) :
    ∃ accepting accepted receiving received earlier between later channel entry witness next frame,
      DistributedSourceExecution images choreo session first accepting earlier ∧
      DistributedSourceStep images choreo session accepting accepted none ∧
      accepted.2 = accepting.2 ++ [entry] ∧
      DistributedSourceExecution images choreo session accepted receiving between ∧
      DistributedSourceStep images choreo session receiving received (some (actor, record)) ∧
      (receiving.1.carriers channel).pollReceive = .delivered frame next ∧
      (sourceChannelEntries receiving.2 channel)[(receiving.1.carriers channel).delivered]? = some entry ∧
      entry.carrierWitness = witness ∧
      SourceReceivePair entry.sample record (images actor) choreo session ∧
      DistributedSourceExecution images choreo session received last later := by
  obtain ⟨receiving, received, _, later, channel, entry, witness, next, frame,
      prefixHistory, receipt, polled, head, identity, paired, suffixHistory⟩ :=
    a_logged_receive_has_its_exact_fifo_acceptance execution member receiveAt
  have present : entry ∈ receiving.2 :=
    (List.mem_filter.mp (List.mem_of_getElem? head)).1
  have absent : entry ∉ first.2 := by simp only [initial, List.not_mem_nil, not_false_eq_true]
  obtain ⟨accepting, accepted, earlier, between, sourcePrefix, acceptance, appended, toReceipt⟩ :=
    an_accepted_distributed_entry_has_its_actual_transport_boundary prefixHistory absent present
  exact ⟨accepting, accepted, receiving, received, earlier, between, later, channel, entry, witness,
    next, frame, sourcePrefix, acceptance, appended, toReceipt, receipt, polled, head, identity, paired,
    suffixHistory⟩

end Hibana
