import Hibana.SourceReceiptWord
import Hibana.SourceActorWord
import Init.Data.List.Perm

namespace Hibana
open EndpointDelegation

private instance : ReflBEq Nat where
  rfl {a} := by
    change decide (a = a) = true
    exact decide_eq_true rfl

private theorem excluding_one_direction_keeps_every_other_channel
    (entries : List SourceAcceptance) (excluded channel : TransportChannel) :
    sourceChannelEntries (entries.filter (fun entry => !decide (entry.channel = excluded))) channel =
      if channel = excluded then [] else sourceChannelEntries entries channel := by
  by_cases same : channel = excluded
  · subst channel
    simp only [sourceChannelEntries, List.filter_filter, Bool.and_not_self, ite_true]
    induction entries with
    | nil => rfl
    | cons entry rest ih =>
      simpa only [List.filter_cons, Bool.false_eq_true, if_false] using ih
  · simp only [sourceChannelEntries, List.filter_filter, if_neg same]
    apply List.filter_congr
    intro entry _
    by_cases atChannel : entry.channel = channel <;> simp_all

private theorem fifo_prefixes_bound_every_receipt_predicate
    (receipts entries : List SourceAcceptance)
    (prefixes : ∀ channel, List.Sublist (sourceChannelEntries receipts channel) (sourceChannelEntries entries channel))
    (predicate : SourceAcceptance → Bool) :
    receipts.countP predicate ≤ entries.countP predicate := by
  cases receipts with
  | nil => simp
  | cons head rest =>
    let belongs : SourceAcceptance → Bool := fun entry => decide (entry.channel = head.channel)
    have within : (sourceChannelEntries (head :: rest) head.channel).countP predicate ≤
        (sourceChannelEntries entries head.channel).countP predicate := (prefixes head.channel).countP_le
    have outsidePrefixes : ∀ channel,
        List.Sublist (sourceChannelEntries ((head :: rest).filter (fun entry => !belongs entry)) channel)
          (sourceChannelEntries (entries.filter (fun entry => !belongs entry)) channel) := by
      intro channel
      change List.Sublist (sourceChannelEntries ((head :: rest).filter (fun entry => !decide (entry.channel = head.channel))) channel) _
      rw [excluding_one_direction_keeps_every_other_channel,
        excluding_one_direction_keeps_every_other_channel]
      split
      · exact .refl _
      · exact prefixes channel
    have outside := fifo_prefixes_bound_every_receipt_predicate
      ((head :: rest).filter (fun entry => !belongs entry))
      (entries.filter (fun entry => !belongs entry)) outsidePrefixes predicate
    have before := List.countP_eq_countP_filter_add (head :: rest) predicate belongs
    have after := List.countP_eq_countP_filter_add entries predicate belongs
    change ((head :: rest).filter belongs).countP predicate ≤ (entries.filter belongs).countP predicate at within
    omega
termination_by receipts.length
decreasing_by
  simp only [List.filter_cons, decide_true, Bool.not_true, Bool.false_eq_true, if_false]
  have bound := List.length_filter_le (fun entry : SourceAcceptance => !decide (entry.channel = head.channel)) rest
  simp_all only [List.length_cons]
  omega

/-- The accepted-frame list bounds every actual source receipt count, across
all physical directions and holders. No finite channel catalogue, common
carrier generation or common actor visit is assumed. The counters in this
statement are erased counts of actual observations, not runtime fields. -/
theorem a_distributed_source_receipt_count_is_bounded_by_actual_acceptances
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat}
    {last : DistributedSourceCut × List SourceAcceptance}
    {records : List (Nat × ActorCommitRecord)}
    (execution : DistributedSourceExecution images choreo session
      (DistributedSourceCut.initial initial, []) last records) (event : Nat) :
    (receivedSourceWord images choreo records).count event ≤
      (last.2.map SourceAcceptance.event).count event := by
  obtain ⟨receipts, word, prefixes⟩ := a_distributed_receipt_word_is_the_exact_accepted_fifo_prefix execution
  have subsequences : ∀ channel, List.Sublist (sourceChannelEntries receipts channel) (sourceChannelEntries last.2 channel) := by
    intro channel
    rw [prefixes channel]
    exact List.take_sublist _ _
  have bound := fifo_prefixes_bound_every_receipt_predicate receipts last.2 subsequences
    (fun entry => entry.event == event)
  simpa only [word, List.count_eq_countP, List.countP_map] using bound

