import Hibana.DescriptorImage

namespace Hibana

-- Keep Nat equality on its direct decision procedure; the generic Ord
-- instance carries an unnecessary classical proof dependency.
private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

/-- A successful byte search cannot select a color already in its actual
prior assignments. Exhaustion remains the unencodable sentinel 256. -/
theorem first_elastic_frame_color_avoids_its_actual_used_colors
    {used : List Nat} (encodable : firstElasticFrameColor used < 256) :
    firstElasticFrameColor used ∉ used := by
  unfold firstElasticFrameColor at encodable ⊢
  cases found : (List.range 256).find? (fun color => !used.contains color) with
  | none => simp only [found] at encodable; omega
  | some color =>
    intro member
    have free := List.find?_some found
    have present : used.contains color = true := by simpa using member
    simp only [present, Bool.not_true] at free
    cases free

private theorem an_elastic_assignment_retains_every_matching_prior_domain
    {markers : List DecodedScopeMarker} {prior : List ElasticFrameAssignment}
    {index : Nat} {atom : ProgramAtomBody} {entry : ElasticFrameAssignment}
    (member : entry ∈ prior)
    (sender : entry.original.sender = atom.sender)
    (receiver : entry.original.receiver = atom.receiver)
    (lane : entry.original.lane = atom.lane)
    (remote : atom.sender ≠ atom.receiver)
    (encodable : (elasticFrameAssignment markers prior index atom).color < 256)
    (color : entry.color = (elasticFrameAssignment markers prior index atom).color) :
    entry.original.frameLabel = atom.frameLabel ∧
      entry.owner = elasticFrameOwner markers index := by
  have chosen : (elasticFrameAssignment markers prior index atom).color =
      firstElasticFrameColor (elasticFrameUsedColors prior atom (elasticFrameOwner markers index)) := by
    simp only [elasticFrameAssignment, beq_iff_eq, if_neg remote]
  have unused := first_elastic_frame_color_avoids_its_actual_used_colors (chosen ▸ encodable)
  by_cases original : entry.original.frameLabel = atom.frameLabel
  · by_cases owner : entry.owner = elasticFrameOwner markers index
    · exact ⟨original, owner⟩
    · have used : entry.color ∈ elasticFrameUsedColors prior atom (elasticFrameOwner markers index) := by
        apply List.mem_filterMap.mpr
        exact ⟨entry, member, by simp [sender, receiver, lane, owner]⟩
      exact False.elim (unused ((color.trans chosen) ▸ used))
  · have used : entry.color ∈ elasticFrameUsedColors prior atom (elasticFrameOwner markers index) := by
      apply List.mem_filterMap.mpr
      exact ⟨entry, member, by simp [sender, receiver, lane, original]⟩
    exact False.elim (unused ((color.trans chosen) ▸ used))

private theorem a_later_elastic_output_retains_every_matching_prior_domain
    {markers : List DecodedScopeMarker} {prior : List ElasticFrameAssignment}
    {index offset : Nat} {atoms : List ProgramAtomBody}
    {original output : ProgramAtomBody} {entry : ElasticFrameAssignment}
    (member : entry ∈ prior)
    (sourceAt : atoms[offset]? = some original)
    (outputAt : (separateElasticFrameDomainsFrom markers prior index atoms)[offset]? = some output)
    (sender : entry.original.sender = original.sender)
    (receiver : entry.original.receiver = original.receiver)
    (lane : entry.original.lane = original.lane)
    (remote : original.sender ≠ original.receiver)
    (encodable : output.frameLabel < 256)
    (color : entry.color = output.frameLabel) :
    entry.original.frameLabel = original.frameLabel ∧
      entry.owner = elasticFrameOwner markers (index + offset) := by
  induction atoms generalizing prior index offset with
  | nil => simp at sourceAt
  | cons atom rest ih =>
    cases offset with
    | zero =>
      simp only [List.getElem?_cons_zero, Option.some.injEq] at sourceAt
      subst original
      simp only [separateElasticFrameDomainsFrom, List.getElem?_cons_zero, Option.some.injEq] at outputAt
      subst output
      simpa only [Nat.add_zero] using
        an_elastic_assignment_retains_every_matching_prior_domain
          member sender receiver lane remote encodable color
    | succ offset =>
      have sourceTail : rest[offset]? = some original := by simpa using sourceAt
      have outputTail : (separateElasticFrameDomainsFrom markers
          (prior ++ [elasticFrameAssignment markers prior index atom]) (index + 1) rest)[offset]? = some output := by
        simpa only [separateElasticFrameDomainsFrom, List.getElem?_cons_succ] using outputAt
      have nextMember : entry ∈ prior ++ [elasticFrameAssignment markers prior index atom] :=
        List.mem_append_left _ member
      simpa only [Nat.add_assoc, Nat.add_comm 1 offset] using
        ih nextMember sourceTail outputTail

