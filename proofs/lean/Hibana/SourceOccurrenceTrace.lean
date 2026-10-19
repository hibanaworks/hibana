import Hibana.SourceOccurrenceProjection
import Hibana.OwnedCallHistory
import Hibana.ActorCandidateAdmission

namespace Hibana
open EndpointDelegation

/-- Erased acceptance-to-occurrence indices. Keys are the actual carrier call
keys used by DistributedCallExecution, not payload bytes or a runtime epoch. -/
abbrev OccurrenceCalls := List ((TransportChannel × Nat) × Nat)

def occurrenceCall? (calls : OccurrenceCalls) (key : TransportChannel × Nat) : Option Nat :=
  (calls.find? fun pair => decide (pair.1 = key)).map Prod.snd

/-- Match one observed operation to a static message origin and one occurrence
in the accepted unfolding. Physical actor, logical peers, payload kind, label
and direction all come from the same source binding. -/
def SourceOccurrenceCut.matchesAction (plan : SourceOccurrenceCut source)
    (image : RustDescriptorImage) (choreo : Choreo) (actor pathIndex row occurrence : Nat)
    (side : Side) : Bool :=
  match (plan.unfolding.occurrences 0)[occurrence]?,
      plan.path.bindings.find? (fun entry => entry.1 == occurrence),
      sourceOperationIdentity? image choreo pathIndex row,
      action? plan.path.bindings ⟨occurrence, side⟩ with
  | some (origin, message), some (_, expected), some (eff, actual, actualSide), some action =>
      decide (origin = eff ∧ message = expected.message ∧ actual = expected ∧
        actualSide = side ∧ action.actor = actor)
  | _, _, _, _ => false

/-- Logical source execution. These points are the same acceptance,
publication and receipt boundaries used by the physical call history. -/
inductive SourceOccurrenceExecution (first : SourceOccurrenceCut source) :
    SourceOccurrenceCut source → List SourcePoint → Prop where
  | nil : SourceOccurrenceExecution first first []
  | tail : SourceOccurrenceExecution first before points →
      SourceOccurrenceStep before point after →
      SourceOccurrenceExecution first after (points ++ [point])

private theorem occurrence_cut_append (plan : SourceOccurrenceCut source)
    (points : List SourcePoint) (point : SourcePoint) :
    occurrenceCut plan (points ++ [point]) =
      { occurrenceCut plan points with observed := fun candidate =>
        if candidate = point then true else (occurrenceCut plan points).observed candidate } := by
  have observed : (fun candidate => (points ++ [point]).contains candidate) =
      (fun candidate => if candidate = point then true else points.contains candidate) := by
    funext candidate
    by_cases same : candidate = point
    · subst candidate
      simp only [List.contains_append, List.contains_cons, List.contains_nil,
        beq_self_eq_true, Bool.or_false, Bool.or_true, ite_true]
    · have different : (candidate == point) = false := by simpa using same
      simp only [List.contains_append, List.contains_cons, List.contains_nil,
        different, Bool.or_false, if_neg same]
  exact congrArg (fun value : SourcePoint → Bool =>
    ({ plan with observed := value } : SourceOccurrenceCut source)) observed

theorem a_checked_occurrence_call_appends_exactly_its_source_point
    (plan : SourceOccurrenceCut source) (points : List SourcePoint) (point : SourcePoint)
    (checked : (occurrenceCut plan points).ready point = true) :
    SourceOccurrenceStep (occurrenceCut plan points) point
      (occurrenceCut plan (points ++ [point])) := by
  obtain ⟨next, step⟩ := a_checked_source_occurrence_has_its_exact_transition _ _ checked
  cases step with
  | advance present pending ready =>
    rw [occurrence_cut_append]
    exact .advance point present pending ready

/-- A complete source-word refinement, with no second independently selected
observation cut. Ordinary frames are included. Receipts and publications must
resolve the exact earlier acceptance key, so equal rolled envelopes cannot
select another occurrence.

