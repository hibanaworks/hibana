private def logicalHandoffRowTable (effect owner expected : Nat) : Bool :=
  ([0, 1, 3] : List Nat).all fun actor =>
    (List.range (distributedImages actor).eventCount).all fun row =>
      match (distributedImages actor).eventEffIndex? row, (distributedImages actor).sourceRowSide? row with
      | some actual, some .send => (actual == effect) == (actor == owner && row == expected)
      | _, _ => true

private theorem logicalHandoffClassifies (effect owner expected : Nat)
    (checked : logicalHandoffRowTable effect owner expected = true)
    (actor row actual : Nat) (roleAt : (distributedImages actor).role = actor)
    (bound : row < (distributedImages actor).eventCount)
    (effectAt : (distributedImages actor).eventEffIndex? row = some actual)
    (sideAt : (distributedImages actor).sourceRowSide? row = some .send) :
    (actual == effect) = (actor == owner && row == expected) := by
  have member : actor ∈ [0, 1, 3] := by
    by_cases one : actor = 1
    · simp [one]
    · by_cases three : actor = 3
      · simp [three]
      · have zero : actor = 0 := by simpa only [distributedImages, if_neg one, if_neg three] using roleAt.symm
        simp [zero]
  have rowCheck := List.all_eq_true.mp (List.all_eq_true.mp checked actor member) row (List.mem_range.mpr bound)
  have equal : ((actual == effect) == (actor == owner && row == expected)) = true := by
    simpa only [effectAt, sideAt] using rowCheck
  exact beq_iff_eq.mp equal

private theorem logicalAcceptedRowCount (entries : List SourceAcceptance) (actor row : Nat) :
    entries.countP (fun entry => entry.sample.image.role == actor && entry.sample.event == row) =
      ((actorAcceptedSends entries actor).map (fun entry => entry.sample.event)).count row := by
  simp only [actorAcceptedSends, List.count_eq_countP, List.countP_map, List.countP_filter, Function.comp_def]
  apply List.countP_congr
  intro entry _
  simp only [Bool.and_comm]

