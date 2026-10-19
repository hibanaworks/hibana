import Hibana.DistributedChoiceHistory
import Hibana.DistributedCallExecution

namespace Hibana
open EndpointDelegation

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

/-- The actual wire selector of a remote actor-local operation. Logical
message labels and payload schemas are deliberately absent: ordinary and
owned operations sharing a wire selector belong to the same source word. -/
structure SourceFrameKey where
  side : Side
  view : Nat
  peer : Nat
  lane : Nat
  color : Nat
  deriving DecidableEq

def RustDescriptorImage.frameKey? (image : RustDescriptorImage) (event : Nat) : Option SourceFrameKey := do
  let view ← image.eventView? event
  let lane ← image.eventLane? event
  let color ← image.eventFrameLabel? event
  match ← image.eventAction? event with
  | .send peer _ _ => pure ⟨.send, view, peer, lane, color⟩
  | .recv peer _ _ => pure ⟨.receive, view, peer, lane, color⟩
  | .local _ _ => none

def RustDescriptorImage.frameClassRows (image : RustDescriptorImage) (key : SourceFrameKey) : List Nat :=
  (List.range image.eventCount).filter fun event => decide (image.frameKey? event = some key)

def RustDescriptorImage.frameClassChoices? (image : RustDescriptorImage) (choreo : Choreo)
    (key : SourceFrameKey) : Option (List (Nat × RouteArm)) := do
  let first ← (image.frameClassRows key).head?
  let eff ← image.eventEffIndex? first
  let entries ← canonicalEventRoutePath? (canonicalProgramSource choreo) eff
  entries.mapM fun (scope, arm) => match arm with
    | 0 => some (scope, .left)
    | 1 => some (scope, .right)
    | _ => none

/-- Immutable source facts for an ordered group of local operations. All
quantifiers range over decoded rows/paths/scopes in the generated image.
A wire group uses every row with its selector. A causal group can include
operations across selectors, such as the complete local fragment between
resource handoffs. These facts contain no observed trace word.
There is no runtime class cursor, epoch, holder map or additional admission
rule. Conditional physical-holder changes require the separate custody link. -/
structure SourceOrderGroup (image : RustDescriptorImage) (choreo : Choreo) where
  rows : List Nat
  distinct : rows.Nodup
  data : DecodedActorOrder
  rolls : List DecodedRollRow
  choices : List (Nat × RouteArm)
  decoded : image.decodeActorOrder? choreo = some data
  decodedRolls : image.decodeRollRows? = some rolls
  nonempty : rows ≠ []
  paths : ∀ event ∈ rows,
    (image.eventEffIndex? event).bind (canonicalEventRoutePath? (canonicalProgramSource choreo)) =
      some (choices.map fun entry => (entry.1, encodeRouteArm entry.2))
  choiceMembers : ∀ entry ∈ choices,
    (choices.map fun pair => (pair.1, encodeRouteArm pair.2)).find? (fun pair => pair.1 == entry.1) =
      some (entry.1, encodeRouteArm entry.2)
  slots : ∀ entry ∈ choices, (List.range image.routeScopeCount).any
    (fun slot => image.decodeRouteScope? slot == some entry.1) = true
  active : ∀ path ∈ data.normalized.paths,
    choices.all (fun entry => sourcePathArm? (canonicalProgramSource choreo).markers path entry.1 == some entry.2) = true →
    ∀ event ∈ rows, (data.activeRows path).any (fun row => row.eventId == event) = true
  ordered : ∀ path ∈ data.normalized.paths,
    ∀ left ∈ rows.zipIdx, ∀ right ∈ rows.zipIdx,
      (data.activeRows path).any (fun row => row.eventId == right.1) = true →
      left.2 < right.2 → left.1 ∈ data.sourcePredecessors path right.1
  uniformReset : ∀ roll ∈ rolls, ∀ event ∈ rows,
    (event ∈ eventRange roll.eventStart roll.eventLength) ↔
      rows.head nonempty ∈ eventRange roll.eventStart roll.eventLength
  outsideChoices : ∀ roll ∈ rolls,
    rows.head nonempty ∉ eventRange roll.eventStart roll.eventLength →
    ∀ entry ∈ choices,
      sourceRollResetsRoute? (canonicalProgramSource choreo) roll.scope entry.1 = some false

/-- Position in a proof is derived from the existing completion bits. This
predicate is not another implementation of progress. The completed rows must
be exactly an initial segment of this ordered source group. -/
def sourceFramePrefix (rows : List Nat) (snapshot : ActorProgressObservation) (position : Nat) : Prop :=
  position ≤ rows.length ∧ ∀ index event, rows[index]? = some event →
    (snapshot.doneState.done event = true ↔ index < position)

def SourceOrderGroup.Known {image : RustDescriptorImage} {choreo : Choreo}
    (group : SourceOrderGroup image choreo) (snapshot : ActorProgressObservation) : Prop :=
  ∀ entry ∈ group.choices,
    snapshot.choices.find? (fun row => row.1 == entry.1) = some (entry.1, some entry.2)

private theorem a_checked_frame_commit_was_unfinished
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    (checked : sample.check image choreo = true) :
    sample.before.doneState.done sample.event = false := by
  have rest := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).2
  cases dataAt : image.decodeActorOrder? choreo with
  | none => simp [dataAt] at rest
  | some data =>
    cases pathAt : data.normalized.paths[sample.pathIndex]? with
    | none => simp [dataAt, pathAt] at rest
    | some path =>
      cases rowAt : data.rows[sample.event]? with
      | none => simp [dataAt, pathAt, rowAt] at rest
      | some row =>
        simp only [dataAt, pathAt, rowAt, Bool.and_eq_true] at rest
        have unfinished := rest.2.2.1.1
        change sample.before.progress[sample.event]?.getD false = false
        rw [beq_iff_eq.mp unfinished]
        rfl

