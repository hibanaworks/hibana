import Hibana.GlobalSyntax

namespace Hibana

-- Keep Nat equality on its direct decision procedure; the generic Ord
-- instance carries an unnecessary classical proof dependency.
private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

private instance : LawfulBEq RouteArm where
  eq_of_beq {first second} equal := by
    cases first <;> cases second <;> first | rfl | cases equal
  rfl {arm} := by cases arm <;> rfl

private instance : LawfulBEq ConflictArm where
  eq_of_beq {left right} equal := by
    cases left with
    | mk scope arm =>
      cases right with
      | mk other chosen =>
        change (scope == other && arm == chosen) = true at equal
        have fields : scope = other ∧ arm = chosen := by
          simpa only [Bool.and_eq_true, beq_iff_eq] using equal
        cases fields.1
        cases fields.2
        rfl
  rfl {entry} := by
    cases entry with
    | mk scope arm =>
      change (scope == scope && arm == arm) = true
      simp

private def rollFrameClassesSeparate (classes : List RollFrameLabelClass) : Prop :=
  ∀ left ∈ classes, ∀ right ∈ classes,
    left.sender = right.sender → left.receiver = right.receiver → left.lane = right.lane →
    left.frameLabel = right.frameLabel → right.frameLabel < 256 → left.conflicts = right.conflicts

private def newRollFrameClass (classes : List RollFrameLabelClass)
    (occurrence : CompiledOccurrence) (conflicts : List ConflictArm) : RollFrameLabelClass :=
  let used := (classes.filter fun entry => entry.sameInboundKey occurrence).map RollFrameLabelClass.frameLabel
  { sender := occurrence.sender, receiver := occurrence.receiver, lane := occurrence.lane, conflicts,
    frameLabel := match firstAvailableFrameLabel used with | some label => label | none => 256 }

private theorem a_fresh_roll_color_does_not_equal_a_prior_inbound_color
    {classes : List RollFrameLabelClass} {occurrence : CompiledOccurrence}
    {conflicts : List ConflictArm} {entry : RollFrameLabelClass}
    (member : entry ∈ classes)
    (sender : entry.sender = occurrence.sender) (receiver : entry.receiver = occurrence.receiver)
    (lane : entry.lane = occurrence.lane)
    (encodable : (newRollFrameClass classes occurrence conflicts).frameLabel < 256) :
    entry.frameLabel ≠ (newRollFrameClass classes occurrence conflicts).frameLabel := by
  let used := (classes.filter fun entry => entry.sameInboundKey occurrence).map RollFrameLabelClass.frameLabel
  have usedAt : entry.frameLabel ∈ used := by
    apply List.mem_map.mpr
    exact ⟨entry, List.mem_filter.mpr ⟨member, by simp [RollFrameLabelClass.sameInboundKey, sender, receiver, lane]⟩, rfl⟩
  change entry.frameLabel ≠ (match firstAvailableFrameLabel used with | some color => color | none => 256)
  change (match firstAvailableFrameLabel used with | some color => color | none => 256) < 256 at encodable
  cases found : firstAvailableFrameLabel used with
  | none => simp only [found] at encodable; omega
  | some color =>
    have unused : color ∉ used := by
      have selected := List.find?_some found
      simpa using selected
    intro same
    exact unused ((by simpa only [found] using same : entry.frameLabel = color) ▸ usedAt)

private theorem a_fresh_roll_class_preserves_separation
    {classes : List RollFrameLabelClass} {occurrence : CompiledOccurrence} {conflicts : List ConflictArm}
    (separate : rollFrameClassesSeparate classes) :
    rollFrameClassesSeparate (classes ++ [newRollFrameClass classes occurrence conflicts]) := by
  intro left leftAt right rightAt sender receiver lane color encodable
  rcases List.mem_append.mp leftAt with oldLeft | newLeft
  · rcases List.mem_append.mp rightAt with oldRight | newRight
    · exact separate left oldLeft right oldRight sender receiver lane color encodable
    · have same : right = newRollFrameClass classes occurrence conflicts := List.mem_singleton.mp newRight
      subst right
      exact False.elim (a_fresh_roll_color_does_not_equal_a_prior_inbound_color
        oldLeft sender receiver lane encodable color)
  · have same : left = newRollFrameClass classes occurrence conflicts := List.mem_singleton.mp newLeft
    subst left
    rcases List.mem_append.mp rightAt with oldRight | newRight
    · have freshBound : (newRollFrameClass classes occurrence conflicts).frameLabel < 256 := color ▸ encodable
      exact False.elim (a_fresh_roll_color_does_not_equal_a_prior_inbound_color
        oldRight sender.symm receiver.symm lane.symm freshBound color.symm)
    · have same : right = newRollFrameClass classes occurrence conflicts := List.mem_singleton.mp newRight
      subst right
      rfl

