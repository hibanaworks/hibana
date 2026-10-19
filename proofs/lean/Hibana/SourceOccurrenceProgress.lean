import Hibana.ActorVisitProgress
import Hibana.SourceResourceOrder

namespace Hibana
open EndpointDelegation

/-- A finite unfolding of the global source, used only by the logical model.
Each roll copy may unfold its nested rolls differently. Copies retain their
own route sites, so different visits may select different arms. No actor is
required to finish one copy before another actor enters the next one.

The output positions distinguish communication occurrences. `origins` below
maps them back to the original source; neither position is a wire epoch or a
resident runtime field. -/
inductive SourceUnfolding : Source → Source → Type where
  | send (message : Message) : SourceUnfolding (.send message) (.send message)
  | seq : SourceUnfolding left first → SourceUnfolding right second →
      SourceUnfolding (.seq left right) (.seq first second)
  | par : SourceUnfolding left first → SourceUnfolding right second →
      SourceUnfolding (.par left right) (.par first second)
  | route (resolver : Option Nat) :
      SourceUnfolding left first → SourceUnfolding right second →
      SourceUnfolding (.route resolver left right) (.route resolver first second)
  | rollOnce : SourceUnfolding body expanded → SourceUnfolding (.roll body) expanded
  | rollMore : SourceUnfolding (.roll body) earlier → SourceUnfolding body next →
      SourceUnfolding (.roll body) (.seq earlier next)
  | resumeAfterContinuation :
      SourceUnfolding (.seq (.roll body) continuation) earlier → SourceUnfolding body next →
      SourceUnfolding (.seq (.roll body) continuation) (.seq earlier next)
  /-- A completed body releases both a later copy and its continuation.
  Neither is ordered after the other. The cut still requires normalization
  of the whole expansion with the original affine issuance. -/
  | resumeAlongsideContinuation :
      SourceUnfolding (.roll body) earlier → SourceUnfolding (.roll body) next →
      SourceUnfolding continuation tail →
      SourceUnfolding (.seq (.roll body) continuation) (.seq earlier (.par next tail))

def EndpointDelegation.Source.oneCopy : Source → Source
  | .send message => .send message
  | .seq left right => .seq left.oneCopy right.oneCopy
  | .par left right => .par left.oneCopy right.oneCopy
  | .route resolver left right => .route resolver left.oneCopy right.oneCopy
  | .roll body => body.oneCopy

/-- A concrete finite witness for prefixes contained in the first copy of
each roll. This does not synchronize participants or impose a runtime bound. -/
def SourceUnfolding.oneCopy : (source : Source) → SourceUnfolding source source.oneCopy
  | .send message => .send message
  | .seq left right => .seq (oneCopy left) (oneCopy right)
  | .par left right => .par (oneCopy left) (oneCopy right)
  | .route resolver left right => .route resolver (oneCopy left) (oneCopy right)
  | .roll body => .rollOnce (oneCopy body)

def SourceUnfolding.occurrences : {source expanded : Source} →
    SourceUnfolding source expanded → Nat → List (Nat × Message)
  | _, _, .send message, start => [(start, message)]
  | .seq left _, _, .seq first second, start
  | .par left _, _, .par first second, start
  | .route _ left _, _, .route _ first second, start =>
      first.occurrences start ++ second.occurrences (start + left.eventCount)
  | _, _, .rollOnce body, start => body.occurrences start
  | _, _, .rollMore earlier next, start => earlier.occurrences start ++ next.occurrences start
  | _, _, .resumeAfterContinuation earlier next, start =>
      earlier.occurrences start ++ next.occurrences start
  | .seq (.roll body) _, _, .resumeAlongsideContinuation earlier next tail, start =>
      earlier.occurrences start ++ next.occurrences start ++
        tail.occurrences (start + body.eventCount)

def SourceUnfolding.origins (unfolding : SourceUnfolding source expanded) (start : Nat) : List Nat :=
  (unfolding.occurrences start).map Prod.fst

