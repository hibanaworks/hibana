import Hibana.SourceFrameWord

namespace Hibana
open EndpointDelegation

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

private theorem a_commit_increases_its_actor_stamp
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

private theorem a_source_boundary_only_records_a_changing_actor
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged) (rows : List Nat) :
    (first.1.actors actor).2 ≤ (last.1.actors actor).2 ∧
      ((first.1.actors actor).2 = (last.1.actors actor).2 →
        committedFrameClassWord rows actor logged.toList = []) := by
  cases step with
  | @quiet cut entries changed after checked =>
    by_cases same : actor = changed
    · subst changed
      simp [DistributedSourceCut.actorAt, committedFrameClassWord]
    · simp [DistributedSourceCut.actorAt, same, committedFrameClassWord]
  | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    exact ⟨Nat.le_refl _, fun _ => rfl⟩
  | @publish cut entries changed after cleared record entry member stamp paired committed =>
    by_cases same : actor = changed
    · subst changed
      have increased := a_commit_increases_its_actor_stamp committed
      refine ⟨by simpa [DistributedSourceCut.actorAt] using Nat.le_of_lt increased, ?_⟩
      intro equal
      have impossible : (cut.actors actor).2 = after.2 := by simpa [DistributedSourceCut.actorAt] using equal
      exact False.elim (by omega)
    · simp [DistributedSourceCut.actorAt, same, Ne.symm same, committedFrameClassWord]
  | @localCommit cut entries changed after cleared record eff binding actorRole localAt available committed =>
    by_cases same : actor = changed
    · subst changed
      have increased := a_commit_increases_its_actor_stamp committed
      refine ⟨by simpa [DistributedSourceCut.actorAt] using Nat.le_of_lt increased, ?_⟩
      intro equal
      have impossible : (cut.actors actor).2 = after.2 := by simpa [DistributedSourceCut.actorAt] using equal
      exact False.elim (by omega)
    · simp [DistributedSourceCut.actorAt, same, Ne.symm same, committedFrameClassWord]
  | @receive cut entries changed channel next frame after cleared record entry witness
      actorRole received entryAt identity paired available committed =>
    by_cases same : actor = changed
    · subst changed
      have increased := a_commit_increases_its_actor_stamp committed
      refine ⟨by simpa [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt] using Nat.le_of_lt increased, ?_⟩
      intro equal
      have impossible : (cut.actors actor).2 = after.2 := by
        simpa [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt] using equal
      exact False.elim (by omega)
    · simp [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt,
        same, Ne.symm same, committedFrameClassWord]

private theorem an_unchanged_actor_stamp_has_no_intervening_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records) (rows : List Nat) :
    (first.1.actors actor).2 ≤ (last.1.actors actor).2 ∧
      ((first.1.actors actor).2 = (last.1.actors actor).2 →
        committedFrameClassWord rows actor records = []) := by
  induction execution with
  | refl => exact ⟨Nat.le_refl _, fun _ => rfl⟩
  | @tail middle finish prior logged history step ih =>
    have boundary := a_source_boundary_only_records_a_changing_actor (actor := actor) step rows
    refine ⟨Nat.le_trans ih.1 boundary.1, ?_⟩
    intro equal
    have earlier : (first.1.actors actor).2 = (middle.1.actors actor).2 := by omega
    have later : (middle.1.actors actor).2 = (finish.1.actors actor).2 := by omega
    simp only [committedFrameClassWord, List.filterMap_append, List.nil_append,
      show prior.filterMap (fun (owner, record) =>
        if owner == actor && rows.contains record.step.event then some record.step.event else none) = [] from ih.2 earlier,
      show logged.toList.filterMap (fun (owner, record) =>
        if owner == actor && rows.contains record.step.event then some record.step.event else none) = [] from boundary.2 later]

private theorem a_boundary_retains_or_appends_its_accepted_entry
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)}
    (step : DistributedSourceStep images choreo session first last logged) :
    last.2 = first.2 ∨ ∃ entry, logged = none ∧ last.2 = first.2 ++ [entry] := by
  cases step with
  | quiet checked => exact .inl rfl
  | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    exact .inr ⟨_, rfl, rfl⟩
  | publish member stamp paired committed => exact .inl rfl
  | localCommit actorRole localAt available committed => exact .inl rfl
  | receive actorRole received entryAt identity paired available committed => exact .inl rfl

