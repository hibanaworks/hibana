import Hibana.SourcePublicationPrefix
import Hibana.OwnedRetirement

namespace Hibana
open EndpointDelegation

/-- An ordinary zero-write interval may change I/O phase, but cannot hide
issuance or retirement. Those header changes need their recorded boundary. -/
def actorQuietHeadersCheck (before after : ActorProgressObservation) : Bool :=
  before.viewHeaders == after.viewHeaders && before.counter == after.counter &&
    before.liveViews == after.liveViews && decide (before.fault = after.fault)

/-- A receive observes EOF on its exact bound logical direction. The record
retains the actual actor cuts; it grants no progress or resource authority. -/
structure ActorClosureObservation where
  channel : TransportChannel
  event : Nat
  pathIndex : Nat := 0
  previousPath : Nat := 0
  retainedRolls : List Nat := []
  before : ActorProgressObservation
  after : ActorProgressObservation

def ActorClosureObservation.progressMatches (sample : ActorClosureObservation) : Bool :=
  sample.after.actor == sample.before.actor && sample.after.run == sample.before.run &&
    sample.after.counter == sample.before.counter && sample.after.liveViews == sample.before.liveViews &&
    decide (sample.after.steps = sample.before.steps) &&
    decide (sample.after.choices = sample.before.choices) &&
    decide (sample.after.viewHeaders = sample.before.viewHeaders)

def ActorClosureObservation.bindingMatches (sample : ActorClosureObservation)
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) : Bool :=
  match image.decodeActorOrder? choreo, image.eventAtom? sample.event,
      choreo.canonicalActorRollRows? image.role with
  | some data, some atom, some rolls =>
    image.actorOrderCheck choreo && image.actorRollCheck choreo &&
      sample.before.valid && sample.after.valid &&
      sample.before.actor == image.role && sample.before.metadataMatches image data.rows &&
      decide sample.retainedRolls.Nodup &&
      sample.retainedRolls.all (fun scope => rolls.any (fun roll => roll.scope == scope)) &&
      (match data.normalized.paths[sample.pathIndex]?, data.rows[sample.event]? with
       | some path, some row =>
         decide (row ∈ data.activeRows path) && decide (row.actionId.side = .receive) &&
           sourceRowViewsIssued path sample.before row &&
           (sourceCurrentCandidateReady image choreo sample.retainedRolls data path sample.before row ||
            sourceFreshCandidateReady image choreo sample.retainedRolls sample.before
              sample.previousPath sample.pathIndex data path row)
       | _, _ => false) &&
      sample.before.viewIssued sample.channel.receiver &&
      image.eventView? sample.event == some sample.channel.receiver &&
      image.eventLane? sample.event == some sample.channel.lane &&
      sample.channel.session == session && atom.sender == sample.channel.sender &&
      atom.receiver == sample.channel.receiver && sample.channel.sender != sample.channel.receiver
  | _, _, _ => false

def ActorClosureObservation.check (sample : ActorClosureObservation)
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat) : Bool :=
  sample.progressMatches && sample.bindingMatches image choreo session &&
    decide (sample.after.fault = sample.before.fault.orElse (fun _ => some .transportClosed))

theorem a_checked_closure_preserves_the_existing_first_fault
    {sample : ActorClosureObservation} {image : RustDescriptorImage}
    {choreo : Choreo} {session : Nat} {cause : SessionFault}
    (checked : sample.check image choreo session = true)
    (existing : sample.before.fault = some cause) : sample.after.fault = some cause := by
  have fault := of_decide_eq_true (Bool.and_eq_true_iff.mp checked).2
  simpa only [existing, Option.orElse_some] using fault

theorem a_checked_closure_disables_local_publication
    {sample : ActorClosureObservation} {image : RustDescriptorImage}
    {choreo : Choreo} {session : Nat}
    (checked : sample.check image choreo session = true) :
    (observedGeneration sample.after.fault).publicationPermitted = false := by
  have fault := of_decide_eq_true (Bool.and_eq_true_iff.mp checked).2
  rw [fault]
  cases sample.before.fault <;>
    simp [observedGeneration, SessionGenerationState.publicationPermitted]

/-- One chronological source boundary. Sender publication is recorded separately
from physical acceptance, including when another operation of that actor commits
between them. The key is the existing erased carrier observation, not a wire ID. -/
inductive DistributedCallBoundary where
  | accept (entry : SourceAcceptance)
  | publish (key : TransportChannel × Nat) (actor : Nat) (record : ActorCommitRecord)
  | receive (key : TransportChannel × Nat) (actor : Nat) (record : ActorCommitRecord)
  | localCommit (actor : Nat) (record : ActorCommitRecord)
  | quiet (actor : Nat)
  | retireView (sample : ViewRetirementObservation)
  | close (channel : TransportChannel)
  | observeClosure (sample : ActorClosureObservation)