Each step checks only its actor's projected local order. Publication and receipt
resolve the earlier accepted call; they do not assume global source readiness.
The source simulation is derived below. This is an erased protocol model, not
a runtime scheduler or another admission guard. Relating every production
history to the projected model remains an implementation refinement obligation;
no future call or application Rust value is assumed. -/
inductive SourceOccurrenceTrace (images : Nat → RustDescriptorImage) (choreo : Choreo)
    (plan : SourceOccurrenceCut source) :
    List DistributedCallBoundary → OccurrenceCalls → List SourcePoint → Prop where
  | nil (sameSource : choreo.ownershipSource? = some source) :
      SourceOccurrenceTrace images choreo plan [] [] []
  | quiet {word calls points actor}
      (prior : SourceOccurrenceTrace images choreo plan word calls points) :
      SourceOccurrenceTrace images choreo plan (word ++ [.quiet actor]) calls points
  | accept {word calls points} {entry : SourceAcceptance} {occurrence : Nat}
      (prior : SourceOccurrenceTrace images choreo plan word calls points)
      (fresh : calls.all (fun pair => decide (pair.1 ≠ entry.callKey)) = true)
      (origin : ((plan.unfolding.occurrences 0)[occurrence]?).map Prod.fst = some entry.event)
      (matched : plan.matchesAction entry.sample.image choreo entry.sample.image.role
        entry.sample.pathIndex entry.sample.event occurrence .send = true)
      (ready : occurrenceLocalReady plan entry.sample.image.role
        (occurrenceLocalHistory plan entry.sample.image.role points) (.accepted occurrence) = true) :
      SourceOccurrenceTrace images choreo plan (word ++ [.accept entry])
        (calls ++ [(entry.callKey, occurrence)]) (points ++ [.accepted occurrence])
  | publish {word calls points key actor occurrence} {record : ActorCommitRecord}
      (prior : SourceOccurrenceTrace images choreo plan word calls points)
      (callAt : occurrenceCall? calls key = some occurrence)
      (matched : plan.matchesAction (images actor) choreo actor
        record.step.pathIndex record.step.event occurrence .send = true)
      (ready : occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points)
        (.committed ⟨occurrence, .send⟩) = true) :
      SourceOccurrenceTrace images choreo plan (word ++ [.publish key actor record]) calls
        (points ++ [.committed ⟨occurrence, .send⟩])
  | receive {word calls points key actor occurrence} {record : ActorCommitRecord}
      (prior : SourceOccurrenceTrace images choreo plan word calls points)
      (callAt : occurrenceCall? calls key = some occurrence)
      (matched : plan.matchesAction (images actor) choreo actor
        record.step.pathIndex record.step.event occurrence .receive = true)
      (ready : occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points)
        (.committed ⟨occurrence, .receive⟩) = true) :
      SourceOccurrenceTrace images choreo plan (word ++ [.receive key actor record]) calls
        (points ++ [.committed ⟨occurrence, .receive⟩])
  | localCommit {word calls points actor occurrence} {record : ActorCommitRecord}
      (prior : SourceOccurrenceTrace images choreo plan word calls points)
      (matched : plan.matchesAction (images actor) choreo actor
        record.step.pathIndex record.step.event occurrence .local = true)
      (ready : occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points)
        (.committed ⟨occurrence, .local⟩) = true) :
      SourceOccurrenceTrace images choreo plan (word ++ [.localCommit actor record]) calls
        (points ++ [.committed ⟨occurrence, .local⟩])

private theorem trace_calls_have_observed_acceptances
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    ∀ pair ∈ calls, SourcePoint.accepted pair.2 ∈ points := by
  induction trace with
  | nil => simp
  | @accept word calls points entry occurrence prior fresh origin matched ready ih =>
    intro pair member
    rcases List.mem_append.mp member with earlier | current
    · exact List.mem_append_left _ (ih pair earlier)
    · have same := List.mem_singleton.mp current
      subst pair
      exact List.mem_append_right _ (by simp)
  | quiet prior ih => exact ih
  | publish prior callAt matched ready ih
  | receive prior callAt matched ready ih
  | localCommit prior matched ready ih =>
    intro pair member
    exact List.mem_append_left _ (ih pair member)

private theorem a_resolved_call_has_actual_acceptance_evidence
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (found : occurrenceCall? calls key = some occurrence) :
    points.contains (SourcePoint.accepted occurrence) = true := by
  unfold occurrenceCall? at found
  cases atPair : calls.find? (fun pair => decide (pair.1 = key)) with
  | none => simp only [atPair, Option.map_none, reduceCtorEq] at found
  | some pair =>
    have same : pair.2 = occurrence := Option.some.inj (by simpa only [atPair, Option.map_some] using found)
    have present := trace_calls_have_observed_acceptances trace pair (List.mem_of_find?_eq_some atPair)
    simpa only [same, List.contains_iff_mem] using present