-- Keep the actual retained record split as well as both boundary histories.
private theorem an_acceptance_origin_preserves_the_same_record_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {entry : SourceAcceptance}
    (execution : DistributedSourceExecution images choreo session first last records)
    (absent : entry ∉ first.2) (present : entry ∈ last.2) :
    ∃ before after prior later,
      DistributedSourceExecution images choreo session first before prior ∧
      DistributedSourceStep images choreo session before after none ∧
      after.2 = before.2 ++ [entry] ∧
      DistributedSourceExecution images choreo session after last later ∧ records = prior ++ later := by
  induction execution with
  | refl => exact False.elim (absent present)
  | @tail middle last records logged history step ih =>
    rcases a_boundary_retains_or_appends_its_accepted_entry step with same | ⟨current, noCommit, appended⟩
    · obtain ⟨before, after, prior, later, pre, boundary, atEntry, suffix, splitAt⟩ := ih (same ▸ present)
      refine ⟨before, after, prior, later ++ logged.toList, pre, boundary, atEntry, .tail suffix step, ?_⟩
      simpa only [List.append_assoc] using congrArg (fun word => word ++ logged.toList) splitAt
    · rw [appended] at present
      rcases List.mem_append.mp present with previous | currentMember
      · obtain ⟨before, after, prior, later, pre, boundary, atEntry, suffix, splitAt⟩ := ih previous
        refine ⟨before, after, prior, later ++ logged.toList, pre, boundary, atEntry, .tail suffix step, ?_⟩
        simpa only [List.append_assoc] using congrArg (fun word => word ++ logged.toList) splitAt
      · have equal : entry = current := by simpa only [List.mem_singleton] using currentMember
        subst current
        exact ⟨middle, last, records, [], history, noCommit ▸ step, appended, .refl, by simp [noCommit]⟩

/-- An actual unconsumed acceptance extends the current measured local source
word, even after arbitrary foreign-actor work and receipts. Its original
prepared base comes from this same execution. The existing mutation stamp
proves that its sender has not published another local step in between.

No successful sender/receiver snapshot, common visit, publication barrier,
new pending field or callable-value assumption is used. Filtering concerns
only this immutable source group; other local operations remain in the log. -/
theorem a_pending_acceptance_extends_its_current_actor_source_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)} {entry : SourceAcceptance}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initialEntries : first.2 = [])
    (initial : ∀ event ∈ group.rows, (first.1.actors actor).1.doneState.done event = false)
    (present : entry ∈ last.2) (stamp : entry.stamps actor (last.1.actors actor).2) :
    sourceFrameWord group.rows (committedFrameClassWord group.rows actor records ++
      if entry.sample.event ∈ group.rows then [entry.sample.event] else []) ∧
      (entry.sample.event ∈ group.rows →
        group.rows[(committedFrameClassWord group.rows actor records).length % group.rows.length]? =
          some entry.sample.event) := by
  by_cases inGroup : entry.sample.event ∈ group.rows
  · obtain ⟨before, after, prior, later, history, boundary, appended, suffix, splitAt⟩ :=
      an_acceptance_origin_preserves_the_same_record_word execution (by simp [initialEntries]) present
    cases boundary with
    | @quiet cut entries changed next checked =>
      have lengths := congrArg List.length appended
      simp only [List.length_append, List.length_singleton] at lengths
      exact False.elim (by omega)
    | @accept cut entries sample channel sent event previousPath witness label data path row
        imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
      have entryEqual : (⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩ : SourceAcceptance) = entry := by
        have same := congrArg (fun values : List SourceAcceptance => values[entries.length]?) appended
        simpa using same
      subst entry
      have sameActor : sample.image.role = actor := stamp.1
      have unchanged : ((cut.carrierAt channel sent).actors actor).2 = (last.1.actors actor).2 := by
        simpa only [DistributedSourceCut.carrierAt, sameActor] using stamp.2
      have noLater := (an_unchanged_actor_stamp_has_no_intervening_word (actor := actor) suffix group.rows).2 unchanged
      have sameWord : committedFrameClassWord group.rows actor records =
          committedFrameClassWord group.rows actor prior := by
        rw [splitAt]
        simpa only [committedFrameClassWord, List.filterMap_append, List.append_nil] using
          congrArg (fun word => committedFrameClassWord group.rows actor prior ++ word) noLater
      obtain ⟨actualData, actualPath, actualRow, sourceEntry, actualDecoded, actualPathAt, actualRowAt,
          _, active, _, ready, _⟩ := checked_source_send_has_its_ready_source_row checked
      have sameData := Option.some.inj (actualDecoded.symm.trans decoded)
      subst actualData
      have samePath := Option.some.inj (actualPathAt.symm.trans pathAt)
      subst actualPath
      have sameRow := Option.some.inj (actualRowAt.symm.trans rowAt)
      subst actualRow
      have imageAtActor : sample.image = images actor := by simpa only [sameActor] using imageAt
      rw [sameActor, imageAtActor] at prepared
      rw [imageAtActor] at decoded
      have next := a_prepared_wire_operation_extends_its_actors_measured_source_word
        (actor := actor) (group := group) history initial decoded pathAt rowAt active ready prepared inGroup
      refine ⟨?_, fun _ => ?_⟩
      · simpa only [if_pos inGroup, sameWord] using next.1
      · simpa only [sameWord] using next.2
  · have ordered := (a_measured_distributed_class_has_its_source_word_and_exact_prefix
      (actor := actor) (group := group) execution initial).1
    refine ⟨by simpa only [if_neg inGroup, List.append_nil] using ordered, ?_⟩
    intro impossible
    exact False.elim (inGroup impossible)

end Hibana
