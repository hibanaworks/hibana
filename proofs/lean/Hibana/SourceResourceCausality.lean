import Hibana.SourceResourceOrder
import Hibana.ActorSourceProgress

namespace Hibana
open EndpointDelegation

/-- Static occurrence coordinates stay within their source fragment. This is
not a temporal order between independent occurrences. -/
theorem source_occurrence_is_within_its_fragment
    {source : Source} {start event : Nat} {message : Message}
    (present : (event, message) ∈ source.messageEntries start) :
    start ≤ event ∧ event < start + source.eventCount := by
  induction source generalizing start with
  | send original =>
      have same := congrArg Prod.fst (List.mem_singleton.mp present)
      dsimp only at same
      simp [Source.eventCount, same]
  | seq left right leftIH rightIH | par left right leftIH rightIH | route resolver left right leftIH rightIH =>
      rcases List.mem_append.mp present with fromLeft | fromRight
      · have inside := leftIH fromLeft
        simp only [Source.eventCount]
        constructor <;> omega
      · have inside := rightIH fromRight
        simp only [Source.eventCount]
        constructor <;> omega
  | roll body bodyIH => exact bodyIH present

theorem composition_preserves_left_fragment_causality
    (left right : Source) (resolver : Option Nat) {first second : Nat}
    (firstInside : first < left.eventCount) (secondInside : second < left.eventCount) :
    (Source.seq left right).before first second = left.before first second ∧
    (Source.par left right).before first second = left.before first second ∧
    (Source.route resolver left right).before first second = left.before first second := by
  simp [Source.before, firstInside, secondInside]

theorem composition_preserves_right_fragment_causality
    (left right : Source) (resolver : Option Nat) (first second : Nat) :
    (Source.seq left right).before (left.eventCount + first) (left.eventCount + second) =
        right.before first second ∧
    (Source.par left right).before (left.eventCount + first) (left.eventCount + second) =
        right.before first second ∧
    (Source.route resolver left right).before (left.eventCount + first) (left.eventCount + second) =
        right.before first second := by
  have firstNotLeft : ¬left.eventCount + first < left.eventCount := by omega
  have secondNotLeft : ¬left.eventCount + second < left.eventCount := by omega
  have firstRight : left.eventCount ≤ left.eventCount + first := by omega
  have secondRight : left.eventCount ≤ left.eventCount + second := by omega
  simp [Source.before, firstNotLeft, secondNotLeft, firstRight, secondRight]

theorem sequential_fragments_have_cross_causality
    (left right : Source) {first second : Nat}
    (firstInside : first < left.eventCount) (secondAfter : left.eventCount ≤ second) :
    (Source.seq left right).before first second = true := by
  have firstNotRight : ¬left.eventCount ≤ first := by omega
  have secondNotLeft : ¬second < left.eventCount := by omega
  simp [Source.before, firstInside, secondAfter, firstNotRight, secondNotLeft]

/-- The same coordinate bound applies to every accepted emitted binding. -/
theorem elaborated_occurrence_is_within_its_fragment
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start event : Nat} {run : Elaboration}
    {binding : SymbolicBinding}
    (accepted : elaborate resources owners path source start = some run)
    (present : (event, binding) ∈ run.bindings) :
    start ≤ event ∧ event < start + source.eventCount := by
  apply source_occurrence_is_within_its_fragment (message := binding.message)
  apply (elaboration_retains_exact_source_message_occurrences accepted).subset
  exact List.mem_map.mpr ⟨(event, binding), present, rfl⟩