private theorem a_known_class_selects_its_source_path
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {snapshot : ActorProgressObservation} {path : NormalizedPath}
    (known : group.Known snapshot)
    (agrees : observedChoicesAgree (canonicalProgramSource choreo) path snapshot = true) :
    group.choices.all (fun entry => sourcePathArm? (canonicalProgramSource choreo).markers path entry.1 == some entry.2) = true := by
  apply List.all_eq_true.mpr
  intro entry member
  have atChoice := known entry member
  have present := List.mem_of_find?_eq_some atChoice
  have agreements := List.all_eq_true.mp agrees (entry.1, some entry.2) present
  exact agreements

private theorem a_checked_commit_retains_class_knowledge
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {record : ActorCommitRecord} {session : Nat}
    (checked : record.check image choreo session = true)
    (known : group.Known record.step.before) : group.Known record.step.after := by
  obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, publication⟩ :=
    accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
  obtain ⟨eff, _, published⟩ := Option.bind_eq_some_iff.mp publication
  intro entry member
  exact a_source_publication_preserves_existing_choice published (known entry member)

private theorem a_checked_commit_consumes_the_next_class_row
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {record : ActorCommitRecord} {session position : Nat}
    (checked : record.check image choreo session = true)
    (initialSegment : sourceFramePrefix group.rows record.step.before position)
    (member : record.step.event ∈ group.rows) :
    group.rows[position]? = some record.step.event ∧
      sourceFramePrefix group.rows record.step.after (position + 1) := by
  obtain ⟨_, data, path, row, binding, decoded, pathAt, rowAt, rowSame, _, active,
      _, parents, completed, _, unchanged, _⟩ :=
    accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
  have same := Option.some.inj (decoded.symm.trans group.decoded)
  subst data
  obtain ⟨index, eventAt⟩ := List.mem_iff_getElem?.mp member
  have bound := (List.getElem?_eq_some_iff.mp eventAt).1
  have pending := a_checked_frame_commit_was_unfinished (checked_record_is_a_checked_commit checked)
  have later : position ≤ index := by
    have excluded : ¬ index < position := by
      intro before
      have done := (initialSegment.2 index record.step.event eventAt).2 before
      rw [pending] at done
      cases done
    omega
  have earlier : index ≤ position := by
    by_cases within : index ≤ position
    · exact within
    apply False.elim
    have positionBound : position < group.rows.length := by omega
    let before := group.rows[position]
    have beforeAt : group.rows[position]? = some before :=
      List.getElem?_eq_some_iff.mpr ⟨positionBound, rfl⟩
    have required := group.ordered path (List.mem_of_getElem? pathAt)
      (before, position) (List.mk_mem_zipIdx_iff_getElem?.mpr beforeAt)
      (record.step.event, index) (List.mk_mem_zipIdx_iff_getElem?.mpr eventAt)
      (List.any_eq_true.mpr ⟨row, active, beq_iff_eq.mpr rowSame⟩) (by omega)
    have done := List.all_eq_true.mp parents before required
    have impossible := (initialSegment.2 position before beforeAt).1 done
    omega
  have positionAt : index = position := by omega
  subst index
  refine ⟨eventAt, by omega, ?_⟩
  intro other event atOther
  by_cases same : event = record.step.event
  · have indexSame : other = position := by
      have distinct : group.rows.Nodup := group.distinct
      have atSame : group.rows[other]? = some record.step.event := by
        simpa only [same] using atOther
      exact (List.getElem?_inj (List.getElem?_eq_some_iff.mp atOther).1 distinct).mp
        (atSame.trans eventAt.symm)
    subst other
    simpa only [same] using (show record.step.after.doneState.done record.step.event = true ↔ position < position + 1 from
      ⟨fun _ => by omega, fun _ => completed⟩)
  · have rowDifferent : other ≠ position := by intro equal; subst other; exact same (Option.some.inj (atOther.symm.trans eventAt))
    rw [unchanged event same]
    exact (initialSegment.2 other event atOther).trans
      ⟨fun less => by omega, fun less => by omega⟩

private theorem a_checked_commit_has_its_class_metadata
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {sample : ActorCommitObservation}
    (decoded : image.decodeActorOrder? choreo = some data)
    (checked : sample.check image choreo = true) :
    sample.before.metadataMatches image data.rows = true := by
  have rest := (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).2
  cases pathAt : data.normalized.paths[sample.pathIndex]? with
  | none => simp [decoded, pathAt] at rest
  | some path =>
    cases rowAt : data.rows[sample.event]? with
    | none => simp [decoded, pathAt, rowAt] at rest
    | some row =>
      simp only [decoded, pathAt, rowAt, Bool.and_eq_true] at rest
      exact rest.1.1.1.1.1.1.1

private theorem a_class_choice_has_its_observed_local_slot
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {snapshot : ActorProgressObservation} {entry : Nat × RouteArm}
    (metadata : snapshot.metadataMatches image group.data.rows = true)
    (member : entry ∈ group.choices) :
    ∃ previous, snapshot.choices.find? (fun row => row.1 == entry.1) = some (entry.1, previous) := by
  simp only [ActorProgressObservation.metadataMatches, Bool.and_eq_true, and_assoc] at metadata
  have slots := metadata.2.1
  simp only [ActorProgressObservation.choicesMatch, Bool.and_eq_true, and_assoc] at slots
  obtain ⟨slot, bound, scopeAt⟩ := List.any_eq_true.mp (group.slots entry member)
  have allocated := List.all_eq_true.mp slots.2.2 slot bound
  simp only [beq_iff_eq] at scopeAt
  rw [scopeAt] at allocated
  obtain ⟨actual, present, matched⟩ := List.any_eq_true.mp allocated
  cases found : snapshot.choices.find? (fun row => row.1 == entry.1) with
  | none =>
    have absent := List.find?_eq_none.mp found actual present
    rw [matched] at absent
    exact False.elim (absent rfl)
  | some result =>
    have key : result.1 = entry.1 := beq_iff_eq.mp
      (List.find?_some (p := fun row : Nat × Option RouteArm => row.1 == entry.1) found)
    exact ⟨result.2, congrArg some (Prod.ext key rfl)⟩

