import Std

namespace OfferLinks

/- The chain contains the constructor-checked forks for one logical role and
lane. A foreign reentry row is normalized before this query; that source walk
is outside this model. -/

/-- A decoded record: its storage position is the right-arm target. -/
structure Link where
  target : Nat
  scope : Nat

def Increasing : List Link → Prop
  | [] => True
  | head :: tail => (∀ next, next ∈ tail → head.target < next.target) ∧ Increasing tail

def firstUnknown (known : Nat → Bool) : List Link → Option Nat
  | [] => none
  | head :: tail =>
    if known head.scope = true then firstUnknown known tail else some head.target

/-- The ancestor query chooses the least eligible target whose scope is unknown. -/
def MinimumUnknown (eligible : Link → Prop) (known : Nat → Bool) : Option Nat → Prop
  | none => ∀ link, eligible link → known link.scope = true
  | some target =>
    (∃ link, eligible link ∧ known link.scope = false ∧ link.target = target) ∧
      ∀ link, eligible link → known link.scope = false → target ≤ link.target

private theorem chain_query_minimum (known : Nat → Bool) (chain : List Link)
    (ordered : Increasing chain) :
    MinimumUnknown (fun link => link ∈ chain) known (firstUnknown known chain) := by
  revert ordered
  induction chain with
  | nil =>
    intro _
    intro link absent
    cases absent
  | cons head tail ih =>
    intro ordered
    obtain ⟨later, tailOrdered⟩ := ordered
    by_cases chosen : known head.scope = true
    · rw [firstUnknown, if_pos chosen]
      have checked := ih tailOrdered
      cases result : firstUnknown known tail with
      | none =>
        simp only [result, MinimumUnknown] at checked
        intro link member
        rcases List.mem_cons.mp member with same | rest
        · subst link
          exact chosen
        · exact checked link rest
      | some target =>
        simp only [result, MinimumUnknown] at checked
        obtain ⟨⟨entry, member, unknown, position⟩, smallest⟩ := checked
        refine ⟨⟨entry, List.mem_cons_of_mem head member, unknown, position⟩, ?_⟩
        intro link member unknown
        rcases List.mem_cons.mp member with same | rest
        · subst link
          rw [chosen] at unknown
          cases unknown
        · exact smallest link rest unknown
    · rw [firstUnknown, if_neg chosen]
      have unknown : known head.scope = false := by
        cases value : known head.scope <;> simp_all
      refine ⟨⟨head, by simp, unknown, rfl⟩, ?_⟩
      intro link member _
      rcases List.mem_cons.mp member with same | rest
      · subst link
        exact Nat.le_refl _
      · exact Nat.le_of_lt (later link rest)

theorem increasing_targets_are_unique (chain : List Link) (ordered : Increasing chain) :
    (chain.map Link.target).Nodup := by
  revert ordered
  induction chain with
  | nil => intro _; simp
  | cons head tail ih =>
    intro ordered
    rw [List.map_cons, List.nodup_cons]
    constructor
    · intro repeated
      obtain ⟨other, member, same⟩ := List.mem_map.mp repeated
      have less := ordered.1 other member
      omega
    · exact ih ordered.2

/-- Exact membership is checked independently from the generated links.
Together with their rank, it rules out missing, extra and repeated targets. -/
theorem exact_chain_query_is_minimum (eligible : Link → Prop) (known : Nat → Bool)
    (chain : List Link) (ordered : Increasing chain)
    (complete : ∀ link, link ∈ chain ↔ eligible link) :
    MinimumUnknown eligible known (firstUnknown known chain) ∧
      (chain.map Link.target).Nodup := by
  have membership : (fun link => link ∈ chain) = eligible :=
    funext fun link => propext (complete link)
  have minimum := chain_query_minimum known chain ordered
  rw [membership] at minimum
  exact ⟨minimum, increasing_targets_are_unique chain ordered⟩

