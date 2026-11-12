import Hibana.PublicOperationCut
import Hibana.ActorCandidateAdmission

namespace Hibana
open EndpointDelegation

/-- The handle's copied epoch identifies a preparation for this borrow, not
another visit counter. Source ranges and completion come from the canonical
choreography. Once consumed, the old borrow cannot prepare that visit again. -/
def borrowedSourceRestart? (choreo : Choreo) (image : RustDescriptorImage)
    (before : ActorProgressObservation) (credits : List Nat)
    (borrow : Option (Nat × Nat × Nat)) : Option Nat := do
  let (raw, prepared, current) ← borrow
  if raw / 8192 != 1 || prepared != current then none else pure ()
  let data ← image.decodeActorOrder? choreo
  let rolls ← choreo.canonicalActorRollRows? image.role
  let roll ← resolveUnique? (rolls.filter fun roll => roll.scope == raw % 8192)
  let complete := data.normalized.paths.any fun path =>
    observedChoicesAgree (canonicalProgramSource choreo) path before &&
      (data.activeRows path).any (fun row =>
        decide (roll.eventStart ≤ row.eventId ∧ row.eventId < roll.eventStart + roll.eventLength) &&
        sourceActorBodyComplete (canonicalProgramSource choreo) data path before roll
          (sourceIndependentRetainedChildren data path rolls credits roll row.eventId))
  if complete then some roll.scope else none

def sourceRestartAllows (prepared : Option Nat) (scope : Nat) : Bool :=
  prepared.all (· == scope)

/-- Raw cursor preparation does not establish custody. A public restart also
needs the source's entry resources. Resources used only by a retained
independent child keep that child's current visit and are not restarted. -/
def sourceRestartEntryHeadersMatch (source : CanonicalProgramSource) (image : RustDescriptorImage)
    (data : DecodedActorOrder) (previous : NormalizedPath)
    (before : ActorProgressObservation) (rolls : List DecodedRollRow)
    (credits : List Nat) (roll : DecodedRollRow) (event : Nat) (preview : Bool) : Bool :=
  (show Option Bool from do
    let entries ← image.decodeRollEntryResources?
    let (_, entries) ← resolveUnique? (entries.filter fun entry => entry.1.scope == roll.scope)
    if entries.isEmpty then return true
    let columns ← image.decodeProgramColumns?
    let preserved := sourceIndependentRetainedChildren data previous rolls credits roll event
    if preserved.isEmpty && !preview then
      return observedEntryHeadersMatch before columns entries
    let rows ← (data.rows.filter fun row =>
      decide (roll.eventStart ≤ row.eventId ∧ row.eventId < roll.eventStart + roll.eventLength)).mapM
      fun row => do
        let atom ← image.eventAtom? row.eventId
        pure (row.eventId, atom)
    let required := entries.filter fun (role, reference) =>
      let uses := rows.filter fun (_, atom) => atom.sender == role || atom.receiver == role ||
        atom.schema == 4294967296 + role
      let retained := !uses.isEmpty && uses.all (fun (row, _) => preserved.any fun child =>
        decide (child.eventStart ≤ row ∧ row < child.eventStart + child.eventLength))
      -- A noncommitting preview may precede an outstanding return. Completion
      -- still needs its issued header. A past, already consumed receipt does
      -- not qualify, even when its containing roll can otherwise repeat.
      let returning := preview && columns.selectedActor? before.doneState.selected reference ==
        some before.actor && data.rows.any fun row =>
          row.actionId.side == .receive && !before.doneState.done row.eventId &&
          (image.eventAtom? row.eventId).any (·.schema == 4294967296 + role) &&
          data.normalized.paths.any (fun path => observedChoicesAgree source path before &&
            (data.activeRows path).any (·.eventId == row.eventId))
      !retained && !returning
    pure (observedEntryHeadersMatch before columns required)).getD false

theorem a_borrow_prepared_for_a_parent_cannot_restart_only_its_child
    {parent child : Nat} (allowed : sourceRestartAllows (some parent) child = true) :
    parent = child := by
  simpa [sourceRestartAllows] using allowed

/-- A completed row under a branch is not evidence that a later enclosing
visit has made that choice. A call inside the same branch may establish its
choice; a call beyond the branch must already know the prerequisite's arm. -/
def sourcePriorChoicesKnown (selected : Nat → Option RouteArm)
    (current prior : List (Nat × Nat)) : Bool :=
  prior.all fun (scope, arm) => current.any (fun entry => entry.1 == scope) ||
    (selected scope).map encodeRouteArm == some arm