/-- The actual final allocation function refines both its original route
color and its innermost elastic owner. This is proved from its append-only
assignment prefix, without a Covers or SameClassUnique premise. It holds for
arbitrary input lists and markers, including nested/coextensive intervals;
separate source facts are needed to interpret owner equality as full ancestry. -/
theorem same_elastic_wire_color_retains_its_original_and_owner
    {markers : List DecodedScopeMarker} {prior : List ElasticFrameAssignment}
    {index left right : Nat} {atoms : List ProgramAtomBody}
    {leftSource rightSource leftOutput rightOutput : ProgramAtomBody}
    (ordered : left < right)
    (leftAt : atoms[left]? = some leftSource)
    (rightAt : atoms[right]? = some rightSource)
    (leftOutputAt : (separateElasticFrameDomainsFrom markers prior index atoms)[left]? = some leftOutput)
    (rightOutputAt : (separateElasticFrameDomainsFrom markers prior index atoms)[right]? = some rightOutput)
    (sender : leftSource.sender = rightSource.sender)
    (receiver : leftSource.receiver = rightSource.receiver)
    (lane : leftSource.lane = rightSource.lane)
    (remote : rightSource.sender ≠ rightSource.receiver)
    (encodable : rightOutput.frameLabel < 256)
    (color : leftOutput.frameLabel = rightOutput.frameLabel) :
    leftSource.frameLabel = rightSource.frameLabel ∧
      elasticFrameOwner markers (index + left) = elasticFrameOwner markers (index + right) := by
  induction atoms generalizing prior index left right with
  | nil => simp at leftAt
  | cons atom rest ih =>
    cases left with
    | zero =>
      simp only [List.getElem?_cons_zero, Option.some.injEq] at leftAt
      subst leftSource
      simp only [separateElasticFrameDomainsFrom, List.getElem?_cons_zero, Option.some.injEq] at leftOutputAt
      subst leftOutput
      cases right with
      | zero => omega
      | succ right =>
        have sourceTail : rest[right]? = some rightSource := by simpa using rightAt
        have outputTail : (separateElasticFrameDomainsFrom markers
            (prior ++ [elasticFrameAssignment markers prior index atom]) (index + 1) rest)[right]? = some rightOutput := by
          simpa only [separateElasticFrameDomainsFrom, List.getElem?_cons_succ] using rightOutputAt
        have member : elasticFrameAssignment markers prior index atom ∈
            prior ++ [elasticFrameAssignment markers prior index atom] :=
          List.mem_append_right _ (List.mem_singleton_self _)
        have refined := a_later_elastic_output_retains_every_matching_prior_domain
          member sourceTail outputTail sender receiver lane remote encodable color
        simpa only [elasticFrameAssignment, Nat.add_zero, Nat.add_assoc, Nat.add_comm 1 right] using refined
    | succ left =>
      cases right with
      | zero => omega
      | succ right =>
        have sourceLeft : rest[left]? = some leftSource := by simpa using leftAt
        have sourceRight : rest[right]? = some rightSource := by simpa using rightAt
        have outputLeft : (separateElasticFrameDomainsFrom markers
            (prior ++ [elasticFrameAssignment markers prior index atom]) (index + 1) rest)[left]? = some leftOutput := by
          simpa only [separateElasticFrameDomainsFrom, List.getElem?_cons_succ] using leftOutputAt
        have outputRight : (separateElasticFrameDomainsFrom markers
            (prior ++ [elasticFrameAssignment markers prior index atom]) (index + 1) rest)[right]? = some rightOutput := by
          simpa only [separateElasticFrameDomainsFrom, List.getElem?_cons_succ] using rightOutputAt
        have refined := ih (Nat.lt_of_succ_lt_succ ordered)
          sourceLeft sourceRight outputLeft outputRight
        simpa only [Nat.add_assoc, Nat.add_comm 1 left, Nat.add_comm 1 right] using refined

/-- Apply the proved final pass directly to the canonical source used by
owned/ordinary source and descriptor checks. No abstract conflict graph or
assumed selector uniqueness is substituted for this allocation. -/
theorem a_canonical_rolled_wire_alias_retains_its_actual_source_domain
    {choreo : Choreo} {left right : Nat}
    {leftSource rightSource leftOutput rightOutput : ProgramAtomBody}
    (rolled : (canonicalControlSource choreo).markers.any
      (fun marker => marker.scope / 8192 == 1 && marker.tag % 4 == 0) = true)
    (ordered : left < right)
    (leftAt : (choreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody)[left]? = some leftSource)
    (rightAt : (choreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody)[right]? = some rightSource)
    (leftOutputAt : (canonicalProgramSource choreo).atoms[left]? = some leftOutput)
    (rightOutputAt : (canonicalProgramSource choreo).atoms[right]? = some rightOutput)
    (sender : leftSource.sender = rightSource.sender)
    (receiver : leftSource.receiver = rightSource.receiver)
    (lane : leftSource.lane = rightSource.lane)
    (remote : rightSource.sender ≠ rightSource.receiver)
    (encodable : rightOutput.frameLabel < 256)
    (color : leftOutput.frameLabel = rightOutput.frameLabel) :
    leftSource.frameLabel = rightSource.frameLabel ∧
      elasticFrameOwner (canonicalControlSource choreo).markers left =
        elasticFrameOwner (canonicalControlSource choreo).markers right := by
  have first : (separateElasticFrameDomainsFrom (canonicalControlSource choreo).markers [] 0
      (choreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody))[left]? = some leftOutput := by
    simpa only [canonicalProgramSource, separateElasticFrameDomains, rolled, if_true] using leftOutputAt
  have second : (separateElasticFrameDomainsFrom (canonicalControlSource choreo).markers [] 0
      (choreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody))[right]? = some rightOutput := by
    simpa only [canonicalProgramSource, separateElasticFrameDomains, rolled, if_true] using rightOutputAt
  simpa only [Nat.zero_add] using
    same_elastic_wire_color_retains_its_original_and_owner ordered leftAt rightAt
      first second sender receiver lane remote encodable color

end Hibana
