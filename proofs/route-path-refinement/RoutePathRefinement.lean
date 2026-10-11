import Std

/-! Exact compact route-path partition refinement, a separate source-lowering
proposal. No hashing and no laminar/nesting assumptions: any finite family of
ternary membership observations is supported. This is the mathematical layer;
production remap-array correspondence and source measurements are separate.
-/
namespace Hibana.RoutePathRefinement

abbrev Event := Nat
abbrev ClassId := Nat
abbrev Membership := Fin 3
abbrev Feature := Event → Membership

def firstIndex {α : Type} [DecidableEq α] : List α → α → Nat
  | [], _ => 0
  | head :: tail, query => if head = query then 0 else firstIndex tail query + 1

theorem firstIndex_lt {α : Type} [DecidableEq α] (domain : List α) (query : α)
    (hq : query ∈ domain) : firstIndex domain query < domain.length := by
  induction domain with
  | nil => simp at hq
  | cons head tail ih =>
      by_cases h : head = query
      · simp [firstIndex, h]
      · have ht : query ∈ tail := by simpa [List.mem_cons, h, Ne.symm h] using hq
        have bound := ih ht
        simp only [firstIndex, if_neg h, List.length_cons]
        omega

theorem firstIndex_injective {α : Type} [DecidableEq α] (domain : List α) (left right : α)
    (hl : left ∈ domain) (hr : right ∈ domain) :
    firstIndex domain left = firstIndex domain right ↔ left = right := by
  induction domain with
  | nil => simp at hl
  | cons head tail ih =>
      by_cases hleft : head = left
      · by_cases hright : head = right
        · have he : left = right := hleft.symm.trans hright
          simp only [firstIndex, if_pos hright, he]
        · have he : left ≠ right := fun h => hright (hleft.trans h)
          simp only [firstIndex, if_pos hleft, if_neg hright]
          simp [he]
      · by_cases hright : head = right
        · have he : left ≠ right := fun h => hleft (hright.trans h.symm)
          simp only [firstIndex, if_neg hleft, if_pos hright]
          simp [he]
        · have lt : left ∈ tail := by simpa [List.mem_cons, hleft, Ne.symm hleft] using hl
          have rt : right ∈ tail := by simpa [List.mem_cons, hright, Ne.symm hright] using hr
          simpa [firstIndex, hleft, hright] using ih lt rt

/-- Any first-occurrence numbering of pairs is a compact mathematical
representative. A streaming memo table may assign different numeric IDs;
only the proved equality relation matters to coloring. -/
def refine (domain : List Event) (old : Event → ClassId) (feature : Feature) : Event → ClassId :=
  fun event => firstIndex (domain.map (fun e => (old e, feature e))) (old event, feature event)

theorem refine_exact (domain : List Event) (old : Event → ClassId) (feature : Feature)
    (left right : Event) (hl : left ∈ domain) (hr : right ∈ domain) :
    refine domain old feature left = refine domain old feature right ↔
    old left = old right ∧ feature left = feature right := by
  unfold refine
  have lm : (old left, feature left) ∈ domain.map (fun e => (old e, feature e)) := by
    exact List.mem_map.mpr ⟨left, hl, rfl⟩
  have rm : (old right, feature right) ∈ domain.map (fun e => (old e, feature e)) := by
    exact List.mem_map.mpr ⟨right, hr, rfl⟩
  simpa using firstIndex_injective (domain.map (fun e => (old e, feature e)))
    (old left, feature left) (old right, feature right) lm rm

theorem refine_bound (domain : List Event) (old : Event → ClassId) (feature : Feature)
    (event : Event) (he : event ∈ domain) : refine domain old feature event < domain.length := by
  have hm : (old event, feature event) ∈ domain.map (fun e => (old e, feature e)) :=
    List.mem_map.mpr ⟨event, he, rfl⟩
  simpa [refine] using firstIndex_lt (domain.map (fun e => (old e, feature e))) (old event, feature event) hm

def refineAll (domain : List Event) : List Feature → (Event → ClassId) → Event → ClassId
  | [], old => old
  | feature :: tail, old => refineAll domain tail (refine domain old feature)

theorem refinement_exact (domain : List Event) (features : List Feature) (old : Event → ClassId)
    (left right : Event) (hl : left ∈ domain) (hr : right ∈ domain) :
    refineAll domain features old left = refineAll domain features old right ↔
    old left = old right ∧ ∀ feature ∈ features, feature left = feature right := by
  induction features generalizing old with
  | nil => simp [refineAll]
  | cons feature tail ih =>
      rw [refineAll, ih, refine_exact domain old feature left right hl hr]
      simp only [List.mem_cons, forall_eq_or_imp]
      exact and_assoc

