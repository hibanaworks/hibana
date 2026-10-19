import Hibana.ActorReentryCommit
import Hibana.ActorVisitProgress

namespace Hibana
open EndpointDelegation

/-- Current completion or a previously completed enclosing visit. Only an
incoming operation outside that visit may use the latter. The ranges come
from the global source; no observed raw-ready bit supplies its own evidence. -/
def sourceRetainedRegion? (image : RustDescriptorImage) (choreo : Choreo)
    (retained : List Nat) (row : DecodedActorRow) (predecessor : Nat) : Option DecodedRollRow :=
  if retained.isEmpty then none
  else if row.actionId.side ≠ .receive then none
  else do
    let rolls ← choreo.canonicalActorRollRows? image.role
    rolls.find? fun roll =>
      retained.contains roll.scope && roll.eventStart ≤ predecessor &&
        predecessor < roll.eventStart + roll.eventLength &&
        !(roll.eventStart ≤ row.eventId && row.eventId < roll.eventStart + roll.eventLength)

def sourceCandidatePredecessorDone (image : RustDescriptorImage) (choreo : Choreo)
    (retained : List Nat) (before : ActorProgressObservation)
    (row : DecodedActorRow) (predecessor : Nat) : Bool :=
  before.doneState.done predecessor || (sourceRetainedRegion? image choreo retained row predecessor).isSome

def sourceCandidateRowReady (image : RustDescriptorImage) (choreo : Choreo)
    (retained : List Nat) (data : DecodedActorOrder) (path : NormalizedPath)
    (before : ActorProgressObservation) (row : DecodedActorRow) : Bool :=
  !before.doneState.done row.eventId &&
    (data.sourcePredecessors path row.eventId).all
      (sourceCandidatePredecessorDone image choreo retained before row)

theorem source_candidate_without_retained_completion_is_current
    (image : RustDescriptorImage) (choreo : Choreo) (data : DecodedActorOrder)
    (path : NormalizedPath) (before : ActorProgressObservation) (row : DecodedActorRow) :
    sourceCandidateRowReady image choreo [] data path before row =
      sourceRowReady data path before row := by
  unfold sourceCandidateRowReady sourceRowReady
  apply congrArg (fun result => !before.doneState.done row.eventId && result)
  induction data.sourcePredecessors path row.eventId with
  | nil => rfl
  | cons event rest ih =>
    simp [sourceCandidatePredecessorDone, sourceRetainedRegion?, ih]

/-- Retaining an old visit cannot authorize another outgoing acceptance or a
local operation. Their prerequisites still belong to the current visit. -/
theorem a_non_receive_candidate_never_uses_retained_completion
    (image : RustDescriptorImage) (choreo : Choreo) (retained : List Nat)
    (data : DecodedActorOrder) (path : NormalizedPath)
    (before : ActorProgressObservation) (row : DecodedActorRow)
    (notReceive : row.actionId.side ≠ .receive) :
    sourceCandidateRowReady image choreo retained data path before row =
      sourceRowReady data path before row := by
  unfold sourceCandidateRowReady sourceRowReady
  apply congrArg (fun result => !before.doneState.done row.eventId && result)
  induction data.sourcePredecessors path row.eventId with
  | nil => rfl
  | cons event rest ih =>
    simp [sourceCandidatePredecessorDone, sourceRetainedRegion?, notReceive, ih]

private theorem current_completion_implies_candidate_readiness
    {image : RustDescriptorImage} {choreo : Choreo} {retained : List Nat}
    {data : DecodedActorOrder} {path : NormalizedPath}
    {before : ActorProgressObservation} {row : DecodedActorRow}
    (ready : sourceRowReady data path before row = true) :
    sourceCandidateRowReady image choreo retained data path before row = true := by
  obtain ⟨pending, predecessors⟩ := Bool.and_eq_true_iff.mp ready
  apply Bool.and_eq_true_iff.mpr
  refine ⟨pending, List.all_eq_true.mpr ?_⟩
  intro event member
  simp only [sourceCandidatePredecessorDone,
    List.all_eq_true.mp predecessors event member, Bool.true_or]

/-- Source-preorder nesting is also used when two projected ranges coincide.
This is the derived state of the existing bits after one restart, not another
runtime counter or an application progress table. -/
def retainedAfterSourceRestart (rolls : List DecodedRollRow)
    (restarting : DecodedRollRow) (retained : List Nat) : List Nat :=
  rolls.filterMap fun roll =>
    if roll.scope = restarting.scope then some roll.scope
    else if retained.contains roll.scope &&
        !(restarting.eventStart ≤ roll.eventStart &&
          roll.eventStart + roll.eventLength ≤ restarting.eventStart + restarting.eventLength &&
          (roll.eventStart != restarting.eventStart || roll.eventLength != restarting.eventLength ||
            restarting.scope < roll.scope)) then some roll.scope
    else none

