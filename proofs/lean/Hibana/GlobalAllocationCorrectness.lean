import Hibana.GlobalSyntax

namespace Hibana

inductive LaneCoverVertex where
  | left (lane : Nat)
  | right (lane : Nat)
  deriving Repr, DecidableEq

private def removeFirst [BEq α] (target : α) : List α → List α
  | [] => []
  | head :: tail =>
      if target == head then tail else head :: removeFirst target tail

private theorem remove_first_length_of_mem
    {α : Type} [BEq α] [LawfulBEq α]
    {target : α} {items : List α} (member : target ∈ items) :
    (removeFirst target items).length + 1 = items.length := by
  induction items with
  | nil => simp at member
  | cons head tail tailIH =>
      cases targetBeq : target == head with
      | false =>
          have targetNe : target ≠ head := not_eq_of_beq_eq_false targetBeq
          have tailMember : target ∈ tail :=
            (List.mem_cons.mp member).resolve_left targetNe
          simp [removeFirst, targetBeq, tailIH tailMember]
      | true =>
          have targetEq : target = head := eq_of_beq targetBeq
          subst target
          simp [removeFirst]

private theorem mem_remove_first_of_ne
    {α : Type} [BEq α] [LawfulBEq α]
    {target item : α} {items : List α}
    (different : item ≠ target) (member : item ∈ items) :
    item ∈ removeFirst target items := by
  induction items with
  | nil => simp at member
  | cons head tail tailIH =>
      cases targetBeq : target == head with
      | false =>
          simp only [removeFirst, targetBeq, Bool.false_eq_true, ↓reduceIte,
            List.mem_cons]
          rcases List.mem_cons.mp member with itemEq | tailMember
          · exact Or.inl itemEq
          · exact Or.inr (tailIH tailMember)
      | true =>
          have targetEq : target = head := eq_of_beq targetBeq
          subst target
          simp [removeFirst, different] at member ⊢
          exact member

private theorem nodup_subset_length_le
    {α : Type} [BEq α] [LawfulBEq α]
    {items domain : List α}
    (distinct : items.Nodup)
    (subset : ∀ item, item ∈ items → item ∈ domain) :
    items.length ≤ domain.length := by
  induction items generalizing domain with
  | nil => simp
  | cons head tail tailIH =>
      have headMember : head ∈ domain := subset head (by simp)
      have distinctParts := List.nodup_cons.mp distinct
      have tailSubset : ∀ item, item ∈ tail → item ∈ removeFirst head domain := by
        intro item itemMember
        have itemNeHead : item ≠ head := by
          intro equal
          subst item
          exact distinctParts.1 itemMember
        exact mem_remove_first_of_ne itemNeHead (subset item (by simp [itemMember]))
      have tailBound := tailIH distinctParts.2 tailSubset
      have removedLength := remove_first_length_of_mem headMember
      simp only [List.length_cons]
      omega

def laneMatchingCharge
    (coverLeft : List Nat) (edge : LaneRemap) : LaneCoverVertex :=
  if coverLeft.contains edge.newLane then
    .left edge.newLane
  else
    .right edge.oldLane

def laneCoverVertices
    (coverLeft coverRight : List Nat) : List LaneCoverVertex :=
  coverLeft.map LaneCoverVertex.left ++ coverRight.map LaneCoverVertex.right

private theorem lane_matching_charges_nodup
    {matching : List LaneRemap} {coverLeft : List Nat}
    (distinct : matching.Pairwise fun left right =>
      left.oldLane ≠ right.oldLane ∧ left.newLane ≠ right.newLane) :
    (matching.map (laneMatchingCharge coverLeft)).Nodup := by
  apply distinct.map
  intro left right different
  simp only [laneMatchingCharge]
  split <;> split <;> simp [different.1, different.2]