/-- Expansion retains actual messages, including Endpoint payloads, labels and
logical peers. An occurrence origin is not merely an arbitrary integer list. -/
theorem source_unfolding_preserves_original_messages
    (unfolding : SourceUnfolding source expanded) (start : Nat) :
    ∀ entry ∈ unfolding.occurrences start, entry ∈ source.messageEntries start := by
  induction unfolding generalizing start with
  | send => exact fun _ present => present
  | seq first second left right =>
    intro entry present
    rcases List.mem_append.mp present with earlier | later
    · exact List.mem_append_left _ (left start entry earlier)
    · exact List.mem_append_right _ (right _ entry later)
  | par first second left right =>
    intro entry present
    rcases List.mem_append.mp present with earlier | later
    · exact List.mem_append_left _ (left start entry earlier)
    · exact List.mem_append_right _ (right _ entry later)
  | route resolver first second left right =>
    intro entry present
    rcases List.mem_append.mp present with earlier | later
    · exact List.mem_append_left _ (left start entry earlier)
    · exact List.mem_append_right _ (right _ entry later)
  | rollOnce body ih => exact ih start
  | rollMore earlier next left right =>
    intro entry present
    rcases List.mem_append.mp present with before | after
    · exact left start entry before
    · exact right start entry after
  | resumeAfterContinuation earlier next left right =>
    intro entry present
    rcases List.mem_append.mp present with before | after
    · exact left start entry before
    · exact List.mem_append_left _ (right start entry after)
  | resumeAlongsideContinuation earlier next tail left right continuation =>
    intro entry present
    rcases List.mem_append.mp present with body | later
    · rcases List.mem_append.mp body with before | after
      · exact List.mem_append_left _ (left start entry before)
      · exact List.mem_append_left _ (right start entry after)
    · exact List.mem_append_right _ (continuation _ entry later)

/-- Every occurrence has an origin, including later copies of the same
message. Equal source ordinals are intentionally retained rather than deduped. -/
theorem source_unfolding_has_one_origin_per_occurrence
    (unfolding : SourceUnfolding source expanded) (start : Nat) :
    (unfolding.origins start).length = expanded.eventCount := by
  suffices (unfolding.occurrences start).length = expanded.eventCount by
    simpa only [SourceUnfolding.origins, List.length_map] using this
  induction unfolding generalizing start with
  | send => rfl
  | seq first second left right =>
    simp [SourceUnfolding.occurrences, Source.eventCount, left, right]
  | par first second left right =>
    simp [SourceUnfolding.occurrences, Source.eventCount, left, right]
  | route resolver first second left right =>
    simp [SourceUnfolding.occurrences, Source.eventCount, left, right]
  | rollOnce body ih => exact ih start
  | rollMore earlier next left right =>
    simp [SourceUnfolding.occurrences, Source.eventCount, left, right]
  | resumeAfterContinuation earlier next left right =>
    simp [SourceUnfolding.occurrences, Source.eventCount, left, right]
  | resumeAlongsideContinuation earlier next tail left right continuation =>
    simp [SourceUnfolding.occurrences, Source.eventCount, left, right, continuation]

/-- Executable source dependency, including the distinct acceptance and
publication boundaries. Message payload bytes do not choose an occurrence. -/
def sourcePointDepends (source : Source) (bindings : List (Nat × Binding)) :
    SourcePoint → SourcePoint → Bool
  | .accepted event, .committed target =>
      event == target.event && decide (target.side = .send ∨ target.side = .receive)
  | .committed before, .accepted event => precedes source bindings before ⟨event, .send⟩
  | .committed before, .committed target =>
      decide (target.side ≠ .send) && precedes source bindings before target
  | .accepted _, .accepted _ => false

theorem source_point_dependency_check_is_exact
    {source : Source} {bindings : List (Nat × Binding)} {before after : SourcePoint} :
    sourcePointDepends source bindings before after = true ↔
      SourcePointEdge source bindings before after := by
  constructor
  · intro checked
    cases before with
    | accepted event =>
      cases after with
      | accepted => contradiction
      | committed target =>
        have fields := Bool.and_eq_true_iff.mp checked
        have same := beq_iff_eq.mp fields.1
        have side := of_decide_eq_true fields.2
        rcases target with ⟨target, kind⟩
        dsimp only at same side
        subst target
        rcases side with rfl | rfl
        · exact .publish event
        · exact .deliver event
    | committed id =>
      cases after with
      | accepted event => exact .ordered checked
      | committed target =>
        have fields := Bool.and_eq_true_iff.mp checked
        have notSend := of_decide_eq_true fields.1
        cases side : target.side with
        | send => exact False.elim (notSend side)
        | receive | «local» =>
          simpa only [ActionId.entryPoint, side] using SourcePointEdge.ordered fields.2
  · intro edge
    cases edge with
    | publish event | deliver event =>
      change decide (event = event) && true = true
      rw [decide_eq_true rfl]
      rfl
    | @ordered prior target ordered =>
      rcases target with ⟨event, side⟩
      cases side <;>
        simpa only [ActionId.entryPoint, sourcePointDepends, decide_true,
          Bool.true_and] using ordered

