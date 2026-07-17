import Std

namespace Hibana

def completedEventWordIndex (step : Nat) : Nat := step / 32

def completedEventBitIndex (step : Nat) : Nat := step % 32

def completedEventWordCount (eventCount : Nat) : Nat :=
  (eventCount + 31) / 32

/-- The compact completion-bit locator loses no event identity. -/
theorem completed_event_word_and_bit_reconstruct_step (step : Nat) :
    completedEventWordIndex step * 32 + completedEventBitIndex step = step := by
  unfold completedEventWordIndex completedEventBitIndex
  omega

/-- The descriptor-derived completion bitmap covers every admitted local event. -/
theorem completed_event_word_count_covers_events (eventCount : Nat) :
    eventCount <= completedEventWordCount eventCount * 32 := by
  unfold completedEventWordCount
  omega

/-- Every admitted event selects an allocated completion word. -/
theorem completed_event_word_index_in_bounds
    {step eventCount : Nat}
    (within : step < eventCount) :
    completedEventWordIndex step < completedEventWordCount eventCount := by
  unfold completedEventWordIndex completedEventWordCount
  omega

inductive SlabRegionKind where
  | resident
  | endpoint

structure SlabRegion where
  kind : SlabRegionKind
  offset : Nat
  bytes : Nat
  align : Nat

def SlabRegion.endOffset (region : SlabRegion) : Nat := region.offset + region.bytes

private def powerOfTwo (value : Nat) : Bool :=
  value != 0 && (value &&& (value - 1)) == 0

def PowerOfTwoAlignment (value : Nat) : Prop :=
  value ≠ 0 /\ (value &&& (value - 1)) = 0

def SlabRegion.aligned (base : Nat) (region : SlabRegion) : Bool :=
  powerOfTwo region.align && (base + region.offset) % region.align == 0

def SlabRegion.disjoint (left right : SlabRegion) : Bool :=
  left.endOffset <= right.offset || right.endOffset <= left.offset

def SlabRegion.InBounds
    (capacity imageFrontier endpointFloor : Nat) (region : SlabRegion) : Prop :=
  0 < region.bytes /\
  region.endOffset <= capacity /\
  match region.kind with
  | .resident => region.endOffset <= imageFrontier
  | .endpoint => endpointFloor <= region.offset

def SlabRegion.checkBounds
    (capacity imageFrontier endpointFloor : Nat) (region : SlabRegion) : Bool :=
  region.bytes > 0 &&
  region.endOffset <= capacity &&
  match region.kind with
  | .resident => region.endOffset <= imageFrontier
  | .endpoint => endpointFloor <= region.offset

def RegionsPairwiseDisjoint : List SlabRegion -> Prop
  | [] => True
  | region :: rest =>
      (forall other, other ∈ rest -> region.disjoint other = true) /\
      RegionsPairwiseDisjoint rest

private def checkPairwiseDisjoint : List SlabRegion -> Bool
  | [] => true
  | region :: rest =>
      rest.all (region.disjoint ·) && checkPairwiseDisjoint rest

structure SlabLayoutCertificate where
  base : Nat
  capacity : Nat
  imageFrontier : Nat
  workspaceBytes : Nat
  endpointFloor : Nat
  regions : List SlabRegion

def SlabLayoutCertificate.WellFormed (certificate : SlabLayoutCertificate) : Prop :=
  certificate.imageFrontier + certificate.workspaceBytes <= certificate.endpointFloor /\
  certificate.endpointFloor <= certificate.capacity /\
  (forall region, region ∈ certificate.regions ->
    PowerOfTwoAlignment region.align /\
    (certificate.base + region.offset) % region.align = 0 /\
    region.InBounds certificate.capacity certificate.imageFrontier certificate.endpointFloor) /\
  RegionsPairwiseDisjoint certificate.regions

def SlabLayoutCertificate.check (certificate : SlabLayoutCertificate) : Bool :=
  certificate.imageFrontier + certificate.workspaceBytes <= certificate.endpointFloor &&
  certificate.endpointFloor <= certificate.capacity &&
  (certificate.regions.all fun region =>
    region.aligned certificate.base &&
    region.checkBounds certificate.capacity certificate.imageFrontier certificate.endpointFloor) &&
  checkPairwiseDisjoint certificate.regions

private theorem region_bounds_checker_sound
    {capacity imageFrontier endpointFloor : Nat} {region : SlabRegion}
    (accepted : region.checkBounds capacity imageFrontier endpointFloor = true) :
    region.InBounds capacity imageFrontier endpointFloor := by
  cases kindCase : region.kind <;>
    simp [SlabRegion.checkBounds, SlabRegion.InBounds, kindCase] at accepted ⊢
  · exact ⟨accepted.1.1, accepted.1.2, accepted.2⟩
  · exact ⟨accepted.1.1, accepted.1.2, accepted.2⟩

private theorem region_alignment_checker_sound
    {base : Nat} {region : SlabRegion}
    (accepted : region.aligned base = true) :
    PowerOfTwoAlignment region.align /\ (base + region.offset) % region.align = 0 := by
  simp [SlabRegion.aligned, powerOfTwo] at accepted
  exact ⟨⟨accepted.1.1, accepted.1.2⟩, accepted.2⟩

private theorem pairwise_checker_sound
    (regions : List SlabRegion)
    (accepted : checkPairwiseDisjoint regions = true) :
    RegionsPairwiseDisjoint regions := by
  induction regions with
  | nil => trivial
  | cons region rest ih =>
      simp [checkPairwiseDisjoint] at accepted
      exact ⟨accepted.1, ih accepted.2⟩

/-- Accepted slab certificates have aligned, bounded, pairwise-disjoint owner regions. -/
theorem slab_layout_certificate_sound
    {certificate : SlabLayoutCertificate}
    (accepted : certificate.check = true) : certificate.WellFormed := by
  have parsed :
      ((certificate.imageFrontier + certificate.workspaceBytes <= certificate.endpointFloor /\
        certificate.endpointFloor <= certificate.capacity) /\
        (forall region, region ∈ certificate.regions ->
          region.aligned certificate.base = true /\
          region.checkBounds certificate.capacity certificate.imageFrontier
            certificate.endpointFloor = true)) /\
      checkPairwiseDisjoint certificate.regions = true := by
    simpa [SlabLayoutCertificate.check] using accepted
  rcases parsed with ⟨⟨⟨frontier, floor⟩, regions⟩, disjoint⟩
  refine ⟨frontier, floor, ?_, pairwise_checker_sound certificate.regions disjoint⟩
  intro region member
  have checked := regions region member
  have alignment := region_alignment_checker_sound checked.1
  exact ⟨alignment.1, alignment.2, region_bounds_checker_sound checked.2⟩

end Hibana