def samePath (features : List Feature) (left right : Event) : Bool :=
  features.all (fun feature => decide (feature left = feature right))

def classes (domain : List Event) (features : List Feature) : Event → ClassId :=
  refineAll domain features (fun _ => 0)

theorem class_equality_iff_original (domain : List Event) (features : List Feature)
    (left right : Event) (hl : left ∈ domain) (hr : right ∈ domain) :
    (classes domain features left = classes domain features right) ↔ samePath features left right = true := by
  simpa [classes, samePath, List.all_eq_true] using
    refinement_exact domain features (fun _ => 0) left right hl hr

theorem classes_bound (domain : List Event) (features : List Feature) (event : Event)
    (he : event ∈ domain) : classes domain features event < domain.length := by
  have nonempty : 0 < domain.length := List.length_pos_of_mem he
  have general : ∀ fs old, (∀ e ∈ domain, old e < domain.length) →
      refineAll domain fs old event < domain.length := by
    intro fs
    induction fs with
    | nil => intro old ho; exact ho event he
    | cons feature tail ih =>
        intro old _
        apply ih
        intro e hm
        exact refine_bound domain old feature e hm
  exact general features (fun _ => 0) (by intro _ _; exact nonempty)

theorem stored_id_avoids_u16_sentinel (domain : List Event) (features : List Feature)
    (event : Event) (he : event ∈ domain) (hn : domain.length ≤ 65535) :
    classes domain features event < 65535 := by
  exact Nat.lt_of_lt_of_le (classes_bound domain features event he) hn

structure Route where
  start : Nat
  split : Nat
  stop : Nat

def membership (route : Route) (event : Event) : Membership :=
  if route.start ≤ event ∧ event < route.split then 0
  else if route.split ≤ event ∧ event < route.stop then 1 else 2

/-- Sufficient uniform-membership test, including disjoint intervals and a
roll body contained entirely in either arm. Merely comparing the membership
of the two interval endpoints would be unsound when a route lies inside. -/
def uniform (route : Route) (bodyStart bodyStop : Nat) : Prop :=
  bodyStop ≤ route.start ∨ route.stop ≤ bodyStart ∨
  (route.start ≤ bodyStart ∧ bodyStop ≤ route.split) ∨
  (route.split ≤ bodyStart ∧ bodyStop ≤ route.stop)

theorem uniform_same_membership (route : Route) (bodyStart bodyStop left right : Nat)
    (hroute : route.start < route.split ∧ route.split < route.stop)
    (hu : uniform route bodyStart bodyStop)
    (hl : bodyStart ≤ left ∧ left < bodyStop) (hr : bodyStart ≤ right ∧ right < bodyStop) :
    membership route left = membership route right := by
  unfold uniform at hu
  by_cases l0 : route.start ≤ left ∧ left < route.split <;>
    by_cases l1 : route.split ≤ left ∧ left < route.stop <;>
    by_cases r0 : route.start ≤ right ∧ right < route.split <;>
    by_cases r1 : route.split ≤ right ∧ right < route.stop <;>
    simp [membership, l0, l1, r0, r1] <;> omega

/-- General exactness allows any route topology; no nested/disjoint/laminar
source assumption is needed for equality of the complete membership vector. -/
theorem routes_exact (domain : List Event) (routes : List Route) (left right : Event)
    (hl : left ∈ domain) (hr : right ∈ domain) :
    classes domain (routes.map membership) left = classes domain (routes.map membership) right ↔
    ∀ route ∈ routes, membership route left = membership route right := by
  simpa [samePath, List.all_eq_true] using
    class_equality_iff_original domain (routes.map membership) left right hl hr

/-- Equality-based consumers, including the unchanged deterministic coloring
loop and its failure result, are invariant under an exactly equivalent relation. -/
theorem consumer_congruence {n : Nat} {Result : Type}
    (old new : Fin n → Fin n → Bool) (consumer : (Fin n → Fin n → Bool) → Result)
    (h : ∀ left right, old left right = new left right) : consumer old = consumer new := by
  have equal : old = new := by funext left right; exact h left right
  rw [equal]

#print axioms refinement_exact
#print axioms class_equality_iff_original
#print axioms classes_bound
#print axioms stored_id_avoids_u16_sentinel
#print axioms uniform_same_membership
#print axioms routes_exact
end Hibana.RoutePathRefinement