/-- Derive every intermediate source state from projected local steps and
actual earlier acceptance. There is no global-ready premise, independent final
Boolean snapshot, common actor visit, or desired next Rust call. -/
theorem a_source_occurrence_trace_is_its_reconstructed_execution
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    SourceOccurrenceExecution (occurrenceCut plan []) (occurrenceCut plan points) points := by
  induction trace with
  | nil => exact .nil
  | quiet prior ih => exact ih
  | accept prior fresh origin matched ready ih =>
    exact .tail ih (a_checked_occurrence_call_appends_exactly_its_source_point _ _ _
      ((projected_occurrence_readiness_is_exact _ _ _).mpr ⟨_, ready, rfl⟩))
  | publish prior callAt matched ready ih =>
    exact .tail ih (a_checked_occurrence_call_appends_exactly_its_source_point _ _ _
      ((projected_occurrence_readiness_is_exact _ _ _).mpr
        ⟨_, ready, a_resolved_call_has_actual_acceptance_evidence prior callAt⟩))
  | receive prior callAt matched ready ih =>
    exact .tail ih (a_checked_occurrence_call_appends_exactly_its_source_point _ _ _
      ((projected_occurrence_readiness_is_exact _ _ _).mpr
        ⟨_, ready, a_resolved_call_has_actual_acceptance_evidence prior callAt⟩))
  | localCommit prior matched ready ih =>
    exact .tail ih (a_checked_occurrence_call_appends_exactly_its_source_point _ _ _
      ((projected_occurrence_readiness_is_exact _ _ _).mpr ⟨_, ready, rfl⟩))

private theorem an_occurrence_call_retains_its_acceptance
    (trace : SourceOccurrenceTrace images choreo plan word calls points) :
    ∀ pair ∈ calls, ∃ earlier later entry,
      word = earlier ++ DistributedCallBoundary.accept entry :: later ∧
      entry.callKey = pair.1 ∧
      ((plan.unfolding.occurrences 0)[pair.2]?).map Prod.fst = some entry.event ∧
      plan.matchesAction entry.sample.image choreo entry.sample.image.role
        entry.sample.pathIndex entry.sample.event pair.2 .send = true := by
  induction trace with
  | nil => simp
  | @accept word calls points entry occurrence prior fresh origin matched ready ih =>
    intro pair present
    rcases List.mem_append.mp present with old | current
    · obtain ⟨earlier, later, accepted, wordAt, key, sourceAt, matchAt⟩ := ih pair old
      refine ⟨earlier, later ++ [.accept entry], accepted, ?_, key, sourceAt, matchAt⟩
      simp only [wordAt, List.append_assoc, List.cons_append]
    · have same := List.mem_singleton.mp current
      subst pair
      exact ⟨word, [], entry, by simp, rfl, origin, matched⟩
  | @quiet word calls points actor prior ih =>
    intro pair present
    obtain ⟨earlier, later, entry, wordAt, key, sourceAt, matchAt⟩ := ih pair present
    refine ⟨earlier, later ++ [.quiet actor], entry, ?_, key, sourceAt, matchAt⟩
    simp only [wordAt, List.append_assoc, List.cons_append]
  | @publish word calls points call actor occurrence record prior callAt matched ready ih =>
    intro pair present
    obtain ⟨earlier, later, entry, wordAt, key, sourceAt, matchAt⟩ := ih pair present
    refine ⟨earlier, later ++ [.publish call actor record], entry, ?_, key, sourceAt, matchAt⟩
    simp only [wordAt, List.append_assoc, List.cons_append]
  | @receive word calls points call actor occurrence record prior callAt matched ready ih =>
    intro pair present
    obtain ⟨earlier, later, entry, wordAt, key, sourceAt, matchAt⟩ := ih pair present
    refine ⟨earlier, later ++ [.receive call actor record], entry, ?_, key, sourceAt, matchAt⟩
    simp only [wordAt, List.append_assoc, List.cons_append]
  | @localCommit word calls points actor occurrence record prior matched ready ih =>
    intro pair present
    obtain ⟨earlier, later, entry, wordAt, key, sourceAt, matchAt⟩ := ih pair present
    refine ⟨earlier, later ++ [.localCommit actor record], entry, ?_, key, sourceAt, matchAt⟩
    simp only [wordAt, List.append_assoc, List.cons_append]

