import Hibana.SourcePublicationPrefix

namespace Hibana
open EndpointDelegation

/-- One chronological source boundary. Sender publication is recorded separately
from physical acceptance, including when another operation of that actor commits
between them. The key is the existing erased carrier observation, not a wire ID. -/
inductive DistributedCallBoundary where
  | accept (entry : SourceAcceptance)
  | publish (key : TransportChannel × Nat) (actor : Nat) (record : ActorCommitRecord)
  | receive (key : TransportChannel × Nat) (actor : Nat) (record : ActorCommitRecord)
  | localCommit (actor : Nat) (record : ActorCommitRecord)
  | quiet (actor : Nat)

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
for complete Rust histories. Source-word validity, custody, enabled-step lifting
and fault/closure composition are further obligations, not premises hidden in
this definition or claims established merely by constructing it. -/
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
        (cut.actors actor).1 (cut.actors actor).2 after (cut.actors actor).2 [] = true) :
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
  | quiet history checked ih => simpa [List.filterMap_append, DistributedCallBoundary.call?] using ih
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
  | quiet history checked ih => exact ih
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
    ActorMeasuredStep image choreo session before after cleared 1 := by
  cases committed with
  | ordinary checked written => exact .commit checked written
  | fresh checked prepared => exact .reentry checked prepared

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
  | quiet history checked ih => exact a_measured_call_update_extends_each_actor ih (.quiet checked)
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      simpa only [DistributedSourceCut.carrierAt] using ih
  | publish history actorAt paired committed ih =>
      exact a_measured_call_update_extends_each_actor ih (a_call_commit_is_its_measured_step committed)
  | localCommit history actorRole localAt committed ih =>
      exact a_measured_call_update_extends_each_actor ih (a_call_commit_is_its_measured_step committed)
  | receive history actorRole received entryAt identity paired committed ih =>
      simpa only [DistributedSourceCut.carrierAt] using
        a_measured_call_update_extends_each_actor ih (a_call_commit_is_its_measured_step committed)

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
  | quiet history checked ih =>
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

end Hibana