theorem logicalHandoffsBoundUsesInEveryPrefix
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records) :
    ∃ ownerWord workerWord : List Nat,
      sourceFrameWord [0, 1, 2, 3] ownerWord ∧ sourceFrameWord [0, 1, 2, 3] workerWord ∧
      workerWord.count 0 ≤ ownerWord.count 2 ∧ ownerWord.count 3 ≤ workerWord.count 3 ∧
      ownerWord.count 0 = ((actorAcceptedSends entries 0).map (fun entry => entry.sample.event)).count 0 ∧
      workerWord.count 1 = ((actorAcceptedSends entries 1).map (fun entry => entry.sample.event)).count 1 := by
  obtain ⟨ownerPending, _, _, ownerOrdered, ownerSends, ownerReceives⟩ :=
    a_distributed_actor_has_its_acceptance_inclusive_source_word
      (actor := 0) (group := logicalActorGroup0) history rfl (by decide) (by decide)
  obtain ⟨workerPending, _, _, workerOrdered, workerSends, workerReceives⟩ :=
    a_distributed_actor_has_its_acceptance_inclusive_source_word
      (actor := 1) (group := logicalActorGroup1) history rfl (by decide) (by decide)
  let ownerWord := committedFrameClassWord (List.range distributedImage0.eventCount) 0 records ++
    ownerPending.filterMap (fun entry => if entry.sample.event ∈ List.range distributedImage0.eventCount then some entry.sample.event else none)
  let workerWord := committedFrameClassWord (List.range distributedImage1.eventCount) 1 records ++
    workerPending.filterMap (fun entry => if entry.sample.event ∈ List.range distributedImage1.eventCount then some entry.sample.event else none)
  have loanAccepted := a_distributed_acceptance_source_count_has_its_static_row_classification
    history rfl (fun actor row => actor == 0 && row == 2)
    (logicalHandoffClassifies 2 0 2 (by decide))
  have returnAccepted := a_distributed_acceptance_source_count_has_its_static_row_classification
    history rfl (fun actor row => actor == 1 && row == 3)
    (logicalHandoffClassifies 5 1 3 (by decide))
  have loanBound : workerWord.count 0 ≤ ownerWord.count 2 := by
    calc
      workerWord.count 0 = (committedFrameClassWord (List.range distributedImage1.eventCount) 1 records).count 0 :=
        workerReceives 0 (by decide) (by decide)
      _ ≤ (receivedSourceWord distributedImages distributedChoreo records).count 2 :=
        a_distributed_receiving_row_is_counted_by_the_source_receipt_word history (by decide) (by decide)
      _ ≤ (entries.map SourceAcceptance.event).count 2 :=
        a_distributed_source_receipt_count_is_bounded_by_actual_acceptances history 2
      _ = entries.countP (fun entry => entry.sample.image.role == 0 && entry.sample.event == 2) := loanAccepted
      _ = ((actorAcceptedSends entries 0).map (fun entry => entry.sample.event)).count 2 := logicalAcceptedRowCount entries 0 2
      _ = ownerWord.count 2 := (ownerSends 2 (by decide) (by decide)).symm
  have returnBound : ownerWord.count 3 ≤ workerWord.count 3 := by
    calc
      ownerWord.count 3 = (committedFrameClassWord (List.range distributedImage0.eventCount) 0 records).count 3 :=
        ownerReceives 3 (by decide) (by decide)
      _ ≤ (receivedSourceWord distributedImages distributedChoreo records).count 5 :=
        a_distributed_receiving_row_is_counted_by_the_source_receipt_word history (by decide) (by decide)
      _ ≤ (entries.map SourceAcceptance.event).count 5 :=
        a_distributed_source_receipt_count_is_bounded_by_actual_acceptances history 5
      _ = entries.countP (fun entry => entry.sample.image.role == 1 && entry.sample.event == 3) := returnAccepted
      _ = ((actorAcceptedSends entries 1).map (fun entry => entry.sample.event)).count 3 := logicalAcceptedRowCount entries 1 3
      _ = workerWord.count 3 := (workerSends 3 (by decide) (by decide)).symm
  exact ⟨ownerWord, workerWord, ownerOrdered, workerOrdered, loanBound, returnBound,
    ownerSends 0 (by decide) (by decide), workerSends 1 (by decide) (by decide)⟩

theorem logicalAcceptedRequestCountsAreBalanced
    {cut : DistributedSourceCut} {entries : List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) (cut, entries) records) :
    (entries.map SourceAcceptance.event).count 3 ≤ (entries.map SourceAcceptance.event).count 0 ∧
      (entries.map SourceAcceptance.event).count 0 ≤ (entries.map SourceAcceptance.event).count 3 + 1 := by
  obtain ⟨ownerWord, workerWord, ownerOrdered, workerOrdered, loan, returned, ownerUses, workerUses⟩ :=
    logicalHandoffsBoundUsesInEveryPrefix history
  have ownerLoan := ordered_source_word_phase_counts_differ_by_at_most_one
    (leftIndex := 0) (rightIndex := 2) (left := 0) (right := 2) (by decide) ownerOrdered (by decide) (by decide) (by decide)
  have ownerReturn := ordered_source_word_phase_counts_differ_by_at_most_one
    (leftIndex := 0) (rightIndex := 3) (left := 0) (right := 3) (by decide) ownerOrdered (by decide) (by decide) (by decide)
  have workerLoan := ordered_source_word_phase_counts_differ_by_at_most_one
    (leftIndex := 0) (rightIndex := 1) (left := 0) (right := 1) (by decide) workerOrdered (by decide) (by decide) (by decide)
  have workerReturn := ordered_source_word_phase_counts_differ_by_at_most_one
    (leftIndex := 1) (rightIndex := 3) (left := 1) (right := 3) (by decide) workerOrdered (by decide) (by decide) (by decide)
  have firstAccepted := a_distributed_acceptance_source_count_has_its_static_row_classification
    history rfl (fun actor row => actor == 0 && row == 0)
    (logicalHandoffClassifies 0 0 0 (by decide))
  have secondAccepted := a_distributed_acceptance_source_count_has_its_static_row_classification
    history rfl (fun actor row => actor == 1 && row == 1)
    (logicalHandoffClassifies 3 1 1 (by decide))
  rw [logicalAcceptedRowCount entries 0 0, ← ownerUses] at firstAccepted
  rw [logicalAcceptedRowCount entries 1 1, ← workerUses] at secondAccepted
  rw [firstAccepted, secondAccepted]
  constructor <;> omega

