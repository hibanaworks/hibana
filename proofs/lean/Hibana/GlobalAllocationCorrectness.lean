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

private theorem range_nodup_constructive : ∀ count, (List.range count).Nodup
  | 0 => by simp
  | count + 1 => by
      rw [List.range_succ]
      exact List.nodup_append.mpr ⟨
        range_nodup_constructive count,
        by simp,
        by
          intro value valueMember other otherMember
          have bound := List.mem_range.mp valueMember
          have otherEq : other = count := by simpa using otherMember
          subst other
          exact Nat.ne_of_lt bound
      ⟩

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

/-- The production first-free search cannot reject while fewer than all 256
wire labels are represented by competing classes. -/
theorem first_available_frame_label_complete
    {used : List Nat} (capacity : used.length < 256) :
    ∃ label, firstAvailableFrameLabel used = some label ∧
      label < 256 ∧ label ∉ used := by
  have available : firstAvailableFrameLabel used ≠ none := by
    intro unavailable
    have allUsed : ∀ label, label ∈ List.range 256 → label ∈ used := by
      intro label labelMember
      have rejected := List.find?_eq_none.mp unavailable label labelMember
      simpa using rejected
    have rangeBound := nodup_subset_length_le (range_nodup_constructive 256) allUsed
    simp only [List.length_range] at rangeBound
    omega
  cases selected : firstAvailableFrameLabel used with
  | none => exact False.elim (available selected)
  | some label =>
      have labelMember : label ∈ List.range 256 :=
        List.mem_of_find?_eq_some selected
      have labelUnused : label ∉ used := by
        have selectedPredicate := List.find?_some selected
        simpa using selectedPredicate
      exact ⟨label, rfl, List.mem_range.mp labelMember, labelUnused⟩

def allocateFrameLabels (used : List Nat) : Nat → Option (List Nat)
  | 0 => some []
  | count + 1 => do
      let label ← firstAvailableFrameLabel used
      let rest ← allocateFrameLabels (label :: used) count
      pure (label :: rest)

/-- Repeated production-order first-free allocation is complete whenever the
existing and pending competing classes fit the one-byte frame-label domain. -/
theorem frame_label_allocation_complete
    {used : List Nat} {count : Nat}
    (capacity : used.length + count ≤ 256) :
    allocateFrameLabels used count ≠ none := by
  induction count generalizing used with
  | zero => simp [allocateFrameLabels]
  | succ count countIH =>
      have usedCapacity : used.length < 256 := by omega
      obtain ⟨label, selected, _, _⟩ :=
        first_available_frame_label_complete usedCapacity
      have remainingCapacity : (label :: used).length + count ≤ 256 := by
        simp only [List.length_cons]
        omega
      cases rest : allocateFrameLabels (label :: used) count with
      | none => exact False.elim ((countIH remainingCapacity) rest)
      | some labels => simp [allocateFrameLabels, selected, rest]

end Hibana