/-- Source readiness for the still-current visit. Unknown choices remain
unknown; only recorded choices must agree with the selected source path. -/
def sourceCurrentCandidateReady (image : RustDescriptorImage) (choreo : Choreo)
    (retained : List Nat) (data : DecodedActorOrder)
    (path : NormalizedPath) (before : ActorProgressObservation) (row : DecodedActorRow) : Bool :=
  observedChoicesAgree (canonicalProgramSource choreo) path before &&
    sourceCandidateRowReady image choreo retained data path before row

/-- A fresh candidate uses an explicit roll from the complete source-derived
scope domain. Preparation reads only the previous cut; it does not hypothesize
a successful commit, resource ownership, or transport delivery. -/
def sourceFreshCandidateReady (image : RustDescriptorImage) (choreo : Choreo)
    (retained : List Nat)
    (before : ActorProgressObservation) (previousPath nextPath : Nat)
    (data : DecodedActorOrder) (path : NormalizedPath) (row : DecodedActorRow) : Bool :=
  match choreo.canonicalActorRollRows? image.role with
  | none => false
  | some rolls => rolls.any fun roll =>
      match prepareObservedActorBody? image choreo before previousPath nextPath roll.scope row.eventId with
      | none => false
      | some (actual, base) => decide (actual = roll) &&
          sourceCandidateRowReady image choreo (retainedAfterSourceRestart rolls actual retained)
            data path base row

/-- Exact read-only cursor qualification across current and fresh candidates.
The row and roll domains come from the source, not supplied witness lists.
Raw cursor readiness remains separate from headers, private future custody,
public-call admission and I/O. This adds no runtime execution state. -/
def ActorAdmissionObservation.candidateCheck
    (sample : ActorAdmissionObservation) (image : RustDescriptorImage)
    (choreo : Choreo) (previousPath : Nat) : Bool :=
  image.actorOrderCheck choreo && image.actorRollCheck choreo && image.actorRouteCheck choreo &&
    sample.snapshot.valid && decide sample.retainedRolls.Nodup &&
    sample.retainedRolls.all (fun scope =>
      match choreo.canonicalActorRollRows? image.role with
      | none => false
      | some rolls => rolls.any (fun roll => roll.scope == scope)) &&
    match image.decodeActorOrder? choreo with
    | none => false
    | some data =>
      match data.normalized.paths[previousPath]?, data.normalized.paths[sample.pathIndex]? with
      | some previous, some path =>
        sample.snapshot.metadataMatches image data.rows &&
        observedChoicesAgree (canonicalProgramSource choreo) previous sample.snapshot &&
        sample.admitted.length == data.rows.length &&
        (data.activeRows path).all fun row =>
          sample.admitted[row.eventId]? == some (
            sourceCurrentCandidateReady image choreo sample.retainedRolls data path sample.snapshot row ||
            sourceFreshCandidateReady image choreo sample.retainedRolls sample.snapshot previousPath sample.pathIndex data path row)
      | _, _ => false

/-- Both positive and negative raw results match the source candidate
semantics, including legal reentry of a statically completed row. -/
theorem checked_actor_candidate_readiness_is_exact
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {previousPath : Nat}
    (checked : sample.candidateCheck image choreo previousPath = true) :
    ∃ data previous path, image.decodeActorOrder? choreo = some data ∧
      data.normalized.paths[previousPath]? = some previous ∧
      data.normalized.paths[sample.pathIndex]? = some path ∧
      ∀ row ∈ data.activeRows path,
        sample.admitted[row.eventId]? = some (
          sourceCurrentCandidateReady image choreo sample.retainedRolls data path sample.snapshot row ||
          sourceFreshCandidateReady image choreo sample.retainedRolls sample.snapshot previousPath sample.pathIndex data path row) := by
  unfold ActorAdmissionObservation.candidateCheck at checked
  have body := (Bool.and_eq_true_iff.mp checked).2
  cases dataAt : image.decodeActorOrder? choreo with
  | none => simp [dataAt] at body
  | some data =>
    cases previousAt : data.normalized.paths[previousPath]? with
    | none => simp [dataAt, previousAt] at body
    | some previous =>
      cases pathAt : data.normalized.paths[sample.pathIndex]? with
      | none => simp [dataAt, previousAt, pathAt] at body
      | some path =>
        simp only [dataAt, previousAt, pathAt, Bool.and_eq_true] at body
        refine ⟨data, previous, path, rfl, previousAt, pathAt, ?_⟩
        intro row member
        exact beq_iff_eq.mp (List.all_eq_true.mp body.2 row member)

/-- Erased source bases for one candidate, using the same pure preparation
that a checked fresh commit uses. This is not a new runtime mode or ledger. -/
inductive ActorSourceCandidateBase (sample : ActorAdmissionObservation)
    (image : RustDescriptorImage) (choreo : Choreo) (previousPath : Nat)
    (path : NormalizedPath) (row : DecodedActorRow) : ActorProgressObservation → Prop where
  | current
      (agrees : observedChoicesAgree (canonicalProgramSource choreo) path sample.snapshot = true) :
      ActorSourceCandidateBase sample image choreo previousPath path row sample.snapshot
  | fresh {rolls : List DecodedRollRow} {roll : DecodedRollRow} {base : ActorProgressObservation}
      (domain : choreo.canonicalActorRollRows? image.role = some rolls)
      (member : roll ∈ rolls)
      (prepared : prepareObservedActorBody? image choreo sample.snapshot
        previousPath sample.pathIndex roll.scope row.eventId = some (roll, base)) :
      ActorSourceCandidateBase sample image choreo previousPath path row base

