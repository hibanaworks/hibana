import Std.Tactic

/- Isolated proposal, not a change to Hibana's Rust compiler or selector.
   Vertices are logical event occurrences, never logical labels or wire colors.
   `conflict` is a caller-supplied, irreflexive overapproximation. The separate
   coverage premise below is indispensable and is NOT proved for Rust here. -/
namespace ReentryColorGate
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000
abbrev Assignment := Nat × Nat

def Fits (conflict : Nat → Nat → Bool) (vertex : Nat)
    (assigned : List Assignment) (color : Nat) : Prop :=
  ∀ prior ∈ assigned,
    (conflict vertex prior.1 = true ∨ conflict prior.1 vertex = true) →
    color ≠ prior.2
instance (conflict : Nat → Nat → Bool) (vertex : Nat)
    (assigned : List Assignment) (color : Nat) :
    Decidable (Fits conflict vertex assigned color) := by
  unfold Fits
  infer_instance

/-- Search all colors without truncation, wrapping, or weakening an edge. -/
def choose (palette : Nat) (conflict : Nat → Nat → Bool) (vertex : Nat)
    (assigned : List Assignment) : Option Nat :=
  (List.range palette).find? (fun color => decide (Fits conflict vertex assigned color))

/-- Successful allocations accumulate in reverse vertex order. -/
def greedy (palette : Nat) (conflict : Nat → Nat → Bool) :
    List Nat → List Assignment → Option (List Assignment)
  | [], assigned => some assigned
  | vertex :: rest, assigned => do
      let color ← choose palette conflict vertex assigned
      greedy palette conflict rest ((vertex, color) :: assigned)

def Valid (palette : Nat) (conflict : Nat → Nat → Bool)
    (assigned : List Assignment) : Prop :=
  (∀ entry ∈ assigned, entry.2 < palette) ∧
  (∀ a ∈ assigned, ∀ b ∈ assigned,
    conflict a.1 b.1 = true → a.2 ≠ b.2)

theorem chosen_is_bounded_and_fits
    (h : choose palette conflict vertex assigned = some color) :
    color < palette ∧ Fits conflict vertex assigned color := by
  have fits : decide (Fits conflict vertex assigned color) = true :=
    List.find?_some (p := fun c => decide (Fits conflict vertex assigned c)) h
  exact ⟨List.mem_range.mp (List.mem_of_find?_eq_some h), of_decide_eq_true fits⟩

/-- Failure means every color is blocked for this prefix; it does NOT mean the
    graph has no coloring under a different order/backtracking strategy. -/
theorem exhaustion_is_exact :
    choose palette conflict vertex assigned = none ↔
      ∀ color, color < palette → ¬ Fits conflict vertex assigned color := by
  simp [choose, List.find?_eq_none]

theorem extend_preserves_valid
    (noSelf : ∀ vertex, conflict vertex vertex = false)
    (valid : Valid palette conflict assigned)
    (bounded : color < palette)
    (fits : Fits conflict vertex assigned color) :
    Valid palette conflict ((vertex,color) :: assigned) := by
  constructor
  · intro a ha
    rcases List.mem_cons.mp ha with rfl | ha
    · exact bounded
    · exact valid.1 a ha
  · intro a ha b hb edge
    rcases List.mem_cons.mp ha with rfl | ha
    · rcases List.mem_cons.mp hb with rfl | hb
      · simp [noSelf] at edge
      · exact fits b hb (Or.inl edge)
    · rcases List.mem_cons.mp hb with rfl | hb
      · exact Ne.symm (fits a ha (Or.inr edge))
      · exact valid.2 a ha b hb edge

/-- General soundness of every successful result, for any finite graph/palette. -/
theorem greedy_preserves_valid
    (noSelf : ∀ vertex, conflict vertex vertex = false)
    (valid : Valid palette conflict assigned)
    (success : greedy palette conflict vertices assigned = some result) :
    Valid palette conflict result := by
  induction vertices generalizing assigned with
  | nil =>
      simp [greedy] at success
      subst result
      exact valid
  | cons vertex rest ih =>
      cases chosen : choose palette conflict vertex assigned with
      | none => simp [greedy, chosen] at success
      | some color =>
          have facts := chosen_is_bounded_and_fits chosen
          apply ih (extend_preserves_valid noSelf valid facts.1 facts.2)
          simpa [greedy, chosen] using success