theorem an_unknown_prior_choice_cannot_certify_a_continuation
    {selected : Nat → Option RouteArm} {current prior : List (Nat × Nat)}
    {scope arm : Nat} (prerequisite : (scope, arm) ∈ prior)
    (outside : current.any (fun entry => entry.1 == scope) = false)
    (unknown : selected scope = none) :
    sourcePriorChoicesKnown selected current prior = false := by
  cases result : sourcePriorChoicesKnown selected current prior with
  | false => rfl
  | true =>
      have known := List.all_eq_true.mp result (scope, arm) prerequisite
      simp [outside, unknown] at known

/-- A forgotten choice can justify a continuation outside a retained enclosing
visit. This only supplies historical choice evidence: candidate readiness still
requires every outgoing prerequisite to be currently complete. -/
def sourceRetainedChoicePrecedes (source : CanonicalProgramSource)
    (rolls : List DecodedRollRow) (retained : List Nat)
    (target scope : Nat) : Bool :=
  rolls.any fun roll => retained.contains roll.scope &&
    !(roll.eventStart ≤ target && target < roll.eventStart + roll.eventLength) &&
    sourceRollResetsRoute? source roll.scope scope == some true

/-- No past choice is published as the current visit's decision. A new branch
can still select either arm after a parallel sibling starts the parent. -/
def sourceOutgoingPriorChoicesKnown (source : CanonicalProgramSource)
    (rolls : List DecodedRollRow) (retained : List Nat)
    (data : DecodedActorOrder) (path : NormalizedPath)
    (before : ActorProgressObservation) (row : DecodedActorRow) : Bool :=
  if row.actionId.side == .receive then true else
  match canonicalEventRoutePath? source row.actionId.event with
  | none => false
  | some current => (data.sourcePredecessors path row.eventId).all fun predecessor =>
      match data.rows[predecessor]? with
      | none => false
      | some previous => match canonicalEventRoutePath? source previous.actionId.event with
        | none => false
        | some prior => prior.all fun (scope, arm) =>
            sourcePriorChoicesKnown before.doneState.selected current [(scope, arm)] ||
              sourceRetainedChoicePrecedes source rolls retained row.eventId scope

/-- Resuming this slot's materialized contract keeps its exact entry.
Different calls share the actual held choice and its ancestors,
not decisions merely encountered while finding a deeper prospective row. -/
def publicQueryPreviewChoices? (source : CanonicalProgramSource)
    (cut : PublicOperationCut) (image : RustDescriptorImage) (matching : List Nat) :
    Option (Option Nat × List (List (Nat × Nat))) := do
  let mut resumed := none
  let mut choices := []
  for slot in cut.slots do
    match slot.offeredEvent, slot.offeredChoice with
    | none, none => pure ()
    | some event, held =>
      let entries ← canonicalEventRoutePath? source event
      let heldScope ← match held with
        | none => if entries.isEmpty then some 0 else none
        | some (scope, arm) => if entries.contains (scope, arm) then some scope else none
      let own := slot.key == (cut.target.view, cut.target.lane)
      let resume := own && matching.any (fun row => image.eventEffIndex? row == some event)
      if resume then resumed := some event
      choices := choices ++ [entries.filter fun (scope, _) => resume || scope ≤ heldScope]
    | _, _ => none
  pure (resumed, choices)

private def previewKnowsQueryScope (choices : List (List (Nat × Nat))) (scope : Nat) : Bool :=
  choices.any fun entries => entries.any (fun entry => entry.1 == scope)

/-- A preview constrains a choice which this candidate still needs to learn.
A known choice belongs to the candidate's current visit; an uncommitted preview
may instead describe a future enclosing visit. The candidate readiness check
separately requires its path to agree with all known choices. -/
def publicPreviewAgrees (source : CanonicalProgramSource) (choices : List (List (Nat × Nat)))
    (before : ActorProgressObservation) (path : NormalizedPath) : Bool :=
  choices.all fun entries => entries.all fun (scope, arm) =>
          (before.doneState.selected scope).isSome ||
            (sourcePathArm? source.markers path scope).map encodeRouteArm == some arm

/-- An unconsumed preview of a later visit does not replace this visit's
committed choice for an independent continuation. Only unknown choices
containing the current operation constrain its current source path. Fresh-visit
preparation checks the choices still unknown in its source-derived base. -/
def publicCurrentPreviewAgrees (source : CanonicalProgramSource) (choices : List (List (Nat × Nat)))
    (before : ActorProgressObservation) (path : NormalizedPath) (event : Nat) : Bool :=
  match canonicalEventRoutePath? source event with
  | none => false
  | some current => choices.all fun entries => entries.all fun (scope, arm) =>
          (before.doneState.selected scope).isSome ||
            !(current.any fun entry => entry.1 == scope) ||
            (sourcePathArm? source.markers path scope).map encodeRouteArm == some arm