/-- The finite occurrence model uses the source's existing send/receive
dependencies, not one common iteration counter or a whole-session roll reset.
The selected path belongs to the unfolded source; different copies have
different route scopes. `observed` is proof interpretation of one source cut,
not an application flag or another runtime progress store. -/
structure SourceOccurrenceCut (source : Source) where
  original : Normalization
  originalAccepted : normalize source = some original
  expanded : Source
  unfolding : SourceUnfolding source expanded
  normalized : Normalization
  accepted : normalize expanded = some normalized
  sameIssuance : normalized.issuance = original.issuance
  path : NormalizedPath
  selected : path ∈ normalized.paths
  observed : SourcePoint → Bool

/-- Reconstruct observations from the chronological operation word. -/
def occurrenceCut (plan : SourceOccurrenceCut source) (points : List SourcePoint) :
    SourceOccurrenceCut source :=
  { plan with observed := fun point => points.contains point }

def SourceOccurrenceCut.Unfinished (cut : SourceOccurrenceCut source) : Prop :=
  ∃ point ∈ sourceVisitPoints cut.path.bindings, cut.observed point = false

def SourceOccurrenceCut.ready (cut : SourceOccurrenceCut source) (point : SourcePoint) : Bool :=
  (sourceVisitPoints cut.path.bindings).contains point && !cut.observed point &&
    (sourceVisitPoints cut.path.bindings).all fun prior =>
      !sourcePointDepends cut.expanded cut.path.bindings prior point || cut.observed prior

/-- One source transition includes sender publication as its own point.
An accepted send cannot disappear from the unfinished domain merely because
its receiver already ran. Carrier delivery and cancellation are distinct
implementation/profile obligations, not arbitrary fallback transitions. -/
inductive SourceOccurrenceStep (cut : SourceOccurrenceCut source) :
    SourcePoint → SourceOccurrenceCut source → Prop where
  | advance (point : SourcePoint)
      (present : point ∈ sourceVisitPoints cut.path.bindings)
      (pending : cut.observed point = false)
      (ready : ∀ prior, SourcePointEdge cut.expanded cut.path.bindings prior point →
        cut.observed prior = true) :
      SourceOccurrenceStep cut point
        { cut with observed := fun candidate => if candidate = point then true else cut.observed candidate }

theorem a_checked_source_occurrence_has_its_exact_transition
    (cut : SourceOccurrenceCut source) (point : SourcePoint) (checked : cut.ready point = true) :
    ∃ next, SourceOccurrenceStep cut point next := by
  have fields := Bool.and_eq_true_iff.mp checked
  have head := Bool.and_eq_true_iff.mp fields.1
  have member : point ∈ sourceVisitPoints cut.path.bindings := by simpa using head.1
  refine ⟨_, .advance point member (by simpa using head.2) ?_⟩
  intro prior edge
  have present := canonical_source_visit_is_dependency_closed member edge
  have actual := List.all_eq_true.mp fields.2 prior present
  have depends := source_point_dependency_check_is_exact.mpr edge
  simpa only [depends, Bool.not_true, Bool.false_or] using actual

/-- Logical progress for every accepted finite unfolding and every unfinished
cut in it, with no common visit assigned to physical actors. This is a source
model theorem, not a proof that an arbitrary Rust application issues or polls
the next call. Applying it to observed execution still requires a source-cut
refinement preserving occurrence identity and the selected carrier profile. -/
theorem an_unfinished_source_occurrence_cut_has_a_transition
    (cut : SourceOccurrenceCut source) (unfinished : cut.Unfinished) :
    ∃ point next, SourceOccurrenceStep cut point next ∧
      cut.observed point = false ∧ next.observed point = true ∧
      next.expanded = cut.expanded ∧
      ∀ other, other ≠ point → next.observed other = cut.observed other := by
  obtain ⟨point, present, pending, ready⟩ :=
    unfinished_canonical_source_visit_has_ready_point
      (source := cut.expanded) unfinished
  refine ⟨point, { cut with observed := fun candidate =>
    if candidate = point then true else cut.observed candidate },
    .advance point present pending ready, pending, ?_, rfl, ?_⟩
  · simp
  · intro other distinct
    simp [distinct]