def DistributedCallBoundary.acceptance? : DistributedCallBoundary → Option SourceAcceptance
  | .accept entry => some entry
  | _ => none

def DistributedCallBoundary.call? : DistributedCallBoundary → Option SourceCallBoundary
  | .accept entry => some (.accept entry)
  | .publish key _ record => some (.publish key record)
  | _ => none

def DistributedCallBoundary.record? : DistributedCallBoundary → Option (Nat × ActorCommitRecord)
  | .publish _ actor record | .receive _ actor record | .localCommit actor record => some (actor, record)
  | _ => none

/-- The logical operation word uses acceptance for a wire send, actual commit
for a receive/local action, and stutters sender publication. It never appends a
pending send at the end of an actor's publication word. -/
def DistributedCallBoundary.sourceLetter? (actor : Nat) : DistributedCallBoundary → Option Nat
  | .accept entry => if entry.sample.image.role == actor then some entry.sample.event else none
  | .receive _ owner record | .localCommit owner record =>
      if owner == actor then some record.step.event else none
  | _ => none

/-- Joint chronological actor/carrier observations with per-call publication
matching. This generalizes the synchronous publication window of
`DistributedSourceExecution`; it does not serialize independent actor fragments.

All three call lists are erased indices derived from this one history. Actual
actor progress is still the existing measured cut. Each acceptance prepares at
that cut, each publication/receive/local action has its actual measured commit,
and each receive consumes the full logical FIFO including ordinary frames.
There is no common actor path, visit count, or shared runtime memory premise.