private def logicalRequestWord (entries : List SourceAcceptance) : List Nat :=
  entries.filterMap fun entry => if entry.event == 0 || entry.event == 3 then some entry.event else none

private theorem logicalRequestWordCounts (entries : List SourceAcceptance) (event : Nat)
    (selected : event = 0 ∨ event = 3) :
    (logicalRequestWord entries).count event = (entries.map SourceAcceptance.event).count event := by
  simp only [logicalRequestWord, List.count_eq_countP, List.countP_filterMap, List.countP_map]
  apply List.countP_congr
  intro entry _
  by_cases same : entry.event = event
  · rcases selected with rfl | rfl <;> simp [same]
  · by_cases inside : entry.event == 0 || entry.event == 3 <;> simp [same, inside]

theorem logicalRequestSourceWordAtEveryCut
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (history : DistributedSourceExecution distributedImages distributedChoreo 9064
      (distributedCut0, distributedEntries0) last records) :
    sourceFrameWord [0, 3] (last.2.filterMap
      (fun entry => if entry.event == 0 || entry.event == 3 then some entry.event else none)) := by
  change sourceFrameWord [0, 3] (logicalRequestWord last.2)
  induction history with
  | refl => intro index event missing; simp [logicalRequestWord, distributedEntries0] at missing
  | @tail middle finish records logged earlier step ih =>
    have balanced := logicalAcceptedRequestCountsAreBalanced (.tail earlier step)
    cases step with
    | quiet checked => exact ih
    | publish member stamp paired committed => exact ih
    | localCommit actorRole localAt available committed => exact ih
    | receive actorRole received entryAt identity paired available committed => exact ih
    | @accept old entries sample channel sent event previousPath witness label data path row
        imageAt checked decoded pathAt rowAt prepared available freshWitness sourceAt accepted identity =>
      have counts : (logicalRequestWord (entries ++ [⟨sample, channel, event,
          (old.actors sample.image.role).2, witness⟩])).count 3 ≤
          (logicalRequestWord (entries ++ [⟨sample, channel, event,
          (old.actors sample.image.role).2, witness⟩])).count 0 ∧
        (logicalRequestWord (entries ++ [⟨sample, channel, event,
          (old.actors sample.image.role).2, witness⟩])).count 0 ≤
          (logicalRequestWord (entries ++ [⟨sample, channel, event,
          (old.actors sample.image.role).2, witness⟩])).count 3 + 1 := by
        simpa only [logicalRequestWordCounts _ 0 (.inl rfl), logicalRequestWordCounts _ 3 (.inr rfl)] using balanced
      by_cases selected : event = 0 ∨ event = 3
      · have test : (event == 0 || event == 3) = true := by simpa using selected
        have extended := a_two_phase_source_word_extends_when_its_actual_counts_stay_balanced
          (by decide : (0 : Nat) ≠ 3) ih selected
          (by simpa only [logicalRequestWord, List.filterMap_append, List.filterMap_cons,
            List.filterMap_nil, test, if_true] using counts)
        simpa only [logicalRequestWord, List.filterMap_append, List.filterMap_cons,
          List.filterMap_nil, test, if_true] using extended
      · have test : (event == 0 || event == 3) = false := by simpa using selected
        simpa only [logicalRequestWord, List.filterMap_append, List.filterMap_cons,
          List.filterMap_nil, test, Bool.false_eq_true, if_false, List.append_nil] using ih