private def pipelineMessage : Message :=
  ⟨⟨0, 0⟩, ⟨0, 1⟩, 94, .wire 0⟩

private def twoCopies : Source :=
  .seq (.send pipelineMessage) (.send pipelineMessage)

private def twoCopyUnfolding : SourceUnfolding (.roll (.send pipelineMessage)) twoCopies :=
  .rollMore (.rollOnce (.send _)) (.send _)

private def pipelineBindings : List (Nat × Binding) :=
  [(0, ⟨pipelineMessage, 0, 1⟩), (1, ⟨pipelineMessage, 0, 1⟩)]

private def firstSendPublished : SourcePoint → Bool
  | .accepted 0 | .committed ⟨0, .send⟩ => true
  | _ => false

private def pipelineCut : SourceOccurrenceCut (.roll (.send pipelineMessage)) := {
  original := ⟨[], [⟨[], [(0, ⟨pipelineMessage, 0, 1⟩)]⟩]⟩
  originalAccepted := by decide
  expanded := twoCopies
  unfolding := twoCopyUnfolding
  normalized := ⟨[], [⟨[], pipelineBindings⟩]⟩
  accepted := by decide
  sameIssuance := rfl
  path := ⟨[], pipelineBindings⟩
  selected := by decide
  observed := firstSendPublished
}

private def pipelineUnreadCut : SourceOccurrenceCut (.roll (.send pipelineMessage)) :=
  { pipelineCut with observed := fun point => pipelineCut.observed point || decide (point = .accepted 1) }

/-- Unlike an atomic global roll, occurrence dependencies allow the second
send while the first receive remains unfinished. No receiver-publication
edge is added to the sender's continuation. -/
theorem source_occurrences_allow_fifo_send_pipelining :
    twoCopyUnfolding.origins 0 = [0, 0] ∧
      pipelineCut.observed (.committed ⟨0, .receive⟩) = false ∧
      pipelineUnreadCut.ready (.committed ⟨1, .receive⟩) = false ∧
      ∃ next, SourceOccurrenceStep pipelineCut (.accepted 1) next :=
  ⟨rfl, rfl, by decide, a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)⟩

private def independentMessage : Message :=
  ⟨⟨0, 0⟩, ⟨0, 2⟩, 95, .wire 0⟩

private def unevenSource : Source :=
  .par (.roll (.send pipelineMessage)) (.roll (.send independentMessage))

private def unevenExpanded : Source := .par
  (.seq twoCopies (.send pipelineMessage)) (.send independentMessage)

private def unevenUnfolding : SourceUnfolding unevenSource unevenExpanded :=
  .par (.rollMore twoCopyUnfolding (.send _)) (.rollOnce (.send _))

private def unevenBindings : List (Nat × Binding) :=
  [(0, ⟨pipelineMessage, 0, 1⟩), (1, ⟨pipelineMessage, 0, 1⟩),
    (2, ⟨pipelineMessage, 0, 1⟩), (3, ⟨independentMessage, 0, 2⟩)]

private def unevenCut : SourceOccurrenceCut unevenSource := {
  original := ⟨[], [⟨[], [(0, ⟨pipelineMessage, 0, 1⟩), (1, ⟨independentMessage, 0, 2⟩)]⟩]⟩
  originalAccepted := by decide
  expanded := unevenExpanded
  unfolding := unevenUnfolding
  normalized := ⟨[], [⟨[], unevenBindings⟩]⟩
  accepted := by decide
  sameIssuance := rfl
  path := ⟨[], unevenBindings⟩
  selected := by decide
  observed := fun point => point = .accepted 0 || point = .committed ⟨0, .send⟩ ||
    point = .accepted 1 || point = .committed ⟨1, .send⟩
}

/-- Independent roll bodies need not have equal visit counts. Even the same
physical sender may enter its third left-hand copy while its first right-hand
send and the first left-hand receive are still unfinished. -/
theorem source_occurrences_preserve_independent_parallel_visits :
    unevenUnfolding.origins 0 = [0, 0, 0, 1] ∧
      unevenCut.observed (.accepted 3) = false ∧
      unevenCut.observed (.committed ⟨0, .receive⟩) = false ∧
      (∃ next, SourceOccurrenceStep unevenCut (.accepted 2) next) ∧
      ∃ next, SourceOccurrenceStep unevenCut (.accepted 3) next :=
  ⟨rfl, rfl, rfl, a_checked_source_occurrence_has_its_exact_transition _ _ (by decide),
    a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)⟩