/-- Source readiness in either authentic base implies raw admission. The
fresh case does not mistake its old done bit for completed-message replay. -/
theorem a_ready_source_candidate_base_has_actual_cursor_admission
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {previousPath : Nat} {data : DecodedActorOrder} {path : NormalizedPath}
    {row : DecodedActorRow} {base : ActorProgressObservation}
    (checked : sample.candidateCheck image choreo previousPath = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (origin : ActorSourceCandidateBase sample image choreo previousPath path row base)
    (ready : sourceRowReady data path base row = true) :
    sample.admitted[row.eventId]? = some true := by
  obtain ⟨actual, previous, chosen, actualAt, _, chosenAt, rows⟩ :=
    checked_actor_candidate_readiness_is_exact checked
  have same := Option.some.inj (actualAt.symm.trans decoded)
  subst actual
  have samePath := Option.some.inj (chosenAt.symm.trans pathAt)
  subst chosen
  rw [(rows row member)]
  apply congrArg some
  cases origin with
  | current agrees =>
    have supported := current_completion_implies_candidate_readiness
      (image := image) (choreo := choreo) (retained := sample.retainedRolls) ready
    simp only [sourceCurrentCandidateReady, agrees, supported, Bool.true_and, Bool.true_or]
  | @fresh rolls roll base domain present prepared =>
    have fresh : sourceFreshCandidateReady image choreo sample.retainedRolls sample.snapshot previousPath sample.pathIndex data path row = true := by
      simp only [sourceFreshCandidateReady, domain]
      apply List.any_eq_true.mpr
      refine ⟨_, present, ?_⟩
      have supported := current_completion_implies_candidate_readiness
        (image := image) (choreo := choreo)
        (retained := retainedAfterSourceRestart rolls roll sample.retainedRolls) ready
      simp only [prepared, decide_true, supported, Bool.true_and]
    simp only [fresh, Bool.or_true]

/-- Current event completions and retained visit completions both justify
the independently observed raw cursor query. The premise is source-derived
prerequisite readiness, not an assumed Rust future or a desired raw result. -/
theorem a_ready_current_candidate_has_actual_cursor_admission
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {previousPath : Nat} {data : DecodedActorOrder} {path : NormalizedPath}
    {row : DecodedActorRow}
    (checked : sample.candidateCheck image choreo previousPath = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (agrees : observedChoicesAgree (canonicalProgramSource choreo) path sample.snapshot = true)
    (ready : sourceCandidateRowReady image choreo sample.retainedRolls data path sample.snapshot row = true) :
    sample.admitted[row.eventId]? = some true := by
  obtain ⟨actual, previous, chosen, actualAt, _, chosenAt, rows⟩ :=
    checked_actor_candidate_readiness_is_exact checked
  have same := Option.some.inj (actualAt.symm.trans decoded)
  subst actual
  have samePath := Option.some.inj (chosenAt.symm.trans pathAt)
  subst chosen
  rw [rows row member]
  simp only [sourceCurrentCandidateReady, agrees, ready, Bool.true_and, Bool.true_or]

/-- Connect the global source dependency graph to actual cursor admission,
for both current and independently prepared fresh bodies. Dependency readiness
is structural; this theorem does not take a runtime-readiness or live-value
oracle. The observation correspondence names each active source row. -/
theorem a_structurally_ready_source_point_has_actual_candidate_admission
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {previousPath : Nat} {data : DecodedActorOrder} {path : NormalizedPath}
    {row : DecodedActorRow} {base : ActorProgressObservation} {observed : SourcePoint → Bool}
    (checked : sample.candidateCheck image choreo previousPath = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (origin : ActorSourceCandidateBase sample image choreo previousPath path row base)
    (correspondence : ∀ prior ∈ data.activeRows path,
      observed (.committed prior.actionId) = base.doneState.done prior.eventId)
    (pending : base.doneState.done row.eventId = false)
    (ready : ∀ before, SourcePointEdge data.source path.bindings before row.actionId.entryPoint →
      observed before = true) :
    sourceRowReady data path base row = true ∧ sample.admitted[row.eventId]? = some true := by
  have sourceReady : sourceRowReady data path base row = true := by
    apply Bool.and_eq_true_iff.mpr
    refine ⟨by simp only [pending, Bool.not_false], List.all_eq_true.mpr ?_⟩
    intro event before
    obtain ⟨prior, present, eventAt, ordered⟩ :=
      a_source_predecessor_has_its_exact_actor_row decoded member before
    have done := ready (.committed prior.actionId) (.ordered ordered)
    rw [correspondence prior present, eventAt] at done
    exact done
  exact ⟨sourceReady, a_ready_source_candidate_base_has_actual_cursor_admission
    checked decoded pathAt member origin sourceReady⟩

end Hibana