/-- An observed receiver/publication resolves an actual earlier acceptance,
and its occurrence has that acceptance's original source ordinal and binding.
The unique key is an erased carrier witness, not a repeated envelope. -/
theorem a_resolved_occurrence_call_has_its_same_word_acceptance
    (trace : SourceOccurrenceTrace images choreo plan word calls points)
    (found : occurrenceCall? calls key = some occurrence) :
    ∃ earlier later entry,
      word = earlier ++ DistributedCallBoundary.accept entry :: later ∧ entry.callKey = key ∧
      ((plan.unfolding.occurrences 0)[occurrence]?).map Prod.fst = some entry.event ∧
      plan.matchesAction entry.sample.image choreo entry.sample.image.role
        entry.sample.pathIndex entry.sample.event occurrence .send = true := by
  unfold occurrenceCall? at found
  cases atPair : calls.find? (fun pair => decide (pair.1 = key)) with
  | none => simp only [atPair, Option.map_none, reduceCtorEq] at found
  | some pair =>
    have indexAt : pair.2 = occurrence := Option.some.inj (by simpa only [atPair, Option.map_some] using found)
    have keyAt : pair.1 = key := of_decide_eq_true (List.find?_some
      (p := fun pair : (TransportChannel × Nat) × Nat => decide (pair.1 = key)) atPair)
    have origin := an_occurrence_call_retains_its_acceptance trace pair (List.mem_of_find?_eq_some atPair)
    simpa only [keyAt, indexAt] using origin

/-- The custody, physical FIFO/call history and reconstructed source cut all
refer to ONE chronological execution. The next operation is enabled by one
actor's projected local history and actual acceptance evidence. This derives
protocol-model progress and affine authority, not complete Rust/kernel or
carrier/fault refinement. No future-existence oracle is required. -/
theorem a_joint_occurrence_refinement_has_custody_fifo_and_source_progress
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {resource : EndpointKey} {value : OwnedCallHistory.Tag}
    {events : List OwnedCallHistory.OwnedCallEvent} {cut : OwnedCallHistory.Cut}
    {source : Source} {plan : SourceOccurrenceCut source}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    {calls : OccurrenceCalls} {points : List SourcePoint}
    (custody : OwnedCallHistory.History images resource choreo value events cut)
    (physical : DistributedCallExecution images choreo session (.initial initial) last
      (OwnedCallHistory.sourceWord events) entries published pending)
    (mapped : SourceOccurrenceTrace images choreo plan
      (OwnedCallHistory.sourceWord events) calls points)
    (unfinished : (occurrenceCut plan points).Unfinished) :
    cut.custody.authorityCount ≤ 1 ∧
      entries.Perm (published.map Prod.fst ++ pending) ∧
      SourceOccurrenceExecution (occurrenceCut plan []) (occurrenceCut plan points) points ∧
      (∀ channel, ∃ history, SendAdmissionTrace choreo session
        (last.carriers channel) history (sourceChannelRecords entries channel)) ∧
      ∃ actor point next,
        occurrenceLocalReady plan actor (occurrenceLocalHistory plan actor points) point = true ∧
        occurrenceWireReady points point = true ∧
        SourceOccurrenceStep (occurrenceCut plan points) point next := by
  obtain ⟨actor, point, next, localChecked, wire, step⟩ :=
    unfinished_source_occurrences_have_a_projected_operation plan points unfinished
  exact ⟨(a_joint_call_history_preserves_affine_custody custody).1,
    (a_distributed_call_execution_partitions_its_actual_acceptances physical).1,
    a_source_occurrence_trace_is_its_reconstructed_execution mapped,
    a_distributed_call_execution_projects_its_complete_fifo_histories physical,
    actor, point, next, localChecked, wire, step⟩

private def occurrenceCandidatePredecessor? (plan : SourceOccurrenceCut source)
    (image : RustDescriptorImage) (choreo : Choreo) (pathIndex : Nat)
    (row : DecodedActorRow) (target : ActionId) : Option ActionId :=
  (sourceVisitActions plan.path.bindings).reverse.find? fun before =>
    plan.matchesAction image choreo image.role pathIndex row.eventId before.event before.side &&
      precedes plan.expanded plan.path.bindings before target

