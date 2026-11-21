import Hibana.DistributedCallExecution
import Hibana.ResourceHeaderTransfer

namespace Hibana.TransportWait

/-- Keep the acceptance and sender publication of one carrier call. A receiver
may already have consumed its frame while its sender is still unpublished. -/
def forCall (key : TransportChannel × Nat) (boundary : DistributedCallBoundary) :
    Option SourceCallBoundary :=
  match boundary.call? with
  | some (.accept entry) => if entry.callKey = key then some (.accept entry) else none
  | some (.publish found record) => if found = key then some (.publish key record) else none
  | none => none

/-- Possible observations of one carrier key in a finite physical prefix.
The indices are the existing acceptance, publication and outstanding-call
inventories and the chronological boundaries for this key. -/
inductive CallPrefix (key : TransportChannel × Nat) : List SourceAcceptance →
    List (SourceAcceptance × ActorCommitRecord) → List SourceAcceptance → List SourceCallBoundary → Prop where
  | fresh : CallPrefix key [] [] [] []
  | outstanding (entry : SourceAcceptance) : CallPrefix key [entry] [] [entry] [.accept entry]
  | published (entry : SourceAcceptance) (record : ActorCommitRecord) :
      CallPrefix key [entry] [(entry, record)] [] [.accept entry, .publish key record]

private theorem unique_middle {before after : List α} {value target : α}
    (single : before ++ value :: after = [target]) :
    before = [] ∧ value = target ∧ after = [] := by
  cases before with
  | nil => simpa using single
  | cons head tail => simp at single

/-- Arbitrarily many unrelated calls, remote receipts, quiet observations,
retirements and transport failures may intervene. A physical acceptance stays
outstanding until the sole publication of that very key. Neither source
readiness, a successful poll, nor eventual transport progress is assumed. -/
theorem physical_send_call_prefix
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending)
    (key : TransportChannel × Nat) :
    CallPrefix key (entries.filter (fun e => decide (e.callKey = key)))
      (published.filter (fun p => decide (p.1.callKey = key)))
      (pending.filter (fun e => decide (e.callKey = key))) (word.filterMap (forCall key)) := by
  induction execution with
  | refl => exact .fresh
  | quiet _ _ _ ih | retireView _ _ _ ih | transportFault _ _ _ ih
  | close _ ih | abort _ ih | observeClosure _ _ _ _ _ ih
  | localCommit _ _ _ _ ih | receive _ _ _ _ _ _ _ ih =>
    simpa only [List.filterMap_append, List.filterMap_cons, List.filterMap_nil, forCall,
      DistributedCallBoundary.call?, List.append_nil] using ih
  | @accept cut word entries published pending sample channel sent event previousPath witness label
      data path row history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
    let entry : SourceAcceptance := ⟨sample, channel, event, (cut.actors sample.image.role).2, witness⟩
    change CallPrefix _ ((entries ++ [entry]).filter _) _ ((pending ++ [entry]).filter _)
      ((word ++ [DistributedCallBoundary.accept entry]).filterMap (forCall key))
    by_cases same : entry.callKey = key
    · have empty : entries.filter (fun e => decide (e.callKey = key)) = [] := by
        apply List.filter_eq_nil_iff.mpr
        intro old member atKey
        have equal : old.callKey = entry.callKey := (of_decide_eq_true atKey).trans same.symm
        have fields : old.channel = channel ∧ old.carrierWitness = witness := Prod.mk.inj equal
        have different := List.all_eq_true.mp freshWitness old member
        simp [fields] at different
      rw [empty] at ih
      generalize pe : published.filter (fun p => decide (p.1.callKey = key)) = publishedEntries at ih
      generalize he : pending.filter (fun e => decide (e.callKey = key)) = waiting at ih
      generalize we : word.filterMap (forCall key) = observed at ih
      cases ih
      simp only [List.filter_append, List.filter_cons, same, decide_true, List.filter_nil,
        List.filterMap_append, List.filterMap_cons, List.filterMap_nil, forCall,
        DistributedCallBoundary.call?, if_pos, List.nil_append, empty, he, we]
      exact .outstanding entry
    · simpa [List.filter_append, List.filterMap_append, forCall, DistributedCallBoundary.call?, same] using ih
  | @publish cut word entries published left right actor after cleared record entry history actorAt sameWire committed ih =>
    by_cases same : entry.callKey = key
    · generalize ae : entries.filter (fun e => decide (e.callKey = key)) = acceptedEntries at ih ⊢
      generalize pe : published.filter (fun p => decide (p.1.callKey = key)) = publishedEntries at ih
      generalize we : word.filterMap (forCall key) = observed at ih
      generalize he : (left ++ entry :: right).filter (fun e => decide (e.callKey = key)) = waiting at ih
      simp only [List.filter_append, List.filter_cons, same, decide_true, if_true] at he
      cases ih with
      | fresh => simp at he
      | published => simp at he
      | outstanding old =>
        obtain ⟨l, rfl, r⟩ := unique_middle he
        simp only [List.filter_append, List.filter_cons, List.filter_nil, List.filterMap_append,
          List.filterMap_cons, List.filterMap_nil, forCall, DistributedCallBoundary.call?, same,
          decide_true, if_true, pe, we, l, r, List.nil_append, List.cons_append]
        exact .published _ record
    · simpa [List.filter_append, List.filter_cons, List.filterMap_append, forCall,
        DistributedCallBoundary.call?, same] using ih