private theorem ordered_source_phases_have_their_exact_prefix_difference
    {rows word : List Nat} {leftIndex rightIndex left right : Nat}
    (distinct : rows.Nodup) (ordered : sourceFrameWord rows word)
    (leftAt : rows[leftIndex]? = some left) (rightAt : rows[rightIndex]? = some right)
    (before : leftIndex < rightIndex) :
    ∀ count, count ≤ word.length →
      (word.take count).count left = (word.take count).count right +
        if leftIndex < count % rows.length ∧ count % rows.length ≤ rightIndex then 1 else 0 := by
  have leftBound := (List.getElem?_eq_some_iff.mp leftAt).1
  have rightBound := (List.getElem?_eq_some_iff.mp rightAt).1
  have positive : 0 < rows.length := by omega
  have two : 1 < rows.length := by omega
  intro count
  induction count with
  | zero => intro bound; simp
  | succ count ih =>
    intro bound
    have within : count < word.length := by omega
    have prior := ih (by omega)
    have nextAt := ordered count word[count] (List.getElem?_eq_some_iff.mpr ⟨within, rfl⟩)
    have positionBound := Nat.mod_lt count positive
    have leftNext : word[count] = left ↔ count % rows.length = leftIndex := by
      constructor
      · intro same
        have atLeft : rows[count % rows.length]? = some left := by simpa only [same] using nextAt
        apply (List.getElem?_inj positionBound distinct).mp
        exact atLeft.trans leftAt.symm
      · intro same
        exact Option.some.inj ((by simpa only [same] using nextAt : rows[leftIndex]? = some word[count]).symm.trans leftAt)
    have rightNext : word[count] = right ↔ count % rows.length = rightIndex := by
      constructor
      · intro same
        have atRight : rows[count % rows.length]? = some right := by simpa only [same] using nextAt
        apply (List.getElem?_inj positionBound distinct).mp
        exact atRight.trans rightAt.symm
      · intro same
        exact Option.some.inj ((by simpa only [same] using nextAt : rows[rightIndex]? = some word[count]).symm.trans rightAt)
    have nextPosition :
        (count + 1) % rows.length =
          if count % rows.length + 1 = rows.length then 0 else count % rows.length + 1 := by
      rw [Nat.add_mod, Nat.mod_eq_of_lt two]
      by_cases wraps : count % rows.length + 1 = rows.length
      · simp only [wraps, Nat.mod_self, if_true]
      · have below : count % rows.length + 1 < rows.length := by omega
        simp only [Nat.mod_eq_of_lt below, if_neg wraps]
    rw [List.take_succ_eq_append_getElem within]
    have leftIncrement : ([word[count]] : List Nat).count left =
        if count % rows.length = leftIndex then 1 else 0 := by
      simp only [List.count_singleton, beq_iff_eq, leftNext]
    have rightIncrement : ([word[count]] : List Nat).count right =
        if count % rows.length = rightIndex then 1 else 0 := by
      simp only [List.count_singleton, beq_iff_eq, rightNext]
    simp only [List.count_append, leftIncrement, rightIncrement]
    rw [nextPosition]
    clear leftNext rightNext
    by_cases wrap : count % rows.length + 1 = rows.length
    all_goals
      simp only [wrap, if_true, if_false]
      by_cases atLeft : count % rows.length = leftIndex <;>
        by_cases atRight : count % rows.length = rightIndex <;>
        by_cases oldLeft : leftIndex < count % rows.length <;>
        by_cases oldRight : count % rows.length ≤ rightIndex <;>
        by_cases newLeft : leftIndex < count % rows.length + 1 <;>
        by_cases newRight : count % rows.length + 1 ≤ rightIndex
      all_goals simp_all only [if_true, if_false, Nat.not_lt_zero, false_and, true_and, and_true,
        Nat.le_refl, Nat.lt_irrefl, not_true_eq_false, not_false_eq_true, Nat.add_zero]
      all_goals first | exact False.elim (by omega) | omega