private theorem lane_matching_charge_mem_cover
    {coverLeft coverRight : List Nat} {edge : LaneRemap}
    (covered : edge.newLane ∈ coverLeft ∨ edge.oldLane ∈ coverRight) :
    laneMatchingCharge coverLeft edge ∈ laneCoverVertices coverLeft coverRight := by
  by_cases leftMember : edge.newLane ∈ coverLeft
  · simp [laneMatchingCharge, laneCoverVertices, leftMember]
  · rcases covered with impossible | rightMember
    · exact False.elim (leftMember impossible)
    · simp [laneMatchingCharge, laneCoverVertices, leftMember, rightMember]

/-- Every matching is bounded by every vertex cover. The certificate is erased
before a role image reaches the runtime. -/
theorem lane_matching_cardinality_le_vertex_cover
    {matching : List LaneRemap} {coverLeft coverRight : List Nat}
    (distinct : matching.Pairwise fun left right =>
      left.oldLane ≠ right.oldLane ∧ left.newLane ≠ right.newLane)
    (covered : ∀ edge, edge ∈ matching →
      edge.newLane ∈ coverLeft ∨ edge.oldLane ∈ coverRight) :
    matching.length ≤ coverLeft.length + coverRight.length := by
  let charges := matching.map (laneMatchingCharge coverLeft)
  have chargesNodup : charges.Nodup := lane_matching_charges_nodup distinct
  have chargesSubset : ∀ vertex, vertex ∈ charges →
      vertex ∈ laneCoverVertices coverLeft coverRight := by
    intro vertex vertexMember
    simp only [charges, List.mem_map] at vertexMember
    rcases vertexMember with ⟨edge, edgeMember, rfl⟩
    exact lane_matching_charge_mem_cover (covered edge edgeMember)
  have bound := nodup_subset_length_le chargesNodup chargesSubset
  simpa [charges, laneCoverVertices] using bound

structure LaneMatchingCertificate where
  matching : List LaneRemap
  coverLeft : List Nat
  coverRight : List Nat
  deriving Repr, DecidableEq

def LaneMatchingCertificate.Valid
    (leftSpan rightSpan : Nat) (compatible : Nat → Nat → Bool)
    (certificate : LaneMatchingCertificate) : Prop :=
  certificate.matching.Pairwise (fun left right =>
      left.oldLane ≠ right.oldLane ∧ left.newLane ≠ right.newLane)
  ∧ certificate.matching.all (fun edge =>
      edge.newLane < leftSpan && edge.oldLane < rightSpan &&
        compatible edge.newLane edge.oldLane) = true
  ∧ certificate.coverLeft.Nodup
  ∧ certificate.coverRight.Nodup
  ∧ certificate.coverLeft.all (· < leftSpan) = true
  ∧ certificate.coverRight.all (· < rightSpan) = true
  ∧ (List.range leftSpan).all (fun leftLane =>
      (List.range rightSpan).all (fun rightLane =>
        !compatible leftLane rightLane ||
          certificate.coverLeft.contains leftLane ||
          certificate.coverRight.contains rightLane)) = true
  ∧ certificate.matching.length =
      certificate.coverLeft.length + certificate.coverRight.length

instance LaneMatchingCertificate.instDecidableValid
    (leftSpan rightSpan : Nat) (compatible : Nat → Nat → Bool)
    (certificate : LaneMatchingCertificate) :
    Decidable (certificate.Valid leftSpan rightSpan compatible) := by
  unfold LaneMatchingCertificate.Valid
  infer_instance

def checkLaneMatchingCertificate
    (leftSpan rightSpan : Nat) (compatible : Nat → Nat → Bool)
    (certificate : LaneMatchingCertificate) : Bool :=
  decide (certificate.Valid leftSpan rightSpan compatible)

theorem lane_matching_certificate_checker_sound
    {leftSpan rightSpan : Nat} {compatible : Nat → Nat → Bool}
    {certificate : LaneMatchingCertificate}
    (accepted : checkLaneMatchingCertificate leftSpan rightSpan compatible certificate = true) :
    certificate.Valid leftSpan rightSpan compatible :=
  of_decide_eq_true accepted