private def dbKey : EndpointKey := ⟨0, 2⟩
private def loanMessage : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint dbKey⟩
private def readMessage : Message := ⟨dbKey, ⟨0, 3⟩, 2, .wire 1⟩
private def valueMessage : Message := ⟨⟨0, 3⟩, dbKey, 3, .wire 1⟩
private def returnMessage : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 4, .endpoint dbKey⟩

private def loanBody : Source := .seq (.send loanMessage)
  (.seq (.send readMessage) (.seq (.send valueMessage) (.send returnMessage)))

private def loanBodyUnfolding : SourceUnfolding loanBody loanBody :=
  .seq (.send _) (.seq (.send _) (.seq (.send _) (.send _)))

private def loanUnfolding : SourceUnfolding (.roll loanBody) (.seq loanBody loanBody) :=
  .rollMore (.rollOnce loanBodyUnfolding) loanBodyUnfolding

private def loanBindings (start : Nat) : List (Nat × Binding) :=
  [(start, ⟨loanMessage, 0, 1⟩), (start + 1, ⟨readMessage, 1, 3⟩),
    (start + 2, ⟨valueMessage, 3, 1⟩), (start + 3, ⟨returnMessage, 1, 0⟩)]

private def loanCut : SourceOccurrenceCut (.roll loanBody) := {
  original := ⟨[⟨dbKey, 0⟩], [⟨[], loanBindings 0⟩]⟩
  originalAccepted := by decide
  expanded := .seq loanBody loanBody
  unfolding := loanUnfolding
  normalized := ⟨[⟨dbKey, 0⟩], [⟨[], loanBindings 0 ++ loanBindings 4⟩]⟩
  accepted := by decide
  sameIssuance := rfl
  path := ⟨[], loanBindings 0 ++ loanBindings 4⟩
  selected := by decide
  observed := fun point => match point with
    | .accepted event => event < 4
    | .committed id => id.event < 4 && !(id.event == 3 && decide (id.side = .send))
}

private def loanBeforeReturnCut : SourceOccurrenceCut (.roll loanBody) :=
  { loanCut with observed := fun point =>
    if point = .committed ⟨3, .receive⟩ then false else loanCut.observed point }

/-- Once the return is received, its new holder may lend again while the old
sender has not published that return. The source model does not make custody
wait for an acknowledgment or introduce a synchronization round. The actual
custody/carrier refinement must preserve these distinct boundaries. -/
theorem source_occurrences_preserve_holder_change_before_old_publication :
    loanUnfolding.origins 0 = [0, 1, 2, 3, 0, 1, 2, 3] ∧
      loanCut.observed (.committed ⟨3, .receive⟩) = true ∧
      loanCut.observed (.committed ⟨3, .send⟩) = false ∧
      loanBeforeReturnCut.ready (.accepted 4) = false ∧
      ∃ next, SourceOccurrenceStep loanCut (.accepted 4) next :=
  ⟨rfl, rfl, rfl, by decide, a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)⟩

private def continuationMessage : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 97, .wire 0⟩
private def loanWithContinuation : Source := .seq (.roll loanBody) (.send continuationMessage)
private def resumedLoanSource : Source :=
  .seq (.seq loanBody (.send continuationMessage)) loanBody
private def resumedLoanUnfolding : SourceUnfolding loanWithContinuation resumedLoanSource :=
  .resumeAfterContinuation (.seq (.rollOnce loanBodyUnfolding) (.send _)) loanBodyUnfolding
private def resumedLoanBindings : List (Nat × Binding) :=
  loanBindings 0 ++ [(4, ⟨continuationMessage, 0, 1⟩)] ++ loanBindings 5
private def resumedLoanCut : SourceOccurrenceCut loanWithContinuation := {
  original := ⟨[⟨dbKey, 0⟩], [⟨[], loanBindings 0 ++ [(4, ⟨continuationMessage, 0, 1⟩)]⟩]⟩
  originalAccepted := by decide
  expanded := resumedLoanSource
  unfolding := resumedLoanUnfolding
  normalized := ⟨[⟨dbKey, 0⟩], [⟨[], resumedLoanBindings⟩]⟩
  accepted := by decide
  sameIssuance := rfl
  path := ⟨[], resumedLoanBindings⟩
  selected := by decide
  observed := fun point => match point with
    | .accepted event => event < 5
    | .committed id => id.event < 5
}