private theorem a_class_commit_publishes_its_source_knowledge
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {record : ActorCommitRecord} {session : Nat}
    (checked : record.check image choreo session = true)
    (member : record.step.event ∈ group.rows) : group.Known record.step.after := by
  have primitive := checked_record_is_a_checked_commit checked
  have metadata := a_checked_commit_has_its_class_metadata group.decoded primitive
  obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, _, publication⟩ :=
    accepted_actor_commit_matches_source_progress primitive
  obtain ⟨eff, effAt, published⟩ := Option.bind_eq_some_iff.mp publication
  have pathAt : canonicalEventRoutePath? (canonicalProgramSource choreo) eff =
      some (group.choices.map fun entry => (entry.1, encodeRouteArm entry.2)) := by
    simpa only [effAt, Option.bind_some] using group.paths record.step.event member
  intro entry inClass
  obtain ⟨previous, atSlot⟩ := a_class_choice_has_its_observed_local_slot metadata inClass
  exact a_source_occurrence_publishes_its_observed_local_choice
    pathAt (group.choiceMembers entry inClass) atSlot published

private theorem a_checked_class_commit_extends_its_word
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {record : ActorCommitRecord} {session position cycles count : Nat}
    (checked : record.check image choreo session = true)
    (initialSegment : sourceFramePrefix group.rows record.step.before position)
    (known : 0 < position → group.Known record.step.before)
    (balance : count = cycles * group.rows.length + position) :
    ∃ nextCycles nextPosition,
      sourceFramePrefix group.rows record.step.after nextPosition ∧
      (0 < nextPosition → group.Known record.step.after) ∧
      count + (if record.step.event ∈ group.rows then 1 else 0) =
        nextCycles * group.rows.length + nextPosition ∧
      (record.step.event ∈ group.rows →
        group.rows[count % group.rows.length]? =
          some record.step.event) := by
  by_cases member : record.step.event ∈ group.rows
  · obtain ⟨atNext, nextSegment⟩ := a_checked_commit_consumes_the_next_class_row checked initialSegment member
    have bound := (List.getElem?_eq_some_iff.mp atNext).1
    have residue : count % group.rows.length = position := by
      simp only [balance, Nat.add_mod, Nat.mul_mod, Nat.mod_self, Nat.mul_zero,
        Nat.zero_mod, Nat.zero_add, Nat.mod_eq_of_lt bound]
    refine ⟨cycles, position + 1, nextSegment,
      fun _ => a_class_commit_publishes_its_source_knowledge checked member, ?_, ?_⟩
    · simp only [if_pos member]; omega
    · intro _; simpa only [residue] using atNext
  · obtain ⟨_, _, _, _, _, _, _, _, _, _, _, _, _, _, _, unchanged, _⟩ :=
      accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked)
    refine ⟨cycles, position, ⟨initialSegment.1, ?_⟩,
      fun positive => a_checked_commit_retains_class_knowledge checked (known positive), ?_, ?_⟩
    · intro index event atEvent
      have different : event ≠ record.step.event := by
        intro same
        exact member (same ▸ List.mem_of_getElem? atEvent)
      rw [unchanged event different]
      exact initialSegment.2 index event atEvent
    · simpa only [if_neg member, Nat.add_zero] using balance
    · exact fun impossible => False.elim (member impossible)

private theorem a_class_reset_has_its_real_source_preflight
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {before base : ActorProgressObservation} {previousPath nextPath scope event : Nat}
    {roll : DecodedRollRow}
    (prepared : prepareObservedActorBody? image choreo before
      previousPath nextPath scope event = some (roll, base)) :
    ∃ previous choices,
      group.data.normalized.paths[previousPath]? = some previous ∧
      roll ∈ group.rolls ∧ roll.scope = scope ∧
      base = resetObservedActor before roll choices ∧
      resetObservedChoices? (canonicalProgramSource choreo) scope before.choices = some choices ∧
      observedChoicesAgree (canonicalProgramSource choreo) previous before = true ∧
      (group.data.activeRows previous).all (fun row =>
        if roll.eventStart ≤ row.eventId && row.eventId < roll.eventStart + roll.eventLength
        then before.doneState.done row.eventId else true) = true := by
  unfold prepareObservedActorBody? at prepared
  obtain ⟨data, dataAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  have sameData := Option.some.inj (dataAt.symm.trans group.decoded)
  subst data
  obtain ⟨previous, previousAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨next, nextAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨rolls, rollsAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  have sameRows := Option.some.inj (rollsAt.symm.trans group.decodedRolls)
  subst rolls
  obtain ⟨chosen, chosenAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  obtain ⟨choices, choicesAt, allowed⟩ := Option.bind_eq_some_iff.mp prepared
  dsimp only at allowed
  split at allowed
  next guarded =>
    have same := Option.some.inj allowed
    cases same
    have filtered : roll ∈ group.rolls.filter (fun row => row.scope == scope) := by
      rw [resolveUnique?_eq_some_iff.mp chosenAt]
      exact List.mem_singleton_self _
    have member := List.mem_filter.mp filtered
    simp only [Bool.and_eq_true, and_assoc] at guarded
    exact ⟨previous, choices, previousAt, member.1, beq_iff_eq.mp member.2,
      rfl, choicesAt, guarded.2.2.2.2.2.2.2.1,
      by simpa only [Bool.and_eq_true] using guarded.2.2.2.2.2.2.2.2.2.1⟩
  next rejected => simp at allowed

private theorem a_class_reset_changes_exactly_its_source_bits
    (snapshot : ActorProgressObservation) (roll : DecodedRollRow)
    (choices : List (Nat × Option RouteArm)) (event : Nat) :
    (resetObservedActor snapshot roll choices).doneState.done event =
      if event ∈ eventRange roll.eventStart roll.eventLength then false else snapshot.doneState.done event := by
  simp only [event_range_membership]
  simp only [resetObservedActor, ActorProgressObservation.doneState,
    ActorProgressObservation.progress, List.getElem?_map, List.getElem?_zipIdx, Nat.zero_add]
  cases snapshot.steps[event]? <;> simp