private theorem a_roll_color_step_keeps_its_actual_class_prefix
    {conflictRows : List (List ConflictArm)} {classes : List RollFrameLabelClass}
    {index : Nat} {occurrence : CompiledOccurrence}
    (separate : rollFrameClassesSeparate classes) :
    rollFrameClassesSeparate (recolorRollOccurrence conflictRows classes index occurrence).2 ∧
      (∀ entry ∈ classes, entry ∈ (recolorRollOccurrence conflictRows classes index occurrence).2) := by
  unfold recolorRollOccurrence
  split
  · exact ⟨separate, fun _ member => member⟩
  · split
    · exact ⟨separate, fun _ member => member⟩
    · split
      · exact ⟨separate, fun _ member => member⟩
      · exact ⟨a_fresh_roll_class_preserves_separation separate, fun _ member => List.mem_append_left _ member⟩

private theorem a_roll_color_step_has_its_exact_source_class
    {conflictRows : List (List ConflictArm)} {classes : List RollFrameLabelClass}
    {index : Nat} {occurrence : CompiledOccurrence} {conflicts : List ConflictArm}
    (remote : occurrence.sender ≠ occurrence.receiver)
    (atRow : conflictRows[index]? = some conflicts) :
    ∃ entry ∈ (recolorRollOccurrence conflictRows classes index occurrence).2,
      entry.matches occurrence conflicts = true ∧
      entry.frameLabel = (recolorRollOccurrence conflictRows classes index occurrence).1.frameLabel := by
  simp only [recolorRollOccurrence, beq_iff_eq, if_neg remote, atRow]
  cases found : classes.find? (fun entry => entry.matches occurrence conflicts) with
  | some entry =>
    exact ⟨entry, List.mem_of_find?_eq_some found,
      List.find?_some (p := fun entry : RollFrameLabelClass => entry.matches occurrence conflicts) found, rfl⟩
  | none =>
    exact ⟨newRollFrameClass classes occurrence conflicts,
      List.mem_append_right _ (List.mem_singleton_self _), by simp [newRollFrameClass, RollFrameLabelClass.matches], rfl⟩

private theorem a_roll_recoloring_run_retains_its_actual_class_evidence
    {conflictRows : List (List ConflictArm)} {classes : List RollFrameLabelClass}
    {index : Nat} {occurrences : List CompiledOccurrence}
    (separate : rollFrameClassesSeparate classes) :
    ∃ finalClasses, rollFrameClassesSeparate finalClasses ∧
      (∀ entry ∈ classes, entry ∈ finalClasses) ∧
      ∀ offset occurrence output conflicts,
        occurrences[offset]? = some occurrence →
        (recolorRollOccurrencesFrom conflictRows classes index occurrences)[offset]? = some output →
        occurrence.sender ≠ occurrence.receiver → conflictRows[index + offset]? = some conflicts →
        ∃ entry ∈ finalClasses, entry.matches occurrence conflicts = true ∧ entry.frameLabel = output.frameLabel := by
  induction occurrences generalizing classes index with
  | nil => exact ⟨classes, separate, fun _ member => member, by intro offset occurrence output conflicts missing; simp at missing⟩
  | cons occurrence rest ih =>
    have step := a_roll_color_step_keeps_its_actual_class_prefix
      (conflictRows := conflictRows) (index := index) (occurrence := occurrence) separate
    obtain ⟨finalClasses, finalSeparate, tailSubset, tailEvidence⟩ := ih step.1
    refine ⟨finalClasses, finalSeparate, fun entry member => tailSubset entry (step.2 entry member), ?_⟩
    intro offset source output conflicts sourceAt outputAt remote rowAt
    cases offset with
    | zero =>
      have same : source = occurrence := (Option.some.inj sourceAt).symm
      subst source
      have outputSame : (recolorRollOccurrence conflictRows classes index occurrence).1 = output := by
        simpa only [recolorRollOccurrencesFrom, List.getElem?_cons_zero, Option.some.injEq] using outputAt
      obtain ⟨entry, member, matched, colored⟩ := a_roll_color_step_has_its_exact_source_class remote (by simpa using rowAt)
      exact ⟨entry, tailSubset entry member, matched, outputSame ▸ colored⟩
    | succ offset =>
      apply tailEvidence offset source output conflicts (by simpa using sourceAt)
      · simpa only [recolorRollOccurrencesFrom, List.getElem?_cons_succ] using outputAt
      · exact remote
      · simpa only [Nat.add_assoc, Nat.add_comm 1 offset] using rowAt