#print axioms physical_send_call_prefix

private theorem prefix_and_last {p : DistributedCallBoundary → Bool}
    (checked : (word ++ [boundary]).all p = true) : word.all p = true ∧ p boundary = true := by
  simpa only [List.all_append, List.all_cons, List.all_nil, Bool.and_true,
    Bool.and_eq_true] using checked

private theorem actor_update_preserves_choices (cut : DistributedSourceCut) (changed actor : Nat)
    (after : ActorProgressObservation × Nat)
    (preserved : after.1.choices = (cut.actors changed).1.choices) :
    ((cut.actorAt changed after).actors actor).1.choices = (cut.actors actor).1.choices := by
  by_cases same : actor = changed
  · simpa [DistributedSourceCut.actorAt, same] using preserved
  · simp [DistributedSourceCut.actorAt, same]

private theorem fault_preserves_choices {before after : ActorProgressObservation}
    (checked : actorFaultProgressMatches before after = true) : after.choices = before.choices :=
  of_decide_eq_true (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).2

/-- A physical carrier acceptance, publication on a peer, or remote receipt
cannot itself teach this actor a route choice. The prefix permits local quiet,
closure, fault, and retirement observations; their checked updates preserve
committed choices. No ordering of a sender publication against receipt is used. -/
theorem remote_commits_preserve_local_choices
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending)
    (actor : Nat)
    (remote : word.all (fun b => b.record?.all (fun r => decide (r.1 ≠ actor))) = true) :
    (last.actors actor).1.choices = (first.actors actor).1.choices := by
  induction execution with
  | refl => rfl
  | close _ ih | abort _ ih
  | accept _ _ _ _ _ _ _ _ _ _ _ _ ih => exact ih (prefix_and_last remote).1
  | @quiet cut word entries published pending changed after history checked headers ih =>
    exact (actor_update_preserves_choices cut changed actor _
      (a_zero_write_measured_gap_preserves_committed_choices checked)).trans (ih (prefix_and_last remote).1)
  | @transportFault cut word entries published pending sample history checked beforeAt ih
  | @observeClosure cut word entries published pending sample history checked beforeAt _ _ ih =>
    have aligned : sample.after.choices = sample.before.choices := fault_preserves_choices
      (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).1
    exact (actor_update_preserves_choices cut sample.before.actor actor _
      (aligned.trans (congrArg ActorProgressObservation.choices beforeAt.symm))).trans
        (ih (prefix_and_last remote).1)
  | @retireView cut word entries published pending sample history checked beforeAt ih =>
    obtain ⟨_, _, _, _, progress, _, _⟩ := accepted_retirement_has_checked_source_and_headers checked
    have aligned : sample.after.choices = sample.before.choices :=
      of_decide_eq_true (Bool.and_eq_true_iff.mp progress).2
    exact (actor_update_preserves_choices cut sample.before.actor actor _
      (aligned.trans (congrArg ActorProgressObservation.choices beforeAt.symm))).trans
        (ih (prefix_and_last remote).1)
  | @publish cut word entries published left right changed after cleared record entry _ _ _ _ ih
  | @localCommit cut word entries published pending changed after cleared record eff binding _ _ _ _ ih
  | @receive cut word entries published pending changed channel next frame after cleared record entry witness _ _ _ _ _ _ _ ih =>
    obtain ⟨earlier, here⟩ := prefix_and_last remote
    have foreign : changed ≠ actor := of_decide_eq_true here
    simpa only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg (Ne.symm foreign)] using ih earlier

