import LeanColorGate

/- A conservative graph extension. This proves the mathematical bridge only.
   Rust reachability, well-formedness, and same-class eligibility uniqueness are
   not derived here. The exact missing premise is stated explicitly below. -/
namespace ReentryRollMembership
open ReentryColorGate

def classGraph (domain : Nat → BaseKey) (oldColor : Nat → Nat)
    (inner : Nat → Option Nat) : Nat → Nat → Bool :=
  finalGraph domain oldColor (fun a b => inner a != inner b)

theorem membership_graph_has_no_self_edges :
    classGraph domain oldColor inner vertex vertex = false := by
  exact final_graph_has_no_self_edges

theorem membership_graph_retains_prior_inequality
    (sameDomain : domain a = domain b) (different : oldColor a ≠ oldColor b) :
    classGraph domain oldColor inner a b = true := by
  exact final_graph_retains_prior_inequality sameDomain different

theorem membership_graph_separates_distinct_roll_classes
    (sameDomain : domain a = domain b) (different : inner a ≠ inner b) :
    classGraph domain oldColor inner a b = true := by
  have unequal : a ≠ b := fun same => different (congrArg inner same)
  apply final_graph_contains_reentry_edge unequal sameDomain
  simp [different]

/-- Reachable/well-formed states should be encoded in State or Eligible. This
    is the remaining concrete coverage obligation, not a proved Rust fact. -/
def SameClassUnique (domain : Nat → BaseKey) (oldColor : Nat → Nat)
    (inner : Nat → Option Nat) (Eligible : State → Nat → Prop) : Prop :=
  ∀ state a b, Eligible state a → Eligible state b → domain a = domain b →
    oldColor a = oldColor b → inner a = inner b → a = b

theorem exact_remaining_obligation_implies_covers
    (unique : SameClassUnique domain oldColor inner Eligible) :
    Covers (classGraph domain oldColor inner) domain Eligible := by
  intro state a b unequal ea eb sameDomain
  by_cases colors : oldColor a = oldColor b
  · by_cases members : inner a = inner b
    · exact False.elim (unequal (unique state a b ea eb sameDomain colors members))
    · exact membership_graph_separates_distinct_roll_classes sameDomain members
  · exact membership_graph_retains_prior_inequality sameDomain colors

/-- Abstract laminar-tree bridge. `ancestor` includes self. The characterization
    means exactly that a vertex's containing Rolls are the ancestors of its
    selected innermost Roll, with no containing Roll for None. It is the source
    emitter's laminarity/canonical tie-break obligation, not an axiom about Rust.
    Equal-range nested Rolls must retain distinct IDs and deterministic depth. -/
theorem innermost_identity_iff_full_membership
    (member : Nat → Nat → Prop) (inner : Nat → Option Nat)
    (ancestor : Nat → Nat → Prop)
    (reflexive : ∀ a, ancestor a a)
    (antisymmetric : ∀ a b, ancestor a b → ancestor b a → a = b)
    (characterizes : ∀ scope vertex,
      member scope vertex ↔ ∃ deepest, inner vertex = some deepest ∧ ancestor scope deepest) :
    inner a = inner b ↔ (∀ scope, member scope a ↔ member scope b) := by
  constructor
  · intro same scope
    rw [characterizes scope a, characterizes scope b, same]
  · intro sameMembers
    cases ha : inner a with
    | none =>
      cases hb : inner b with
      | none => rfl
      | some ib =>
        have mb : member ib b := (characterizes ib b).mpr ⟨ib, hb, reflexive ib⟩
        have ma : member ib a := (sameMembers ib).mpr mb
        obtain ⟨ia, impossible, _⟩ := (characterizes ib a).mp ma
        simp [ha] at impossible
    | some ia =>
      have ma : member ia a := (characterizes ia a).mpr ⟨ia, ha, reflexive ia⟩
      obtain ⟨ib, hb, ab⟩ := (characterizes ia b).mp ((sameMembers ia).mp ma)
      have mb : member ib b := (characterizes ib b).mpr ⟨ib, hb, reflexive ib⟩
      obtain ⟨ia', ha', ba⟩ := (characterizes ib a).mp ((sameMembers ib).mpr mb)
      have eqIa : ia = ia' := Option.some.inj (ha.symm.trans ha')
      subst ia'
      have same : ia = ib := antisymmetric ia ib ab ba
      simpa [same] using hb.symm

/-- Both temporal directions are separated: reentry can preview an older inner
    body while another candidate previews an enclosing Roll's earlier prefix. -/
theorem membership_graph_is_symmetric :
    classGraph domain oldColor inner a b = classGraph domain oldColor inner b a := by
  unfold classGraph finalGraph
  dsimp only
  rw [bne_comm (a := b) (b := a),
    bne_comm (a := oldColor b) (b := oldColor a),
    bne_comm (a := inner b) (b := inner a)]
  have sameDomain : decide (domain a = domain b) = decide (domain b = domain a) := by
    apply Bool.eq_iff_iff.mpr
    simp only [decide_eq_true_eq]
    exact eq_comm
  rw [sameDomain]

#print axioms membership_graph_has_no_self_edges
#print axioms membership_graph_retains_prior_inequality
#print axioms membership_graph_separates_distinct_roll_classes
#print axioms exact_remaining_obligation_implies_covers
#print axioms innermost_identity_iff_full_membership
#print axioms membership_graph_is_symmetric
end ReentryRollMembership
