import Std

namespace Hibana.CanonicalRouteChain

/-- A descriptor chain follows one parent at every row and ends at a root. -/
inductive ToRoot (parent : Nat → Option Nat) : Nat → List Nat → Prop where
  | root {node} : parent node = none → ToRoot parent node [node]
  | cons {node next tail} : parent node = some next →
      ToRoot parent next tail → ToRoot parent node (node :: tail)

theorem unique (left : ToRoot parent node xs) (right : ToRoot parent node ys) :
    xs = ys := by
  induction left generalizing ys with
  | root atRoot =>
      cases right with
      | root => rfl
      | cons edge _ => simp [atRoot] at edge
  | @cons node next tail edge path ih =>
      cases right with
      | root atRoot => simp [atRoot] at edge
      | @cons _ other rest otherEdge otherPath =>
          have same : next = other := Option.some.inj (edge.symm.trans otherEdge)
          subst other
          rw [ih otherPath]

theorem member_path (path : ToRoot parent node chain) (member : key ∈ chain) :
    ∃ suffix, ToRoot parent key suffix ∧ suffix.length ≤ chain.length := by
  induction path with
  | @root node atRoot =>
      have same : key = node := by simpa using member
      subst key
      exact ⟨[node], .root atRoot, Nat.le_refl _⟩
  | @cons node next tail edge path ih =>
      rcases List.mem_cons.mp member with same | inside
      · subst key
        exact ⟨node :: tail, .cons edge path, Nat.le_refl _⟩
      · obtain ⟨suffix, valid, bound⟩ := ih inside
        exact ⟨suffix, valid, by simp only [List.length_cons]; omega⟩

/-- A complete chain cannot contain a duplicate scope: a repeated scope would
have both the full path and a strictly shorter path to the same root. -/
theorem no_duplicate_scope (path : ToRoot parent node chain) : chain.Nodup := by
  induction path with
  | root => simp
  | @cons node next tail edge path ih =>
      apply List.nodup_cons.mpr
      constructor
      · intro repeated
        obtain ⟨suffix, valid, bound⟩ := member_path path repeated
        have same := unique (ToRoot.cons edge path) valid
        have length := congrArg List.length same
        simp only [List.length_cons] at length
        omega
      · exact ih

theorem suffix_path (path : ToRoot parent node chain)
    (split : chain = earlier ++ key :: suffix) :
    ToRoot parent key (key :: suffix) := by
  induction path generalizing earlier with
  | @root node atRoot =>
      cases earlier with
      | nil =>
          simp only [List.nil_append, List.cons.injEq] at split
          rcases split with ⟨same, tail⟩
          subst key
          subst suffix
          exact .root atRoot
      | cons head rest =>
          have length := congrArg List.length split
          simp only [List.length_cons, List.length_append, List.length_nil] at length
          exfalso
          omega
  | @cons node next tail edge path ih =>
      cases earlier with
      | nil =>
          simp only [List.nil_append, List.cons.injEq] at split
          rcases split with ⟨same, tail⟩
          subst key
          subst suffix
          exact .cons edge path
      | cons head rest =>
          simp only [List.cons_append, List.cons.injEq] at split
          exact ih split.2

/-- Reversing a root-first descriptor chain puts every row after a cut
before that cut's root. Each such row's own path contains the cut root. -/
theorem rows_above_cut_retain_its_root
    (path : ToRoot parent node (earlier ++ root :: tail)) (member : key ∈ earlier) :
    ∃ chain, ToRoot parent key chain ∧ root ∈ chain := by
  obtain ⟨before, after, split⟩ := List.append_of_mem member
  subst earlier
  have kept : ToRoot parent key (key :: (after ++ root :: tail)) :=
    suffix_path path (earlier := before) (by simp [List.append_assoc])
  exact ⟨_, kept, by simp⟩

#print axioms unique
#print axioms member_path
#print axioms no_duplicate_scope
#print axioms suffix_path
#print axioms rows_above_cut_retain_its_root

theorem root_first_offset_from_own_path_length
    (path : ToRoot parent node chain)
    (split : chain = earlier ++ key :: suffix)
    (own : ToRoot parent key ownChain) :
    chain.reverse[ownChain.length - 1]? = some key := by
  have ownSuffix := suffix_path path split
  have same := unique own ownSuffix
  rw [same, split]
  simp [List.reverse_append, List.reverse_cons]

#print axioms root_first_offset_from_own_path_length

end Hibana.CanonicalRouteChain