/-- A change in the observed branch slots has an actual commit at this actor
in the same word, even if other actors published or received in the meantime. -/
theorem changed_choices_have_a_local_commit
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending)
    (actor : Nat) (changed : (last.actors actor).1.choices ≠ (first.actors actor).1.choices) :
    ∃ boundary ∈ word, ∃ record, boundary.record? = some (actor, record) := by
  have failed := Bool.eq_false_iff.mpr (fun all => changed (remote_commits_preserve_local_choices execution actor all))
  obtain ⟨boundary, present, localRecord⟩ := List.all_eq_false.mp failed
  cases atRecord : boundary.record? with
  | none => simp [atRecord] at localRecord
  | some pair =>
    obtain ⟨who, record⟩ := pair
    have same : who = actor := by simpa [atRecord] using localRecord
    exact ⟨boundary, present, record, same ▸ atRecord⟩

#print axioms remote_commits_preserve_local_choices
#print axioms changed_choices_have_a_local_commit

/-- The physical boundaries that may issue or retire a header on this actor.
A carrier acceptance alone has no actor update. Quiet, closure and fault
observations have checked header preservation. -/
def localHeaderBoundary (actor : Nat) : DistributedCallBoundary → Bool
  | .retireView sample => decide (sample.before.actor = actor)
  | .publish _ other _ | .receive _ other _ | .localCommit other _ => decide (other = actor)
  | _ => false

private theorem tag_after_update (cut : DistributedSourceCut) (changed actor role : Nat)
    (after : ActorProgressObservation × Nat)
    (sameTag : observedResourceTag? after.1 role = observedResourceTag? (cut.actors changed).1 role) :
    observedResourceTag? ((cut.actorAt changed after).actors actor).1 role =
      observedResourceTag? (cut.actors actor).1 role := by
  by_cases same : actor = changed
  · simpa [DistributedSourceCut.actorAt, same] using sameTag
  · simp [DistributedSourceCut.actorAt, same]

private theorem checked_quiet_preserves_tag {image : RustDescriptorImage} {choreo : Choreo}
    {session counter : Nat} {before after : ActorProgressObservation}
    (checked : actorCommitMutationHistoryCheck image choreo session before counter after counter [] = true)
    (headers : actorQuietHeadersCheck before after = true) (role : Nat) :
    observedResourceTag? after role = observedResourceTag? before role := by
  have gap : actorCommitGapCheck before after = true := (Bool.and_eq_true_iff.mp
    (Bool.and_eq_true_iff.mp checked).1).1
  simp only [actorCommitGapCheck, Bool.and_eq_true, beq_iff_eq] at gap
  simp only [actorQuietHeadersCheck, Bool.and_eq_true, beq_iff_eq] at headers
  exact unchanged_headers_preserve_resource_tags gap.1.1.symm gap.1.2.symm headers.1.1.1.symm role

private theorem checked_fault_preserves_tag {before after : ActorProgressObservation}
    (checked : actorFaultProgressMatches before after = true) (role : Nat) :
    observedResourceTag? after role = observedResourceTag? before role := by
  simp only [actorFaultProgressMatches, Bool.and_eq_true, beq_iff_eq, decide_eq_true_eq] at checked
  exact unchanged_headers_preserve_resource_tags checked.1.1.1.1.1.1 checked.1.1.1.1.1.2 checked.2 role

private theorem latest_retirement_has_no_tag
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending)
    (retired : ViewRetirementObservation)
    (latest : (word.filter (localHeaderBoundary retired.before.actor)).getLast? =
      some (.retireView retired)) :
    observedResourceTag? (last.actors retired.before.actor).1 retired.view = none := by
  induction execution with
  | refl => simp at latest
  | close _ ih | abort _ ih | accept _ _ _ _ _ _ _ _ _ _ _ _ ih =>
    apply ih
    simpa [List.filter_append, localHeaderBoundary] using latest
  | @quiet cut word entries published pending changed after history checked headers ih =>
    apply (tag_after_update cut changed retired.before.actor retired.view _
      (checked_quiet_preserves_tag checked headers retired.view)).trans
    apply ih
    simpa [List.filter_append, localHeaderBoundary] using latest
  | @transportFault cut word entries published pending sample history checked beforeAt ih
  | @observeClosure cut word entries published pending sample history checked beforeAt _ _ ih =>
    have sameTag := checked_fault_preserves_tag
      (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).1 retired.view
    rw [← beforeAt] at sameTag
    apply (tag_after_update cut sample.before.actor retired.before.actor retired.view _ sameTag).trans
    apply ih
    simpa [List.filter_append, localHeaderBoundary] using latest
  | @retireView cut word entries published pending sample history checked beforeAt ih =>
    by_cases same : sample.before.actor = retired.before.actor
    · have sameSample : sample = retired := by
        simpa [List.filter_append, localHeaderBoundary, same] using latest
      subst sample
      obtain ⟨_, _, _, gone, _⟩ := checked_retirement_invalidates_its_exact_observed_tag checked
      simpa [DistributedSourceCut.actorAt] using gone
    · simp only [DistributedSourceCut.actorAt, if_neg (Ne.symm same)]
      apply ih
      simpa [List.filter_append, localHeaderBoundary, same] using latest
  | @publish cut word entries published left right changed after cleared record entry _ _ _ _ ih
  | @localCommit cut word entries published pending changed after cleared record eff binding _ _ _ _ ih
  | @receive cut word entries published pending changed channel next frame after cleared record entry witness _ _ _ _ _ _ _ ih =>
    by_cases same : changed = retired.before.actor
    · simp [List.filter_append, localHeaderBoundary, same] at latest
    · simp only [DistributedSourceCut.carrierAt, DistributedSourceCut.actorAt, if_neg (Ne.symm same)]
      apply ih
      simpa [List.filter_append, localHeaderBoundary, same] using latest