private theorem a_roll_class_match_has_its_exact_fields
    {entry : RollFrameLabelClass} {occurrence : CompiledOccurrence} {conflicts : List ConflictArm}
    (matched : entry.matches occurrence conflicts = true) :
    entry.sender = occurrence.sender ∧ entry.receiver = occurrence.receiver ∧
      entry.lane = occurrence.lane ∧ entry.conflicts = conflicts := by
  simpa only [RollFrameLabelClass.matches, Bool.and_eq_true, beq_iff_eq, and_assoc] using matched

/-- The actual roll recoloring algorithm separates different complete source
route paths for an inbound key. Its class invariant is derived from the empty
initial prefix, not assumed as SameClassUnique or an abstract conflict cover.
Missing path rows and palette exhaustion remain unencodable. -/
theorem same_roll_wire_color_retains_its_exact_source_route_path
    {occurrences : List CompiledOccurrence} {conflictRows : List (List ConflictArm)}
    {left right : Nat} {leftSource rightSource leftOutput rightOutput : CompiledOccurrence}
    {leftPath rightPath : List ConflictArm}
    (leftAt : occurrences[left]? = some leftSource)
    (rightAt : occurrences[right]? = some rightSource)
    (leftOutputAt : (recolorRollOccurrences occurrences conflictRows)[left]? = some leftOutput)
    (rightOutputAt : (recolorRollOccurrences occurrences conflictRows)[right]? = some rightOutput)
    (leftPathAt : conflictRows[left]? = some leftPath)
    (rightPathAt : conflictRows[right]? = some rightPath)
    (sender : leftSource.sender = rightSource.sender)
    (receiver : leftSource.receiver = rightSource.receiver)
    (lane : leftSource.lane = rightSource.lane)
    (remote : rightSource.sender ≠ rightSource.receiver)
    (encodable : rightOutput.frameLabel < 256)
    (color : leftOutput.frameLabel = rightOutput.frameLabel) : leftPath = rightPath := by
  have initial : rollFrameClassesSeparate [] := by intro _ missing; simp at missing
  obtain ⟨classes, separated, _, evidence⟩ := a_roll_recoloring_run_retains_its_actual_class_evidence
    (conflictRows := conflictRows) (index := 0) (occurrences := occurrences) initial
  have leftRemote : leftSource.sender ≠ leftSource.receiver := by simpa only [sender, receiver] using remote
  obtain ⟨leftEntry, leftMember, leftMatch, leftColor⟩ := evidence left leftSource leftOutput leftPath
    leftAt leftOutputAt leftRemote (by simpa using leftPathAt)
  obtain ⟨rightEntry, rightMember, rightMatch, rightColor⟩ := evidence right rightSource rightOutput rightPath
    rightAt rightOutputAt remote (by simpa using rightPathAt)
  have lf := a_roll_class_match_has_its_exact_fields leftMatch
  have rf := a_roll_class_match_has_its_exact_fields rightMatch
  have same := separated leftEntry leftMember rightEntry rightMember
    (lf.1.trans (sender.trans rf.1.symm))
    (lf.2.1.trans (receiver.trans rf.2.1.symm))
    (lf.2.2.1.trans (lane.trans rf.2.2.1.symm))
    (leftColor.trans (color.trans rightColor.symm)) (rightColor ▸ encodable)
  exact lf.2.2.2.symm.trans (same.trans rf.2.2.2)

end Hibana