private theorem an_actual_class_reset_keeps_an_empty_or_complete_word_boundary
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {before base : ActorProgressObservation} {previousPath nextPath scope event position : Nat}
    {roll : DecodedRollRow}
    (prepared : prepareObservedActorBody? image choreo before
      previousPath nextPath scope event = some (roll, base))
    (initialSegment : sourceFramePrefix group.rows before position)
    (known : 0 < position → group.Known before) :
    (sourceFramePrefix group.rows base position ∧
      (0 < position → group.Known base)) ∨
    ((position = 0 ∨ position = group.rows.length) ∧
      sourceFramePrefix group.rows base 0) := by
  obtain ⟨previous, choices, previousAt, member, scopeAt, baseAt, choicesAt, agrees, complete⟩ :=
    a_class_reset_has_its_real_source_preflight prepared
  by_cases inside : group.rows.head group.nonempty ∈ eventRange roll.eventStart roll.eventLength
  · right
    have ends : position = 0 ∨ position = group.rows.length := by
      by_cases empty : position = 0
      · exact .inl empty
      right
      have active := group.active previous (List.mem_of_getElem? previousAt)
        (a_known_class_selects_its_source_path (known (by omega)) agrees)
      have allDone : ∀ target ∈ group.rows, before.doneState.done target = true := by
        intro target present
        obtain ⟨row, rowAt, eventAt⟩ := List.any_eq_true.mp (active target present)
        have done := List.all_eq_true.mp complete row rowAt
        have resetAt := (group.uniformReset roll member target present).2 inside
        have range := event_range_membership _ _ _ |>.mp resetAt
        have same : row.eventId = target := beq_iff_eq.mp eventAt
        simpa only [same, range.1, range.2, decide_true, Bool.true_and, if_true] using done
      have bound := initialSegment.1
      by_cases equal : position = group.rows.length
      · exact equal
      have indexBound : position < group.rows.length := by omega
      have atRow : group.rows[position]? =
          some group.rows[position] := List.getElem?_eq_some_iff.mpr ⟨indexBound, rfl⟩
      have done := allDone _ (List.mem_of_getElem? atRow)
      have impossible := (initialSegment.2 _ _ atRow).1 done
      omega
    refine ⟨ends, ⟨by omega, ?_⟩⟩
    intro index target targetAt
    have resetAt := (group.uniformReset roll member target (List.mem_of_getElem? targetAt)).2 inside
    rw [baseAt, a_class_reset_changes_exactly_its_source_bits, if_pos resetAt]
    simp
  · left
    refine ⟨⟨initialSegment.1, ?_⟩, ?_⟩
    · intro index target targetAt
      have outside : target ∉ eventRange roll.eventStart roll.eventLength :=
        fun present => inside ((group.uniformReset roll member target (List.mem_of_getElem? targetAt)).1 present)
      rw [baseAt, a_class_reset_changes_exactly_its_source_bits, if_neg outside]
      exact initialSegment.2 index target targetAt
    · intro positive entry inChoices
      have outside := group.outsideChoices roll member inside entry inChoices
      rw [scopeAt] at outside
      have retained := a_source_choice_reset_preserves_outside_knowledge choicesAt outside (known positive entry inChoices)
      simpa only [baseAt, resetObservedActor] using retained

private theorem a_measured_class_commit_extends_its_actual_word
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat}
    {record : ActorCommitRecord} {session position cycles count : Nat}
    (committed : ActorMeasuredCommit image choreo session before after cleared record)
    (initialSegment : sourceFramePrefix group.rows before.1 position)
    (known : 0 < position → group.Known before.1)
    (balance : count = cycles * group.rows.length + position) :
    ∃ nextCycles nextPosition,
      sourceFramePrefix group.rows after.1 nextPosition ∧
      (0 < nextPosition → group.Known after.1) ∧
      count + (if record.step.event ∈ group.rows then 1 else 0) =
        nextCycles * group.rows.length + nextPosition ∧
      (record.step.event ∈ group.rows →
        group.rows[count % group.rows.length]? =
          some record.step.event) := by
  cases committed with
  | ordinary checked written =>
    exact a_checked_class_commit_extends_its_word checked initialSegment known balance
  | @fresh sample wire first final roll operation checked prepared =>
    have actualCommit := ActorMeasuredCommit.fresh checked prepared
    have valid := (a_measured_commit_retains_its_checked_source_and_wire actualCommit).1
    obtain ⟨base, baseAt, beforeAt, _, afterAt⟩ := prepared_body_reentry_is_derived_before_commit prepared
    rcases an_actual_class_reset_keeps_an_empty_or_complete_word_boundary baseAt initialSegment known with kept | reset
    · have operationPrefix : sourceFramePrefix group.rows operation.before position := by
        simpa only [beforeAt] using kept.1
      have operationKnown : 0 < position → group.Known operation.before := by
        simpa only [beforeAt] using kept.2
      simpa only [afterAt] using a_checked_class_commit_extends_its_word valid operationPrefix operationKnown balance
    · have operationPrefix : sourceFramePrefix group.rows operation.before 0 := by
        simpa only [beforeAt] using reset.2
      have operationKnown : 0 < 0 → group.Known operation.before :=
        fun impossible => False.elim (Nat.lt_irrefl 0 impossible)
      rcases reset.1 with empty | full
      · have startBalance : count = cycles * group.rows.length + 0 := by
          simpa only [empty] using balance
        simpa only [afterAt] using a_checked_class_commit_extends_its_word valid operationPrefix operationKnown startBalance
      · have startBalance : count = (cycles + 1) * group.rows.length + 0 := by
          simpa only [Nat.add_mul, Nat.one_mul, Nat.add_zero, full] using balance
        simpa only [afterAt] using a_checked_class_commit_extends_its_word valid operationPrefix operationKnown startBalance

/-- The word erased from actual commits of one actor and complete wire
class. All other actors and selectors are retained in the original history;
filtering here does not impose any order on independent operations. -/
def committedFrameClassWord (rows : List Nat) (actor : Nat)
    (records : List (Nat × ActorCommitRecord)) : List Nat :=
  records.filterMap fun (owner, record) =>
    if owner == actor && rows.contains record.step.event then some record.step.event else none