/-- No vertex is silently omitted, duplicated, or invented by successful coloring.
    If the input vertex list has no duplicates, neither does the output ID list. -/
theorem greedy_preserves_vertex_inventory
    (success : greedy palette conflict vertices assigned = some result) :
    result.map Prod.fst = vertices.reverse ++ assigned.map Prod.fst := by
  induction vertices generalizing assigned with
  | nil =>
      simp [greedy] at success
      subst result
      simp
  | cons vertex rest ih =>
      cases chosen : choose palette conflict vertex assigned with
      | none => simp [greedy, chosen] at success
      | some color =>
          have tailSuccess : greedy palette conflict rest ((vertex,color)::assigned) =
              some result := by simpa [greedy, chosen] using success
          simpa [List.reverse_cons, List.append_assoc] using ih tailSuccess

/-- Preserve all old mandatory color inequalities as well as new reentry edges.
    This is why independent local renamings are not enough for nested routes. -/
def augmented (base additional : Nat → Nat → Bool) (a b : Nat) : Bool :=
  base a b || additional a b

theorem augmented_valid_preserves_old_edges
    (valid : Valid palette (augmented base additional) assigned) :
    Valid palette base assigned := by
  refine ⟨valid.1, ?_⟩
  intro a ha b hb edge
  exact valid.2 a ha b hb (by simp [augmented, edge])

theorem augmented_valid_separates_new_edges
    (valid : Valid palette (augmented base additional) assigned) :
    Valid palette additional assigned := by
  refine ⟨valid.1, ?_⟩
  intro a ha b hb edge
  exact valid.2 a ha b hb (by simp [augmented, edge])

/-- The immutable source/destination/lane domain excludes the wire color.
    `Eligible` must mean the SAME unchanged runtime guards, including reentry,
    row/dependency/conflict/lane-head checks and offer-entry qualification. -/
structure BaseKey where
  source : Nat
  receiver : Nat
  lane : Nat
  deriving DecidableEq

def Covers (conflict : Nat → Nat → Bool) (domain : Nat → BaseKey)
    (Eligible : State → Nat → Prop) : Prop :=
  ∀ state a b, a ≠ b → Eligible state a → Eligible state b →
    domain a = domain b → conflict a b = true

/-- Exactly the mathematical obligation needed by an unchanged UniqueMatch:
    two eligible descriptors with the same full wire key have one occurrence ID.
    Completeness/coverage of a concrete compiler graph remains a separate task. -/
theorem same_wire_key_has_unique_eligible_occurrence
    (valid : Valid palette conflict assigned)
    (covers : Covers conflict domain Eligible)
    (ha : (a, ca) ∈ assigned) (hb : (b, cb) ∈ assigned)
    (ea : Eligible state a) (eb : Eligible state b)
    (sameDomain : domain a = domain b) (sameColor : ca = cb) : a = b := by
  by_cases same : a = b
  · exact same
  · have edge := covers state a b same ea eb sameDomain
    exact False.elim (valid.2 (a, ca) ha (b, cb) hb edge sameColor)

/-- Proposed final-pass graph schema. `rollOverlap` is the structural edge
    producer to verify separately. Preexisting inequalities are never discarded.
    Domains include receiver because allocation is global, not endpoint-local. -/
def finalGraph (domain : Nat → BaseKey) (oldColor : Nat → Nat)
    (rollOverlap : Nat → Nat → Bool) (a b : Nat) : Bool :=
  a != b && decide (domain a = domain b) &&
    (oldColor a != oldColor b || rollOverlap a b || rollOverlap b a)

theorem final_graph_has_no_self_edges :
    finalGraph domain oldColor rollOverlap vertex vertex = false := by
  simp [finalGraph]

theorem final_graph_retains_prior_inequality
    (sameDomain : domain a = domain b) (distinctColor : oldColor a ≠ oldColor b) :
    finalGraph domain oldColor rollOverlap a b = true := by
  have different : a ≠ b := fun same => distinctColor (congrArg oldColor same)
  simp [finalGraph, different, sameDomain, distinctColor]