The call-slot exclusion and retained-call extraction must still be justified
for complete Rust histories. `SourceActorWord` derives ordered source-group
words from this relation. Custody, enabled-step lifting and fault/closure
composition are further obligations, not premises hidden in this definition
or claims established merely by constructing it. -/
inductive DistributedCallExecution
    (images : Nat → RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    DistributedSourceCut → DistributedSourceCut → List DistributedCallBoundary →
      List SourceAcceptance → List (SourceAcceptance × ActorCommitRecord) →
      List SourceAcceptance → Prop where
  | refl {cut} : DistributedCallExecution images choreo session cut cut [] [] [] []
  | quiet {first cut : DistributedSourceCut} {word accepted published pending}
      {actor : Nat} {after : ActorProgressObservation}
      (history : DistributedCallExecution images choreo session first cut word accepted published pending)
      (checked : actorCommitMutationHistoryCheck (images actor) choreo session
        (cut.actors actor).1 (cut.actors actor).2 after (cut.actors actor).2 [] = true)
      (headers : actorQuietHeadersCheck (cut.actors actor).1 after = true) :
      DistributedCallExecution images choreo session first
        (cut.actorAt actor (after, (cut.actors actor).2))
        (word ++ [.quiet actor]) accepted published pending
  | accept {first cut : DistributedSourceCut} {word entries published pending}
      {sample : SourceSendAdmission} {channel : TransportChannel} {sent : TransportState}
      {event previousPath witness : Nat} {label : Fin 256}
      {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
      (history : DistributedCallExecution images choreo session first cut word entries published pending)
      (imageAt : sample.image = images sample.image.role)
      (checked : sample.check choreo session = true)
      (decoded : sample.image.decodeActorOrder? choreo = some data)
      (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
      (rowAt : data.rows[sample.event]? = some row)
      (prepared : ActorSourceCandidateBase
        { snapshot := (cut.actors sample.image.role).1, pathIndex := sample.pathIndex, admitted := [] }
        sample.image choreo previousPath path row sample.base)
      (slotFree : pending.all (fun old => !(old.sample.image.role == sample.image.role &&
        old.sample.event == sample.event)) = true)
      (freshWitness : entries.all (fun entry => entry.channel != channel || entry.carrierWitness != witness) = true)
      (sourceAt : sample.effIndex? = some event)
      (accepted : (cut.carriers channel).send label = some sent)
      (identity : sample.wire.identifies {
        channel := (cut.carriers channel).channel,
        sequence := (cut.carriers channel).frames.length, frameLabel := label }) :
      DistributedCallExecution images choreo session first (cut.carrierAt channel sent)
        (word ++ [.accept ⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩])
        (entries ++ [⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩]) published
        (pending ++ [⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩])
  | publish {first cut : DistributedSourceCut} {word entries published left right}
      {actor : Nat} {after : ActorProgressObservation × Nat} {cleared : List Nat}
      {record : ActorCommitRecord} {entry : SourceAcceptance}
      (history : DistributedCallExecution images choreo session first cut word entries published (left ++ entry :: right))
      (actorAt : entry.sample.image.role = actor)
      (paired : retainedSendPublicationCheck entry record choreo session = true)
      (committed : ActorMeasuredCommit (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedCallExecution images choreo session first (cut.actorAt actor after)
        (word ++ [.publish entry.callKey actor record]) entries
        (published ++ [(entry, record)]) (left ++ right)
  | localCommit {first cut : DistributedSourceCut} {word entries published pending}
      {actor : Nat} {after : ActorProgressObservation × Nat} {cleared : List Nat}
      {record : ActorCommitRecord} {eff : Nat} {binding : Binding}
      (history : DistributedCallExecution images choreo session first cut word entries published pending)
      (actorRole : (images actor).role = actor)
      (localAt : sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event =
        some (eff, binding, .local))
      (committed : ActorMeasuredCommit (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedCallExecution images choreo session first (cut.actorAt actor after)
        (word ++ [.localCommit actor record]) entries published pending
  | receive {first cut : DistributedSourceCut} {word entries published pending}
      {actor : Nat} {channel : TransportChannel} {next : TransportState} {frame : TransportFrame}
      {after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
      {entry : SourceAcceptance} {witness : Nat}
      (history : DistributedCallExecution images choreo session first cut word entries published pending)
      (actorRole : (images actor).role = actor)
      (received : (cut.carriers channel).pollReceive = .delivered frame next)
      (entryAt : (sourceChannelEntries entries channel)[(cut.carriers channel).delivered]? = some entry)
      (identity : entry.carrierWitness = witness)
      (paired : SourceReceivePair entry.sample record (images actor) choreo session)
      (committed : ActorMeasuredCommit (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedCallExecution images choreo session first
        ((cut.actorAt actor after).carrierAt channel next)
        (word ++ [.receive entry.callKey actor record]) entries published pending

/-- Publication matching is derived from the same complete chronological word
as actor commits and FIFO receives. Another actor's or another fragment's
commits do not consume, replace or forbid an outstanding send call. -/
theorem a_distributed_call_execution_has_its_same_history_publication_matching
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    SourceCallHistory choreo session (word.filterMap DistributedCallBoundary.call?) entries published pending := by
  induction execution with
  | refl => exact .nil
  | quiet history checked _headers ih => simpa [List.filterMap_append, DistributedCallBoundary.call?] using ih
  | @accept cut word entries published pending sample channel sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      let entry : SourceAcceptance := ⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩
      have fresh : entries.all (fun old => old.callKey != entry.callKey) = true := by
        apply List.all_eq_true.mpr
        intro old member
        apply bne_iff_ne.mpr
        intro equal
        have fields : old.channel = channel ∧ old.carrierWitness = witness := Prod.mk.inj equal
        rcases Bool.or_eq_true_iff.mp (List.all_eq_true.mp freshWitness old member) with left | right
        · exact (bne_iff_ne.mp left) fields.1
        · exact (bne_iff_ne.mp right) fields.2
      simpa only [List.filterMap_append, List.filterMap_cons, List.filterMap_nil,
        DistributedCallBoundary.call?] using
        SourceCallHistory.accept (entry := entry) ih checked fresh slotFree
  | publish history actorAt paired committed ih =>
      simpa [List.filterMap_append, DistributedCallBoundary.call?] using SourceCallHistory.publish ih paired
  | localCommit history actorRole localAt committed ih =>
      simpa [List.filterMap_append, DistributedCallBoundary.call?] using ih
  | receive history actorRole received entryAt identity paired committed ih =>
      simpa [List.filterMap_append, DistributedCallBoundary.call?] using ih

/-- The same distributed execution, with arbitrarily many independent pending
calls, conserves acceptances without imposing an actor-wide publication prefix. -/
theorem a_distributed_call_execution_partitions_its_actual_acceptances
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    entries.Perm (published.map Prod.fst ++ pending) ∧
      ((published.map (fun pair => pair.1.callKey)) ++ pending.map SourceAcceptance.callKey).Nodup := by
  have calls := a_distributed_call_execution_has_its_same_history_publication_matching execution
  exact ⟨a_source_call_history_partitions_its_acceptances calls,
    a_source_call_history_never_duplicates_a_publication calls⟩

/-- Accepted source checks and immutable actor images are retained by the same
chronology. A later publication or receipt cannot substitute another image or
infer the source occurrence from repeated payload bytes. -/
theorem a_distributed_call_execution_retains_each_accepted_source
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    ∀ entry ∈ entries, entry.sample.image = images entry.sample.image.role ∧
      entry.sample.check choreo session = true ∧ entry.sample.effIndex? = some entry.event := by
  induction execution with
  | refl => simp
  | quiet history checked _headers ih => exact ih
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      intro entry member
      rcases List.mem_append.mp member with earlier | current
      · exact ih entry earlier
      · have same := List.mem_singleton.mp current
        subst entry
        exact ⟨imageAt, checked, sourceAt⟩
  | publish history actorAt paired committed ih => exact ih
  | localCommit history actorRole localAt committed ih => exact ih
  | receive history actorRole received entryAt identity paired committed ih => exact ih

private theorem a_call_commit_is_its_measured_step
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
    (committed : ActorMeasuredCommit image choreo session before after cleared record) :
    ∃ writes, ActorMeasuredStep image choreo session before after cleared writes := by
  cases committed with
  | ordinary checked written => exact ⟨_, .commit checked written⟩
  | fresh checked prepared => exact ⟨_, .reentry checked prepared⟩

private theorem a_measured_call_update_extends_each_actor
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session changed : Nat}
    {first cut : DistributedSourceCut} {after : ActorProgressObservation × Nat}
    {more : List Nat} {added : Nat}
    (histories : ∀ actor, ∃ cleared commits,
      ActorMeasuredHistory (images actor) choreo session (first.actors actor) (cut.actors actor) cleared commits)
    (step : ActorMeasuredStep (images changed) choreo session (cut.actors changed) after more added) :
    ∀ actor, ∃ cleared commits,
      ActorMeasuredHistory (images actor) choreo session
        (first.actors actor) ((cut.actorAt changed after).actors actor) cleared commits := by
  intro actor
  obtain ⟨cleared, commits, prior⟩ := histories actor
  by_cases same : actor = changed
  · subst changed
    exact ⟨_, _, by simpa only [DistributedSourceCut.actorAt, if_pos rfl] using ActorMeasuredHistory.tail prior step⟩
  · exact ⟨cleared, commits, by simpa only [DistributedSourceCut.actorAt, if_neg same] using prior⟩

/-- Actual actor histories, including fresh body resets, come from the same
chronology as overlapping accepts and publications. Acceptance and foreign
work are stutters of this actor, without imposing any publication barrier. -/
theorem a_distributed_call_execution_projects_each_actual_actor_history
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    ∀ actor, ∃ cleared commits,
      ActorMeasuredHistory (images actor) choreo session (first.actors actor) (last.actors actor) cleared commits := by
  induction execution with
  | refl => exact fun _ => ⟨[], 0, .refl⟩
  | quiet history checked _headers ih => exact a_measured_call_update_extends_each_actor ih (.quiet checked)
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      simpa only [DistributedSourceCut.carrierAt] using ih
  | publish history actorAt paired committed ih =>
      obtain ⟨_, measured⟩ := a_call_commit_is_its_measured_step committed
      exact a_measured_call_update_extends_each_actor ih measured
  | localCommit history actorRole localAt committed ih =>
      obtain ⟨_, measured⟩ := a_call_commit_is_its_measured_step committed
      exact a_measured_call_update_extends_each_actor ih measured
  | receive history actorRole received entryAt identity paired committed ih =>
      obtain ⟨_, measured⟩ := a_call_commit_is_its_measured_step committed
      simpa only [DistributedSourceCut.carrierAt] using
        a_measured_call_update_extends_each_actor ih measured

private theorem call_channel_records_append
    (entries : List SourceAcceptance) (entry : SourceAcceptance) (channel : TransportChannel) :
    sourceChannelRecords (entries ++ [entry]) channel = sourceChannelRecords entries channel ++
      if entry.channel = channel then [(entry.sample, entry.event)] else [] := by
  by_cases same : entry.channel = channel <;>
    simp [sourceChannelRecords, sourceChannelEntries, List.filter_append, same]

private theorem a_call_execution_preserves_each_full_fifo
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    (∀ channel, ∃ history, SendAdmissionTrace choreo session (first.carriers channel) history []) →
    ∀ channel, ∃ history,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) := by
  induction execution with
  | refl =>
      intro initial
      simpa only [sourceChannelRecords, sourceChannelEntries, List.filter_nil, List.map_nil] using initial
  | quiet history checked _headers ih =>
      intro initial
      simpa only [DistributedSourceCut.actorAt] using ih initial
  | @accept cut word entries published pending sample changed sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      intro initial channel
      obtain ⟨prior, trace⟩ := ih initial channel
      by_cases same : channel = changed
      · subst changed
        refine ⟨prior.appendOccurrence event, ?_⟩
        simpa only [DistributedSourceCut.carrierAt, if_pos rfl,
          call_channel_records_append] using
          SendAdmissionTrace.accept trace checked sourceAt accepted identity
      · refine ⟨prior, ?_⟩
        simpa only [DistributedSourceCut.carrierAt, if_neg same, call_channel_records_append,
          if_neg (Ne.symm same), List.append_nil] using trace
  | publish history actorAt paired committed ih =>
      intro initial
      simpa only [DistributedSourceCut.actorAt] using ih initial
  | localCommit history actorRole localAt committed ih =>
      intro initial
      simpa only [DistributedSourceCut.actorAt] using ih initial
  | @receive cut word entries published pending actor changed next frame after cleared record entry witness
      history actorRole received entryAt identity paired committed ih =>
      intro initial channel
      obtain ⟨prior, trace⟩ := ih initial channel
      by_cases same : channel = changed
      · subst changed
        obtain ⟨_, _, _, following, _, _, _, _, _, _, _, _, _, advanced⟩ :=
          a_fifo_receive_has_its_actual_accepted_source_occurrence trace received
        exact ⟨following, by simpa only [DistributedSourceCut.carrierAt,
          DistributedSourceCut.actorAt, if_pos rfl] using advanced⟩
      · exact ⟨prior, by simpa only [DistributedSourceCut.carrierAt,
          DistributedSourceCut.actorAt, if_neg same] using trace⟩

/-- Full logical FIFO histories, including ordinary messages, project from the
same execution as per-call publication and actual actor commits. Receipts need
not wait for sender publication; the next holder's access remains a carrier
premise, not a shared-memory queue or a sender acknowledgement requirement. -/
theorem a_distributed_call_execution_projects_its_complete_fifo_histories
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {word : List DistributedCallBoundary} {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session
      (.initial initial) last word entries published pending) :
    ∀ channel, ∃ history,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) := by
  apply a_call_execution_preserves_each_full_fifo execution
  exact fun channel => ⟨_, .initial channel⟩

/-- Physical calls, updates and complete logical FIFOs from one chronological
word. Unlike the selected-visit execution above, an incoming commit need not
re-establish an old causal prefix in the current visit's event bits. Its
source order and physical actor binding are supplied by SourceOccurrenceTrace
for this SAME word. Neither predicate alone is the distributed refinement.

The before/after actor cuts, real mutation counts, owned envelope checks and
actual FIFO positions remain mandatory. This is erased proof data, not a new
runtime path, owner table or scheduling restriction. -/
inductive DistributedPhysicalExecution
    (images : Nat → RustDescriptorImage) (choreo : Choreo) (session : Nat) :
    DistributedSourceCut → DistributedSourceCut → List DistributedCallBoundary →
      List SourceAcceptance → List (SourceAcceptance × ActorCommitRecord) →
      List SourceAcceptance → Prop where
  | refl {cut} : DistributedPhysicalExecution images choreo session cut cut [] [] [] []
  | close {first cut : DistributedSourceCut} {word accepted published pending}
      {channel : TransportChannel}
      (history : DistributedPhysicalExecution images choreo session first cut word accepted published pending) :
      DistributedPhysicalExecution images choreo session first
        (cut.carrierAt channel (cut.carriers channel).closePeer)
        (word ++ [.close channel]) accepted published pending
  | observeClosure {first cut : DistributedSourceCut} {word accepted published pending}
      {sample : ActorClosureObservation}
      (history : DistributedPhysicalExecution images choreo session first cut word accepted published pending)
      (checked : sample.check (images sample.before.actor) choreo session = true)
      (beforeAt : (cut.actors sample.before.actor).1 = sample.before)
      (bound : (cut.carriers sample.channel).channel = sample.channel)
      (closed : (cut.carriers sample.channel).pollReceive = .peerClosed) :
      DistributedPhysicalExecution images choreo session first
        (cut.actorAt sample.before.actor (sample.after, (cut.actors sample.before.actor).2))
        (word ++ [.observeClosure sample]) accepted published pending
  | retireView {first cut : DistributedSourceCut} {word accepted published pending}
      {sample : ViewRetirementObservation}
      (history : DistributedPhysicalExecution images choreo session first cut word accepted published pending)
      (checked : sample.check (images sample.before.actor) choreo = true)
      (beforeAt : (cut.actors sample.before.actor).1 = sample.before) :
      DistributedPhysicalExecution images choreo session first
        (cut.actorAt sample.before.actor (sample.after, (cut.actors sample.before.actor).2))
        (word ++ [.retireView sample]) accepted published pending
  | quiet {first cut : DistributedSourceCut} {word accepted published pending}
      {actor : Nat} {after : ActorProgressObservation}
      (history : DistributedPhysicalExecution images choreo session first cut word accepted published pending)
      (checked : actorCommitMutationHistoryCheck (images actor) choreo session
        (cut.actors actor).1 (cut.actors actor).2 after (cut.actors actor).2 [] = true)
      (headers : actorQuietHeadersCheck (cut.actors actor).1 after = true) :
      DistributedPhysicalExecution images choreo session first
        (cut.actorAt actor (after, (cut.actors actor).2))
        (word ++ [.quiet actor]) accepted published pending
  | accept {first cut : DistributedSourceCut} {word entries published pending}
      {sample : SourceSendAdmission} {channel : TransportChannel} {sent : TransportState}
      {event previousPath witness : Nat} {label : Fin 256}
      {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
      (history : DistributedPhysicalExecution images choreo session first cut word entries published pending)
      (imageAt : sample.image = images sample.image.role)
      (checked : sample.check choreo session = true)
      (decoded : sample.image.decodeActorOrder? choreo = some data)
      (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
      (rowAt : data.rows[sample.event]? = some row)
      (prepared : ActorSourceCandidateBase
        { snapshot := (cut.actors sample.image.role).1, pathIndex := sample.pathIndex, admitted := [] }
        sample.image choreo previousPath path row sample.base)
      (slotFree : pending.all (fun old => !(old.sample.image.role == sample.image.role &&
        old.sample.event == sample.event)) = true)
      (freshWitness : entries.all (fun entry => entry.channel != channel || entry.carrierWitness != witness) = true)
      (sourceAt : sample.effIndex? = some event)
      (accepted : (cut.carriers channel).send label = some sent)
      (identity : sample.wire.identifies {
        channel := (cut.carriers channel).channel,
        sequence := (cut.carriers channel).frames.length, frameLabel := label }) :
      DistributedPhysicalExecution images choreo session first (cut.carrierAt channel sent)
        (word ++ [.accept ⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩])
        (entries ++ [⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩]) published
        (pending ++ [⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩])
  | publish {first cut : DistributedSourceCut} {word entries published left right}
      {actor : Nat} {after : ActorProgressObservation × Nat} {cleared : List Nat}
      {record : ActorCommitRecord} {entry : SourceAcceptance}
      (history : DistributedPhysicalExecution images choreo session first cut word entries published (left ++ entry :: right))
      (actorAt : entry.sample.image.role = actor)
      (sameWire : entry.sample.wire = record.wire)
      (committed : ActorMeasuredTransition (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedPhysicalExecution images choreo session first (cut.actorAt actor after)
        (word ++ [.publish entry.callKey actor record]) entries
        (published ++ [(entry, record)]) (left ++ right)
  | localCommit {first cut : DistributedSourceCut} {word entries published pending}
      {actor : Nat} {after : ActorProgressObservation × Nat} {cleared : List Nat}
      {record : ActorCommitRecord} {eff : Nat} {binding : Binding}
      (history : DistributedPhysicalExecution images choreo session first cut word entries published pending)
      (actorRole : (images actor).role = actor)
      (localAt : sourceOperationIdentity? (images actor) choreo record.step.pathIndex record.step.event =
        some (eff, binding, .local))
      (committed : ActorMeasuredTransition (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedPhysicalExecution images choreo session first (cut.actorAt actor after)
        (word ++ [.localCommit actor record]) entries published pending
  | receive {first cut : DistributedSourceCut} {word entries published pending}
      {actor : Nat} {channel : TransportChannel} {next : TransportState} {frame : TransportFrame}
      {after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
      {entry : SourceAcceptance} {witness : Nat}
      (history : DistributedPhysicalExecution images choreo session first cut word entries published pending)
      (actorRole : (images actor).role = actor)
      (received : (cut.carriers channel).pollReceive = .delivered frame next)
      (entryAt : (sourceChannelEntries entries channel)[(cut.carriers channel).delivered]? = some entry)
      (identity : entry.carrierWitness = witness)
      (sameWire : entry.sample.wire = record.wire)
      (committed : ActorMeasuredTransition (images actor) choreo session
        (cut.actors actor) after cleared record) :
      DistributedPhysicalExecution images choreo session first
        ((cut.actorAt actor after).carrierAt channel next)
        (word ++ [.receive entry.callKey actor record]) entries published pending

/-- Every previously qualified selected-visit execution keeps its exact cuts,
records and carrier identities in the physical projection. -/
theorem a_selected_visit_execution_has_its_same_physical_execution
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    DistributedPhysicalExecution images choreo session first last word entries published pending := by
  induction execution with
  | refl => exact .refl
  | quiet history checked headers ih => exact .quiet ih checked headers
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    exact .accept ih imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity
  | publish history actorAt paired committed ih =>
    have wire := of_decide_eq_true
      (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp paired).1).2
    exact .publish ih actorAt wire (a_measured_source_commit_has_its_same_physical_transition committed)
  | localCommit history actorRole localAt committed ih =>
    exact .localCommit ih actorRole localAt (a_measured_source_commit_has_its_same_physical_transition committed)
  | receive history actorRole received entryAt identity paired committed ih =>
    exact .receive ih actorRole received entryAt identity
      (a_corresponding_receive_pair_preserves_its_observed_boundaries paired).2.2
      (a_measured_source_commit_has_its_same_physical_transition committed)

theorem a_physical_execution_partitions_its_actual_acceptances
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending) :
    entries.Perm (published.map Prod.fst ++ pending) := by
  induction execution with
  | refl => exact .nil
  | close history ih => exact ih
  | observeClosure history checked beforeAt bound closed ih => exact ih
  | retireView history checked beforeAt ih => exact ih
  | quiet history checked _headers ih => exact ih
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    simpa only [List.append_assoc] using ih.append_right [_]
  | publish history actorAt sameWire committed ih =>
    simpa only [List.map_append, List.map_cons, List.map_nil, List.append_assoc,
      List.cons_append, List.nil_append] using
      ih.trans ((List.perm_middle : (_ ++ _ :: _).Perm (_ :: (_ ++ _))).append_left _)
  | localCommit history actorRole localAt committed ih => exact ih
  | receive history actorRole received entryAt identity sameWire committed ih => exact ih

private theorem a_physical_execution_preserves_each_full_fifo
    {first last : DistributedSourceCut}
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending) :
    (∀ channel, ∃ history, SendAdmissionTrace choreo session (first.carriers channel) history []) →
    ∀ channel, ∃ history,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) := by
  induction execution with
  | refl =>
    intro initial
    simpa only [sourceChannelRecords, sourceChannelEntries, List.filter_nil, List.map_nil] using initial
  | @close cut word entries published pending changed history ih =>
    intro initial channel
    obtain ⟨prior, trace⟩ := ih initial channel
    by_cases same : channel = changed
    · subst changed
      exact ⟨prior, by simpa only [DistributedSourceCut.carrierAt, if_pos rfl] using
        SendAdmissionTrace.close trace⟩
    · exact ⟨prior, by simpa only [DistributedSourceCut.carrierAt, if_neg same] using trace⟩
  | observeClosure history checked beforeAt bound closed ih =>
    intro initial
    simpa only [DistributedSourceCut.actorAt] using ih initial
  | retireView history checked beforeAt ih =>
    intro initial
    simpa only [DistributedSourceCut.actorAt] using ih initial
  | quiet history checked _headers ih =>
    intro initial
    simpa only [DistributedSourceCut.actorAt] using ih initial
  | @accept cut word entries published pending sample changed sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    intro initial channel
    obtain ⟨prior, trace⟩ := ih initial channel
    by_cases same : channel = changed
    · subst changed
      refine ⟨prior.appendOccurrence event, ?_⟩
      simpa only [DistributedSourceCut.carrierAt, if_pos rfl, call_channel_records_append] using
        SendAdmissionTrace.accept trace checked sourceAt accepted identity
    · refine ⟨prior, ?_⟩
      simpa only [DistributedSourceCut.carrierAt, if_neg same, call_channel_records_append,
        if_neg (Ne.symm same), List.append_nil] using trace
  | publish history actorAt sameWire committed ih =>
    intro initial
    simpa only [DistributedSourceCut.actorAt] using ih initial
  | localCommit history actorRole localAt committed ih =>
    intro initial
    simpa only [DistributedSourceCut.actorAt] using ih initial
  | @receive cut word entries published pending actor changed next frame after cleared record entry witness
      history actorRole received entryAt identity sameWire committed ih =>
    intro initial channel
    obtain ⟨prior, trace⟩ := ih initial channel
    by_cases same : channel = changed
    · subst changed
      obtain ⟨_, _, _, following, _, _, _, _, _, _, _, _, _, advanced⟩ :=
        a_fifo_receive_has_its_actual_accepted_source_occurrence trace received
      exact ⟨following, by simpa only [DistributedSourceCut.carrierAt,
        DistributedSourceCut.actorAt, if_pos rfl] using advanced⟩
    · exact ⟨prior, by simpa only [DistributedSourceCut.carrierAt,
        DistributedSourceCut.actorAt, if_neg same] using trace⟩

theorem a_physical_execution_projects_its_complete_fifo_histories
    {initial : Nat → ActorProgressObservation × Nat}
    (execution : DistributedPhysicalExecution images choreo session
      (.initial initial) last word entries published pending) :
    ∀ channel, ∃ history,
      SendAdmissionTrace choreo session (last.carriers channel) history (sourceChannelRecords entries channel) := by
  exact a_physical_execution_preserves_each_full_fifo execution (fun channel => ⟨_, .initial channel⟩)

/-- First-fault continuity is a property of this same physical history. A
retirement can add a cause but cannot replace one; quiet observations cannot
erase it; successful commits require live cuts. Remote actors and already
accepted carrier frames do not acquire this actor's fault by shared memory.
Co-resident endpoints may share their existing runtime/session fault state. -/
theorem a_distributed_physical_history_preserves_each_first_fault
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending) :
    ∀ actor cause, (first.actors actor).1.fault = some cause →
      (last.actors actor).1.fault = some cause := by
  induction execution with
  | refl => exact fun _ _ faulted => faulted
  | close history ih => simpa only [DistributedSourceCut.carrierAt] using ih
  | @observeClosure cut word entries published pending sample history checked beforeAt bound closed ih =>
    intro actor cause faulted
    have prior := ih actor cause faulted
    by_cases same : actor = sample.before.actor
    · subst actor
      have beforeFault : sample.before.fault = some cause := by
        rw [← beforeAt]
        exact prior
      simpa only [DistributedSourceCut.actorAt, if_pos rfl] using
        a_checked_closure_preserves_the_existing_first_fault checked beforeFault
    · simpa only [DistributedSourceCut.actorAt, if_neg same] using prior
  | @retireView cut word entries published pending sample history checked beforeAt ih =>
    intro actor cause faulted
    have prior := ih actor cause faulted
    by_cases same : actor = sample.before.actor
    · subst actor
      have beforeFault : sample.before.fault = some cause := by
        rw [← beforeAt]
        exact prior
      simpa only [DistributedSourceCut.actorAt, if_pos rfl] using
        accepted_retirement_preserves_first_fault checked beforeFault
    · simpa only [DistributedSourceCut.actorAt, if_neg same] using prior
  | @quiet cut word entries published pending changed after history checked headers ih =>
    intro actor cause faulted
    have prior := ih actor cause faulted
    by_cases same : actor = changed
    · subst actor
      have sameFault : (cut.actors changed).1.fault = after.fault :=
        of_decide_eq_true (Bool.and_eq_true_iff.mp headers).2
      simpa only [DistributedSourceCut.actorAt, if_pos rfl] using sameFault.symm.trans prior
    · simpa only [DistributedSourceCut.actorAt, if_neg same] using prior
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    simpa only [DistributedSourceCut.carrierAt] using ih
  | @publish cut word entries published left right changed after cleared record entry
      history actorAt sameWire committed ih =>
    intro actor cause faulted
    have prior := ih actor cause faulted
    by_cases same : actor = changed
    · subst actor
      have live := a_measured_physical_transition_uses_live_actor_cuts committed
      have impossible := live.1.symm.trans prior
      contradiction
    · simpa only [DistributedSourceCut.actorAt, if_neg same] using prior
  | @localCommit cut word entries published pending changed after cleared record eff binding
      history actorRole localAt committed ih =>
    intro actor cause faulted
    have prior := ih actor cause faulted
    by_cases same : actor = changed
    · subst actor
      have live := a_measured_physical_transition_uses_live_actor_cuts committed
      have impossible := live.1.symm.trans prior
      contradiction
    · simpa only [DistributedSourceCut.actorAt, if_neg same] using prior
  | @receive cut word entries published pending changed channel next frame after cleared record entry witness
      history actorRole received entryAt identity sameWire committed ih =>
    intro actor cause faulted
    have prior := ih actor cause faulted
    by_cases same : actor = changed
    · subst actor
      have live := a_measured_physical_transition_uses_live_actor_cuts committed
      have impossible := live.1.symm.trans prior
      contradiction
    · simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg same] using prior

end Hibana