/-- Arbitrary finite prefixes of the same cyclic source word. A class may be
skipped by a choice, or reset together with an enclosing body. Empty visits
add no letters. No common visit is imposed on different actors or classes. -/
def sourceFrameWord (rows word : List Nat) : Prop :=
  ∀ index event, word[index]? = some event → rows[index % rows.length]? = some event

/-- Append the next actual source row to an ordered finite word. -/
theorem a_source_word_extends_by_its_next_actual_letter
    {rows word : List Nat} {event : Nat}
    (ordered : sourceFrameWord rows word)
    (next : rows[word.length % rows.length]? = some event) :
    sourceFrameWord rows (word ++ [event]) := by
  intro index actual atIndex
  by_cases earlier : index < word.length
  · exact ordered index actual (by simpa only [List.getElem?_append_left earlier] using atIndex)
  · have bound : index < word.length + 1 := by
      have := (List.getElem?_eq_some_iff.mp atIndex).1
      simpa only [List.length_append, List.length_singleton] using this
    have sameIndex : index = word.length := by omega
    subst index
    have same : event = actual := by simpa using atIndex
    simpa only [same] using next

private def SourceOrderGroup.WordCut {image : RustDescriptorImage} {choreo : Choreo}
    (group : SourceOrderGroup image choreo) (snapshot : ActorProgressObservation) (word : List Nat) : Prop :=
  ∃ cycles position,
    sourceFramePrefix group.rows snapshot position ∧
    (0 < position → group.Known snapshot) ∧
    word.length = cycles * group.rows.length + position ∧
    sourceFrameWord group.rows word

private theorem a_measured_class_boundary_extends_its_word_cut
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {before after : ActorProgressObservation × Nat} {cleared : List Nat}
    {record : ActorCommitRecord} {session : Nat} {word : List Nat}
    (committed : ActorMeasuredCommit image choreo session before after cleared record)
    (cut : group.WordCut before.1 word) :
    group.WordCut after.1 (word ++ if record.step.event ∈ group.rows then [record.step.event] else []) := by
  obtain ⟨cycles, position, initialSegment, known, balance, ordered⟩ := cut
  obtain ⟨nextCycles, nextPosition, nextSegment, nextKnown, nextBalance, nextLetter⟩ :=
    a_measured_class_commit_extends_its_actual_word committed initialSegment known balance
  refine ⟨nextCycles, nextPosition, nextSegment, nextKnown, ?_, ?_⟩
  · by_cases member : record.step.event ∈ group.rows
    · simpa only [List.length_append, if_pos member, List.length_singleton] using nextBalance
    · simpa only [List.length_append, if_neg member, List.length_nil] using nextBalance
  · by_cases member : record.step.event ∈ group.rows
    · simpa only [if_pos member] using a_source_word_extends_by_its_next_actual_letter ordered (nextLetter member)
    · simpa only [if_neg member, List.append_nil] using ordered

private theorem a_quiet_boundary_preserves_its_word_cut
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {before after : ActorProgressObservation} {count session : Nat} {word : List Nat}
    (checked : actorCommitMutationHistoryCheck image choreo session before count after count [] = true)
    (cut : group.WordCut before word) : group.WordCut after word := by
  have progress := beq_iff_eq.mp (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).2).1
  have choices := a_zero_write_measured_gap_preserves_committed_choices checked
  obtain ⟨cycles, position, initialSegment, known, balance, ordered⟩ := cut
  refine ⟨cycles, position, ⟨initialSegment.1, ?_⟩, ?_, balance, ordered⟩
  · intro index event atEvent
    simpa only [ActorProgressObservation.doneState, ← progress] using initialSegment.2 index event atEvent
  · intro positive entry member
    simpa only [choices] using known positive entry member

private theorem a_class_record_contributes_exactly_its_own_letter
    (rows : List Nat) (actor : Nat) (record : ActorCommitRecord) :
    committedFrameClassWord rows actor [(actor, record)] =
      if record.step.event ∈ rows then [record.step.event] else [] := by
  by_cases member : record.step.event ∈ rows
  · simp [committedFrameClassWord, member]
  · simp [committedFrameClassWord, member]

private theorem a_distributed_class_boundary_extends_its_word_cut
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {logged : Option (Nat × ActorCommitRecord)} {word : List Nat}
    (step : DistributedSourceStep images choreo session first last logged)
    (cut : group.WordCut (first.1.actors actor).1 word) :
    group.WordCut (last.1.actors actor).1
      (word ++ committedFrameClassWord group.rows actor logged.toList) := by
  cases step with
  | @quiet source entries changed after checked =>
    by_cases same : actor = changed
    · subst changed
      simpa only [DistributedSourceCut.actorAt, if_pos rfl,
        Option.toList_none, committedFrameClassWord, List.filterMap_nil, List.append_nil] using
        a_quiet_boundary_preserves_its_word_cut checked cut
    · simpa only [DistributedSourceCut.actorAt, if_neg same,
        Option.toList_none, committedFrameClassWord, List.filterMap_nil, List.append_nil] using cut
  | accept imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
    simpa only [DistributedSourceCut.carrierAt, Option.toList_none, committedFrameClassWord,
      List.filterMap_nil, List.append_nil] using cut
  | @publish source entries changed after cleared record entry member stamp paired committed =>
    by_cases same : actor = changed
    · subst changed
      have next := a_measured_class_boundary_extends_its_word_cut committed cut
      simpa only [DistributedSourceCut.actorAt, if_pos rfl, Option.toList_some,
        a_class_record_contributes_exactly_its_own_letter] using next
    · simpa [DistributedSourceCut.actorAt, committedFrameClassWord, same, Ne.symm same] using cut
  | @localCommit source entries changed after cleared record eff binding actorRole localAt available committed =>
    by_cases same : actor = changed
    · subst changed
      have next := a_measured_class_boundary_extends_its_word_cut committed cut
      simpa only [DistributedSourceCut.actorAt, if_pos rfl, Option.toList_some,
        a_class_record_contributes_exactly_its_own_letter] using next
    · simpa [DistributedSourceCut.actorAt, committedFrameClassWord, same, Ne.symm same] using cut
  | @receive source entries changed channel next frame after cleared record entry witness
      actorRole received entryAt identity paired available committed =>
    by_cases same : actor = changed
    · subst changed
      have advanced := a_measured_class_boundary_extends_its_word_cut committed cut
      simpa only [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt, if_pos rfl,
        Option.toList_some, a_class_record_contributes_exactly_its_own_letter] using advanced
    · simpa [DistributedSourceCut.actorAt, DistributedSourceCut.carrierAt,
        committedFrameClassWord, same, Ne.symm same] using cut