theorem a_preview_cannot_restrict_a_current_unbranched_operation
    {source : CanonicalProgramSource} {choices : List (List (Nat × Nat))}
    {before : ActorProgressObservation} {path : NormalizedPath} {event : Nat}
    (current : canonicalEventRoutePath? source event = some []) :
    publicCurrentPreviewAgrees source choices before path event = true := by
  simp only [publicCurrentPreviewAgrees, current]
  apply List.all_eq_true.mpr
  intro entries _
  simp

/-- Historical branch evidence never replaces an outgoing operation's current
prerequisites. This is independent of the retained-choice search algorithm. -/
theorem a_retained_outgoing_choice_still_requires_current_prerequisites
    (source : CanonicalProgramSource) (rolls : List DecodedRollRow)
    (image : RustDescriptorImage) (choreo : Choreo) (retained : List Nat)
    (data : DecodedActorOrder) (path : NormalizedPath)
    (before : ActorProgressObservation) (row : DecodedActorRow)
    (outgoing : row.actionId.side ≠ .receive)
    (ready : (sourceOutgoingPriorChoicesKnown source rolls retained data path before row &&
      sourceCurrentCandidateReady image choreo retained data path before row) = true) :
    sourceRowReady data path before row = true := by
  have candidate := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp ready).2).2
  simpa only [a_non_receive_candidate_never_uses_retained_completion
    image choreo retained data path before row outgoing] using candidate

theorem a_held_preview_keeps_its_source_branch
    (source : CanonicalProgramSource) (choices : List (List (Nat × Nat)))
    (before : ActorProgressObservation) (path : NormalizedPath)
    (scope arm : Nat) (entries : List (Nat × Nat))
    (agrees : publicPreviewAgrees source choices before path = true)
    (present : entries ∈ choices)
    (member : (scope, arm) ∈ entries)
    (unknown : before.doneState.selected scope = none) :
    ((sourcePathArm? source.markers path scope).map encodeRouteArm == some arm) = true := by
  have selected := List.all_eq_true.mp agrees entries present
  simpa [unknown] using List.all_eq_true.mp selected (scope, arm) member

theorem a_nested_candidate_cannot_replace_a_held_parent_choice
    (source : CanonicalProgramSource) (choices : List (List (Nat × Nat)))
    (before : ActorProgressObservation) (path : NormalizedPath)
    (scope arm : Nat) (entries : List (Nat × Nat))
    (present : entries ∈ choices) (member : (scope, arm) ∈ entries)
    (unknown : before.doneState.selected scope = none)
    (opposite : ((sourcePathArm? source.markers path scope).map encodeRouteArm == some arm) = false) :
    publicPreviewAgrees source choices before path = false := by
  cases result : publicPreviewAgrees source choices before path with
  | false => rfl
  | true =>
      have required := a_held_preview_keeps_its_source_branch
        source choices before path scope arm entries result present member unknown
      simp [opposite] at required

/-- Only a fresh outgoing controller operation consults these callback
inputs. A retained preview and a committed choice both keep their knowledge.
Receives get their new branch evidence from the actual wire head instead. -/
def publicQueryInputAgrees (source : CanonicalProgramSource) (cut : PublicOperationCut)
    (choices : List (List (Nat × Nat)))
    (before : ActorProgressObservation)
    (row : DecodedActorRow) (input : List Nat) : Bool :=
  if row.actionId.side = .receive then true else
  match canonicalEventRoutePath? source row.actionId.event with
  | none => false
  | some memberships => memberships.all fun (scope, arm) =>
    match source.resolvers.find? (fun resolver => resolver.scope == scope) with
    | none => false
    | some resolver => match resolver.authority with
      | .intrinsic => true
      | .dynamic id => resolver.controller != some cut.target.view ||
          (before.doneState.selected scope).isSome || previewKnowsQueryScope choices scope ||
          arm == (if input.contains id then 1 else 0)