/-- Locate the checked retirement in the full replay, so the suffix retains
pending calls and carrier frames accepted before this view was dropped. -/
theorem a_retired_local_entry_stays_absent_after_remote_activity
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending)
    (retired : ViewRetirementObservation) {earlier suffix : List DistributedCallBoundary}
    (history : word = earlier ++ .retireView retired :: suffix)
    (remote : suffix.all (fun b => !localHeaderBoundary retired.before.actor b) = true) :
    observedResourceTag? (last.actors retired.before.actor).1 retired.view = none := by
  apply latest_retirement_has_no_tag execution retired
  have empty : suffix.filter (localHeaderBoundary retired.before.actor) = [] := by
    apply List.filter_eq_nil_iff.mpr
    intro b member
    simpa using List.all_eq_true.mp remote b member
  simp [history, List.filter_append, localHeaderBoundary, empty]

/-- A source-required local entry needs its issued header at the renewed cut.
For production the entry must belong to the restarted portion of the roll:
an independently preserved child keeps its own visit and entry ownership. -/
theorem a_retired_required_entry_precludes_a_checked_reentry
    (execution : DistributedPhysicalExecution images choreo session first last word entries published pending)
    (retired : ViewRetirementObservation) {earlier suffix : List DistributedCallBoundary}
    (history : word = earlier ++ .retireView retired :: suffix)
    (remote : suffix.all (fun b => !localHeaderBoundary retired.before.actor b) = true)
    (candidate : ActorReentryCommitObservation)
    (cut : candidate.before = (last.actors retired.before.actor).1)
    {columns : DecodedProgramColumns} {rows : OwnedEntryResources} {reference : Nat}
    {roll : DecodedRollRow} {required : List (Nat × Nat)}
    (columnsAt : (images retired.before.actor).decodeProgramColumns? = some columns)
    (entryTable : (images retired.before.actor).ownedEntryResourcesCheck = true)
    (expected : (images retired.before.actor).expectedRollEntryResources? = some rows)
    (scopeAt : resolveUnique? (rows.filter fun row => row.1.scope == candidate.scope) =
      some (roll, required))
    (member : (retired.view, reference) ∈ required)
    (resolved : columns.selectedActor? candidate.before.doneState.selected reference =
      some candidate.before.actor) :
    candidate.check (images retired.before.actor) choreo = false := by
  have gone := a_retired_local_entry_stays_absent_after_remote_activity execution retired history remote
  rw [← cut] at gone
  obtain ⟨rows', expected', decoded⟩ := accepted_owned_entry_resources_match_source entryTable
  have same : rows' = rows := Option.some.inj (expected'.symm.trans expected)
  subst rows'
  apply Bool.eq_false_iff.mpr
  intro accepted
  obtain ⟨view, found, run, odd⟩ :=
    accepted_body_reentry_uses_issued_entry_resource accepted columnsAt decoded scopeAt member resolved
  have issued : candidate.before.viewIssued retired.view = true := by
    simp [ActorProgressObservation.viewIssued, found, run, odd]
  obtain ⟨_, _, _, _, tag⟩ := an_issued_resource_header_has_its_exact_observed_tag issued
  rw [gone] at tag
  contradiction

#print axioms a_retired_local_entry_stays_absent_after_remote_activity
#print axioms a_retired_required_entry_precludes_a_checked_reentry

end Hibana.TransportWait