/-- Equal matching and vertex-cover cardinalities certify a maximum matching
for every finite lane graph, not only the concrete regression graphs. -/
theorem lane_matching_certificate_is_maximum
    {leftSpan rightSpan : Nat} {compatible : Nat → Nat → Bool}
    {certificate : LaneMatchingCertificate}
    (valid : certificate.Valid leftSpan rightSpan compatible)
    {candidate : List LaneRemap}
    (candidateDistinct : candidate.Pairwise fun left right =>
      left.oldLane ≠ right.oldLane ∧ left.newLane ≠ right.newLane)
    (candidateEdges : candidate.all (fun edge =>
      edge.newLane < leftSpan && edge.oldLane < rightSpan &&
        compatible edge.newLane edge.oldLane) = true) :
    candidate.length ≤ certificate.matching.length := by
  rcases valid with ⟨_, _, _, _, _, _, coverEdges, equalCardinality⟩
  have covered : ∀ edge, edge ∈ candidate →
      edge.newLane ∈ certificate.coverLeft ∨
        edge.oldLane ∈ certificate.coverRight := by
    intro edge edgeMember
    have candidateEdge := List.all_eq_true.mp candidateEdges edge edgeMember
    simp only [Bool.and_eq_true] at candidateEdge
    rcases candidateEdge with ⟨⟨leftBound, rightBound⟩, compatibleEdge⟩
    have leftMember : edge.newLane ∈ List.range leftSpan :=
      List.mem_range.mpr (of_decide_eq_true leftBound)
    have rightMember : edge.oldLane ∈ List.range rightSpan :=
      List.mem_range.mpr (of_decide_eq_true rightBound)
    have leftRows := List.all_eq_true.mp coverEdges edge.newLane leftMember
    have coverRow := List.all_eq_true.mp leftRows edge.oldLane rightMember
    simp only [Bool.or_eq_true] at coverRow
    rcases coverRow with (incompatible | leftMember) | rightMember
    · simp [compatibleEdge] at incompatible
    · exact Or.inl (by simpa using leftMember)
    · exact Or.inr (by simpa using rightMember)
  have bound := lane_matching_cardinality_le_vertex_cover candidateDistinct covered
  omega

/-- Reassign lane classes inside a selected set of source occurrences. -/
def routeAlignedLane {α : Type} (inside : α → Bool) (lane : α → Nat)
    (remap : Nat → Nat) (event : α) : Nat :=
  if inside event then remap (lane event) else lane event

/-- The matching graph excludes each incompatible cross-boundary pair; an
injective arm map preserves the inside pairs. No new collision can appear. -/
theorem route_lane_alignment_introduces_no_conflict
    {α : Type} {inside : α → Bool} {lane : α → Nat} {remap : Nat → Nat}
    {conflict : α → α → Prop}
    (injective : ∀ a b, inside a = true → inside b = true →
      remap (lane a) = remap (lane b) → lane a = lane b)
    (cross : ∀ a b, inside a = true → inside b = false →
      (conflict a b ∨ conflict b a) → remap (lane a) ≠ lane b)
    {a b : α} (incompatible : conflict a b)
    (collision : routeAlignedLane inside lane remap a = routeAlignedLane inside lane remap b) :
    lane a = lane b := by
  cases ha : inside a <;> cases hb : inside b
  · simpa [routeAlignedLane, ha, hb] using collision
  · exact False.elim ((cross b a hb ha (Or.inr incompatible))
      (by simpa [routeAlignedLane, ha, hb] using collision.symm))
  · exact False.elim ((cross a b ha hb (Or.inl incompatible))
      (by simpa [routeAlignedLane, ha, hb] using collision))
  · exact injective a b ha hb (by simpa [routeAlignedLane, ha, hb] using collision)