/-- Changing ownership and using that same resource cannot be causally
independent in one accepted visit. This does not order unrelated resources. -/
theorem accepted_source_orders_transfer_against_uses
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start first second : Nat} {run : Elaboration}
    {firstBinding secondBinding : SymbolicBinding} {key : EndpointKey}
    (accepted : elaborate resources owners path source start = some run)
    (firstPresent : (first, firstBinding) ∈ run.bindings)
    (secondPresent : (second, secondBinding) ∈ run.bindings)
    (transferred : firstBinding.message.payload = .endpoint key)
    (used : secondBinding.message.sender = key ∨ secondBinding.message.receiver = key ∨
      secondBinding.message.payload = .endpoint key) :
    first = second ∨ source.before (first - start) (second - start) = true ∨
      source.before (second - start) (first - start) = true := by
  induction source generalizing owners start run with
  | send message =>
      simp only [elaborate] at accepted
      split at accepted
      next invalid => simp at accepted
      next valid =>
        cases payload : message.payload with
        | wire schema =>
            simp only [payload] at accepted
            have same := Option.some.inj accepted
            subst run
            have atFirst := congrArg Prod.fst (List.mem_singleton.mp firstPresent)
            have atSecond := congrArg Prod.fst (List.mem_singleton.mp secondPresent)
            exact Or.inl (atFirst.trans atSecond.symm)
        | endpoint resource =>
            simp only [payload] at accepted
            have same := Option.some.inj accepted
            subst run
            have atFirst := congrArg Prod.fst (List.mem_singleton.mp firstPresent)
            have atSecond := congrArg Prod.fst (List.mem_singleton.mp secondPresent)
            exact Or.inl (atFirst.trans atSecond.symm)
  | seq left right leftIH rightIH =>
      change (elaborate resources owners path left start).bind _ = some run at accepted
      obtain ⟨leftRun, leftAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
      obtain ⟨rightRun, rightAt, result⟩ := Option.bind_eq_some_iff.mp tail
      have same := Option.some.inj result
      subst run
      rcases List.mem_append.mp firstPresent with firstLeft | firstRight
      · rcases List.mem_append.mp secondPresent with secondLeft | secondRight
        · have firstBound := elaborated_occurrence_is_within_its_fragment leftAt firstLeft
          have secondBound := elaborated_occurrence_is_within_its_fragment leftAt secondLeft
          have insideFirst : first - start < left.eventCount := by omega
          have insideSecond : second - start < left.eventCount := by omega
          simpa only [(composition_preserves_left_fragment_causality left right none insideFirst insideSecond).1,
            (composition_preserves_left_fragment_causality left right none insideSecond insideFirst).1]
            using leftIH leftAt firstLeft secondLeft
        · have firstBound := elaborated_occurrence_is_within_its_fragment leftAt firstLeft
          have secondBound := elaborated_occurrence_is_within_its_fragment rightAt secondRight
          exact .inr (.inl (sequential_fragments_have_cross_causality left right (by omega) (by omega)))
      · rcases List.mem_append.mp secondPresent with secondLeft | secondRight
        · have firstBound := elaborated_occurrence_is_within_its_fragment rightAt firstRight
          have secondBound := elaborated_occurrence_is_within_its_fragment leftAt secondLeft
          exact .inr (.inr (sequential_fragments_have_cross_causality left right (by omega) (by omega)))
        · have firstBound := elaborated_occurrence_is_within_its_fragment rightAt firstRight
          have secondBound := elaborated_occurrence_is_within_its_fragment rightAt secondRight
          have firstShift : first - start = left.eventCount + (first - (start + left.eventCount)) := by omega
          have secondShift : second - start = left.eventCount + (second - (start + left.eventCount)) := by omega
          simpa only [firstShift, secondShift,
            (composition_preserves_right_fragment_causality left right none
              (first - (start + left.eventCount)) (second - (start + left.eventCount))).1,
            (composition_preserves_right_fragment_causality left right none
              (second - (start + left.eventCount)) (first - (start + left.eventCount))).1]
            using rightIH rightAt firstRight secondRight
  | par left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next incompatible => simp at accepted
      next compatible =>
        have separated : incompatibleParallel left right = false := by
          cases result : incompatibleParallel left right <;> simp_all
        obtain ⟨leftRun, leftAt, tail⟩ := Option.bind_eq_some_iff.mp accepted
        obtain ⟨rightRun, rightAt, result⟩ := Option.bind_eq_some_iff.mp tail
        have same := Option.some.inj result
        subst run
        rcases List.mem_append.mp firstPresent with firstLeft | firstRight
        · rcases List.mem_append.mp secondPresent with secondLeft | secondRight
          · have firstBound := elaborated_occurrence_is_within_its_fragment leftAt firstLeft
            have secondBound := elaborated_occurrence_is_within_its_fragment leftAt secondLeft
            have insideFirst : first - start < left.eventCount := by omega
            have insideSecond : second - start < left.eventCount := by omega
            simpa only [(composition_preserves_left_fragment_causality left right none insideFirst insideSecond).2.1,
              (composition_preserves_left_fragment_causality left right none insideSecond insideFirst).2.1]
              using leftIH leftAt firstLeft secondLeft
          · exact False.elim (accepted_parallel_elaborations_cannot_alias_transfer_and_use
              separated leftAt rightAt firstLeft secondRight transferred used)
        · rcases List.mem_append.mp secondPresent with secondLeft | secondRight
          · have swapped : incompatibleParallel right left = false := by
              simpa only [incompatibleParallel, Bool.or_comm] using separated
            exact False.elim (accepted_parallel_elaborations_cannot_alias_transfer_and_use
              swapped rightAt leftAt firstRight secondLeft transferred used)
          · have firstBound := elaborated_occurrence_is_within_its_fragment rightAt firstRight
            have secondBound := elaborated_occurrence_is_within_its_fragment rightAt secondRight
            have firstShift : first - start = left.eventCount + (first - (start + left.eventCount)) := by omega
            have secondShift : second - start = left.eventCount + (second - (start + left.eventCount)) := by omega
            simpa only [firstShift, secondShift,
              (composition_preserves_right_fragment_causality left right none
                (first - (start + left.eventCount)) (second - (start + left.eventCount))).2.1,
              (composition_preserves_right_fragment_causality left right none
                (second - (start + left.eventCount)) (first - (start + left.eventCount))).2.1]
              using rightIH rightAt firstRight secondRight
  | route resolver left right leftIH rightIH =>
      simp only [elaborate] at accepted
      split at accepted
      next invalid => simp at accepted
      next valid =>
        obtain ⟨arm, armAt, selected⟩ := Option.bind_eq_some_iff.mp accepted
        cases arm with
        | left =>
            have firstBound := elaborated_occurrence_is_within_its_fragment selected firstPresent
            have secondBound := elaborated_occurrence_is_within_its_fragment selected secondPresent
            have insideFirst : first - start < left.eventCount := by omega
            have insideSecond : second - start < left.eventCount := by omega
            simpa only [(composition_preserves_left_fragment_causality left right resolver insideFirst insideSecond).2.2,
              (composition_preserves_left_fragment_causality left right resolver insideSecond insideFirst).2.2]
              using leftIH selected firstPresent secondPresent
        | right =>
            have firstBound := elaborated_occurrence_is_within_its_fragment selected firstPresent
            have secondBound := elaborated_occurrence_is_within_its_fragment selected secondPresent
            have firstShift : first - start = left.eventCount + (first - (start + left.eventCount)) := by omega
            have secondShift : second - start = left.eventCount + (second - (start + left.eventCount)) := by omega
            simpa only [firstShift, secondShift,
              (composition_preserves_right_fragment_causality left right resolver
                (first - (start + left.eventCount)) (second - (start + left.eventCount))).2.2,
              (composition_preserves_right_fragment_causality left right resolver
                (second - (start + left.eventCount)) (first - (start + left.eventCount))).2.2]
              using rightIH selected firstPresent secondPresent
  | roll body bodyIH =>
      change (elaborate resources owners path body start).bind _ = some run at accepted
      obtain ⟨inner, innerAt, result⟩ := Option.bind_eq_some_iff.mp accepted
      have same := Option.some.inj result
      subst run
      have inside : first = second ∨ body.before (first - start) (second - start) = true ∨
          body.before (second - start) (first - start) = true :=
        bodyIH innerAt firstPresent secondPresent
      simpa only [Source.before] using inside