private theorem a_call_commit_updates_its_actual_group_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor changed : Nat}
    {group : SourceOrderGroup (images actor) choreo} {cut : DistributedSourceCut}
    {after : ActorProgressObservation × Nat} {cleared : List Nat}
    {record : ActorCommitRecord} {word : List Nat}
    (committed : ActorMeasuredCommit (images changed) choreo session (cut.actors changed) after cleared record)
    (prior : group.WordCut (cut.actors actor).1 word) :
    group.WordCut ((cut.actorAt changed after).actors actor).1
      (word ++ committedFrameClassWord group.rows actor [(changed, record)]) := by
  by_cases same : actor = changed
  · subst changed
    simpa only [DistributedSourceCut.actorAt, if_pos rfl,
      a_class_record_contributes_exactly_its_own_letter] using
      a_measured_class_boundary_extends_its_word_cut committed prior
  · simpa [DistributedSourceCut.actorAt, committedFrameClassWord, same, Ne.symm same] using prior

private theorem a_call_execution_has_its_actual_group_word_cut
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    group.WordCut (first.actors actor).1 [] →
      group.WordCut (last.actors actor).1
        (committedFrameClassWord group.rows actor (word.filterMap DistributedCallBoundary.record?)) := by
  induction execution with
  | refl => intro prior; exact prior
  | @quiet cut word entries published pending changed after history checked ih =>
      intro initial
      have prior := ih initial
      by_cases same : actor = changed
      · subst changed
        simpa [DistributedSourceCut.actorAt, List.filterMap_append, DistributedCallBoundary.record?] using
          a_quiet_boundary_preserves_its_word_cut checked prior
      · simpa [DistributedSourceCut.actorAt, same, List.filterMap_append, DistributedCallBoundary.record?] using prior
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      intro initial
      simpa [DistributedSourceCut.carrierAt, List.filterMap_append, DistributedCallBoundary.record?] using ih initial
  | @publish cut word entries published left right changed after cleared record entry history actorAt paired committed ih =>
      intro initial
      have next := a_call_commit_updates_its_actual_group_word committed (ih initial)
      simpa [List.filterMap_append, DistributedCallBoundary.record?, committedFrameClassWord] using next
  | @localCommit cut word entries published pending changed after cleared record eff binding history actorRole localAt committed ih =>
      intro initial
      have next := a_call_commit_updates_its_actual_group_word committed (ih initial)
      simpa [List.filterMap_append, DistributedCallBoundary.record?, committedFrameClassWord] using next
  | @receive cut word entries published pending changed channel next frame after cleared record entry witness
      history actorRole received entryAt identity paired committed ih =>
      intro initial
      have advanced := a_call_commit_updates_its_actual_group_word committed (ih initial)
      simpa [DistributedSourceCut.carrierAt, List.filterMap_append,
        DistributedCallBoundary.record?, committedFrameClassWord] using advanced

/-- The actual committed word retains source order even with multiple
outstanding calls and independent same-actor commits. This is the publication
projection of the chronological execution. Its acceptance-inclusive logical
word still needs the pending-call refinement; the two words are not equated. -/
theorem a_distributed_call_execution_keeps_its_committed_source_group_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false) :
    sourceFrameWord group.rows
      (committedFrameClassWord group.rows actor (word.filterMap DistributedCallBoundary.record?)) ∧
      ∃ cycles position,
        sourceFramePrefix group.rows (last.actors actor).1 position ∧
        (0 < position → group.Known (last.actors actor).1) ∧
        (committedFrameClassWord group.rows actor (word.filterMap DistributedCallBoundary.record?)).length =
          cycles * group.rows.length + position := by
  have initialCut : group.WordCut (first.actors actor).1 [] := by
    refine ⟨0, 0, ⟨by omega, ?_⟩, ?_, by simp, ?_⟩
    · intro index event atEvent
      rw [initial event (List.mem_of_getElem? atEvent)]
      simp
    · exact fun impossible => False.elim (Nat.lt_irrefl 0 impossible)
    · intro index event missing; simp at missing
  obtain ⟨cycles, position, finalSegment, known, balance, ordered⟩ :=
    a_call_execution_has_its_actual_group_word_cut execution initialCut
  exact ⟨ordered, cycles, position, finalSegment, known, balance⟩

/-- An actual measured actor history produces a prefix of its ordered
group's cyclic source word. The proof reads checked commits, source-derived
resets and committed branch knowledge from the same distributed execution.
Independent classes/actors can advance or reset arbitrarily between letters.
The cycles and position in the conclusion are erased witnesses derived from
the real record word and done bits, not extra runtime progress fields.