private theorem minimum_unique (eligible : Link → Prop) (known : Nat → Bool)
    (left right : Option Nat) (leftChecked : MinimumUnknown eligible known left)
    (rightChecked : MinimumUnknown eligible known right) : left = right := by
  cases left with
  | none =>
    cases right with
    | none => rfl
    | some target =>
      obtain ⟨link, member, unknown, _⟩ := rightChecked.1
      have chosen := leftChecked link member
      rw [unknown] at chosen
      cases chosen
  | some target =>
    cases right with
    | none =>
      obtain ⟨link, member, unknown, _⟩ := leftChecked.1
      have chosen := rightChecked link member
      rw [unknown] at chosen
      cases chosen
    | some other =>
      obtain ⟨leftLink, leftMember, leftUnknown, leftPosition⟩ := leftChecked.1
      obtain ⟨rightLink, rightMember, rightUnknown, rightPosition⟩ := rightChecked.1
      have forward := leftChecked.2 rightLink rightMember rightUnknown
      have backward := rightChecked.2 leftLink leftMember leftUnknown
      rw [rightPosition] at forward
      rw [leftPosition] at backward
      exact congrArg some (Nat.le_antisymm forward backward)

theorem exact_chain_matches_old_minimum (eligible : Link → Prop) (known : Nat → Bool)
    (chain : List Link) (ordered : Increasing chain)
    (complete : ∀ link, link ∈ chain ↔ eligible link) (old : Option Nat)
    (oldMinimum : MinimumUnknown eligible known old) : firstUnknown known chain = old :=
  minimum_unique eligible known _ old
    (exact_chain_query_is_minimum eligible known chain ordered complete).1 oldMinimum

theorem selected_link_is_after_row (row : Nat) (eligible : Link → Prop)
    (known : Nat → Bool) (chain : List Link) (ordered : Increasing chain)
    (complete : ∀ link, link ∈ chain ↔ eligible link)
    (afterRow : ∀ link, eligible link → row < link.target)
    (found : firstUnknown known chain = some target) : row < target := by
  have minimum := (exact_chain_query_is_minimum eligible known chain ordered complete).1
  rw [found] at minimum
  obtain ⟨link, member, _, position⟩ := minimum.1
  simpa only [position] using afterRow link member

private def wrongKnownSkip (known : Nat → Bool) : List Link → Option Nat
  | [] => none
  | head :: tail =>
    if known head.scope = true then some head.target else wrongKnownSkip known tail

theorem omitted_fork_changes_candidate :
    firstUnknown (fun _ => false) [⟨2, 0⟩, ⟨4, 1⟩] = some 2 ∧
    firstUnknown (fun _ => false) [⟨4, 1⟩] = some 4 := by
  constructor <;> rfl

theorem wrong_known_skip_changes_candidate :
    firstUnknown (fun _ => false) [⟨2, 0⟩, ⟨4, 1⟩] = some 2 ∧
    wrongKnownSkip (fun _ => false) [⟨2, 0⟩, ⟨4, 1⟩] = none := by
  constructor <;> rfl

theorem nonmonotonic_targets_change_minimum :
    firstUnknown (fun _ => false) [⟨4, 0⟩, ⟨2, 1⟩] = some 4 ∧
    ¬ MinimumUnknown (fun link => link ∈ [⟨4, 0⟩, ⟨2, 1⟩])
      (fun _ => false) (some 4) := by
  refine ⟨rfl, ?_⟩
  intro minimum
  have tooSmall := minimum.2 ⟨2, 1⟩ (by simp) rfl
  change 4 ≤ 2 at tooSmall
  omega

#print axioms increasing_targets_are_unique
#print axioms exact_chain_query_is_minimum
#print axioms exact_chain_matches_old_minimum
#print axioms selected_link_is_after_row
#print axioms omitted_fork_changes_candidate
#print axioms wrong_known_skip_changes_candidate
#print axioms nonmonotonic_targets_change_minimum
end OfferLinks
