import Hibana.DistributedSourceExecution

namespace Hibana
open EndpointDelegation

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

/-- Actual physical acceptances by one actor, including ordinary payloads.
This is a projection of the shared observation log, not a runtime queue. -/
def actorAcceptedSends (entries : List SourceAcceptance) (actor : Nat) : List SourceAcceptance :=
  entries.filter fun entry => entry.sample.image.role == actor

def sourceSendRecord (image : RustDescriptorImage) (choreo : Choreo)
    (record : ActorCommitRecord) : Bool :=
  match sourceOperationIdentity? image choreo record.step.pathIndex record.step.event with
  | some (_, _, .send) => true
  | _ => false

/-- Published remote sends in the same distributed history. Receives and
local operations remain in that history but add no sender-publication letter. -/
def actorPublishedSends (images : Nat → RustDescriptorImage) (choreo : Choreo)
    (records : List (Nat × ActorCommitRecord)) (actor : Nat) : List ActorCommitRecord :=
  records.filterMap fun (owner, record) =>
    if owner == actor && sourceSendRecord (images actor) choreo record then some record else none

/-- A physical acceptance is identified by its existing logical direction and
carrier witness. This is an erased observation, not a new wire or runtime ID.
Equal bytes in different rolled visits do not identify the same acceptance. -/
def SourceAcceptance.callKey (entry : SourceAcceptance) : TransportChannel × Nat :=
  (entry.channel, entry.carrierWitness)

/-- Match one retained send call to its publication. Independent fragments may
have changed progress and choice knowledge since its physical acceptance.
The lifecycle below consumes the exact outstanding acceptance once; matching
bytes or a source row alone is deliberately insufficient to establish a call.
This checker observes existing source checks and never admits a Rust operation. -/
def retainedSendPublicationCheck (entry : SourceAcceptance) (record : ActorCommitRecord)
    (choreo : Choreo) (session : Nat) : Bool :=
  entry.sample.check choreo session && record.check entry.sample.image choreo session &&
    decide (entry.sample.wire = record.wire) &&
    match sourceOperationIdentity? entry.sample.image choreo entry.sample.pathIndex entry.sample.event,
        sourceOperationIdentity? entry.sample.image choreo record.step.pathIndex record.step.event with
    | some (sendEff, sendBinding, .send), some (pubEff, pubBinding, .send) =>
        sendEff == pubEff && decide (sendBinding = pubBinding) &&
          entry.sample.image.role == sendBinding.senderActor
    | _, _ => false

/-- The send-call projection of one chronological boundary history. Receives
and local commits are retained by the complete distributed execution, not
invented as publications here. Publication order need not be acceptance order. -/
inductive SourceCallBoundary where
  | accept (entry : SourceAcceptance)
  | publish (key : TransportChannel × Nat) (record : ActorCommitRecord)

/-- Match physical acceptances and completed sends from the same retained
calls. The three lists are proof outputs derived from one boundary word, not
runtime queues, progress flags or an actor ownership table.

An outstanding row excludes a second call to that row, not other fragments of
the actor. The actual operation-slot/borrow correspondence must establish this
fact when extracting Rust histories. The relation does not assume an actor-wide
mutation stamp, identical whole-actor snapshots, or a shared roll iteration.
It is the publication component of the distributed reduction, not a second
executor or a completed distributed-progress theorem. -/
inductive SourceCallHistory (choreo : Choreo) (session : Nat) :
    List SourceCallBoundary → List SourceAcceptance →
      List (SourceAcceptance × ActorCommitRecord) → List SourceAcceptance → Prop where
  | nil : SourceCallHistory choreo session [] [] [] []
  | accept {word accepted published pending} {entry : SourceAcceptance}
      (history : SourceCallHistory choreo session word accepted published pending)
      (checked : entry.sample.check choreo session = true)
      (fresh : accepted.all (fun old => old.callKey != entry.callKey) = true)
      (slotFree : pending.all (fun old => !(old.sample.image.role == entry.sample.image.role &&
        old.sample.event == entry.sample.event)) = true) :
      SourceCallHistory choreo session (word ++ [.accept entry])
        (accepted ++ [entry]) published (pending ++ [entry])
  | publish {word accepted published left right} {entry : SourceAcceptance}
      {record : ActorCommitRecord}
      (history : SourceCallHistory choreo session word accepted published (left ++ entry :: right))
      (paired : retainedSendPublicationCheck entry record choreo session = true) :
      SourceCallHistory choreo session (word ++ [.publish entry.callKey record])
        accepted (published ++ [(entry, record)]) (left ++ right)