`SourceOrderGroup` supplies finite static projection facts. Their derivation
for arbitrary generated images and class continuity across changing physical
holders are separate obligations; this theorem does not replace them with
an assumption that all actor paths/visits already agree. -/
theorem a_measured_distributed_class_has_its_source_word_and_exact_prefix
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : ∀ event ∈ group.rows,
      (first.1.actors actor).1.doneState.done event = false) :
    sourceFrameWord group.rows
        (committedFrameClassWord group.rows actor records) ∧
      ∃ cycles position,
        sourceFramePrefix group.rows (last.1.actors actor).1 position ∧
        (0 < position → group.Known (last.1.actors actor).1) ∧
        (committedFrameClassWord group.rows actor records).length =
          cycles * group.rows.length + position := by
  have lift : ∀ {before after : DistributedSourceCut × List SourceAcceptance}
      {entries : List (Nat × ActorCommitRecord)},
      DistributedSourceExecution images choreo session before after entries →
      group.WordCut (before.1.actors actor).1 [] →
      group.WordCut (after.1.actors actor).1
        (committedFrameClassWord group.rows actor entries) := by
    intro before after entries run
    induction run with
    | refl => intro cut; exact cut
    | tail history step ih =>
      intro initialCut
      have extended := a_distributed_class_boundary_extends_its_word_cut step (ih initialCut)
      simpa only [committedFrameClassWord, List.filterMap_append] using extended
  have initialCut : group.WordCut (first.1.actors actor).1 [] := by
    refine ⟨0, 0, ⟨by omega, ?_⟩, ?_, by simp, ?_⟩
    · intro index event atEvent
      rw [initial event (List.mem_of_getElem? atEvent)]
      simp
    · exact fun impossible => False.elim (Nat.lt_irrefl 0 impossible)
    · intro index event missing; simp at missing
  obtain ⟨cycles, position, finalSegment, known, balance, ordered⟩ := lift execution initialCut
  exact ⟨ordered, cycles, position, finalSegment, known, balance⟩

/-- Qualify the next raw measured commit before asserting a matching source
send/receive pair for that boundary. A fresh operation is checked against its
real reset base; an old completed bit cannot substitute the previous letter.
This proves source-word fidelity, not existence of a callable value or I/O. -/
theorem a_prospective_class_commit_extends_its_measured_source_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    {after : ActorProgressObservation × Nat} {cleared : List Nat} {record : ActorCommitRecord}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : ∀ event ∈ group.rows,
      (first.1.actors actor).1.doneState.done event = false)
    (committed : ActorMeasuredCommit (images actor) choreo session (last.1.actors actor) after cleared record)
    (member : record.step.event ∈ group.rows) :
    sourceFrameWord group.rows
        (committedFrameClassWord group.rows actor records ++ [record.step.event]) ∧
      group.rows[
        (committedFrameClassWord group.rows actor records).length %
          group.rows.length]? = some record.step.event := by
  obtain ⟨ordered, cycles, position, initialSegment, known, balance⟩ :=
    a_measured_distributed_class_has_its_source_word_and_exact_prefix execution initial
  have cut : group.WordCut (last.1.actors actor).1
      (committedFrameClassWord group.rows actor records) :=
    ⟨cycles, position, initialSegment, known, balance, ordered⟩
  obtain ⟨_, _, _, _, _, nextOrdered⟩ := a_measured_class_boundary_extends_its_word_cut committed cut
  obtain ⟨_, _, _, _, _, nextLetter⟩ :=
    a_measured_class_commit_extends_its_actual_word committed initialSegment known balance
  exact ⟨by simpa only [if_pos member] using nextOrdered, nextLetter member⟩

/-- Equal immutable source cycles identify each corresponding letter even
when local row numbers, total progress, and all unrelated route knowledge
differ. The two input words are obtained from the measured-history theorem,
not from assuming matching source identities for each receipt. -/
theorem source_frame_words_match_at_each_common_class_position
    {sentRows receivedRows sentWord receivedWord : List Nat}
    {sentSource receivedSource : Nat → Option Nat} {index sent received : Nat}
    (cycle : sentRows.map sentSource = receivedRows.map receivedSource)
    (sentOrdered : sourceFrameWord sentRows sentWord)
    (receivedOrdered : sourceFrameWord receivedRows receivedWord)
    (sentAt : sentWord[index]? = some sent)
    (receivedAt : receivedWord[index]? = some received) :
    sentSource sent = receivedSource received := by
  have lengthAt := congrArg List.length cycle
  simp only [List.length_map] at lengthAt
  have sentPosition := sentOrdered index sent sentAt
  have receivedPosition := receivedOrdered index received receivedAt
  have first : (sentRows.map sentSource)[index % sentRows.length]? = some (sentSource sent) := by
    simp only [List.getElem?_map, sentPosition, Option.map_some]
  have second : (receivedRows.map receivedSource)[index % sentRows.length]? = some (receivedSource received) := by
    simp only [lengthAt, List.getElem?_map, receivedPosition, Option.map_some]
  exact Option.some.inj (first.symm.trans ((congrArg (fun rows : List (Option Nat) => rows[index % sentRows.length]?) cycle).trans second))

