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

private def sourceSendRecord (image : RustDescriptorImage) (choreo : Choreo)
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
    (paired : sourceReceivePairCheck sample record image choreo session = true) :
    sourceSendRecord image choreo record = false := by
  unfold sourceReceivePairCheck at paired
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

The bound applies only after physical acceptance within the short mutation
lease. Independent futures may wait before acceptance, and other actors may
complete arbitrarily many operations while this publication is suspended.

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