theorem final_graph_contains_reentry_edge
    (different : a ≠ b) (sameDomain : domain a = domain b)
    (overlap : rollOverlap a b = true) :
    finalGraph domain oldColor rollOverlap a b = true := by
  simp [finalGraph, different, sameDomain, overlap]

/-- An actual top-level allocation result has bounded, edge-separated colors and
    exactly the input occurrence inventory. No assumptions about input size. -/
theorem complete_allocation_sound
    (noSelf : ∀ vertex, conflict vertex vertex = false)
    (success : greedy palette conflict vertices [] = some result) :
    Valid palette conflict result ∧ result.map Prod.fst = vertices.reverse := by
  have emptyValid : Valid palette conflict [] := by simp [Valid]
  exact ⟨greedy_preserves_valid noSelf emptyValid success,
    by simpa using greedy_preserves_vertex_inventory success⟩

/-- Noninterfering ordered occurrences can share colors; IDs remain distinct. -/
theorem safe_reuse_for_noninterfering_occurrences :
    greedy 256 (fun _ _ => false) [0,1,2] [] =
      some [(2,0),(1,0),(0,0)] := by decide

def clique (a b : Nat) : Bool := a != b

theorem saturated_prefix_exhausts (palette : Nat) :
    choose palette clique palette ((List.range palette).map (fun i => (i,i))) = none := by
  apply exhaustion_is_exact.mpr
  intro color bound fits
  have member : (color,color) ∈ (List.range palette).map (fun i => (i,i)) :=
    List.mem_map.mpr ⟨color, List.mem_range.mpr bound, rfl⟩
  have edge : clique palette color = true := by
    simp [clique, Nat.ne_of_gt bound]
  exact fits (color,color) member (Or.inl edge) rfl

theorem byte_palette_exhaustion_is_reported :
    choose 256 clique 256 ((List.range 256).map (fun i => (i,i))) = none :=
  saturated_prefix_exhausts 256

theorem final_byte_color_is_available :
    choose 256 clique 255 ((List.range 255).map (fun i => (i,i))) = some 255 := by
  decide

/- Source-linked diagnostic slice. Both vertices have source 0, receiver 1,
   lane 0. run.log observes IDs 2 (Inspect52) and 6 (Open55) eligible together.
   This fixture does not claim to derive Rust eligibility in Lean. -/
def diagnosticConflict (a b : Nat) : Bool :=
  (a == 2 && b == 6) || (a == 6 && b == 2)

theorem old_shared_colors_violate_required_edge :
    ¬ Valid 256 diagnosticConflict [(2,0),(6,0)] := by
  unfold Valid
  decide

theorem greedy_separates_diagnostic_pair :
    greedy 256 diagnosticConflict [2,6] [] = some [(6,1),(2,0)] := by decide

theorem ordered_prefix_color_reuse_survives_reentry_separation :
    greedy 256 diagnosticConflict [0,1,2,4,5,6] [] =
      some [(6,1),(5,0),(4,0),(2,0),(1,0),(0,0)] := by decide

#print axioms chosen_is_bounded_and_fits
#print axioms exhaustion_is_exact
#print axioms extend_preserves_valid
#print axioms greedy_preserves_valid
#print axioms greedy_preserves_vertex_inventory
#print axioms augmented_valid_preserves_old_edges
#print axioms augmented_valid_separates_new_edges
#print axioms same_wire_key_has_unique_eligible_occurrence
#print axioms final_graph_has_no_self_edges
#print axioms final_graph_retains_prior_inequality
#print axioms final_graph_contains_reentry_edge
#print axioms complete_allocation_sound
#print axioms safe_reuse_for_noninterfering_occurrences
#print axioms saturated_prefix_exhausts
#print axioms byte_palette_exhaustion_is_reported
#print axioms final_byte_color_is_available
#print axioms old_shared_colors_violate_required_edge
#print axioms greedy_separates_diagnostic_pair
#print axioms ordered_prefix_color_reuse_survives_reentry_separation
end ReentryColorGate