private theorem a_ready_source_row_is_the_next_class_row
    {image : RustDescriptorImage} {choreo : Choreo} {group : SourceOrderGroup image choreo}
    {event position : Nat} {snapshot : ActorProgressObservation} {data : DecodedActorOrder}
    {path : NormalizedPath} {row : DecodedActorRow}
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathMember : path ∈ data.normalized.paths) (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path snapshot row = true)
    (initialSegment : sourceFramePrefix group.rows snapshot position)
    (member : event ∈ group.rows) :
    group.rows[position]? = some event := by
  have rowSame := decoded_actor_row_lookup_preserves_its_index (decoded_actor_order_fields decoded).2.2.2.2.1 rowAt
  have sameData := Option.some.inj (decoded.symm.trans group.decoded)
  subst data
  have readyParts := Bool.and_eq_true_iff.mp ready
  have unfinished : snapshot.doneState.done event = false := by
    simpa only [sourceRowReady, rowSame, Bool.not_eq_true'] using readyParts.1
  have parents : (group.data.sourcePredecessors path event).all snapshot.doneState.done = true := by
    simpa only [rowSame] using readyParts.2
  obtain ⟨index, eventAt⟩ := List.mem_iff_getElem?.mp member
  have later : position ≤ index := by
    have excluded : ¬ index < position := by
      intro before
      have done := (initialSegment.2 index event eventAt).2 before
      rw [unfinished] at done
      cases done
    omega
  have earlier : index ≤ position := by
    by_cases within : index ≤ position
    · exact within
    apply False.elim
    have bound := (List.getElem?_eq_some_iff.mp eventAt).1
    have positionBound : position < group.rows.length := by omega
    let previous := group.rows[position]
    have previousAt : group.rows[position]? = some previous :=
      List.getElem?_eq_some_iff.mpr ⟨positionBound, rfl⟩
    have required := group.ordered path pathMember
      (previous, position) (List.mk_mem_zipIdx_iff_getElem?.mpr previousAt)
      (event, index) (List.mk_mem_zipIdx_iff_getElem?.mpr eventAt)
      (List.any_eq_true.mpr ⟨row, active, beq_iff_eq.mpr rowSame⟩) (by omega)
    have done := List.all_eq_true.mp parents previous required
    have impossible := (initialSegment.2 position previous previousAt).1 done
    omega
  have same : index = position := by omega
  simpa only [same] using eventAt

private theorem a_prepared_source_group_base_has_its_next_word_letter
    {image : RustDescriptorImage} {choreo : Choreo} {previousPath pathIndex event : Nat}
    {group : SourceOrderGroup image choreo} {before : ActorProgressObservation} {word : List Nat}
    {base : ActorProgressObservation} {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    (cut : group.WordCut before word)
    (decoded : image.decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path)
    (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := before, pathIndex, admitted := [] } image choreo previousPath path row base)
    (member : event ∈ group.rows) :
    sourceFrameWord group.rows (word ++ [event]) ∧
      group.rows[word.length % group.rows.length]? = some event := by
  obtain ⟨cycles, position, initialSegment, known, balance, ordered⟩ := cut
  have qualify : ∀ {rounds point},
      sourceFramePrefix group.rows base point →
      word.length = rounds * group.rows.length + point →
      group.rows[word.length % group.rows.length]? = some event := by
    intro rounds point initialPart equation
    have next := a_ready_source_row_is_the_next_class_row decoded (List.mem_of_getElem? pathAt)
      rowAt active ready initialPart member
    have bound := (List.getElem?_eq_some_iff.mp next).1
    have residue : word.length % group.rows.length = point := by
      simp only [equation, Nat.add_mod, Nat.mul_mod, Nat.mod_self, Nat.mul_zero,
        Nat.zero_mod, Nat.zero_add, Nat.mod_eq_of_lt bound]
    simpa only [residue] using next
  have next : group.rows[word.length % group.rows.length]? = some event := by
    cases origin with
    | current agrees => exact qualify initialSegment balance
    | fresh domain rollMember prepared =>
      rcases an_actual_class_reset_keeps_an_empty_or_complete_word_boundary
          prepared initialSegment known with kept | reset
      · exact qualify kept.1 balance
      · rcases reset.1 with empty | full
        · apply qualify reset.2 (rounds := cycles)
          simpa only [empty] using balance
        · apply qualify reset.2 (rounds := cycles + 1)
          simpa only [Nat.add_mul, Nat.one_mul, Nat.add_zero, full] using balance
  exact ⟨a_source_word_extends_by_its_next_actual_letter ordered next, next⟩

/-- Qualify a send or receive directly from its actual current/fresh prepared
base. Neither a successful post-operation snapshot nor sender publication is
used. This states the next source letter, not value custody or I/O readiness. -/
theorem a_prepared_wire_operation_extends_its_actors_measured_source_word
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor previousPath pathIndex event : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    {base : ActorProgressObservation} {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    (execution : DistributedSourceExecution images choreo session first last records)
    (initial : ∀ event ∈ group.rows, (first.1.actors actor).1.doneState.done event = false)
    (decoded : (images actor).decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path) (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := (last.1.actors actor).1, pathIndex, admitted := [] }
      (images actor) choreo previousPath path row base)
    (member : event ∈ group.rows) :
    sourceFrameWord group.rows (committedFrameClassWord group.rows actor records ++ [event]) ∧
      group.rows[(committedFrameClassWord group.rows actor records).length % group.rows.length]? = some event := by
  obtain ⟨ordered, cycles, position, initialSegment, known, balance⟩ :=
    a_measured_distributed_class_has_its_source_word_and_exact_prefix execution initial
  exact a_prepared_source_group_base_has_its_next_word_letter
    ⟨cycles, position, initialSegment, known, balance, ordered⟩ decoded pathAt rowAt active ready origin member

/-- Prepare the next source operation in the general asynchronous chronology,
without the earlier actor-wide acceptance/publication restriction. This supplies
its next committed-word letter before I/O. `SourceActorWord` then retains pending
letters across boundaries and derives the acceptance-inclusive source word. -/
theorem a_prepared_call_operation_has_its_next_committed_source_letter
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session actor previousPath pathIndex event : Nat}
    {group : SourceOrderGroup (images actor) choreo}
    {first last : DistributedSourceCut} {word : List DistributedCallBoundary}
    {entries pending : List SourceAcceptance} {published : List (SourceAcceptance × ActorCommitRecord)}
    {base : ActorProgressObservation} {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    (execution : DistributedCallExecution images choreo session first last word entries published pending)
    (initial : ∀ event ∈ group.rows, (first.actors actor).1.doneState.done event = false)
    (decoded : (images actor).decodeActorOrder? choreo = some data)
    (pathAt : data.normalized.paths[pathIndex]? = some path) (rowAt : data.rows[event]? = some row)
    (active : row ∈ data.activeRows path) (ready : sourceRowReady data path base row = true)
    (origin : ActorSourceCandidateBase
      { snapshot := (last.actors actor).1, pathIndex, admitted := [] }
      (images actor) choreo previousPath path row base)
    (member : event ∈ group.rows) :
    sourceFrameWord group.rows
      (committedFrameClassWord group.rows actor (word.filterMap DistributedCallBoundary.record?) ++ [event]) ∧
      group.rows[(committedFrameClassWord group.rows actor
        (word.filterMap DistributedCallBoundary.record?)).length % group.rows.length]? = some event := by
  obtain ⟨ordered, cycles, position, initialSegment, known, balance⟩ :=
    a_distributed_call_execution_keeps_its_committed_source_group_word execution initial
  exact a_prepared_source_group_base_has_its_next_word_letter
    ⟨cycles, position, initialSegment, known, balance, ordered⟩ decoded pathAt rowAt active ready origin member

end Hibana