/-- Elastic reentry may occur after the sequential continuation. Its completed
work remains observed; an unfolding must not force every future body copy to
precede that continuation. This adds no runtime reset or synchronization. -/
theorem source_occurrences_preserve_reentry_after_the_continuation :
    resumedLoanUnfolding.origins 0 = [0, 1, 2, 3, 4, 0, 1, 2, 3] ∧
      resumedLoanCut.observed (.committed ⟨4, .receive⟩) = true ∧
      ∃ next, SourceOccurrenceStep resumedLoanCut (.accepted 5) next ∧
        next.observed (.committed ⟨4, .receive⟩) = true := by
  obtain ⟨next, step⟩ := a_checked_source_occurrence_has_its_exact_transition resumedLoanCut (.accepted 5) (by decide)
  refine ⟨rfl, rfl, next, step, ?_⟩
  cases step
  rfl

private def exchangeStart : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 201, .wire 6⟩
private def exchangeAck : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 202, .wire 6⟩
private def exchangeFinish : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 203, .wire 6⟩
private def exchangeBody : Source := .seq (.send exchangeStart) (.send exchangeAck)
private def exchangeSource : Source := .seq (.roll exchangeBody) (.send exchangeFinish)
private def exchangeExpansion : Source := .seq exchangeBody
  (.par exchangeBody (.send exchangeFinish))
private def exchangeBodyUnfolding : SourceUnfolding exchangeBody exchangeBody :=
  .seq (.send _) (.send _)
private def exchangeUnfolding : SourceUnfolding exchangeSource exchangeExpansion :=
  .resumeAlongsideContinuation (.rollOnce exchangeBodyUnfolding)
    (.rollOnce exchangeBodyUnfolding) (.send _)
private def exchangeBindings : List (Nat × Binding) :=
  [(0, ⟨exchangeStart, 0, 1⟩), (1, ⟨exchangeAck, 1, 0⟩),
    (2, ⟨exchangeStart, 0, 1⟩), (3, ⟨exchangeAck, 1, 0⟩),
    (4, ⟨exchangeFinish, 1, 0⟩)]
private def exchangePlan : SourceOccurrenceCut exchangeSource := {
  original := ⟨[], [⟨[], [(0, ⟨exchangeStart, 0, 1⟩), (1, ⟨exchangeAck, 1, 0⟩),
    (2, ⟨exchangeFinish, 1, 0⟩)]⟩]⟩
  originalAccepted := by decide
  expanded := exchangeExpansion
  unfolding := exchangeUnfolding
  normalized := ⟨[], [⟨[], exchangeBindings⟩]⟩
  accepted := by decide
  sameIssuance := rfl
  path := ⟨[], exchangeBindings⟩
  selected := by decide
  observed := fun _ => false
}

/-- This is the operation order of the real queued-continuation regression:
the second Start precedes receipt of Finish, while Finish is sent before the
second Ack. No placement wholly before or after the continuation represents
both actors' order. FIFO receipt still belongs to the physical word. -/
private def exchangePoints : List SourcePoint := [
  .accepted 0, .committed ⟨0, .send⟩, .committed ⟨0, .receive⟩,
  .accepted 1, .committed ⟨1, .send⟩, .committed ⟨1, .receive⟩,
  .accepted 4, .committed ⟨4, .send⟩,
  .accepted 2, .committed ⟨2, .send⟩, .committed ⟨2, .receive⟩,
  .accepted 3, .committed ⟨3, .send⟩,
  .committed ⟨4, .receive⟩, .committed ⟨3, .receive⟩]

private def exchangeBeforeContinuation : SourceOccurrenceCut exchangeSource := {
  exchangePlan with
  expanded := .seq (.seq exchangeBody exchangeBody) (.send exchangeFinish)
  unfolding := .seq (.rollMore (.rollOnce exchangeBodyUnfolding) exchangeBodyUnfolding)
    (.send _)
  accepted := by decide
}