/-- A valid coloring stays valid after a matching that meets the arm's graph. -/
theorem route_lane_alignment_preserves_valid_coloring
    {α : Type} {inside : α → Bool} {lane : α → Nat} {remap : Nat → Nat}
    {conflict : α → α → Prop}
    (injective : ∀ a b, inside a = true → inside b = true →
      remap (lane a) = remap (lane b) → lane a = lane b)
    (cross : ∀ a b, inside a = true → inside b = false →
      (conflict a b ∨ conflict b a) → remap (lane a) ≠ lane b)
    (valid : ∀ a b, conflict a b → lane a ≠ lane b) :
    ∀ a b, conflict a b →
      routeAlignedLane inside lane remap a ≠ routeAlignedLane inside lane remap b := by
  intro a b incompatible collision
  exact valid a b incompatible
    (route_lane_alignment_introduces_no_conflict injective cross incompatible collision)


/-- Removing a witnessed incompatible edge cannot remove a valid matching. -/
theorem incompatible_lane_edge_preserves_valid_matchings
    (allowed : Nat → Nat → Bool) (compatible : Nat → Nat → Prop)
    (mapping : Nat → Nat) (row color : Nat)
    (invalid : ¬ compatible row color)
    (valid : ∀ r, compatible r (mapping r))
    (selected : ∀ r, allowed r (mapping r) = true) :
    ∀ r, (allowed r (mapping r) && !(decide (r = row) && decide (mapping r = color))) = true := by
  intro r
  by_cases same : r = row
  · subst r
    have distinct : mapping row ≠ color := by
      intro equal
      exact invalid (equal ▸ valid row)
    simp [selected row, distinct]
  · simp [selected r, same]

/-- Packing only the lane classes present in an arm preserves the wire-class
injectivity required by remapping. Absent classes require no matching vertex. -/
theorem compact_lane_vertices_preserve_class_injectivity
    (active : Nat → Prop) (vertex color : Nat → Nat)
    (packed : ∀ a b, active a → active b → vertex a = vertex b → a = b)
    (matched : ∀ a b, active a → active b →
      color (vertex a) = color (vertex b) → vertex a = vertex b) :
    ∀ a b, active a → active b → color (vertex a) = color (vertex b) → a = b := by
  intro a b first second same
  exact packed a b first second (matched a b first second same)

/-- A certificate covers every source occurrence in its old lane class.
Changing another matching edge cannot alter this source-only predicate. -/
def LaneClassCertificate {α : Type} (inside : α → Bool) (lane : α → Nat)
    (conflict : α → α → Prop) (row color : Nat) : Prop :=
  ∀ a b, inside a = true → lane a = row → inside b = false → lane b = color →
    ¬ (conflict a b ∨ conflict b a)

theorem confirming_a_whole_lane_class_preserves_certificate_soundness
    {α : Type} (inside : α → Bool) (lane : α → Nat) (conflict : α → α → Prop)
    (confirmed : Nat → Nat → Bool) (row color : Nat)
    (sound : ∀ r c, confirmed r c = true → LaneClassCertificate inside lane conflict r c)
    (checked : LaneClassCertificate inside lane conflict row color) :
    ∀ r c, (if r = row ∧ c = color then true else confirmed r c) = true →
      LaneClassCertificate inside lane conflict r c := by
  intro r c available
  by_cases same : r = row ∧ c = color
  · rcases same with ⟨rfl, rfl⟩
    exact checked
  · exact sound r c (by simpa only [if_neg same] using available)

theorem confirmed_lane_classes_supply_all_cross_boundary_checks
    {α : Type} (inside : α → Bool) (lane : α → Nat) (conflict : α → α → Prop)
    (confirmed : Nat → Nat → Bool) (remap : Nat → Nat)
    (sound : ∀ r c, confirmed r c = true → LaneClassCertificate inside lane conflict r c)
    (selected : ∀ a, inside a = true → confirmed (lane a) (remap (lane a)) = true) :
    ∀ a b, inside a = true → inside b = false →
      (conflict a b ∨ conflict b a) → remap (lane a) ≠ lane b := by
  intro a b first second incompatible equal
  exact sound (lane a) (remap (lane a)) (selected a first)
    a b first rfl second equal.symm incompatible

end Hibana