/-- Prepare the immutable source interpretation of one actual public-call
cut once. The returned function answers every contract/input question for that
same cut; it neither records progress nor consumes an observation or request. -/
def PublicOperationCut.sourceQueries? (cut : PublicOperationCut)
    (image : RustDescriptorImage) (choreo : Choreo) (before : ActorProgressObservation)
    (retained credits : List Nat) (borrow : Option (Nat × Nat × Nat)) (preview : Bool := false) :
    Option (Nat → Bool → List Nat → Option (List Nat)) := do
  let data ← image.decodeActorOrder? choreo
  let rolls ← choreo.canonicalActorRollRows? image.role
  let borrowValid := match borrow with
    | none => true
    | some (raw, prepared, current) => !cut.target.whole && raw / 8192 == 1 &&
        decide (prepared < 2 ^ 64 ∧ current < 2 ^ 64) &&
        rolls.any (fun roll => roll.scope == raw % 8192)
  if !(cut.matches image && cut.previewsMatch image && before.valid &&
      before.metadataMatches image data.rows && image.actorOrderCheck choreo &&
      image.actorRollCheck choreo && image.actorRouteCheck choreo && borrowValid &&
      decide retained.Nodup && retained.all (fun scope => rolls.any (·.scope == scope)) &&
      decide credits.Nodup && credits.all retained.contains)
    then none else pure ()
  let source := canonicalProgramSource choreo
  let prepared := borrowedSourceRestart? choreo image before credits borrow
  let restartQuery ← observedActorBodyQueries? image choreo before credits
  let contracts ← cut.contractQueries? image
  pure fun origin receive input => do
    let matching ← contracts origin receive
    let (resumed, previews) ← publicQueryPreviewChoices? source cut image matching
    let guard := before.fault.isNone &&
      cut.permitsFreshCall (if receive then .beginRecv else .beginSend)
    if !guard then pure [] else
      let candidates := fun (resumed : Option Nat) previews =>
        let rows := data.rows.filter fun row => matching.contains row.eventId &&
          resumed.all (· == row.actionId.event)
        rows.filterMap fun row =>
        if (prepared.isNone && data.normalized.paths.any fun path =>
            (data.activeRows path).any (·.eventId == row.eventId) &&
            publicCurrentPreviewAgrees source previews before path row.actionId.event &&
            publicQueryInputAgrees source cut previews before row input &&
            sourceOutgoingPriorChoicesKnown source rolls retained data path before row &&
            sourceCurrentCandidateReady image choreo retained data path before row) ||
          (data.normalized.paths.zipIdx.any fun (path, next) =>
            (data.activeRows path).any (·.eventId == row.eventId) && rolls.any fun roll =>
              sourceRestartAllows prepared roll.scope &&
              data.normalized.paths.zipIdx.any fun (previousPath, previous) =>
                match restartQuery previous next roll.scope row.eventId with
                | none => false
                | some (actual, base) =>
                    sourceRestartEntryHeadersMatch source image data previousPath before rolls credits actual row.eventId
                      (preview && (image.eventAction? row.eventId).any (fun action =>
                        match action with | .send .. => false | _ => true)) &&
                    publicPreviewAgrees source previews base path &&
                    publicQueryInputAgrees source cut previews base row input &&
                    sourceOutgoingPriorChoicesKnown source rolls
                      (retainedAfterSourceRestart rolls actual retained) data path base row &&
                    sourceCandidateRowReady image choreo (retainedAfterSourceRestart rolls actual retained)
                      data path base row)
        then some row.eventId else none
      let preferred := candidates resumed previews
      -- Dropping a preview releases its operation lease. An idle typed send
      -- can use another enabled occurrence of the same contract when that
      -- old entry is no longer ready. Its held choice still supplies only
      -- that choice and its ancestors. An active RouteBranch remains exact.
      if !receive && cut.targetPhase? == some .idle && resumed.isSome && preferred.isEmpty then
        let (_, shared) ← publicQueryPreviewChoices? source cut image []
        pure (candidates none shared)
      else pure preferred

/-- Source candidates before transport/FIFO and owned-payload qualification.
This is not a successful-call or liveness verdict. No observed Rust readiness
bit is an input. A single question uses the same preparation as a whole batch. -/
def PublicOperationCut.sourceQueryCandidates? (cut : PublicOperationCut)
    (image : RustDescriptorImage) (choreo : Choreo) (before : ActorProgressObservation)
    (retained credits : List Nat) (borrow : Option (Nat × Nat × Nat))
    (origin : Nat) (receive : Bool) (input : List Nat) : Option (List Nat) := do
  let query ← cut.sourceQueries? image choreo before retained credits borrow
  query origin receive input

theorem a_prepared_public_source_query_equals_fresh_preparation
    {cut : PublicOperationCut} {image : RustDescriptorImage} {choreo : Choreo}
    {before : ActorProgressObservation} {retained credits : List Nat}
    {borrow : Option (Nat × Nat × Nat)}
    {query : Nat → Bool → List Nat → Option (List Nat)}
    (prepared : cut.sourceQueries? image choreo before retained credits borrow = some query)
    (origin : Nat) (receive : Bool) (input : List Nat) :
    query origin receive input =
      cut.sourceQueryCandidates? image choreo before retained credits borrow origin receive input := by
  simp [PublicOperationCut.sourceQueryCandidates?, prepared]

end Hibana