/-- Local phase counts in a derived cyclic source word differ by at most
one. The count is erased from the word; it is not an extra runtime iteration. -/
theorem ordered_source_word_phase_counts_differ_by_at_most_one
    {rows word : List Nat} {leftIndex rightIndex left right : Nat}
    (distinct : rows.Nodup) (ordered : sourceFrameWord rows word)
    (leftAt : rows[leftIndex]? = some left) (rightAt : rows[rightIndex]? = some right)
    (before : leftIndex < rightIndex) :
    word.count right ≤ word.count left ∧ word.count left ≤ word.count right + 1 := by
  have equation := ordered_source_phases_have_their_exact_prefix_difference
    distinct ordered leftAt rightAt before word.length (Nat.le_refl _)
  simp only [List.take_length] at equation
  by_cases active : leftIndex < word.length % rows.length ∧ word.length % rows.length ≤ rightIndex
  · simp only [if_pos active] at equation
    constructor <;> omega
  · simp only [if_neg active] at equation
    constructor <;> omega

/-- A real new letter cannot overtake the two-phase source order when the
resulting actual counts remain balanced. This is used after deriving the
balance from local words and real handoff/return FIFO histories. -/
theorem a_two_phase_source_word_extends_when_its_actual_counts_stay_balanced
    {first second event : Nat} {word : List Nat}
    (different : first ≠ second) (ordered : sourceFrameWord [first, second] word)
    (letter : event = first ∨ event = second)
    (balance : (word ++ [event]).count second ≤ (word ++ [event]).count first ∧
      (word ++ [event]).count first ≤ (word ++ [event]).count second + 1) :
    sourceFrameWord [first, second] (word ++ [event]) := by
  have distinct : [first, second].Nodup := by simp [different]
  have equation := ordered_source_phases_have_their_exact_prefix_difference
    distinct ordered (leftIndex := 0) (rightIndex := 1) (left := first) (right := second)
    rfl rfl (by decide) word.length (Nat.le_refl _)
  simp only [List.take_length, List.length_cons, List.length_nil] at equation
  have bound := Nat.mod_lt word.length (by decide : 0 < 2)
  have next : [first, second][word.length % 2]? = some event := by
    rcases letter with isFirst | isSecond
    · rw [isFirst] at balance ⊢
      by_cases zero : word.length % 2 = 0
      · simp [zero]
      · have one : word.length % 2 = 1 := by omega
        have shifted : word.count first = word.count second + 1 := by simpa [one] using equation
        simp [different] at balance
        exact False.elim (by omega)
    · rw [isSecond] at balance ⊢
      by_cases one : word.length % 2 = 1
      · simp [one]
      · have zero : word.length % 2 = 0 := by omega
        have aligned : word.count first = word.count second := by simpa [zero] using equation
        simp [Ne.symm different] at balance
        exact False.elim (by omega)
  intro index actual atIndex
  by_cases earlier : index < word.length
  · have previous : word[index]? = some actual := by
      simpa only [List.getElem?_append_left earlier] using atIndex
    exact ordered index actual previous
  · have bound : index < word.length + 1 := by
      simpa only [List.length_append, List.length_singleton] using (List.getElem?_eq_some_iff.mp atIndex).1
    have last : index = word.length := by omega
    subst index
    have same : event = actual := by simpa using atIndex
    exact next.trans (congrArg some same)

/-- Every predicate of actual receipts is bounded by physical acceptances in
the same asynchronous chronology, across all directions and changing holders.
No common generation, finite channel catalogue or actor visit is required. -/
theorem a_distributed_call_receipt_predicate_is_bounded_by_actual_acceptances
    {images : Nat → RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {initial : Nat → ActorProgressObservation × Nat} {last : DistributedSourceCut}
    {word : List DistributedCallBoundary} {entries pending : List SourceAcceptance}
    {published : List (SourceAcceptance × ActorCommitRecord)}
    (execution : DistributedCallExecution images choreo session
      (.initial initial) last word entries published pending) (selected : Nat → Bool) :
    (receivedSourceWord images choreo (word.filterMap DistributedCallBoundary.record?)).countP selected ≤
      (entries.map SourceAcceptance.event).countP selected := by
  obtain ⟨receipts, actual, prefixes⟩ := a_distributed_call_receipt_word_is_the_exact_accepted_fifo_prefix execution
  have subsequences : ∀ channel, List.Sublist (sourceChannelEntries receipts channel) (sourceChannelEntries entries channel) := by
    intro channel
    rw [prefixes channel]
    exact List.take_sublist _ _
  have bound := fifo_prefixes_bound_every_receipt_predicate receipts entries subsequences
    (fun entry => selected entry.event)
  simpa only [actual, List.countP_map, Function.comp_def] using bound

end Hibana