/-- Numeric order only orients a causal relation already proved for the same
resource. It does not turn the occurrence list into a parallel schedule. -/
theorem an_earlier_transfer_causally_precedes_a_resource_use
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start first second : Nat} {run : Elaboration}
    {firstBinding secondBinding : SymbolicBinding} {key : EndpointKey}
    (accepted : elaborate resources owners path source start = some run)
    (firstPresent : (first, firstBinding) ∈ run.bindings)
    (secondPresent : (second, secondBinding) ∈ run.bindings)
    (transferred : firstBinding.message.payload = .endpoint key)
    (used : secondBinding.message.sender = key ∨ secondBinding.message.receiver = key ∨
      secondBinding.message.payload = .endpoint key)
    (earlier : first < second) : source.before (first - start) (second - start) = true := by
  rcases accepted_source_orders_transfer_against_uses accepted firstPresent secondPresent
      transferred used with same | forward | backward
  · exact False.elim (Nat.ne_of_lt earlier same)
  · exact forward
  · have reversed := source_order_strictly_increases_event backward
    have firstBound := elaborated_occurrence_is_within_its_fragment accepted firstPresent
    have secondBound := elaborated_occurrence_is_within_its_fragment accepted secondPresent
    omega

theorem an_earlier_resource_use_causally_precedes_a_transfer
    {source : Source} {resources : List EndpointKey} {owners : OwnerMap}
    {path : List Selection} {start first second : Nat} {run : Elaboration}
    {firstBinding secondBinding : SymbolicBinding} {key : EndpointKey}
    (accepted : elaborate resources owners path source start = some run)
    (firstPresent : (first, firstBinding) ∈ run.bindings)
    (secondPresent : (second, secondBinding) ∈ run.bindings)
    (transferred : firstBinding.message.payload = .endpoint key)
    (used : secondBinding.message.sender = key ∨ secondBinding.message.receiver = key ∨
      secondBinding.message.payload = .endpoint key)
    (earlier : second < first) : source.before (second - start) (first - start) = true := by
  rcases accepted_source_orders_transfer_against_uses accepted firstPresent secondPresent
      transferred used with same | forward | backward
  · exact False.elim (Nat.ne_of_lt earlier same.symm)
  · have reversed := source_order_strictly_increases_event forward
    have firstBound := elaborated_occurrence_is_within_its_fragment accepted firstPresent
    have secondBound := elaborated_occurrence_is_within_its_fragment accepted secondPresent
    omega
  · exact backward

end Hibana