/-- Independent sends may publish in a different order, but each acceptance is
still exactly one published call or one outstanding call. There is no bound of
one outstanding acceptance per actor. This is a multiset conservation statement
over the same chronological observations, not equality of publication prefixes. -/
theorem a_source_call_history_partitions_its_acceptances
    {choreo : Choreo} {session : Nat} {word : List SourceCallBoundary}
    {accepted pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (history : SourceCallHistory choreo session word accepted published pending) :
    accepted.Perm (published.map Prod.fst ++ pending) := by
  induction history with
  | nil => exact .nil
  | accept history checked fresh slotFree ih =>
      simpa only [List.append_assoc] using ih.append_right [_]
  | @publish word accepted published left right entry record history paired ih =>
      simpa only [List.map_append, List.map_cons, List.map_nil, List.append_assoc,
        List.cons_append, List.nil_append] using
        ih.trans ((List.perm_middle :
          (left ++ entry :: right).Perm (entry :: (left ++ right))).append_left _)

/-- The carrier identity is never reused in a checked call history, even when
the source event and the complete wire bytes repeat in a later rolled visit. -/
theorem a_source_call_history_never_reuses_an_acceptance_key
    {choreo : Choreo} {session : Nat} {word : List SourceCallBoundary}
    {accepted pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (history : SourceCallHistory choreo session word accepted published pending) :
    (accepted.map SourceAcceptance.callKey).Nodup := by
  induction history with
  | nil => simp
  | @accept word accepted published pending entry history checked fresh slotFree ih =>
      simp only [List.map_append, List.map_cons, List.map_nil]
      apply List.nodup_append.mpr
      refine ⟨ih, by simp, ?_⟩
      intro key member other singleton equal
      have same : key = entry.callKey := equal.trans (by simpa using singleton)
      obtain ⟨old, inAccepted, atKey⟩ := List.mem_map.mp member
      have different := List.all_eq_true.mp fresh old inAccepted
      simp only [bne_iff_ne] at different
      exact different (atKey.trans same)
  | publish history paired ih => exact ih

/-- Publication consumes an existing outstanding call. A repeated or missing
publication cannot create another accepted occurrence or another live call. -/
theorem a_source_call_history_never_duplicates_a_publication
    {choreo : Choreo} {session : Nat} {word : List SourceCallBoundary}
    {accepted pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (history : SourceCallHistory choreo session word accepted published pending) :
    ((published.map (fun pair => pair.1.callKey)) ++
      pending.map SourceAcceptance.callKey).Nodup := by
  have permutation := (a_source_call_history_partitions_its_acceptances history).map
    SourceAcceptance.callKey
  have distinct := a_source_call_history_never_reuses_an_acceptance_key history
  simpa only [List.map_append, List.map_map] using permutation.nodup_iff.mp distinct

/-- Every published call retains its checked physical bytes and exact source
binding. Advancing another fragment never replaces these observations with a
freshly invented acceptance, a different frame, or a different logical send. -/
theorem a_retained_publication_preserves_its_source_and_wire
    {entry : SourceAcceptance} {record : ActorCommitRecord} {choreo : Choreo} {session : Nat}
    (paired : retainedSendPublicationCheck entry record choreo session = true) :
    entry.sample.check choreo session = true ∧
      record.check entry.sample.image choreo session = true ∧
      entry.sample.wire = record.wire ∧
      ∃ eff binding,
        sourceOperationIdentity? entry.sample.image choreo entry.sample.pathIndex entry.sample.event =
          some (eff, binding, .send) ∧
        sourceOperationIdentity? entry.sample.image choreo record.step.pathIndex record.step.event =
          some (eff, binding, .send) ∧
        entry.sample.image.role = binding.senderActor := by
  unfold retainedSendPublicationCheck at paired
  have header := (Bool.and_eq_true_iff.mp paired).1
  have localChecks := (Bool.and_eq_true_iff.mp header).1
  refine ⟨(Bool.and_eq_true_iff.mp localChecks).1, (Bool.and_eq_true_iff.mp localChecks).2,
    of_decide_eq_true (Bool.and_eq_true_iff.mp header).2, ?_⟩
  have matched := (Bool.and_eq_true_iff.mp paired).2
  cases sentAt : sourceOperationIdentity? entry.sample.image choreo entry.sample.pathIndex entry.sample.event with
  | none => simp [sentAt] at matched
  | some sent =>
      rcases sent with ⟨eff, binding, side⟩
      cases pubAt : sourceOperationIdentity? entry.sample.image choreo record.step.pathIndex record.step.event with
      | none => cases side <;> simp_all
      | some pub =>
          rcases pub with ⟨pubEff, pubBinding, pubSide⟩
          cases side <;> cases pubSide <;> simp_all
          exact ⟨pubEff, pubBinding, ⟨rfl, rfl⟩, rfl⟩

/-- Matching records are outputs of the lifecycle, including records whose
whole-actor base changed. No publication can avoid the source/wire check. -/
theorem a_source_call_history_checks_every_publication
    {choreo : Choreo} {session : Nat} {word : List SourceCallBoundary}
    {accepted pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (history : SourceCallHistory choreo session word accepted published pending) :
    ∀ pair ∈ published, retainedSendPublicationCheck pair.1 pair.2 choreo session = true := by
  induction history with
  | nil => simp
  | accept history checked fresh slotFree ih => exact ih
  | @publish word accepted published left right entry record history paired ih =>
      intro pair member
      rcases List.mem_append.mp member with previous | current
      · exact ih pair previous
      · have equal : pair = (entry, record) := by simpa using current
        subst pair
        exact paired

private theorem publication_is_send
    {sample : SourceSendAdmission} {record : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : sourcePublicationPairCheck sample record image choreo session = true) :
    sourceSendRecord image choreo record = true := by
  unfold sourcePublicationPairCheck at paired
  have matched := (Bool.and_eq_true_iff.mp paired).2
  cases leftAt : sourceOperationIdentity? sample.image choreo sample.pathIndex sample.event with
  | none => simp [leftAt] at matched
  | some left =>
    rcases left with ⟨eff, binding, side⟩
    cases side <;> simp only [leftAt] at matched
    all_goals
      cases rightAt : sourceOperationIdentity? image choreo record.step.pathIndex record.step.event with
      | none => simp [rightAt] at matched
      | some right =>
        rcases right with ⟨eff', binding', side'⟩
        cases side' <;> simp_all [sourceSendRecord]

private theorem receipt_is_not_send
    {sample : SourceSendAdmission} {record : ActorCommitRecord}
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (paired : SourceReceivePair sample record image choreo session) :
    sourceSendRecord image choreo record = false := by
  obtain ⟨_, _, _, _, received⟩ := a_corresponding_receive_pair_has_its_original_source_halves paired
  simp only [sourceSendRecord, received]

private theorem a_commit_increases_the_existing_mutation_stamp
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
    (commit : ActorMeasuredCommit image choreo session before after cleared record) :
    before.2 < after.2 := by
  cases commit with
  | ordinary checked written => dsimp; omega
  | fresh checked prepared =>
    obtain ⟨_, _, _, _, _, written⟩ := a_measured_reentry_has_its_source_reset_and_exact_writes checked
    dsimp
    omega

private def PublicationPrefix
    (images : Nat → RustDescriptorImage) (choreo : Choreo) (session actor : Nat)
    (cut : DistributedSourceCut × List SourceAcceptance) (records : List (Nat × ActorCommitRecord)) : Prop :=
  ∃ pairs : List (SourceAcceptance × ActorCommitRecord), ∃ pending : List SourceAcceptance,
    actorAcceptedSends cut.2 actor = pairs.map Prod.fst ++ pending ∧
    actorPublishedSends images choreo records actor = pairs.map Prod.snd ∧
    (∀ pair ∈ pairs, sourcePublicationPairCheck pair.1.sample pair.2 (images actor) choreo session = true ∧
      pair.1.mutation < (cut.1.actors actor).2) ∧
    pending.length ≤ 1 ∧
    (∀ entry ∈ pending, entry.stamps actor (cut.1.actors actor).2)

private theorem pending_is_empty_when_the_actor_is_available
    {entries pending : List SourceAcceptance} {pairs : List (SourceAcceptance × ActorCommitRecord)}
    {actor mutation : Nat}
    (correspondence : actorAcceptedSends entries actor = pairs.map Prod.fst ++ pending)
    (stamped : ∀ entry ∈ pending, entry.stamps actor mutation)
    (available : entries.all (fun entry => !(entry.sample.image.role == actor && entry.mutation == mutation)) = true) :
    pending = [] := by
  cases pending with
  | nil => rfl
  | cons entry rest =>
    have atStamp := stamped entry (by simp)
    have present : entry ∈ actorAcceptedSends entries actor := by rw [correspondence]; simp
    have inEntries := (List.mem_filter.mp present).1
    have absent := List.all_eq_true.mp available entry inEntries
    have both := Bool.and_eq_true_iff.mpr
      ⟨beq_iff_eq.mpr atStamp.1, beq_iff_eq.mpr atStamp.2⟩
    rw [both] at absent
    cases absent

private theorem a_pending_entry_is_the_only_unpublished_one
    {entries pending : List SourceAcceptance} {pairs : List (SourceAcceptance × ActorCommitRecord)}
    {actor mutation : Nat} {entry : SourceAcceptance}
    (correspondence : actorAcceptedSends entries actor = pairs.map Prod.fst ++ pending)
    (earlier : ∀ pair ∈ pairs, pair.1.mutation < mutation)
    (short : pending.length ≤ 1) (member : entry ∈ entries) (stamp : entry.stamps actor mutation) :
    pending = [entry] := by
  have present : entry ∈ actorAcceptedSends entries actor :=
    List.mem_filter.mpr ⟨member, beq_iff_eq.mpr stamp.1⟩
  rw [correspondence] at present
  have current : entry ∈ pending := by
    rcases List.mem_append.mp present with old | current
    · obtain ⟨pair, inPairs, same⟩ := List.mem_map.mp old
      have bound := earlier pair inPairs
      have stampAt := congrArg SourceAcceptance.mutation same
      have atStamp := stamp.2
      omega
    · exact current
  cases pending with
  | nil => simp at current
  | cons first rest =>
    cases rest with
    | nil =>
      have equal : entry = first := by simpa only [List.mem_singleton] using current
      exact congrArg (fun value => [value]) equal.symm
    | cons second tail => simp at short

private theorem publication_prefix_step
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged)
    (correspondence : PublicationPrefix images choreo session actor first records) :
    PublicationPrefix images choreo session actor last (records ++ logged.toList) := by
  obtain ⟨pairs, pending, acceptances, publications, earlier, short, stamps⟩ := correspondence
  cases step with
  | @quiet cut entries changed after checked =>
    by_cases same : actor = changed
    · subst changed
      refine ⟨pairs, pending, acceptances, ?_, ?_, short, ?_⟩
      · simpa [actorPublishedSends] using publications
      · simpa [DistributedSourceCut.actorAt] using earlier
      · simpa [DistributedSourceCut.actorAt] using stamps
    · refine ⟨pairs, pending, acceptances, ?_, ?_, short, ?_⟩
      · simpa [actorPublishedSends] using publications
      · simpa [DistributedSourceCut.actorAt, same] using earlier
      · simpa [DistributedSourceCut.actorAt, same] using stamps
  | @accept cut entries sample channel sent event previousPath witness label data path row
      imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    let entry : SourceAcceptance := ⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩
    by_cases same : sample.image.role = actor
    · have availableAt : entries.all (fun entry => !(entry.sample.image.role == actor &&
          entry.mutation == (cut.actors actor).2)) = true := by simpa only [same] using available
      have empty := pending_is_empty_when_the_actor_is_available acceptances stamps availableAt
      subst pending
      refine ⟨pairs, [entry], ?_, ?_, earlier, by simp, ?_⟩
      · simpa [actorAcceptedSends, List.filter_append, entry, same] using
          congrArg (fun values => values ++ [entry]) acceptances
      · simpa [actorPublishedSends] using publications
      · intro current member
        have equal : current = entry := by simpa using member
        subst current
        exact ⟨same, by dsimp [entry, DistributedSourceCut.carrierAt]; rw [same]⟩
    · refine ⟨pairs, pending, ?_, ?_, earlier, short, stamps⟩
      · simpa [actorAcceptedSends, List.filter_append, same] using acceptances
      · simpa [actorPublishedSends] using publications
  | @publish cut entries changed after cleared record entry member stamp paired committed =>
    by_cases same : changed = actor
    · subst changed
      have only := a_pending_entry_is_the_only_unpublished_one acceptances
        (fun pair inPairs => (earlier pair inPairs).2) short member stamp
      subst pending
      have increased := a_commit_increases_the_existing_mutation_stamp committed
      refine ⟨pairs ++ [(entry, record)], [], ?_, ?_, ?_, by simp, ?_⟩
      · simpa only [List.map_append, List.map_cons, List.map_nil, List.append_nil] using acceptances
      · simpa [actorPublishedSends, List.filterMap_append, publication_is_send paired] using
          congrArg (fun values => values ++ [record]) publications
      · intro pair inPairs
        rcases List.mem_append.mp inPairs with old | current
        · have previous := earlier pair old
          exact ⟨previous.1, by simpa [DistributedSourceCut.actorAt] using Nat.lt_trans previous.2 increased⟩
        · have equal : pair = (entry, record) := by simpa using current
          subst pair
          exact ⟨paired, by simpa [DistributedSourceCut.actorAt, ← stamp.2] using increased⟩
      · intro current absent; simp at absent
    · refine ⟨pairs, pending, acceptances, ?_, ?_, short, ?_⟩
      · simpa [actorPublishedSends, List.filterMap_append, same] using publications
      · simpa [DistributedSourceCut.actorAt, Ne.symm same] using earlier
      · simpa [DistributedSourceCut.actorAt, Ne.symm same] using stamps
  | @localCommit cut entries changed after cleared record eff binding actorRole localAt available committed =>
    by_cases same : changed = actor
    · subst changed
      have empty := pending_is_empty_when_the_actor_is_available acceptances stamps available
      subst pending
      have increased := a_commit_increases_the_existing_mutation_stamp committed
      refine ⟨pairs, [], ?_, ?_, ?_, by simp, ?_⟩
      · exact acceptances
      · simpa [actorPublishedSends, List.filterMap_append, sourceSendRecord, localAt] using publications
      · intro pair member
        have previous := earlier pair member
        exact ⟨previous.1, by simpa [DistributedSourceCut.actorAt] using Nat.lt_trans previous.2 increased⟩
      · intro current absent; simp at absent
    · refine ⟨pairs, pending, acceptances, ?_, ?_, short, ?_⟩
      · simpa [actorPublishedSends, List.filterMap_append, same] using publications
      · simpa [DistributedSourceCut.actorAt, Ne.symm same] using earlier
      · simpa [DistributedSourceCut.actorAt, Ne.symm same] using stamps
  | @receive cut entries changed channel next frame after cleared record entry witness
      actorRole received entryAt identity paired available committed =>
    by_cases same : changed = actor
    · subst changed
      have empty := pending_is_empty_when_the_actor_is_available acceptances stamps available
      subst pending
      have increased := a_commit_increases_the_existing_mutation_stamp committed
      refine ⟨pairs, [], acceptances, ?_, ?_, by simp, ?_⟩
      · simpa [actorPublishedSends, List.filterMap_append, receipt_is_not_send paired] using publications
      · intro pair member
        have previous := earlier pair member
        exact ⟨previous.1, by
          simpa [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt] using
            (Nat.lt_trans previous.2 increased)⟩
      · intro current absent; simp at absent
    · refine ⟨pairs, pending, acceptances, ?_, ?_, short, ?_⟩
      · simpa [actorPublishedSends, List.filterMap_append, same] using publications
      · simpa [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt, Ne.symm same] using earlier
      · simpa [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt, Ne.symm same] using stamps

/-- Actual acceptances and sender publications form the same ordered prefix,
with at most one already accepted but not yet published frame for this actor.
The unmatched frame is derived at the existing mutation stamp. Receivers may
consume it before publication; no peer acknowledgement or source-wide visit
is required. The paired records retain exact source identity, wire bytes and
the same prepared cursor through `sourcePublicationPairCheck`.

The bound follows from the restricted `DistributedSourceExecution` relation,
which excludes this actor's independent commits in its publication window.
It does not cover an asynchronously pending accepted send while another par
fragment in the same actor commits; the TCP fixture exercises that legal case.
Other actors may complete arbitrarily many operations in this relation.

This is an observation theorem. It adds no runtime pending field or ledger,
and does not infer a callable Endpoint from source readiness. -/
theorem a_distributed_actors_acceptances_lead_publications_by_at_most_one
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : first.2 = []) :
    ∃ pairs : List (SourceAcceptance × ActorCommitRecord), ∃ pending : List SourceAcceptance,
      actorAcceptedSends last.2 actor = pairs.map Prod.fst ++ pending ∧
      actorPublishedSends images choreo records actor = pairs.map Prod.snd ∧
      (∀ pair ∈ pairs, sourcePublicationPairCheck pair.1.sample pair.2 (images actor) choreo session = true ∧
        pair.1.mutation < (last.1.actors actor).2) ∧
      pending.length ≤ 1 ∧
      (∀ entry ∈ pending, entry.stamps actor (last.1.actors actor).2) := by
  have start : PublicationPrefix images choreo session actor first [] := by
    refine ⟨[], [], ?_, rfl, ?_, by simp, ?_⟩
    · simp [actorAcceptedSends, initial]
    · intro pair absent; simp at absent
    · intro entry absent; simp at absent
  have advance : ∀ {before after : DistributedSourceCut × List SourceAcceptance}
      {logged : List (Nat × ActorCommitRecord)},
      DistributedSourceExecution images choreo session before after logged →
      PublicationPrefix images choreo session actor before [] →
      PublicationPrefix images choreo session actor after logged := by
    intro before after logged history
    induction history with
    | refl => intro base; exact base
    | tail prior step ih => intro base; exact publication_prefix_step step (ih base)
  exact advance execution start

end Hibana