/-- Compare the authentic current/fresh base with the same chronological
occurrence history. The predecessor domain is derived from the complete source
descriptor. For each row the last matching causal occurrence is used, not an
old equal envelope or a caller-supplied subset. No raw admission result or
desired Rust value is assumed by this check. -/
def SourceOccurrenceCut.candidateHistoryMatches (plan : SourceOccurrenceCut source)
    (image : RustDescriptorImage) (choreo : Choreo) (pathIndex : Nat)
    (data : DecodedActorOrder) (path : NormalizedPath) (row : DecodedActorRow)
    (base : ActorProgressObservation) (points : List SourcePoint) (target : ActionId) : Bool :=
  plan.matchesAction image choreo image.role pathIndex row.eventId target.event target.side &&
    !points.contains (.committed target) &&
    decide (base.doneState.done row.eventId = points.contains (.committed target)) &&
    (data.sourcePredecessors path row.eventId).all fun event =>
      match (data.activeRows path).find? (fun prior => prior.eventId == event) with
      | none => false
      | some prior =>
        match occurrenceCandidatePredecessor? plan image choreo pathIndex prior target with
        | none => false
        | some before => decide (base.doneState.done event = points.contains (.committed before))

/-- Lift an enabled occurrence through the actual descriptor's current/fresh
base to its measured raw cursor result. Only past-history correspondence is
checked; source readiness is derived from the projected query and acceptance.
This does not claim a callable future or transport completion. -/
theorem a_projected_occurrence_has_actual_candidate_admission
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {previousPath : Nat} {data : DecodedActorOrder} {path : NormalizedPath}
    {row : DecodedActorRow} {base : ActorProgressObservation}
    {plan : SourceOccurrenceCut source} {points : List SourcePoint} {target : ActionId}
    (checked : sample.candidateCheck image choreo previousPath = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (origin : ActorSourceCandidateBase sample image choreo previousPath path row base)
    (correspondence : plan.candidateHistoryMatches image choreo sample.pathIndex
      data path row base points target = true)
    (localReady : occurrenceLocalReady plan image.role
      (occurrenceLocalHistory plan image.role points) target.entryPoint = true)
    (wire : occurrenceWireReady points target.entryPoint = true) :
    sourceRowReady data path base row = true ∧ sample.admitted[row.eventId]? = some true := by
  have globalReady := (projected_occurrence_readiness_is_exact plan points target.entryPoint).mpr
    ⟨image.role, localReady, wire⟩
  obtain ⟨after, step⟩ := a_checked_source_occurrence_has_its_exact_transition
    (occurrenceCut plan points) target.entryPoint globalReady
  have predecessors : ∀ before, precedes plan.expanded plan.path.bindings before target = true →
      points.contains (.committed before) = true := by
    intro before ordered
    cases step with
    | advance present pending ready => exact ready _ (.ordered ordered)
  unfold SourceOccurrenceCut.candidateHistoryMatches at correspondence
  simp only [Bool.and_eq_true, Bool.not_eq_true', decide_eq_true_eq] at correspondence
  rcases correspondence with ⟨⟨⟨_matched, pending⟩, targetAt⟩, rows⟩
  have ready : sourceRowReady data path base row = true := by
    apply Bool.and_eq_true_iff.mpr
    refine ⟨?_, List.all_eq_true.mpr ?_⟩
    · rw [targetAt, pending]
      rfl
    · intro event present
      have same := List.all_eq_true.mp rows event present
      cases priorAt : (data.activeRows path).find? (fun prior => prior.eventId == event) with
      | none => simp only [priorAt, Bool.false_eq_true] at same
      | some prior =>
        simp only [priorAt] at same
        cases beforeAt : occurrenceCandidatePredecessor? plan image choreo sample.pathIndex prior target with
        | none => simp only [beforeAt, Bool.false_eq_true] at same
        | some before =>
          have doneAt : base.doneState.done event = points.contains (.committed before) := by
            simpa only [beforeAt, decide_eq_true_eq] using same
          unfold occurrenceCandidatePredecessor? at beforeAt
          have predicate := List.find?_some
            (p := fun earlier : ActionId => plan.matchesAction image choreo image.role sample.pathIndex
              prior.eventId earlier.event earlier.side &&
                precedes plan.expanded plan.path.bindings earlier target) beforeAt
          have order := (Bool.and_eq_true_iff.mp predicate).2
          exact doneAt.trans (predecessors before order)
  exact ⟨ready, a_ready_source_candidate_base_has_actual_cursor_admission
    checked decoded pathAt member origin ready⟩

end Hibana