private def exchangeAfterContinuation : SourceOccurrenceCut exchangeSource := {
  exchangePlan with
  expanded := .seq (.seq exchangeBody (.send exchangeFinish)) exchangeBody
  unfolding := .resumeAfterContinuation
    (.seq (.rollOnce exchangeBodyUnfolding) (.send _)) exchangeBodyUnfolding
  normalized := ⟨[], [⟨[], [(0, ⟨exchangeStart, 0, 1⟩), (1, ⟨exchangeAck, 1, 0⟩),
    (2, ⟨exchangeFinish, 1, 0⟩), (3, ⟨exchangeStart, 0, 1⟩),
    (4, ⟨exchangeAck, 1, 0⟩)]⟩]⟩
  accepted := by decide
  sameIssuance := rfl
  path := ⟨[], [(0, ⟨exchangeStart, 0, 1⟩), (1, ⟨exchangeAck, 1, 0⟩),
    (2, ⟨exchangeFinish, 1, 0⟩), (3, ⟨exchangeStart, 0, 1⟩),
    (4, ⟨exchangeAck, 1, 0⟩)]⟩
  selected := by decide
}

/-- Every boundary of the queued-continuation schedule is enabled using only
its preceding observations. In particular neither next-visit entry nor the
earlier continuation is serialized behind the other. This is a model
regression, not by itself the Rust/carrier refinement theorem. -/
theorem source_occurrences_preserve_a_queued_continuation_during_reentry :
    exchangeUnfolding.origins 0 = [0, 1, 0, 1, 2] ∧
      exchangePoints.zipIdx.all (fun (point, index) =>
        (occurrenceCut exchangePlan (exchangePoints.take index)).ready point) = true ∧
      (occurrenceCut exchangeBeforeContinuation (exchangePoints.take 6)).ready
        (.accepted 4) = false ∧
      (occurrenceCut exchangeAfterContinuation
        (exchangePoints.take 6 ++ [.accepted 2, .committed ⟨2, .send⟩])).ready
        (.accepted 3) = false :=
  ⟨rfl, by decide, by decide, by decide⟩

private def rightChoiceMessage : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 96, .wire 0⟩
private def choiceBody : Source := .route none (.send pipelineMessage) (.send rightChoiceMessage)
private def choiceBodyUnfolding : SourceUnfolding choiceBody choiceBody :=
  .route none (.send _) (.send _)

private def choiceBinding (start : Nat) : RouteArm → Nat × Binding
  | .left => (start, ⟨pipelineMessage, 0, 1⟩)
  | .right => (start + 1, ⟨rightChoiceMessage, 0, 1⟩)

private def choicePath (first second : RouteArm) : NormalizedPath := {
  selections := [⟨⟨0, 1, 2⟩, first⟩, ⟨⟨2, 3, 4⟩, second⟩]
  bindings := [choiceBinding 0 first, choiceBinding 2 second]
}

private def choiceCut : SourceOccurrenceCut (.roll choiceBody) := {
  original := ⟨[], [
    ⟨[⟨⟨0, 1, 2⟩, .left⟩], [choiceBinding 0 .left]⟩,
    ⟨[⟨⟨0, 1, 2⟩, .right⟩], [choiceBinding 0 .right]⟩]⟩
  originalAccepted := by decide
  expanded := .seq choiceBody choiceBody
  unfolding := .rollMore (.rollOnce choiceBodyUnfolding) choiceBodyUnfolding
  normalized := ⟨[], [choicePath .left .left, choicePath .left .right,
    choicePath .right .left, choicePath .right .right]⟩
  accepted := by decide
  sameIssuance := rfl
  path := choicePath .left .right
  selected := by decide
  observed := firstSendPublished
}

private def choiceUnreadCut : SourceOccurrenceCut (.roll choiceBody) :=
  { choiceCut with observed := fun point => choiceCut.observed point || decide (point = .accepted 3) }

/-- The next visit can select Right while the receiver still has the previous
Left message to consume. Choosing one arm for all active visits would reject
this legal source execution. Receiver-side FIFO remains an explicit carrier
obligation; no new route choice is inferred from payload bytes. -/
theorem source_occurrences_keep_separate_choices_for_overlapping_visits :
    choiceCut.path.bindings = [choiceBinding 0 .left, choiceBinding 2 .right] ∧
      choiceCut.observed (.committed ⟨0, .receive⟩) = false ∧
      choiceUnreadCut.ready (.committed ⟨3, .receive⟩) = false ∧
      ∃ next, SourceOccurrenceStep choiceCut (.accepted 3) next :=
  ⟨rfl, rfl, by decide, a_checked_source_occurrence_has_its_exact_transition _ _ (by decide)⟩

end Hibana
