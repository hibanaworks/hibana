import Hibana.Refinement
import Hibana.OperationAdmission
import Hibana.DescriptorParticipants

namespace Hibana

def productionProgramAtomStride : Nat := 11

def productionProgramRouteResolverStride : Nat := 8

def productionProgramRouteParticipantStride : Nat := 1

def productionProgramScopeMarkerStride : Nat := 5

def productionRoleEventStride : Nat := 10

def productionRoleLaneStride : Nat := 1

def productionRoleDependencyStride : Nat := 8

def productionRoleConflictStride : Nat := 2

def productionRoleRouteScopeStride : Nat := 2

def productionRoleRouteArmStride : Nat := 8

def productionRoleU16Stride : Nat := 2

def productionRoleLaneRangeStride : Nat := 4

def productionRoleRouteArmLaneStepStride : Nat := 5

def productionRoleRollScopeStride : Nat := 6

def productionEventIdentityCapacity : Nat := 65535

def productionDescriptorByteCapacity : Nat := 65535

def productionAtomOnlyEventCapacity : Nat :=
  productionDescriptorByteCapacity / productionProgramAtomStride

theorem production_atom_only_event_capacity_exact :
    productionAtomOnlyEventCapacity = 5957 := by
  decide

theorem descriptor_byte_ceiling_dominates_event_identity_domain :
    productionAtomOnlyEventCapacity < productionEventIdentityCapacity := by
  decide

/-- Exact host-side source and final program-image counts. Dynamic resolver
markers are a subset of route-resolver rows because intrinsic routes still own
descriptor rows. This witness is erased after projection. -/
structure ProgramSourceCapacityWitness where
  eventCount : Nat
  scopeMarkerCount : Nat
  resolverMarkerCount : Nat
  routeResolverCount : Nat
  routeParticipantCount : Nat
  programByteLength : Nat
  resolverMarkersCovered : resolverMarkerCount ≤ routeResolverCount
  programBytesExact : programByteLength =
    eventCount * productionProgramAtomStride +
    routeResolverCount * productionProgramRouteResolverStride +
    routeParticipantCount * productionProgramRouteParticipantStride +
    scopeMarkerCount * productionProgramScopeMarkerStride
  descriptorFits : programByteLength ≤ productionDescriptorByteCapacity

def ProgramSourceCapacityWitness.sourceRowCount
    (witness : ProgramSourceCapacityWitness) : Nat :=
  witness.eventCount + witness.scopeMarkerCount + witness.resolverMarkerCount

/-- Every source row consumes at least one byte in the exact final program
image. The stable-Rust source arena therefore has no independent capacity
limit below the descriptor representation it produces. -/
theorem program_descriptor_bytes_dominate_source_rows
    (witness : ProgramSourceCapacityWitness) :
    witness.sourceRowCount ≤ witness.programByteLength := by
  unfold ProgramSourceCapacityWitness.sourceRowCount
  have resolverCovered := witness.resolverMarkersCovered
  have bytesExact := witness.programBytesExact
  simp only [productionProgramAtomStride, productionProgramRouteResolverStride,
    productionProgramRouteParticipantStride, productionProgramScopeMarkerStride,
    Nat.mul_one] at bytesExact
  omega

theorem descriptor_byte_ceiling_covers_source_lowering_rows
    (witness : ProgramSourceCapacityWitness) :
    witness.sourceRowCount ≤ productionDescriptorByteCapacity :=
  Nat.le_trans (program_descriptor_bytes_dominate_source_rows witness)
    witness.descriptorFits

def productionScopeCapacity : Nat := 8192

def productionPackedU16Capacity : Nat := 65536

def productionLocalStepCapacity : Nat := productionPackedU16Capacity

abbrev productionCursorPositionRepresentable (position : Nat) : Prop :=
  position < productionPackedU16Capacity

/-- The terminal cursor after all compact event identities have been consumed
is a value, not another event identity, and therefore may use `u16::MAX`. -/
theorem production_cursor_accepts_terminal_event_count :
    productionCursorPositionRepresentable productionEventIdentityCapacity := by
  decide

def productionResolverCapacity : Nat := productionPackedU16Capacity

def productionRouteResolverDynamicScopeBit : Nat := 32768

/-- Capacity facts emitted by a well-formed program image. Every structured
scope owns at least an enter/exit marker pair, and each route-commit row is
owned by a distinct active route scope. -/
structure ProgramCapacityWitness where
  scopeCount : Nat
  scopeMarkerCount : Nat
  routeCommitCount : Nat
  programByteLength : Nat
  scopeMarkersCoverScopes : scopeCount * 2 ≤ scopeMarkerCount
  routeCommitsCoveredByScopes : routeCommitCount ≤ scopeCount
  scopeMarkerBytesFit : scopeMarkerCount * productionProgramScopeMarkerStride ≤ programByteLength
  descriptorFits : programByteLength ≤ productionDescriptorByteCapacity

/-- The 64 KiB descriptor ceiling is stricter than both the packed scope-id
domain and the resident `u16` route-commit count. No additional `u8` chain
limit is needed by an accepted image. -/
theorem descriptor_byte_ceiling_dominates_scope_and_route_commit_counts
    (witness : ProgramCapacityWitness) :
    witness.scopeCount < productionScopeCapacity ∧
      witness.routeCommitCount < productionLocalStepCapacity := by
  have markerBytes : witness.scopeMarkerCount * 5 ≤ witness.programByteLength :=
    witness.scopeMarkerBytesFit
  have descriptorFits : witness.programByteLength ≤ 65535 :=
    witness.descriptorFits
  have scopesCovered : witness.scopeCount * 2 ≤ witness.scopeMarkerCount :=
    witness.scopeMarkersCoverScopes
  have routeCovered : witness.routeCommitCount ≤ witness.scopeCount :=
    witness.routeCommitsCoveredByScopes
  constructor
  · dsimp [productionScopeCapacity]
    omega
  · dsimp [productionLocalStepCapacity, productionPackedU16Capacity]
    omega

def routeCommit257CapacityWitness : ProgramCapacityWitness := {
  scopeCount := 257
  scopeMarkerCount := 1028
  routeCommitCount := 257
  programByteLength := 5140
  scopeMarkersCoverScopes := by decide
  routeCommitsCoveredByScopes := by decide
  scopeMarkerBytesFit := by decide
  descriptorFits := by decide
}

theorem route_commit_count_257_fits_the_descriptor_derived_domain :
    routeCommit257CapacityWitness.routeCommitCount = 257 ∧
      routeCommit257CapacityWitness.programByteLength < productionDescriptorByteCapacity := by
  decide

/-- Loop-iteration bound for traversing an acyclic parent chain. The extra one
is the terminating iteration, not retained route-commit capacity. -/
def productionRouteTraversalBound (routeScopeCount : Nat) : Nat :=
  routeScopeCount + 1

theorem production_route_traversal_bound_covers_every_acyclic_parent_chain
    (routeScopeCount chainLength : Nat)
    (chainCoveredByScopes : chainLength ≤ routeScopeCount) :
    chainLength < productionRouteTraversalBound routeScopeCount := by
  simp only [productionRouteTraversalBound]
  omega

theorem production_route_traversal_covers_full_compact_step_domain :
    productionRouteTraversalBound 257 = 258 := by
  decide

/-- One emitted route-arm lane relation is a compact witness that a lane can
own a selected arm for that route. Every live runtime `(lane, route)` state must
map to one of those descriptor rows. -/
structure RouteHistoryCapacityWitness where
  activeLaneCount : Nat
  routeCommitCount : Nat
  emittedLaneRelationCount : Nat
  liveRouteStateCount : Nat
  liveStatesCoveredByRelations : liveRouteStateCount ≤ emittedLaneRelationCount
  relationColumnFits : emittedLaneRelationCount ≤ productionDescriptorByteCapacity

def productionRouteHistoryCapacity (witness : RouteHistoryCapacityWitness) : Nat :=
  witness.emittedLaneRelationCount

theorem production_route_history_capacity_covers_every_live_state
    (witness : RouteHistoryCapacityWitness) :
    witness.liveRouteStateCount ≤ productionRouteHistoryCapacity witness := by
  exact witness.liveStatesCoveredByRelations

theorem production_route_history_capacity_fits_the_compact_descriptor_domain
    (witness : RouteHistoryCapacityWitness) :
    productionRouteHistoryCapacity witness ≤ productionDescriptorByteCapacity := by
  exact witness.relationColumnFits

def sparseRouteHistoryExample : RouteHistoryCapacityWitness := {
  activeLaneCount := 200
  routeCommitCount := 300
  emittedLaneRelationCount := 17
  liveRouteStateCount := 17
  liveStatesCoveredByRelations := by decide
  relationColumnFits := by decide
}

theorem sparse_route_history_avoids_the_lane_depth_product :
    productionRouteHistoryCapacity sparseRouteHistoryExample = 17 ∧
      productionRouteHistoryCapacity sparseRouteHistoryExample <
        sparseRouteHistoryExample.activeLaneCount * sparseRouteHistoryExample.routeCommitCount := by
  decide

def productionLaneCapacity : Nat := 256

/-- Semantic route-arm lane relations. The production emitter preserves
first-occurrence row order separately; relation membership is the bounded lane
set below. -/
def projectedRouteArmLanes (lanes : List Nat) : List Nat :=
  (List.range productionLaneCapacity).filter fun lane => lane ∈ lanes

theorem projected_route_arm_lanes_are_exact
    (lanes : List Nat) (lane : Nat)
    (inDomain : lane < productionLaneCapacity) :
    lane ∈ projectedRouteArmLanes lanes ↔ lane ∈ lanes := by
  simp [projectedRouteArmLanes, inDomain]

theorem projected_route_arm_lanes_exclude_out_of_domain
    (lanes : List Nat) (lane : Nat)
    (outOfDomain : productionLaneCapacity ≤ lane) :
    lane ∉ projectedRouteArmLanes lanes := by
  simp [projectedRouteArmLanes, Nat.not_lt.mpr outOfDomain]

theorem projected_route_arm_lane_relation_count_is_domain_bounded
    (lanes : List Nat) :
    (projectedRouteArmLanes lanes).length ≤ productionLaneCapacity := by
  unfold projectedRouteArmLanes
  calc
    _ ≤ (List.range productionLaneCapacity).length := List.length_filter_le _ _
    _ = productionLaneCapacity := List.length_range

theorem projected_route_arm_lane_duplicates_do_not_allocate_relations
    (lanes : List Nat) (lane : Nat) :
    projectedRouteArmLanes (lanes ++ [lane, lane]) =
      projectedRouteArmLanes (lanes ++ [lane]) := by
  simp [projectedRouteArmLanes]

/-- Runtime lane-indexed storage is sized by the exact projected lane span. No
extra binding lanes are reserved because production never creates a lane that
is absent from the accepted descriptor. -/
structure RoleLaneSpanWitness where
  activeLaneCount : Nat
  endpointLaneSlotCount : Nat
  nonemptySpan : 0 < endpointLaneSlotCount
  activeLanesCovered : activeLaneCount ≤ endpointLaneSlotCount
  spanFitsWireDomain : endpointLaneSlotCount ≤ productionLaneCapacity

def productionLogicalLaneCount (witness : RoleLaneSpanWitness) : Nat :=
  witness.endpointLaneSlotCount

theorem production_logical_lane_count_is_exact_descriptor_span
    (witness : RoleLaneSpanWitness) :
    productionLogicalLaneCount witness = witness.endpointLaneSlotCount := rfl

theorem production_logical_lane_count_covers_active_lanes
    (witness : RoleLaneSpanWitness) :
    witness.activeLaneCount ≤ productionLogicalLaneCount witness := by
  exact witness.activeLanesCovered

theorem production_logical_lane_count_has_no_reserved_lane_overhead
    (witness : RoleLaneSpanWitness) :
    productionLogicalLaneCount witness ≤ productionLaneCapacity := by
  exact witness.spanFitsWireDomain

/-- Runtime offer entries have one exact `(scope, local entry)` key and retain
the active lane that owns each key. Several exact keys may target the same
cursor entry, but no key can exist without an owning active lane. -/
structure FrontierCapacityWitness where
  activeLaneCount : Nat
  activeEntryCount : Nat
  visitedEntryCount : Nat
  entriesOwnedByActiveLanes : activeEntryCount ≤ activeLaneCount
  visitedEntriesOwnedByActiveEntries : visitedEntryCount ≤ activeEntryCount
  activeLanesFitWireDomain : activeLaneCount ≤ productionLaneCapacity

/-- Frontier storage is exactly the number of active lanes. Empty projections
need no frontier cell; their session lane remains represented independently. -/
def productionFrontierCapacity (activeLaneCount : Nat) : Nat :=
  activeLaneCount

theorem production_frontier_capacity_is_exact_active_lane_count
    (activeLaneCount : Nat) :
    productionFrontierCapacity activeLaneCount = activeLaneCount := by
  rfl

theorem production_frontier_capacity_covers_all_active_entries
    (witness : FrontierCapacityWitness) :
    witness.activeEntryCount ≤ productionFrontierCapacity witness.activeLaneCount := by
  exact witness.entriesOwnedByActiveLanes

/-- Visit tracking uses exact local-entry identity, not route scope identity.
Every visited entry therefore belongs to the same active-entry domain and the
active-lane-derived allocation never needs truncation. -/
theorem production_frontier_capacity_covers_all_visited_entries
    (witness : FrontierCapacityWitness) :
    witness.visitedEntryCount ≤ productionFrontierCapacity witness.activeLaneCount := by
  exact Nat.le_trans witness.visitedEntriesOwnedByActiveEntries
    (production_frontier_capacity_covers_all_active_entries witness)

theorem production_frontier_capacity_is_bounded_by_the_lane_domain
    (activeLaneCount : Nat)
    (laneBound : activeLaneCount ≤ productionLaneCapacity) :
    productionFrontierCapacity activeLaneCount ≤ productionLaneCapacity := by
  exact laneBound

theorem production_frontier_capacity_reaches_full_lane_domain :
    productionFrontierCapacity 9 = 9 ∧
      productionFrontierCapacity 256 = productionLaneCapacity := by
  decide

/-- The production frontier walk retains only the first admissible candidate
and the first candidate matching the current frontier as transient selection
state. The state disappears after the walk; no candidate array is required. -/
structure FrontierProgressSelectionModel (α : Type) where
  preferred : Option α
  firstAdmissible : Option α

def FrontierProgressSelectionModel.retainFirst
    (current next : Option α) : Option α :=
  match current with
  | some value => some value
  | none => next

def FrontierProgressSelectionModel.consider
    (admissible : α → Bool)
    (matchesFrontier : α → Bool)
    (selection : FrontierProgressSelectionModel α)
    (candidate : α) : FrontierProgressSelectionModel α :=
  if admissible candidate then
    { preferred := FrontierProgressSelectionModel.retainFirst selection.preferred
        (if matchesFrontier candidate then some candidate else none)
      firstAdmissible := FrontierProgressSelectionModel.retainFirst
        selection.firstAdmissible (some candidate) }
  else
    selection

def scanFrontierProgress
    (admissible : α → Bool)
    (matchesFrontier : α → Bool)
    (selection : FrontierProgressSelectionModel α)
    (candidates : List α) : FrontierProgressSelectionModel α :=
  match candidates with
  | [] => selection
  | candidate :: rest =>
      scanFrontierProgress admissible matchesFrontier
        (selection.consider admissible matchesFrontier candidate) rest

def firstAdmissible (admissible : α → Bool) : List α → Option α
  | [] => none
  | candidate :: rest =>
      if admissible candidate then some candidate
      else firstAdmissible admissible rest

def firstAdmissibleFrontierMatch
    (admissible matchesFrontier : α → Bool) : List α → Option α
  | [] => none
  | candidate :: rest =>
      if admissible candidate && matchesFrontier candidate then some candidate
      else firstAdmissibleFrontierMatch admissible matchesFrontier rest

def referenceFrontierProgressSelection
    (admissible : α → Bool)
    (matchesFrontier : α → Bool)
    (candidates : List α) : Option α :=
  FrontierProgressSelectionModel.retainFirst
    (firstAdmissibleFrontierMatch admissible matchesFrontier candidates)
    (firstAdmissible admissible candidates)

def streamingFrontierProgressSelection
    (admissible : α → Bool)
    (matchesFrontier : α → Bool)
    (candidates : List α) : Option α :=
  let selection := scanFrontierProgress admissible matchesFrontier
    { preferred := none, firstAdmissible := none } candidates
  FrontierProgressSelectionModel.retainFirst
    selection.preferred selection.firstAdmissible

theorem scan_frontier_progress_retains_first_admissible
    (admissible matchesFrontier : α → Bool)
    (selection : FrontierProgressSelectionModel α)
    (candidates : List α) :
    (scanFrontierProgress admissible matchesFrontier selection candidates).firstAdmissible =
      FrontierProgressSelectionModel.retainFirst
        selection.firstAdmissible (firstAdmissible admissible candidates) := by
  induction candidates generalizing selection with
  | nil =>
      cases selected : selection.firstAdmissible <;>
        simp [scanFrontierProgress, firstAdmissible,
          FrontierProgressSelectionModel.retainFirst, selected]
  | cons candidate rest ih =>
      rw [scanFrontierProgress, ih]
      cases selected : selection.firstAdmissible <;>
        cases accepted : admissible candidate <;>
          simp [FrontierProgressSelectionModel.consider, firstAdmissible,
            FrontierProgressSelectionModel.retainFirst, selected, accepted]

theorem scan_frontier_progress_retains_first_match
    (admissible matchesFrontier : α → Bool)
    (selection : FrontierProgressSelectionModel α)
    (candidates : List α) :
    (scanFrontierProgress admissible matchesFrontier selection candidates).preferred =
      FrontierProgressSelectionModel.retainFirst selection.preferred
        (firstAdmissibleFrontierMatch admissible matchesFrontier candidates) := by
  induction candidates generalizing selection with
  | nil =>
      cases selected : selection.preferred <;>
        simp [scanFrontierProgress, firstAdmissibleFrontierMatch,
          FrontierProgressSelectionModel.retainFirst, selected]
  | cons candidate rest ih =>
      rw [scanFrontierProgress, ih]
      cases selected : selection.preferred <;>
        cases accepted : admissible candidate <;>
          cases isMatch : matchesFrontier candidate <;>
            simp [FrontierProgressSelectionModel.consider,
              firstAdmissibleFrontierMatch,
              FrontierProgressSelectionModel.retainFirst,
              selected, accepted, isMatch]

/-- For every finite active frontier, the constant-state production walk
applies the same admission filter and priority as the reference semantics. -/
theorem streaming_frontier_progress_selection_is_reference_exact
    (admissible : α → Bool)
    (matchesFrontier : α → Bool)
    (candidates : List α) :
    streamingFrontierProgressSelection admissible matchesFrontier candidates =
      referenceFrontierProgressSelection admissible matchesFrontier candidates := by
  simp only [streamingFrontierProgressSelection, referenceFrontierProgressSelection]
  rw [scan_frontier_progress_retains_first_match,
    scan_frontier_progress_retains_first_admissible]
  simp [FrontierProgressSelectionModel.retainFirst]

/-- Exact fields used by the production frontier admission and priority scan. -/
structure FrontierProgressCandidateIdentity where
  scope : Nat
  entry : Nat
  parallelRoot : Option Nat
  frontier : Nat
deriving DecidableEq

def productionFrontierProgressAdmissible
    (currentScope currentEntry : Nat)
    (currentParallelRoot : Option Nat)
    (visitedEntries : List Nat)
    (candidate : FrontierProgressCandidateIdentity) : Bool :=
  decide ((candidate.scope ≠ currentScope ∨ candidate.entry ≠ currentEntry) ∧
    (currentParallelRoot = none ∨ candidate.parallelRoot = currentParallelRoot) ∧
    candidate.entry ∉ visitedEntries)

theorem current_offer_owner_is_not_frontier_progress_admissible
    (currentScope currentEntry frontier : Nat)
    (currentParallelRoot : Option Nat)
    (visitedEntries : List Nat) :
    productionFrontierProgressAdmissible
      currentScope currentEntry currentParallelRoot visitedEntries
      { scope := currentScope
        entry := currentEntry
        parallelRoot := currentParallelRoot
        frontier } = false := by
  simp [productionFrontierProgressAdmissible]

theorem visited_entry_is_not_frontier_progress_admissible
    (currentScope currentEntry : Nat)
    (currentParallelRoot : Option Nat)
    (visitedEntries : List Nat)
    (candidate : FrontierProgressCandidateIdentity)
    (visited : candidate.entry ∈ visitedEntries) :
    productionFrontierProgressAdmissible
      currentScope currentEntry currentParallelRoot visitedEntries candidate = false := by
  simp [productionFrontierProgressAdmissible, visited]

theorem foreign_parallel_root_is_not_frontier_progress_admissible
    (currentScope currentEntry currentParallelRoot : Nat)
    (visitedEntries : List Nat)
    (candidate : FrontierProgressCandidateIdentity)
    (foreign : candidate.parallelRoot ≠ some currentParallelRoot) :
    productionFrontierProgressAdmissible
      currentScope currentEntry (some currentParallelRoot) visitedEntries candidate = false := by
  simp [productionFrontierProgressAdmissible, foreign]

/-- The production owner/current/root/visited filter and frontier preference are
checked in one streaming pass and are extensionally equal to filtered reference
selection for every finite candidate list. -/
theorem production_frontier_progress_selection_is_filtered_reference_exact
    (currentScope currentEntry currentFrontier : Nat)
    (currentParallelRoot : Option Nat)
    (visitedEntries : List Nat)
    (candidates : List FrontierProgressCandidateIdentity) :
    streamingFrontierProgressSelection
      (productionFrontierProgressAdmissible
        currentScope currentEntry currentParallelRoot visitedEntries)
      (fun candidate => decide (candidate.frontier = currentFrontier)) candidates =
    referenceFrontierProgressSelection
      (productionFrontierProgressAdmissible
        currentScope currentEntry currentParallelRoot visitedEntries)
      (fun candidate => decide (candidate.frontier = currentFrontier)) candidates := by
  exact streaming_frontier_progress_selection_is_reference_exact _ _ _

/-- Exact identity used by production offer ownership before cursor-target
observations erase scope while retaining one row per witness. -/
structure ActiveOfferKey where
  entry : Nat
  scope : Nat
deriving DecidableEq

/-- The production active-entry set stores one concrete owning lane for every
exact key. Root, frontier, and flags stay unconstrained here so membership
proves that lookup returns the owner's metadata verbatim. -/
structure ActiveOfferLane where
  lane : Nat
  entry : Nat
  scope : Nat
  parallelRoot : Option Nat
  frontier : Nat
  flags : Nat
deriving DecidableEq

def ActiveOfferLane.key (lane : ActiveOfferLane) : ActiveOfferKey :=
  { entry := lane.entry, scope := lane.scope }

def activeOfferKeys : List ActiveOfferLane → List ActiveOfferKey
  | [] => []
  | lane :: rest =>
      let keys := activeOfferKeys rest
      if lane.key ∈ keys then keys else lane.key :: keys

theorem active_offer_key_count_is_bounded_by_active_lane_count
    (lanes : List ActiveOfferLane) :
    (activeOfferKeys lanes).length ≤ lanes.length := by
  induction lanes with
  | nil => simp [activeOfferKeys]
  | cons lane rest ih =>
      simp only [activeOfferKeys]
      split
      · exact Nat.le_trans ih (Nat.le_succ rest.length)
      · exact Nat.succ_le_succ ih

theorem active_offer_key_mem_iff_owned
    (target : ActiveOfferKey)
    (lanes : List ActiveOfferLane) :
    target ∈ activeOfferKeys lanes ↔
      ∃ owner ∈ lanes, owner.key = target := by
  induction lanes with
  | nil => simp [activeOfferKeys]
  | cons lane rest ih =>
      simp only [activeOfferKeys]
      by_cases duplicate : lane.key ∈ activeOfferKeys rest
      · rw [if_pos duplicate]
        constructor
        · intro member
          obtain ⟨owner, inRest, exactKey⟩ := ih.mp member
          exact ⟨owner, List.mem_cons_of_mem lane inRest, exactKey⟩
        · rintro ⟨owner, inLanes, exactKey⟩
          rcases List.mem_cons.mp inLanes with rfl | inRest
          · simpa [exactKey] using duplicate
          · exact ih.mpr ⟨owner, inRest, exactKey⟩
      · rw [if_neg duplicate]
        constructor
        · intro member
          rcases List.mem_cons.mp member with head | inRest
          · exact ⟨lane, List.mem_cons_self, head.symm⟩
          · obtain ⟨owner, ownerInRest, exactKey⟩ := ih.mp inRest
            exact ⟨owner, List.mem_cons_of_mem lane ownerInRest, exactKey⟩
        · rintro ⟨owner, inLanes, exactKey⟩
          rcases List.mem_cons.mp inLanes with rfl | inRest
          · exact List.mem_cons.mpr (Or.inl exactKey.symm)
          · exact List.mem_cons.mpr (Or.inr (ih.mpr ⟨owner, inRest, exactKey⟩))

theorem production_frontier_capacity_covers_every_active_offer_key_witness
    (lanes : List ActiveOfferLane) :
    (activeOfferKeys lanes).length ≤ productionFrontierCapacity lanes.length := by
  simpa [productionFrontierCapacity] using
    active_offer_key_count_is_bounded_by_active_lane_count lanes

def firstOwningLane
    (target : ActiveOfferKey) : List ActiveOfferLane → Option ActiveOfferLane
  | [] => none
  | lane :: rest =>
      if lane.key = target then some lane else firstOwningLane target rest

theorem first_owning_lane_is_exact_owner
    {target : ActiveOfferKey}
    {lanes : List ActiveOfferLane}
    {owner : ActiveOfferLane}
    (found : firstOwningLane target lanes = some owner) :
    owner ∈ lanes ∧ owner.key = target := by
  induction lanes with
  | nil => simp [firstOwningLane] at found
  | cons head tail ih =>
      by_cases owns : head.key = target
      · simp [firstOwningLane, owns] at found
        subst owner
        exact ⟨by simp, owns⟩
      · simp [firstOwningLane, owns] at found
        have tailOwner := ih found
        exact ⟨by simp [tailOwner.1], tailOwner.2⟩

theorem active_offer_entry_has_representative_iff_owned
    (target : ActiveOfferKey)
    (lanes : List ActiveOfferLane) :
    firstOwningLane target lanes ≠ none ↔
      ∃ owner ∈ lanes, owner.key = target := by
  induction lanes with
  | nil => simp [firstOwningLane]
  | cons head tail ih =>
      by_cases owns : head.key = target
      · simp [firstOwningLane, owns]
      · simp [firstOwningLane, owns, ih]

theorem first_owning_lane_preserves_scope
    {target : ActiveOfferKey}
    {lanes : List ActiveOfferLane}
    {owner : ActiveOfferLane}
    (found : firstOwningLane target lanes = some owner) :
    owner.scope = target.scope := by
  have exactOwner := first_owning_lane_is_exact_owner found
  exact congrArg ActiveOfferKey.scope exactOwner.2

theorem offer_keys_with_same_entry_and_distinct_scope_are_distinct
    (entry leftScope rightScope : Nat)
    (scopeNe : leftScope ≠ rightScope) :
    ({ entry := entry, scope := leftScope } : ActiveOfferKey) ≠
      ({ entry := entry, scope := rightScope } : ActiveOfferKey) := by
  intro keyEq
  exact scopeNe (congrArg ActiveOfferKey.scope keyEq)

structure ActiveOfferObservation where
  key : ActiveOfferKey
  flags : Nat
  selectable : Bool
deriving DecidableEq

/-- The current offer keeps its full owner key. Scope erasure is not involved
in admission, readiness, progress, or authority for this observation. -/
def exactCurrentOfferObservations
    (target : ActiveOfferKey)
    (observations : List ActiveOfferObservation) : List ActiveOfferObservation :=
  observations.filter fun observation => decide (observation.key = target)

theorem exact_current_offer_observations_are_key_exact
    (target : ActiveOfferKey)
    (observations : List ActiveOfferObservation)
    (observation : ActiveOfferObservation) :
    observation ∈ exactCurrentOfferObservations target observations ↔
      observation ∈ observations ∧ observation.key = target := by
  simp [exactCurrentOfferObservations]

/-- A foreign scope sharing the current cursor entry cannot affect any exact
current-scope predicate, regardless of its flags or admission bit. -/
theorem exact_current_offer_observations_ignore_same_entry_foreign_scope
    (target : ActiveOfferKey)
    (foreign : ActiveOfferObservation)
    (observations : List ActiveOfferObservation)
    (_sameEntry : foreign.key.entry = target.entry)
    (scopeNe : foreign.key.scope ≠ target.scope) :
    exactCurrentOfferObservations target (foreign :: observations) =
      exactCurrentOfferObservations target observations := by
  have keyNe : foreign.key ≠ target := by
    intro keyEq
    exact scopeNe (congrArg ActiveOfferKey.scope keyEq)
  simp [exactCurrentOfferObservations, keyNe]

/-- Descriptor-owned current state may remain when there is no active exact
observation. If an exact observation is present, retention requires that one
exact witness to be admitted; an excluded witness cannot fall through to the
descriptor-only retention path. Duplicate exact owners fail closed. -/
def exactCurrentRetentionAllowed
    (target : ActiveOfferKey)
    (observations : List ActiveOfferObservation) : Bool :=
  match exactCurrentOfferObservations target observations with
  | [] => true
  | [observation] => observation.selectable
  | _ => false

theorem excluded_exact_current_offer_cannot_authorize_retention
    (target : ActiveOfferKey)
    (observation : ActiveOfferObservation)
    (observations : List ActiveOfferObservation)
    (exactRows : exactCurrentOfferObservations target observations = [observation])
    (excluded : observation.selectable = false) :
    exactCurrentRetentionAllowed target observations = false := by
  simp [exactCurrentRetentionAllowed, exactRows, excluded]

theorem exact_current_retention_ignores_same_entry_foreign_scope
    (target : ActiveOfferKey)
    (foreign : ActiveOfferObservation)
    (observations : List ActiveOfferObservation)
    (sameEntry : foreign.key.entry = target.entry)
    (scopeNe : foreign.key.scope ≠ target.scope) :
    exactCurrentRetentionAllowed target (foreign :: observations) =
      exactCurrentRetentionAllowed target observations := by
  unfold exactCurrentRetentionAllowed
  rw [exact_current_offer_observations_ignore_same_entry_foreign_scope
    target foreign observations sameEntry scopeNe]

/-- Scope-erased rows are used only to move to a different cursor entry. The
current entry is authorized exclusively by `exactCurrentOfferObservations`. -/
def alternativeCursorObservations
    (current : ActiveOfferKey)
    (observations : List ActiveOfferObservation) : List ActiveOfferObservation :=
  observations.filter fun observation =>
    decide (observation.key.entry ≠ current.entry)

theorem alternative_cursor_observations_have_distinct_entry
    (current : ActiveOfferKey)
    (observations : List ActiveOfferObservation)
    (observation : ActiveOfferObservation) :
    observation ∈ alternativeCursorObservations current observations ↔
      observation ∈ observations ∧ observation.key.entry ≠ current.entry := by
  simp [alternativeCursorObservations]

theorem same_entry_foreign_scope_is_not_alternative_cursor_candidate
    (current : ActiveOfferKey)
    (foreign : ActiveOfferObservation)
    (observations : List ActiveOfferObservation)
    (sameEntry : foreign.key.entry = current.entry) :
    foreign ∉ alternativeCursorObservations current (foreign :: observations) := by
  simp [alternativeCursorObservations, sameEntry]

/-- Scope-specific evidence remains one row per exact witness while alignment
groups rows that have the same cursor transition target. -/
def cursorTargetObservations
    (entry : Nat)
    (observations : List ActiveOfferObservation) : List ActiveOfferObservation :=
  observations.filter fun observation => decide (observation.key.entry = entry)

theorem cursor_target_observations_are_entry_exact
    (entry : Nat)
    (observations : List ActiveOfferObservation)
    (observation : ActiveOfferObservation) :
    observation ∈ cursorTargetObservations entry observations ↔
      observation ∈ observations ∧ observation.key.entry = entry := by
  simp [cursorTargetObservations]

structure ErasedCursorObservation where
  entry : Nat
  flags : Nat
  selectable : Bool
deriving DecidableEq

def ActiveOfferObservation.erase : ActiveOfferObservation → ErasedCursorObservation
  | observation =>
      { entry := observation.key.entry
        flags := observation.flags
        selectable := observation.selectable }

def erasedCursorTargetObservations
    (entry : Nat)
    (observations : List ActiveOfferObservation) : List ErasedCursorObservation :=
  (cursorTargetObservations entry observations).map ActiveOfferObservation.erase

theorem erased_cursor_target_observations_preserve_witness_count
    (entry : Nat)
    (observations : List ActiveOfferObservation) :
    (erasedCursorTargetObservations entry observations).length =
      (cursorTargetObservations entry observations).length := by
  simp [erasedCursorTargetObservations]

theorem erased_cursor_target_observation_has_exact_source
    (entry : Nat)
    (observations : List ActiveOfferObservation)
    (erased : ErasedCursorObservation) :
    erased ∈ erasedCursorTargetObservations entry observations ↔
      ∃ observation ∈ observations,
        observation.key.entry = entry ∧ observation.erase = erased := by
  simp [erasedCursorTargetObservations, cursorTargetObservations, and_assoc]

/-- Every predicate evaluated on one scope-erased row still has one exact
scope source. Group-level cursor scheduling may compare several rows, but the
result is only a realignment target and never operation authority. -/
theorem erased_cursor_target_predicate_has_exact_source
    (predicate : ErasedCursorObservation → Prop)
    (entry : Nat)
    (observations : List ActiveOfferObservation)
    (erased : ErasedCursorObservation)
    (member : erased ∈ erasedCursorTargetObservations entry observations)
    (holds : predicate erased) :
    ∃ observation ∈ observations,
      observation.key.entry = entry ∧
        observation.erase = erased ∧ predicate observation.erase := by
  obtain ⟨observation, sourceMember, entryExact, sourceExact⟩ :=
    (erased_cursor_target_observation_has_exact_source entry observations erased).mp member
  refine ⟨observation, sourceMember, entryExact, sourceExact, ?_⟩
  rwa [sourceExact]

inductive CursorAlignmentDecision where
  | keepCurrent
  | realign (entry : Nat)
deriving DecidableEq

/-- Only the exact-current branch can retain operation authority. A decision
derived from scope-erased rows can request cursor movement but cannot authorize
an endpoint operation. -/
def CursorAlignmentDecision.authorizesOperation : CursorAlignmentDecision → Bool
  | .keepCurrent => true
  | .realign _ => false

theorem scope_erased_realign_never_authorizes_operation (entry : Nat) :
    (CursorAlignmentDecision.realign entry).authorizesOperation = false := by
  rfl

/-- After any realignment, operation authority is reconstructed from exactly
one admitted observation whose complete `(scope, entry)` key matches the new
current owner. Zero, duplicate, or foreign-scope witnesses fail closed. -/
def exactCurrentOperationAuthorized
    (target : ActiveOfferKey)
    (observations : List ActiveOfferObservation) : Bool :=
  match exactCurrentOfferObservations target observations with
  | [observation] => observation.selectable
  | _ => false

theorem exact_current_operation_authority_has_one_exact_source
    (target : ActiveOfferKey)
    (observations : List ActiveOfferObservation)
    (authorized : exactCurrentOperationAuthorized target observations = true) :
    ∃ observation,
      exactCurrentOfferObservations target observations = [observation] ∧
      observation ∈ observations ∧
      observation.key = target ∧
      observation.selectable = true := by
  unfold exactCurrentOperationAuthorized at authorized
  generalize rowsEq : exactCurrentOfferObservations target observations = rows at authorized
  cases rows with
  | nil => simp at authorized
  | cons observation tail =>
      cases tail with
      | nil =>
          have exactMember : observation ∈ exactCurrentOfferObservations target observations := by
            rw [rowsEq]
            simp
          have source :=
            (exact_current_offer_observations_are_key_exact
              target observations observation).mp exactMember
          exact ⟨observation, rfl, source.1, source.2, authorized⟩
      | cons second rest => simp at authorized

/-- Production records each alignment source before moving. A repeated source
therefore denotes a deterministic cursor cycle and is rejected instead of
recursing or consuming stack. -/
def recordAlignmentSource (source : Nat) (visited : List Nat) : Option (List Nat) :=
  if source ∈ visited then none else some (source :: visited)

theorem repeated_alignment_source_is_rejected
    (source : Nat)
    (visited : List Nat)
    (seen : source ∈ visited) :
    recordAlignmentSource source visited = none := by
  simp [recordAlignmentSource, seen]

theorem fresh_alignment_source_is_recorded_exactly
    (source : Nat)
    (visited : List Nat)
    (fresh : source ∉ visited) :
    recordAlignmentSource source visited = some (source :: visited) := by
  simp [recordAlignmentSource, fresh]

theorem fresh_alignment_source_preserves_no_duplicates
    (source : Nat)
    (visited : List Nat)
    (fresh : source ∉ visited)
    (unique : visited.Nodup) :
    (source :: visited).Nodup := by
  exact List.nodup_cons.mpr ⟨fresh, unique⟩

/-- Proof-side counterpart of production's stable insertion into the erased
cursor-target observation buffer. Equal entries remain adjacent. -/
def insertErasedCursorObservation
    (incoming : ErasedCursorObservation) :
    List ErasedCursorObservation → List ErasedCursorObservation
  | [] => [incoming]
  | head :: tail =>
      if incoming.entry < head.entry then
        incoming :: head :: tail
      else
        head :: insertErasedCursorObservation incoming tail

theorem insert_erased_cursor_observation_mem_iff
    (incoming row : ErasedCursorObservation)
    (rows : List ErasedCursorObservation) :
    row ∈ insertErasedCursorObservation incoming rows ↔
      row = incoming ∨ row ∈ rows := by
  induction rows with
  | nil => simp [insertErasedCursorObservation]
  | cons head tail ih =>
      simp only [insertErasedCursorObservation]
      split <;> simp_all [eq_comm, or_left_comm]

theorem insert_erased_cursor_observation_length
    (incoming : ErasedCursorObservation)
    (rows : List ErasedCursorObservation) :
    (insertErasedCursorObservation incoming rows).length = rows.length + 1 := by
  induction rows with
  | nil => simp [insertErasedCursorObservation]
  | cons head tail ih =>
      simp only [insertErasedCursorObservation]
      split <;> simp_all

theorem insert_erased_cursor_observation_preserves_entry_order
    (incoming : ErasedCursorObservation)
    (rows : List ErasedCursorObservation)
    (ordered : rows.Pairwise fun left right => left.entry ≤ right.entry) :
    (insertErasedCursorObservation incoming rows).Pairwise
      (fun left right => left.entry ≤ right.entry) := by
  induction rows with
  | nil => simp [insertErasedCursorObservation]
  | cons head tail ih =>
      rw [insertErasedCursorObservation]
      by_cases before : incoming.entry < head.entry
      · rw [if_pos before]
        apply List.pairwise_cons.mpr
        constructor
        · intro row member
          rcases List.mem_cons.mp member with rfl | inTail
          · exact Nat.le_of_lt before
          · exact Nat.le_trans (Nat.le_of_lt before)
              ((List.pairwise_cons.mp ordered).1 row inTail)
        · exact ordered
      · rw [if_neg before]
        apply List.pairwise_cons.mpr
        constructor
        · intro row member
          rcases (insert_erased_cursor_observation_mem_iff incoming row tail).mp member with
            rfl | inTail
          · exact Nat.le_of_not_gt before
          · exact (List.pairwise_cons.mp ordered).1 row inTail
        · exact ih (List.pairwise_cons.mp ordered).2

def productionRoleEventOnlyCapacity : Nat :=
  productionDescriptorByteCapacity / productionRoleEventStride

theorem production_role_event_only_capacity_exact :
    productionRoleEventOnlyCapacity = 6553 := by
  decide

theorem descriptor_bytes_dominate_local_step_field :
    productionDescriptorByteCapacity < productionLocalStepCapacity := by
  decide

theorem role_event_bytes_dominate_local_step_field :
    productionRoleEventOnlyCapacity < productionLocalStepCapacity := by
  decide

def packedU16Absent : Nat := 65535

def encodeOptionalPackedU16Index? (row : Nat) : Option Nat :=
  if row < packedU16Absent then some row else none

theorem optional_packed_u16_index_acceptance_is_exact (row : Nat) :
    (encodeOptionalPackedU16Index? row).isSome = true <-> row < packedU16Absent := by
  simp [encodeOptionalPackedU16Index?]

def packedU32Absent : Nat := 4294967295

def packedConflictReentryWithoutParent : Nat := 49152

def decodePackedRouteAuthority?
    (packedScope resolver : Nat) : Option (Nat × RouteAuthority) :=
  if packedScope < productionRouteResolverDynamicScopeBit then
    if resolver = 0 then some (packedScope, .intrinsic) else none
  else if packedScope < productionPackedU16Capacity ∧
      resolver < productionResolverCapacity then
    some (packedScope - productionRouteResolverDynamicScopeBit, .dynamic resolver)
  else
    none

theorem packed_route_authority_dynamic_roundtrip
    (scope resolver : Nat)
    (scopeBound : scope < productionScopeCapacity)
    (resolverBound : resolver < productionResolverCapacity) :
    decodePackedRouteAuthority?
      (productionRouteResolverDynamicScopeBit + scope) resolver =
        some (scope, .dynamic resolver) := by
  dsimp [productionScopeCapacity] at scopeBound
  dsimp [productionResolverCapacity, productionPackedU16Capacity] at resolverBound
  unfold decodePackedRouteAuthority?
  dsimp [productionRouteResolverDynamicScopeBit, productionPackedU16Capacity,
    productionResolverCapacity]
  have dynamicTag : ¬32768 + scope < 32768 := by omega
  rw [if_neg dynamicTag]
  have bounds : 32768 + scope < 65536 ∧ resolver < 65536 := by
    constructor
    · omega
    · exact resolverBound
  rw [if_pos bounds]
  rw [Nat.add_sub_cancel_left]

theorem packed_route_authority_intrinsic_is_canonical
    (scope resolver : Nat)
    (scopeBound : scope < productionScopeCapacity) :
    decodePackedRouteAuthority? scope resolver = some (scope, .intrinsic) ↔
      resolver = 0 := by
  have scopeSmall : scope < 32768 := by
    dsimp [productionScopeCapacity] at scopeBound
    omega
  by_cases resolverZero : resolver = 0 <;>
    simp [decodePackedRouteAuthority?, productionRouteResolverDynamicScopeBit,
      scopeSmall, resolverZero]

structure DecodedProgramAtom where
  effIndex : Nat
  sender : Nat
  receiver : Nat
  label : Nat
  schema : Nat
  origin : Nat
  lane : Nat
  deriving Repr, DecidableEq

structure DecodedScopeMarker where
  offset : Nat
  scope : Nat
  tag : Nat
  deriving Repr, DecidableEq

structure ProgramAtomBody where
  sender : Nat
  receiver : Nat
  label : Nat
  schema : Nat
  origin : Nat
  lane : Nat
  frameLabel : Nat

structure CanonicalControlSource where
  resolvers : List DecodedRouteResolver
  markers : List DecodedScopeMarker
  scopeBudget : Nat

structure CanonicalProgramSource where
  atoms : List ProgramAtomBody
  resolvers : List DecodedRouteResolver
  markers : List DecodedScopeMarker
  scopeBudget : Nat
  laneSpan : Nat

def CompiledOccurrence.programAtomBody
    (occurrence : CompiledOccurrence) : ProgramAtomBody := {
  sender := occurrence.sender
  receiver := occurrence.receiver
  label := occurrence.label
  schema := occurrence.schema
  origin := 0
  lane := occurrence.lane
  frameLabel := occurrence.frameLabel
}

def DecodedScopeMarker.rebase
    (marker : DecodedScopeMarker)
    (atomOffset scopeOffset : Nat)
    (markRouteEnters : Bool) : DecodedScopeMarker :=
  let routeEnter := marker.scope / 8192 = 0 && marker.tag % 4 = 0
  {
    offset := marker.offset + atomOffset
    scope := marker.scope + scopeOffset
    tag := if markRouteEnters && routeEnter then marker.tag % 4 + 4 else marker.tag
  }

/-- Insert with the stable ordering used by `EffList::push_scope_marker_full_mut`:
equal-offset rows already present remain before the new row. -/
def insertScopeMarkerAfterEquals
    (marker : DecodedScopeMarker) : List DecodedScopeMarker -> List DecodedScopeMarker
  | [] => [marker]
  | head :: tail =>
      if marker.offset < head.offset then marker :: head :: tail
      else head :: insertScopeMarkerAfterEquals marker tail

/-- Insert with the outer-enter ordering used by
`EffList::push_scope_enter_marker_mut`. -/
def insertScopeMarkerBeforeEquals
    (marker : DecodedScopeMarker) : List DecodedScopeMarker -> List DecodedScopeMarker
  | [] => [marker]
  | head :: tail =>
      if marker.offset ≤ head.offset then marker :: head :: tail
      else head :: insertScopeMarkerBeforeEquals marker tail

def insertScopeMarkersAfterEquals
    (markers additions : List DecodedScopeMarker) : List DecodedScopeMarker :=
  additions.foldl (fun current marker => insertScopeMarkerAfterEquals marker current) markers

def rebaseControlSource
    (source : CanonicalControlSource)
    (atomOffset scopeOffset : Nat)
    (markRouteEnters : Bool) : CanonicalControlSource := {
  resolvers := source.resolvers.map (·.rebase scopeOffset)
  markers := source.markers.map (·.rebase atomOffset scopeOffset markRouteEnters)
  scopeBudget := source.scopeBudget
}

def canonicalControlSource : Choreo -> CanonicalControlSource
  | .send _ _ _ _ => {
      resolvers := []
      markers := []
      scopeBudget := 0
    }
  | .seq left right =>
      let leftSource := canonicalControlSource left
      let rightSource := rebaseControlSource
        (canonicalControlSource right)
        left.eventCount leftSource.scopeBudget false
      {
        resolvers := leftSource.resolvers ++ rightSource.resolvers
        markers := insertScopeMarkersAfterEquals leftSource.markers rightSource.markers
        scopeBudget := leftSource.scopeBudget + rightSource.scopeBudget
      }
  | .par left right =>
      let leftOriginal := canonicalControlSource left
      let rightOriginal := canonicalControlSource right
      let leftSource := rebaseControlSource leftOriginal 0 1 false
      let rightSource := rebaseControlSource rightOriginal
        left.eventCount (1 + leftOriginal.scopeBudget) false
      let nestedMarkers := insertScopeMarkersAfterEquals leftSource.markers rightSource.markers
      let entered := insertScopeMarkerBeforeEquals
        { offset := 0, scope := 16384, tag := 0 } nestedMarkers
      let split := insertScopeMarkerAfterEquals
        { offset := left.eventCount, scope := 16384, tag := 1 } entered
      let exited := insertScopeMarkerAfterEquals
        { offset := left.eventCount + right.eventCount,
          scope := 16384, tag := 2 } split
      {
        resolvers := leftSource.resolvers ++ rightSource.resolvers
        markers := exited
        scopeBudget := 1 + leftOriginal.scopeBudget + rightOriginal.scopeBudget
      }
  | .route authority left right =>
      let leftOriginal := canonicalControlSource left
      let rightOriginal := canonicalControlSource right
      let leftSource := rebaseControlSource leftOriginal 0 1 false
      let rightSource := rebaseControlSource rightOriginal
        left.eventCount (1 + leftOriginal.scopeBudget) false
      let enteredLeft := insertScopeMarkerBeforeEquals
        { offset := 0, scope := 0, tag := 0 } leftSource.markers
      let exitedLeft := insertScopeMarkerAfterEquals
        { offset := left.eventCount, scope := 0, tag := 2 } enteredLeft
      let enteredRight := insertScopeMarkerAfterEquals
        { offset := left.eventCount, scope := 0, tag := 0 } exitedLeft
      let nestedMarkers := insertScopeMarkersAfterEquals enteredRight rightSource.markers
      let exitedRight := insertScopeMarkerAfterEquals
        { offset := left.eventCount + right.eventCount,
          scope := 0, tag := 2 } nestedMarkers
      let controller := uniqueController
        ((left.firstVisibleSenders ++ right.firstVisibleSenders).eraseDups)
      {
        resolvers := {
          scope := 0
          authority
          controller
          leftParticipants := canonicalParticipants left.participants
          rightParticipants := canonicalParticipants right.participants
        } ::
          (leftSource.resolvers ++ rightSource.resolvers)
        markers := exitedRight
        scopeBudget := 1 + leftOriginal.scopeBudget + rightOriginal.scopeBudget
      }
  | .roll body =>
      let original := canonicalControlSource body
      let source := rebaseControlSource original 0 1 true
      let entered := insertScopeMarkerBeforeEquals
        { offset := 0, scope := 8192, tag := 0 } source.markers
      let exited := insertScopeMarkerAfterEquals
        { offset := body.eventCount, scope := 8192, tag := 2 } entered
      {
        source with
        markers := exited
        scopeBudget := 1 + original.scopeBudget
      }

def canonicalProgramSource (choreo : Choreo) : CanonicalProgramSource :=
  let control := canonicalControlSource choreo
  let compiled := choreo.compiledOccurrences
  {
    atoms := compiled.occurrences.map CompiledOccurrence.programAtomBody
    resolvers := control.resolvers
    markers := control.markers
    scopeBudget := control.scopeBudget
    laneSpan := compiled.laneSpan
  }

def enumerateProgramAtoms : Nat -> List ProgramAtomBody -> List DecodedProgramAtom
  | _, [] => []
  | index, atom :: rest =>
      {
        effIndex := index
        sender := atom.sender
        receiver := atom.receiver
        label := atom.label
        schema := atom.schema
        origin := atom.origin
        lane := atom.lane
      } :: enumerateProgramAtoms (index + 1) rest

def Choreo.canonicalProgramAtoms (choreo : Choreo) : List DecodedProgramAtom :=
  enumerateProgramAtoms 0 (canonicalProgramSource choreo).atoms

private theorem enumerate_program_atoms_eff_index_lower_bound
    (index : Nat) (atoms : List ProgramAtomBody) (atom : DecodedProgramAtom)
    (member : atom ∈ enumerateProgramAtoms index atoms) :
    index ≤ atom.effIndex := by
  induction atoms generalizing index with
  | nil => simp [enumerateProgramAtoms] at member
  | cons head tail tailIH =>
      simp only [enumerateProgramAtoms, List.mem_cons] at member
      rcases member with rfl | member
      · simp
      · have lower := tailIH (index := index + 1) member
        omega

private theorem enumerate_program_atoms_eff_indices_pairwise
    (index : Nat) (atoms : List ProgramAtomBody) :
    (enumerateProgramAtoms index atoms).Pairwise
      (fun left right => left.effIndex < right.effIndex) := by
  induction atoms generalizing index with
  | nil => simp [enumerateProgramAtoms]
  | cons head tail tailIH =>
      rw [enumerateProgramAtoms]
      apply List.pairwise_cons.mpr
      constructor
      · intro atom member
        have lower := enumerate_program_atoms_eff_index_lower_bound
          (index + 1) tail atom member
        change index < atom.effIndex
        omega
      · exact tailIH (index + 1)

/-- Canonical atom rows are strictly ordered by compact event identity, which
justifies the erased runtime's logarithmic descriptor lookup without an index
column. -/
theorem canonical_program_atom_eff_indices_strict (choreo : Choreo) :
    choreo.canonicalProgramAtoms.Pairwise
      (fun left right => left.effIndex < right.effIndex) := by
  simpa [Choreo.canonicalProgramAtoms] using
    enumerate_program_atoms_eff_indices_pairwise 0
      (canonicalProgramSource choreo).atoms

def Choreo.canonicalRouteResolvers (choreo : Choreo) : List DecodedRouteResolver :=
  (canonicalProgramSource choreo).resolvers

def Choreo.canonicalScopeMarkers (choreo : Choreo) : List DecodedScopeMarker :=
  (canonicalProgramSource choreo).markers

/-- Canonical resident column counts. Offsets are derived in production order,
so a certificate cannot choose an alternate byte interpretation. -/
structure RustDescriptorImage where
  roleCount : Nat
  role : Nat
  logicalLaneCount : Nat
  activeLaneCount : Nat
  endpointLaneSlotCount : Nat
  maxRouteCommitCount : Nat
  firstActiveLane : Nat
  activeLaneStart : Nat
  activeLaneLength : Nat
  atomCount : Nat
  routeResolverCount : Nat
  routeParticipantCount : Nat
  scopeMarkerCount : Nat
  eventCount : Nat
  dependencyRowCount : Nat
  conflictRowCount : Nat
  routeScopeCount : Nat
  residentBoundaryCount : Nat
  laneBitCount : Nat
  routeArmLaneStepCount : Nat
  routeCommitRowCount : Nat
  rollScopeCount : Nat
  programBytes : List Nat
  roleBytes : List Nat
  deriving Repr, DecidableEq

def readByte? (bytes : List Nat) (offset : Nat) : Option Nat :=
  match bytes[offset]? with
  | some byte => if byte < 256 then some byte else none
  | none => none

def readU16LE? (bytes : List Nat) (offset : Nat) : Option Nat := do
  let low ← readByte? bytes offset
  let high ← readByte? bytes (offset + 1)
  pure (low + high * 256)

def readU32LE? (bytes : List Nat) (offset : Nat) : Option Nat := do
  let low ← readU16LE? bytes offset
  let high ← readU16LE? bytes (offset + 2)
  pure (low + high * 65536)

def readByteRange?
    (bytes : List Nat) (offset start finish : Nat) : Option (List Nat) := do
  if start ≤ finish then pure () else none
  (List.range (finish - start)).mapM fun index =>
    readByte? bytes (offset + start + index)

def RustDescriptorImage.programRouteResolverOffset (image : RustDescriptorImage) : Nat :=
  image.atomCount * productionProgramAtomStride

def RustDescriptorImage.programRouteParticipantOffset
    (image : RustDescriptorImage) : Nat :=
  image.programRouteResolverOffset +
    image.routeResolverCount * productionProgramRouteResolverStride

def RustDescriptorImage.programScopeMarkerOffset (image : RustDescriptorImage) : Nat :=
  image.programRouteParticipantOffset +
    image.routeParticipantCount * productionProgramRouteParticipantStride

def RustDescriptorImage.programBlobLen (image : RustDescriptorImage) : Nat :=
  image.programScopeMarkerOffset +
    image.scopeMarkerCount * productionProgramScopeMarkerStride

def RustDescriptorImage.roleLaneOffset (image : RustDescriptorImage) : Nat :=
  image.eventCount * productionRoleEventStride

def RustDescriptorImage.roleDependencyOffset (image : RustDescriptorImage) : Nat :=
  image.roleLaneOffset + image.eventCount * productionRoleLaneStride

def RustDescriptorImage.roleConflictOffset (image : RustDescriptorImage) : Nat :=
  image.roleDependencyOffset +
    image.dependencyRowCount * productionRoleDependencyStride

def RustDescriptorImage.roleRouteScopeOffset (image : RustDescriptorImage) : Nat :=
  image.roleConflictOffset + image.conflictRowCount * productionRoleConflictStride

def RustDescriptorImage.roleRouteScopeConflictOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleRouteScopeOffset + image.routeScopeCount * productionRoleRouteScopeStride

def RustDescriptorImage.roleRouteArmOffset (image : RustDescriptorImage) : Nat :=
  image.roleRouteScopeConflictOffset +
    image.routeScopeCount * productionRoleConflictStride

def RustDescriptorImage.roleResidentBoundaryOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleRouteArmOffset +
    image.routeScopeCount * 2 * productionRoleRouteArmStride

def RustDescriptorImage.roleLaneBitOffset (image : RustDescriptorImage) : Nat :=
  image.roleResidentBoundaryOffset +
    image.residentBoundaryCount * productionRoleU16Stride

def RustDescriptorImage.roleRouteArmLaneRowOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleLaneBitOffset + image.laneBitCount

def RustDescriptorImage.roleRouteOfferLaneRowOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleRouteArmLaneRowOffset +
    image.routeScopeCount * 2 * productionRoleLaneRangeStride

def RustDescriptorImage.roleRouteArmLaneStepOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleRouteOfferLaneRowOffset +
    image.routeScopeCount * productionRoleLaneRangeStride

def RustDescriptorImage.roleRouteCommitRangeOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleRouteArmLaneStepOffset +
    image.routeArmLaneStepCount * productionRoleRouteArmLaneStepStride

def RustDescriptorImage.roleRouteCommitRowOffset
    (image : RustDescriptorImage) : Nat :=
  image.roleRouteCommitRangeOffset +
    image.routeScopeCount * 2 * productionRoleLaneRangeStride

def RustDescriptorImage.roleRollScopeOffset (image : RustDescriptorImage) : Nat :=
  image.roleRouteCommitRowOffset +
    image.routeCommitRowCount * productionRoleConflictStride

def RustDescriptorImage.roleBlobLen (image : RustDescriptorImage) : Nat :=
  image.roleRollScopeOffset + image.rollScopeCount * productionRoleRollScopeStride

def RustDescriptorImage.decodeAtomRow?
    (image : RustDescriptorImage) (row : Nat) : Option DecodedProgramAtom := do
  if row < image.atomCount then pure () else none
  let offset := row * productionProgramAtomStride
  let effIndex ← readU16LE? image.programBytes offset
  let sender ← readByte? image.programBytes (offset + 2)
  let receiver ← readByte? image.programBytes (offset + 3)
  let label ← readByte? image.programBytes (offset + 4)
  let schema ← readU32LE? image.programBytes (offset + 5)
  let origin ← readByte? image.programBytes (offset + 9)
  let lane ← readByte? image.programBytes (offset + 10)
  if effIndex < productionEventIdentityCapacity ∧
      0 < image.roleCount ∧
      sender < image.roleCount ∧
      receiver < image.roleCount ∧
      origin < 2 then
    pure { effIndex, sender, receiver, label, schema, origin, lane }
  else
    none

def RustDescriptorImage.decodeAtoms?
    (image : RustDescriptorImage) : Option (List DecodedProgramAtom) :=
  (List.range image.atomCount).mapM image.decodeAtomRow?

def DecodedProgramAtom.globalEvent
    (atom : DecodedProgramAtom) : GlobalEvent := {
  sender := atom.sender
  receiver := atom.receiver
  label := atom.label
  schema := atom.schema
  lane := atom.lane
}

def ProgramAtomBody.globalEventFrom
    (atom : ProgramAtomBody) (laneBase : Nat) : GlobalEvent := {
  sender := atom.sender
  receiver := atom.receiver
  label := atom.label
  schema := atom.schema
  lane := atom.lane + laneBase
}

theorem canonical_program_source_lane_span (choreo : Choreo) :
    (canonicalProgramSource choreo).laneSpan = choreo.laneSpan := rfl

theorem canonical_program_source_frame_labels (choreo : Choreo) :
    (canonicalProgramSource choreo).atoms.map ProgramAtomBody.frameLabel =
      choreo.compiledOccurrences.occurrences.map CompiledOccurrence.frameLabel := by
  simp [canonicalProgramSource, CompiledOccurrence.programAtomBody]

theorem canonical_program_source_global_events_from
    (choreo : Choreo) (laneBase : Nat) :
    (canonicalProgramSource choreo).atoms.map (·.globalEventFrom laneBase) =
      choreo.globalEventsFrom laneBase := by
  simp [canonicalProgramSource, Choreo.globalEventsFrom,
    CompiledOccurrence.programAtomBody, ProgramAtomBody.globalEventFrom,
    CompiledOccurrence.globalEventFrom, List.map_map, Nat.add_comm]

theorem enumerate_program_atoms_global_events
    (index : Nat) (atoms : List ProgramAtomBody) :
    (enumerateProgramAtoms index atoms).map DecodedProgramAtom.globalEvent =
      atoms.map (·.globalEventFrom 0) := by
  induction atoms generalizing index with
  | nil => rfl
  | cons head tail tailIH =>
      simp [enumerateProgramAtoms, DecodedProgramAtom.globalEvent,
        ProgramAtomBody.globalEventFrom, tailIH]

/-- The host-side canonical compiler assigns exactly the physical lanes used
by the global asynchronous semantics for every normalized choreography. -/
theorem canonical_program_atoms_global_events (choreo : Choreo) :
    choreo.canonicalProgramAtoms.map DecodedProgramAtom.globalEvent =
      choreo.globalEvents := by
  rw [Choreo.canonicalProgramAtoms, enumerate_program_atoms_global_events]
  simpa [Choreo.globalEvents] using
    canonical_program_source_global_events_from choreo 0

def RustDescriptorImage.decodeGlobalEvents?
    (image : RustDescriptorImage) : Option (List GlobalEvent) :=
  image.decodeAtoms?.map fun atoms => atoms.map DecodedProgramAtom.globalEvent

def DecodedProgramAtom.localAction?
    (atom : DecodedProgramAtom) (role : Nat) : Option LocalAction :=
  Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema

def Choreo.canonicalRoleEffIndices (choreo : Choreo) (role : Nat) : List Nat :=
  choreo.canonicalProgramAtoms.filterMap fun atom =>
    if (atom.localAction? role).isSome then some atom.effIndex else none

def Choreo.canonicalFrameLabel (choreo : Choreo) (index : Nat) : Nat :=
  match (canonicalProgramSource choreo).atoms[index]? with
  | some atom => atom.frameLabel
  | none => 256

def Choreo.canonicalRoleFrameLabels (choreo : Choreo) (role : Nat) : List Nat :=
  (canonicalProgramSource choreo).atoms.filterMap fun atom =>
    if (Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema).isSome then
      some atom.frameLabel
    else none

structure ScopeSelection where
  scope : Nat
  start : Nat
  stop : Nat

def scopeSegmentEnd
    (markers : List DecodedScopeMarker)
    (index limit : Nat)
    (marker : DecodedScopeMarker) : Nat :=
  match (markers.drop (index + 1)).find? fun candidate =>
      candidate.scope = marker.scope && candidate.tag % 4 = 2 with
  | some exit => exit.offset
  | none => limit

def selectInnermostScope
    (current : Option ScopeSelection)
    (scope start stop : Nat) : Option ScopeSelection :=
  match current with
  | none => some { scope, start, stop }
  | some selected =>
      if selected.start < start then
        some { scope, start, stop }
      else if selected.start = start && stop < selected.stop then
        some { scope, start, stop }
      else if selected.start = start && stop = selected.stop &&
          selected.scope % 8192 < scope % 8192 then
        some { scope, start, stop }
      else
        current

theorem select_innermost_scope_equal_range_uses_source_preorder
    (outer inner start stop : Nat)
    (preorder : outer % 8192 < inner % 8192) :
    selectInnermostScope (some { scope := outer, start, stop }) inner start stop =
      some { scope := inner, start, stop } := by
  simp [selectInnermostScope, preorder]

def canonicalScopeAt
    (markers : List DecodedScopeMarker) (atomCount effIndex : Nat) : Nat :=
  let selected := (List.range markers.length).foldl (fun current index =>
    match markers[index]? with
    | none => current
    | some marker =>
        if marker.offset ≤ effIndex && marker.tag % 4 = 0 then
          let stop := scopeSegmentEnd markers index atomCount marker
          if effIndex < stop then
            selectInnermostScope current marker.scope marker.offset stop
          else
            current
        else
          current) none
  match selected with
  | none => packedU16Absent
  | some scope => scope.scope

def Choreo.canonicalRoleEventScopes (choreo : Choreo) (role : Nat) : List Nat :=
  let atoms := choreo.canonicalProgramAtoms
  let markers := choreo.canonicalScopeMarkers
  atoms.filterMap fun atom =>
    if (atom.localAction? role).isSome then
      some (canonicalScopeAt markers atoms.length atom.effIndex)
    else
      none

/-- Equal projected event ranges do not collapse nested roll identity: source
preorder selects the innermost of three erased iteration scopes. -/
theorem canonical_role_event_scope_for_triple_roll_is_innermost :
    Choreo.canonicalRoleEventScopes
        (.roll (.roll (.roll (.send 0 1 203 1)))) 0 = [8194] := by
  decide

structure RouteArmSelection where
  scope : Nat
  arm : Nat
  start : Nat
  stop : Nat

def routeArmRanges?
    (markers : List DecodedScopeMarker) (scope : Nat) :
    Option ((Nat × Nat) × (Nat × Nat)) := do
  let starts := markers.filterMap fun marker =>
    if marker.scope = scope && marker.tag % 4 = 0 then some marker.offset else none
  let stops := markers.filterMap fun marker =>
    if marker.scope = scope && marker.tag % 4 = 2 then some marker.offset else none
  match starts, stops with
  | [leftStart, rightStart], [leftStop, rightStop] =>
      some ((leftStart, leftStop), (rightStart, rightStop))
  | _, _ => none

def containingRouteArm?
    (markers : List DecodedScopeMarker) (scope effIndex : Nat) :
    Option RouteArmSelection := do
  let (left, right) ← routeArmRanges? markers scope
  if left.1 ≤ effIndex && effIndex < left.2 then
    some { scope, arm := 0, start := left.1, stop := left.2 }
  else if right.1 ≤ effIndex && effIndex < right.2 then
    some { scope, arm := 1, start := right.1, stop := right.2 }
  else
    none

def selectInnermostRouteArm
    (current : Option RouteArmSelection)
    (candidate : RouteArmSelection) : Option RouteArmSelection :=
  match current with
  | none => some candidate
  | some selected =>
      let candidateSpan := candidate.stop - candidate.start
      let selectedSpan := selected.stop - selected.start
      if candidateSpan < selectedSpan ||
          (candidateSpan = selectedSpan && selected.start < candidate.start) ||
          (candidate.start = selected.start && candidate.stop = selected.stop &&
            selected.scope % 8192 < candidate.scope % 8192) then
        some candidate
      else
        current

theorem select_innermost_route_arm_equal_range_uses_source_preorder
    (outer inner arm start stop : Nat)
    (preorder : outer % 8192 < inner % 8192) :
    selectInnermostRouteArm
        (some { scope := outer, arm, start, stop })
        { scope := inner, arm, start, stop } =
      some { scope := inner, arm, start, stop } := by
  simp [selectInnermostRouteArm, preorder]

def canonicalRouteArmAt?
    (source : CanonicalProgramSource) (effIndex : Nat) : Option RouteArmSelection :=
  source.resolvers.foldl (fun selected resolver =>
    match containingRouteArm? source.markers resolver.scope effIndex with
    | none => selected
    | some candidate => selectInnermostRouteArm selected candidate) none

def canonicalChoiceFlag
    (source : CanonicalProgramSource) (role : Nat) (atom : DecodedProgramAtom) : Nat :=
  match canonicalRouteArmAt? source atom.effIndex with
  | none => 0
  | some arm =>
      let firstRecv := (List.range (arm.stop - arm.start)).findSome? fun offset =>
        let effIndex := arm.start + offset
        match source.atoms[effIndex]? with
        | some candidate =>
            if candidate.receiver = role && candidate.sender ≠ role then some effIndex else none
        | none => none
      if firstRecv = some atom.effIndex then 1 else 0

def Choreo.canonicalRoleEventFlags (choreo : Choreo) (role : Nat) : List Nat :=
  let source := canonicalProgramSource choreo
  let atoms := enumerateProgramAtoms 0 source.atoms
  atoms.filterMap fun atom =>
    if (atom.localAction? role).isSome then
      some (canonicalChoiceFlag source role atom)
    else
      none

def canonicalPackedEventConflict?
    (source : CanonicalProgramSource) (atom : DecodedProgramAtom) : Option Nat := do
  let arm ← canonicalRouteArmAt? source atom.effIndex
  pure (arm.scope * 2 + arm.arm)

def Choreo.canonicalRoleEventConflicts
    (choreo : Choreo) (role : Nat) : List (Option Nat) :=
  let source := canonicalProgramSource choreo
  let atoms := enumerateProgramAtoms 0 source.atoms
  atoms.filterMap fun atom =>
    if (atom.localAction? role).isSome then
      some (canonicalPackedEventConflict? source atom)
    else
      none

def compactConflictReferences {α : Type} : Nat -> List (Option α) -> List Nat
  | _, [] => []
  | next, none :: rest =>
      packedU16Absent :: compactConflictReferences next rest
  | next, some _ :: rest =>
      next :: compactConflictReferences (next + 1) rest

def Choreo.canonicalRoleEventConflictRows
    (choreo : Choreo) (role : Nat) : List Nat :=
  compactConflictReferences 0 (choreo.canonicalRoleEventConflicts role)

def Choreo.canonicalRoleConflicts
    (choreo : Choreo) (role : Nat) : List Nat :=
  (choreo.canonicalRoleEventConflicts role).filterMap id

def routeBounds?
    (markers : List DecodedScopeMarker) (scope : Nat) : Option (Nat × Nat) := do
  let (left, right) ← routeArmRanges? markers scope
  pure (min left.1 right.1, max left.2 right.2)

def canonicalEnclosingRouteArm?
    (source : CanonicalProgramSource)
    (scope : Nat)
    (target : Nat × Nat) : Option RouteArmSelection :=
  source.resolvers.foldl (fun selected resolver =>
    if resolver.scope = scope then selected
    else
      match routeBounds? source.markers resolver.scope,
          containingRouteArm? source.markers resolver.scope target.1 with
      | some parentBounds, some candidate =>
          if parentBounds.1 ≤ target.1 && target.2 ≤ parentBounds.2 then
            selectInnermostRouteArm selected candidate
          else
            selected
      | _, _ => selected) none

def canonicalParentRouteArm?
    (source : CanonicalProgramSource) (scope : Nat) : Option RouteArmSelection := do
  let target ← routeBounds? source.markers scope
  canonicalEnclosingRouteArm? source scope target

def canonicalRouteScopeConflict
    (source : CanonicalProgramSource) (scope : Nat) : Nat :=
  let parent := canonicalParentRouteArm? source scope
  let reentrant := source.markers.any fun marker =>
    marker.scope = scope && marker.tag % 4 = 0 && 4 ≤ marker.tag
  match parent, reentrant with
  | none, false => packedU16Absent
  | none, true => packedConflictReentryWithoutParent
  | some arm, false => arm.scope * 2 + arm.arm
  | some arm, true => arm.scope * 2 + arm.arm + 16384

def Choreo.canonicalRouteScopes (choreo : Choreo) : List Nat :=
  (canonicalProgramSource choreo).resolvers.map DecodedRouteResolver.scope

def Choreo.canonicalRouteScopeConflicts (choreo : Choreo) : List Nat :=
  let source := canonicalProgramSource choreo
  source.resolvers.map fun resolver =>
    canonicalRouteScopeConflict source resolver.scope

def RustDescriptorImage.eventEffIndex?
    (image : RustDescriptorImage) (eventId : Nat) : Option Nat := do
  if eventId < image.eventCount then pure () else none
  readU16LE? image.roleBytes (eventId * productionRoleEventStride)

def RustDescriptorImage.eventLane?
    (image : RustDescriptorImage) (eventId : Nat) : Option Nat := do
  if eventId < image.eventCount then pure () else none
  readByte? image.roleBytes (image.roleLaneOffset + eventId)

def RustDescriptorImage.eventFrameLabel?
    (image : RustDescriptorImage) (eventId : Nat) : Option Nat := do
  if eventId < image.eventCount then pure () else none
  readByte? image.roleBytes (eventId * productionRoleEventStride + 8)

def RustDescriptorImage.eventScope?
    (image : RustDescriptorImage) (eventId : Nat) : Option Nat := do
  if eventId < image.eventCount then pure () else none
  readU16LE? image.roleBytes (eventId * productionRoleEventStride + 6)

def RustDescriptorImage.eventFlags?
    (image : RustDescriptorImage) (eventId : Nat) : Option Nat := do
  if eventId < image.eventCount then pure () else none
  readByte? image.roleBytes (eventId * productionRoleEventStride + 9)

def RustDescriptorImage.eventConflictRow?
    (image : RustDescriptorImage) (eventId : Nat) : Option Nat := do
  if eventId < image.eventCount then pure () else none
  readU16LE? image.roleBytes (eventId * productionRoleEventStride + 4)

def RustDescriptorImage.decodeEventEffIndices?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.eventCount).mapM image.eventEffIndex?

def RustDescriptorImage.decodeEventFrameLabels?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.eventCount).mapM image.eventFrameLabel?

def RustDescriptorImage.decodeEventScopes?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.eventCount).mapM image.eventScope?

def RustDescriptorImage.decodeEventFlags?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.eventCount).mapM image.eventFlags?

def RustDescriptorImage.decodeEventConflictRows?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.eventCount).mapM image.eventConflictRow?

def RustDescriptorImage.decodeConflictRows?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.conflictRowCount).mapM fun row =>
    readU16LE? image.roleBytes
      (image.roleConflictOffset + row * productionRoleConflictStride)

def RustDescriptorImage.decodeRouteScopeRows?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.routeScopeCount).mapM fun row =>
    readU16LE? image.roleBytes
      (image.roleRouteScopeOffset + row * productionRoleRouteScopeStride)

def RustDescriptorImage.decodeRouteScopeConflictRows?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.routeScopeCount).mapM fun row =>
    readU16LE? image.roleBytes
      (image.roleRouteScopeConflictOffset + row * productionRoleConflictStride)

def RustDescriptorImage.eventAtom?
    (image : RustDescriptorImage) (eventId : Nat) : Option DecodedProgramAtom := do
  let effIndex ← image.eventEffIndex? eventId
  let atoms ← image.decodeAtoms?
  resolveUnique? (atoms.filter fun candidate => candidate.effIndex = effIndex)

theorem decoded_event_atom_is_unique
    {image : RustDescriptorImage} {eventId : Nat} {atom : DecodedProgramAtom}
    (decoded : image.eventAtom? eventId = some atom) :
    ∃ effIndex atoms,
      image.eventEffIndex? eventId = some effIndex ∧
      image.decodeAtoms? = some atoms ∧
      atoms.filter (fun candidate => candidate.effIndex = effIndex) = [atom] := by
  unfold RustDescriptorImage.eventAtom? at decoded
  cases effIndexCase : image.eventEffIndex? eventId with
  | none => simp [effIndexCase] at decoded
  | some effIndex =>
      cases atomsCase : image.decodeAtoms? with
      | none => simp [effIndexCase, atomsCase] at decoded
      | some atoms =>
          refine ⟨effIndex, atoms, rfl, rfl, ?_⟩
          exact resolveUnique?_eq_some_iff.mp
            (by simpa [effIndexCase, atomsCase] using decoded)

def RustDescriptorImage.eventAction?
    (image : RustDescriptorImage) (eventId : Nat) : Option LocalAction := do
  if image.role < image.roleCount then pure () else none
  let atom ← image.eventAtom? eventId
  let lane ← image.eventLane? eventId
  if lane = atom.lane then atom.localAction? image.role else none

/-- Complete resident identity of one descriptor operation. `eventId` is the
local occurrence selected by the cursor; `lane` and `frameLabel` are decoded
from the exact role image rather than recomputed by the monitor. -/
structure DescriptorOperation where
  eventId : Nat
  lane : Nat
  frameLabel : Nat
  action : LocalAction
  deriving Repr, DecidableEq

def DescriptorOperation.request (operation : DescriptorOperation) : OperationRequest := {
  eventId := operation.eventId
  action := operation.action
}

def RustDescriptorImage.eventOperation?
    (image : RustDescriptorImage) (eventId : Nat) : Option DescriptorOperation := do
  let action ← image.eventAction? eventId
  let lane ← image.eventLane? eventId
  let frameLabel ← image.eventFrameLabel? eventId
  pure { eventId, lane, frameLabel, action }

theorem decoded_event_operation_binds_exact_columns
    {image : RustDescriptorImage} {eventId : Nat}
    {operation : DescriptorOperation}
    (decoded : image.eventOperation? eventId = some operation) :
    operation.eventId = eventId ∧
      image.eventAction? eventId = some operation.action ∧
      image.eventLane? eventId = some operation.lane ∧
      image.eventFrameLabel? eventId = some operation.frameLabel := by
  unfold RustDescriptorImage.eventOperation? at decoded
  cases actionCase : image.eventAction? eventId with
  | none => simp [actionCase] at decoded
  | some action =>
      cases laneCase : image.eventLane? eventId with
      | none => simp [actionCase, laneCase] at decoded
      | some lane =>
          cases labelCase : image.eventFrameLabel? eventId with
          | none => simp [actionCase, laneCase, labelCase] at decoded
          | some frameLabel =>
              have exact :
                  ({ eventId, lane, frameLabel, action } : DescriptorOperation) = operation :=
                Option.some.inj (by simpa [actionCase, laneCase, labelCase] using decoded)
              rw [← exact]
              simp

def RustDescriptorImage.decodeOperations?
    (image : RustDescriptorImage) : Option (List DescriptorOperation) :=
  (List.range image.eventCount).mapM image.eventOperation?

private theorem event_operation_mapM_member_binds_exact_event
    {image : RustDescriptorImage}
    {indices : List Nat} {operations : List DescriptorOperation}
    {operation : DescriptorOperation}
    (decoded : indices.mapM image.eventOperation? = some operations)
    (member : operation ∈ operations) :
    image.eventOperation? operation.eventId = some operation := by
  induction indices generalizing operations with
  | nil => simp at decoded; subst operations; simp at member
  | cons index rest ih =>
      cases headCase : image.eventOperation? index with
      | none => simp [headCase] at decoded
      | some head =>
          cases tailCase : rest.mapM image.eventOperation? with
          | none => simp [headCase, tailCase] at decoded
          | some tail =>
              have exact : operations = head :: tail := by
                exact (Option.some.inj (by simpa [headCase, tailCase] using decoded)).symm
              rw [exact] at member
              simp only [List.mem_cons] at member
              cases member with
              | inl same =>
                  rw [same]
                  have eventIdExact :=
                    (decoded_event_operation_binds_exact_columns headCase).1
                  simpa [eventIdExact] using headCase
              | inr tailMember => exact ih tailCase tailMember

theorem decoded_operations_member_binds_exact_event_columns
    {image : RustDescriptorImage} {operations : List DescriptorOperation}
    {operation : DescriptorOperation}
    (decoded : image.decodeOperations? = some operations)
    (member : operation ∈ operations) :
    image.eventOperation? operation.eventId = some operation := by
  unfold RustDescriptorImage.decodeOperations? at decoded
  exact event_operation_mapM_member_binds_exact_event decoded member

/-- Carrier-visible identity of an inbound operation. Session and target role
are fixed by the attached endpoint and therefore do not duplicate descriptor
state here. -/
structure InboundOperationKey where
  source : Nat
  lane : Nat
  frameLabel : Nat
  deriving Repr, DecidableEq

def DescriptorOperation.matchesInbound
    (operation : DescriptorOperation) (key : InboundOperationKey) : Bool :=
  match operation.action with
  | .recv peer _ _ =>
      operation.lane == key.lane && operation.frameLabel == key.frameLabel && peer == key.source
  | .send _ _ _ | .local _ _ => false

def DescriptorOperation.enabledCandidate
    (operation : DescriptorOperation) (graph : EventGraph) (state : CommitState) : Bool :=
  match graph.events[operation.eventId]? with
  | none => false
  | some event =>
      decide (event.operationRequest = operation.request) && eventEnabled graph state event

def RustDescriptorImage.matchingInboundOperations?
    (image : RustDescriptorImage) (graph : EventGraph) (state : CommitState)
    (key : InboundOperationKey) :
    Option (List DescriptorOperation) :=
  match image.decodeOperations? with
  | none => none
  | some operations => some (operations.filter fun operation =>
      operation.matchesInbound key && operation.enabledCandidate graph state)

private def resolveUniqueDescriptorOperation?
    (operations : Option (List DescriptorOperation)) : Option DescriptorOperation :=
  match operations with
  | none => none
  | some candidates => resolveUnique? candidates

/-- Inbound evidence is admitted only when the complete source/lane/frame-label
key identifies exactly one resident operation. Missing and ambiguous evidence
both fail closed. -/
def RustDescriptorImage.resolveInboundOperation?
    (image : RustDescriptorImage) (graph : EventGraph) (state : CommitState)
    (key : InboundOperationKey) : Option DescriptorOperation :=
  resolveUniqueDescriptorOperation? (image.matchingInboundOperations? graph state key)

private theorem resolved_unique_descriptor_operation_is_singleton
    {operations : Option (List DescriptorOperation)}
    {operation : DescriptorOperation}
    (resolved : resolveUniqueDescriptorOperation? operations = some operation) :
    operations = some [operation] := by
  unfold resolveUniqueDescriptorOperation? at resolved
  cases matching : operations with
  | none => simp [matching] at resolved
  | some candidates =>
      have singleton : candidates = [operation] :=
        resolveUnique?_eq_some_iff.mp (by simpa [matching] using resolved)
      simp [singleton]

theorem resolved_inbound_operation_is_unique
    {image : RustDescriptorImage} {graph : EventGraph} {state : CommitState}
    {key : InboundOperationKey}
    {operation : DescriptorOperation}
    (resolved : image.resolveInboundOperation? graph state key = some operation) :
    image.matchingInboundOperations? graph state key = some [operation] := by
  unfold RustDescriptorImage.resolveInboundOperation? at resolved
  exact resolved_unique_descriptor_operation_is_singleton resolved

theorem resolved_inbound_operation_matches_complete_enabled_key
    {image : RustDescriptorImage} {graph : EventGraph} {state : CommitState}
    {key : InboundOperationKey}
    {operation : DescriptorOperation}
    (resolved : image.resolveInboundOperation? graph state key = some operation) :
    operation.matchesInbound key = true ∧
      operation.enabledCandidate graph state = true := by
  have unique := resolved_inbound_operation_is_unique resolved
  unfold RustDescriptorImage.matchingInboundOperations? at unique
  cases decoded : image.decodeOperations? with
  | none => simp [decoded] at unique
  | some operations =>
      have filtered :
          operations.filter (fun candidate =>
            candidate.matchesInbound key && candidate.enabledCandidate graph state) =
            [operation] := by
        exact Option.some.inj (by simpa [decoded] using unique)
      have member : operation ∈ operations.filter (fun candidate =>
          candidate.matchesInbound key && candidate.enabledCandidate graph state) := by
        rw [filtered]
        simp
      exact Bool.and_eq_true_iff.mp (List.mem_filter.mp member).2

theorem resolved_inbound_operation_binds_exact_event_columns
    {image : RustDescriptorImage} {graph : EventGraph} {state : CommitState}
    {key : InboundOperationKey}
    {operation : DescriptorOperation}
    (resolved : image.resolveInboundOperation? graph state key = some operation) :
    image.eventOperation? operation.eventId = some operation := by
  have unique := resolved_inbound_operation_is_unique resolved
  unfold RustDescriptorImage.matchingInboundOperations? at unique
  cases decoded : image.decodeOperations? with
  | none => simp [decoded] at unique
  | some operations =>
      have filtered :
          operations.filter (fun candidate =>
            candidate.matchesInbound key && candidate.enabledCandidate graph state) =
            [operation] := by
        exact Option.some.inj (by simpa [decoded] using unique)
      have member : operation ∈ operations.filter (fun candidate =>
          candidate.matchesInbound key && candidate.enabledCandidate graph state) := by
        rw [filtered]
        simp
      exact decoded_operations_member_binds_exact_event_columns decoded
        (List.mem_filter.mp member).1

/-- Headerless deterministic ingress contributes no peer or frame-label
observation. The endpoint call supplies logical label and schema, while its
lane-bound Rx handle supplies the lane. Such evidence is admissible only when
those facts identify one enabled resident receive operation. -/
structure DeterministicInboundKey where
  lane : Nat
  label : Nat
  schema : Nat
  deriving Repr, DecidableEq

def DescriptorOperation.matchesDeterministicInbound
    (operation : DescriptorOperation) (key : DeterministicInboundKey) : Bool :=
  match operation.action with
  | .recv _ label schema =>
      operation.lane == key.lane && label == key.label && schema == key.schema
  | .send _ _ _ | .local _ _ => false

def RustDescriptorImage.matchingDeterministicInboundOperations?
    (image : RustDescriptorImage) (graph : EventGraph) (state : CommitState)
    (key : DeterministicInboundKey) : Option (List DescriptorOperation) :=
  match image.decodeOperations? with
  | none => none
  | some operations => some (operations.filter fun operation =>
      operation.matchesDeterministicInbound key && operation.enabledCandidate graph state)

/-- A headerless frame can select an operation only when its lane-bound direct
receive request has one enabled descriptor interpretation. -/
def RustDescriptorImage.resolveDeterministicInboundOperation?
    (image : RustDescriptorImage) (graph : EventGraph) (state : CommitState)
    (key : DeterministicInboundKey) : Option DescriptorOperation :=
  resolveUniqueDescriptorOperation?
    (image.matchingDeterministicInboundOperations? graph state key)

theorem resolved_deterministic_inbound_operation_is_unique
    {image : RustDescriptorImage} {graph : EventGraph} {state : CommitState}
    {key : DeterministicInboundKey}
    {operation : DescriptorOperation}
    (resolved : image.resolveDeterministicInboundOperation? graph state key = some operation) :
    image.matchingDeterministicInboundOperations? graph state key = some [operation] := by
  unfold RustDescriptorImage.resolveDeterministicInboundOperation? at resolved
  exact resolved_unique_descriptor_operation_is_singleton resolved

theorem resolved_deterministic_inbound_operation_matches_complete_enabled_key
    {image : RustDescriptorImage} {graph : EventGraph} {state : CommitState}
    {key : DeterministicInboundKey}
    {operation : DescriptorOperation}
    (resolved : image.resolveDeterministicInboundOperation? graph state key = some operation) :
    operation.matchesDeterministicInbound key = true ∧
      operation.enabledCandidate graph state = true := by
  have unique := resolved_deterministic_inbound_operation_is_unique resolved
  unfold RustDescriptorImage.matchingDeterministicInboundOperations? at unique
  cases decoded : image.decodeOperations? with
  | none => simp [decoded] at unique
  | some operations =>
      have filtered :
          operations.filter (fun candidate =>
            candidate.matchesDeterministicInbound key &&
              candidate.enabledCandidate graph state) = [operation] := by
        exact Option.some.inj (by simpa [decoded] using unique)
      have member : operation ∈ operations.filter (fun candidate =>
          candidate.matchesDeterministicInbound key &&
            candidate.enabledCandidate graph state) := by
        rw [filtered]
        simp
      exact Bool.and_eq_true_iff.mp (List.mem_filter.mp member).2

theorem resolved_deterministic_inbound_operation_binds_exact_event_columns
    {image : RustDescriptorImage} {graph : EventGraph} {state : CommitState}
    {key : DeterministicInboundKey}
    {operation : DescriptorOperation}
    (resolved : image.resolveDeterministicInboundOperation? graph state key = some operation) :
    image.eventOperation? operation.eventId = some operation := by
  have unique := resolved_deterministic_inbound_operation_is_unique resolved
  unfold RustDescriptorImage.matchingDeterministicInboundOperations? at unique
  cases decoded : image.decodeOperations? with
  | none => simp [decoded] at unique
  | some operations =>
      have filtered :
          operations.filter (fun candidate =>
            candidate.matchesDeterministicInbound key &&
              candidate.enabledCandidate graph state) = [operation] := by
        exact Option.some.inj (by simpa [decoded] using unique)
      have member : operation ∈ operations.filter (fun candidate =>
          candidate.matchesDeterministicInbound key &&
            candidate.enabledCandidate graph state) := by
        rw [filtered]
        simp
      exact decoded_operations_member_binds_exact_event_columns decoded
        (List.mem_filter.mp member).1

def RustDescriptorImage.decodeActions?
    (image : RustDescriptorImage) : Option (List LocalAction) :=
  (List.range image.eventCount).mapM image.eventAction?

/-- A decoded descriptor action is jointly owned by its role-event row and the
program atom it names. The two resident lane bytes cannot silently disagree. -/
theorem decoded_event_action_binds_exact_lane
    {image : RustDescriptorImage} {eventId : Nat} {action : LocalAction}
    (decoded : image.eventAction? eventId = some action) :
    ∃ atom lane,
      image.eventAtom? eventId = some atom ∧
      image.eventLane? eventId = some lane ∧
      lane = atom.lane := by
  unfold RustDescriptorImage.eventAction? at decoded
  by_cases roleValid : image.role < image.roleCount
  · simp only [roleValid, ↓reduceIte, Option.bind_eq_bind] at decoded
    cases atomCase : image.eventAtom? eventId with
    | none => simp [atomCase] at decoded
    | some atom =>
        cases laneCase : image.eventLane? eventId with
        | none => simp [atomCase, laneCase] at decoded
        | some lane =>
            by_cases sameLane : lane = atom.lane
            · exact ⟨atom, lane, rfl, rfl, sameLane⟩
            · simp [atomCase, laneCase, sameLane] at decoded
  · simp [roleValid] at decoded

def RustDescriptorImage.decodeRouteResolverRow?
    (image : RustDescriptorImage) (row : Nat) : Option DecodedRouteResolver := do
  if row < image.routeResolverCount then pure () else none
  let offset := image.programRouteResolverOffset +
    row * productionProgramRouteResolverStride
  let packedScope ← readU16LE? image.programBytes offset
  let resolver ← readU16LE? image.programBytes (offset + 2)
  let (scope, authority) ← decodePackedRouteAuthority? packedScope resolver
  let controller ← readByte? image.programBytes (offset + 4)
  let participantStart ← readU16LE? image.programBytes (offset + 5)
  let leftLenMinusOne ← readByte? image.programBytes (offset + 7)
  let participantEnd ←
    if row + 1 < image.routeResolverCount then
      readU16LE? image.programBytes
        (offset + productionProgramRouteResolverStride + 5)
    else
      some image.routeParticipantCount
  let participantMid := participantStart + leftLenMinusOne + 1
  if (row = 0 → participantStart = 0) ∧
      participantMid < participantEnd ∧
      participantEnd - participantMid ≤ 256 ∧
      participantEnd ≤ packedU16Absent ∧
      participantEnd ≤ image.routeParticipantCount then pure () else none
  let leftParticipants ← readByteRange?
    image.programBytes image.programRouteParticipantOffset
    participantStart participantMid
  let rightParticipants ← readByteRange?
    image.programBytes image.programRouteParticipantOffset
    participantMid participantEnd
  if scope < productionScopeCapacity ∧
      controller < image.roleCount ∧
      validRouteParticipants leftParticipants controller image.roleCount ∧
      validRouteParticipants rightParticipants controller image.roleCount then
    pure {
      scope
      authority
      controller := some controller
      leftParticipants
      rightParticipants
    }
  else
    none

def RustDescriptorImage.decodeRouteResolvers?
    (image : RustDescriptorImage) : Option (List DecodedRouteResolver) :=
  (List.range image.routeResolverCount).mapM image.decodeRouteResolverRow?

def RustDescriptorImage.decodeRouteAuthorities?
    (image : RustDescriptorImage) : Option (List RouteAuthority) :=
  image.decodeRouteResolvers?.map fun resolvers =>
    resolvers.map DecodedRouteResolver.authority

def validScopeMarkerTag (tag : Nat) : Bool :=
  tag = 0 || tag = 1 || tag = 2 || tag = 4 || tag = 5 || tag = 6

def RustDescriptorImage.decodeScopeMarkerRow?
    (image : RustDescriptorImage) (row : Nat) : Option DecodedScopeMarker := do
  if row < image.scopeMarkerCount then pure () else none
  let offset := image.programScopeMarkerOffset +
    row * productionProgramScopeMarkerStride
  let atomOffset ← readU16LE? image.programBytes offset
  let scope ← readU16LE? image.programBytes (offset + 2)
  let tag ← readByte? image.programBytes (offset + 4)
  if atomOffset ≤ image.atomCount ∧
      scope < 3 * productionScopeCapacity ∧
      scope % productionScopeCapacity < productionScopeCapacity ∧
      validScopeMarkerTag tag then
    pure { offset := atomOffset, scope, tag }
  else
    none

def RustDescriptorImage.decodeScopeMarkers?
    (image : RustDescriptorImage) : Option (List DecodedScopeMarker) :=
  (List.range image.scopeMarkerCount).mapM image.decodeScopeMarkerRow?

def EventGraph.routeAuthoritiesByConflict (graph : EventGraph) : List RouteAuthority :=
  normalizeProjectionRoutes (graph.routes.map fun route => {
    conflict := route.conflict
    authority := route.authority
    leftEvents := route.leftEvents
    rightEvents := route.rightEvents
    reentry := route.reentry
  }) |>.map ProjectionRoute.authority

def findRouteResolver?
    (scope : Nat) : List DecodedRouteResolver -> Nat -> Option (Nat × RouteAuthority)
  | [], _ => none
  | resolver :: rest, conflict =>
      if resolver.scope = scope then some (conflict, resolver.authority)
      else findRouteResolver? scope rest (conflict + 1)

def decodeRouteConflictReentry? (raw : Nat) : Option ReentryMode :=
  if raw = packedU16Absent then
    some .singlePass
  else if raw = packedConflictReentryWithoutParent then
    some .rolled
  else if raw < 32768 ∧
      (raw % 16384) / 2 < productionScopeCapacity then
    some (if raw / 16384 = 1 then .rolled else .singlePass)
  else
    none

def decodePackedLaneRange?
    (raw capacity : Nat) : Option (Nat × Nat) :=
  if raw = packedU32Absent then
    none
  else
    let start := raw / 65536
    let length := raw % 65536
    if start + length ≤ capacity then some (start, length) else none

def eventRange (start length : Nat) : List Nat :=
  (List.range length).map (start + ·)

theorem decode_packed_lane_range_accepts_bounded
    (raw capacity : Nat)
    (present : raw ≠ packedU32Absent)
    (bounded : raw / 65536 + raw % 65536 ≤ capacity) :
    decodePackedLaneRange? raw capacity =
      some (raw / 65536, raw % 65536) := by
  unfold decodePackedLaneRange?
  rw [if_neg present, if_pos bounded]

theorem decode_packed_lane_range_acceptance_implies_bounds
    (raw capacity : Nat)
    (accepted : decodePackedLaneRange? raw capacity =
      some (raw / 65536, raw % 65536)) :
    raw ≠ packedU32Absent ∧ raw / 65536 + raw % 65536 ≤ capacity := by
  have present : raw ≠ packedU32Absent := by
    intro absent
    unfold decodePackedLaneRange? at accepted
    rw [if_pos absent] at accepted
    cases accepted
  refine ⟨present, ?_⟩
  by_cases bounded : raw / 65536 + raw % 65536 ≤ capacity
  · exact bounded
  · unfold decodePackedLaneRange? at accepted
    rw [if_neg present, if_neg bounded] at accepted
    cases accepted

theorem packed_lane_range_rejects_end_outside_compact_domain :
    decodePackedLaneRange? (65535 * 65536 + 1) 65535 = none := by
  decide

structure DecodedPackedRouteArm where
  eventStart : Nat
  eventLength : Nat
  laneStepLength : Nat
  childSlot : Nat
  deriving Repr, DecidableEq

def decodePackedRouteArm?
    (eventRangeRaw laneStepLenAndChild eventCapacity : Nat) :
    Option DecodedPackedRouteArm := do
  let (eventStart, eventLength) ←
    decodePackedLaneRange? eventRangeRaw eventCapacity
  let childSlot := laneStepLenAndChild % 65536
  let encodedLaneStepLength := (laneStepLenAndChild / 65536) % 256
  let laneStepLength := if eventLength = 0 then 0 else encodedLaneStepLength + 1
  if laneStepLenAndChild / 16777216 = 0 ∧
      (eventLength = 0 → eventStart = 0) ∧
      (eventLength = 0 → encodedLaneStepLength = 0) then
    pure {
      eventStart
      eventLength
      laneStepLength
      childSlot
    }
  else
    none

theorem packed_route_arm_accepts_4096_event_start :
    decodePackedRouteArm?
      (4096 * 65536 + 1) 3 4097 =
      some {
        eventStart := 4096
        eventLength := 1
        laneStepLength := 1
        childSlot := 3
      } := by
  decide

theorem packed_route_arm_reaches_last_u16_event :
    decodePackedRouteArm?
      (65534 * 65536 + 1) 65535 65535 =
      some {
        eventStart := 65534
        eventLength := 1
        laneStepLength := 1
        childSlot := 65535
      } := by
  decide

theorem packed_route_arm_encodes_full_lane_domain :
    decodePackedRouteArm? 256 (255 * 65536 + 65535) 256 =
      some {
        eventStart := 0
        eventLength := 256
        laneStepLength := 256
        childSlot := 65535
      } := by
  decide

theorem packed_route_arm_rejects_noncanonical_empty_event_range :
    decodePackedRouteArm? 65536 65535 1 = none := by
  decide

def RustDescriptorImage.decodeRouteArmEvents?
    (image : RustDescriptorImage) (slot arm : Nat) : Option (List Nat) := do
  if slot < image.routeScopeCount ∧ arm < 2 then pure () else none
  let offset := image.roleRouteArmOffset +
    (slot * 2 + arm) * productionRoleRouteArmStride
  let eventRangeRaw ← readU32LE? image.roleBytes offset
  let laneStepLenAndChild ← readU32LE? image.roleBytes (offset + 4)
  let decoded ← decodePackedRouteArm? eventRangeRaw laneStepLenAndChild image.eventCount
  let childSlot := decoded.childSlot
  if childSlot = 65535 ∨
      (slot < childSlot ∧ childSlot < image.routeScopeCount) then pure () else none
  pure (eventRange decoded.eventStart decoded.eventLength)

def RustDescriptorImage.decodeRouteScope?
    (image : RustDescriptorImage) (slot : Nat) : Option Nat := do
  if slot < image.routeScopeCount then pure () else none
  let scope ← readU16LE? image.roleBytes
    (image.roleRouteScopeOffset + slot * productionRoleRouteScopeStride)
  if scope < productionScopeCapacity then some scope else none

def RustDescriptorImage.decodeRoute?
    (image : RustDescriptorImage)
    (resolvers : List DecodedRouteResolver)
    (slot : Nat) : Option (Option ProjectionRoute) := do
  let scope ← image.decodeRouteScope? slot
  let conflictRaw ← readU16LE? image.roleBytes
    (image.roleRouteScopeConflictOffset + slot * productionRoleConflictStride)
  let reentry ← decodeRouteConflictReentry? conflictRaw
  let leftEvents ← image.decodeRouteArmEvents? slot 0
  let rightEvents ← image.decodeRouteArmEvents? slot 1
  let (conflict, authority) ← findRouteResolver? scope resolvers 0
  if leftEvents.isEmpty && rightEvents.isEmpty then
    pure none
  else
    pure <| some { conflict, authority, leftEvents, rightEvents, reentry }

def RustDescriptorImage.decodeRoutes?
    (image : RustDescriptorImage) : Option (List ProjectionRoute) := do
  let resolvers ← image.decodeRouteResolvers?
  let routes ← (List.range image.routeScopeCount).mapM (image.decodeRoute? resolvers)
  pure (normalizeProjectionRoutes (routes.filterMap id))

/-- Empty role-local route slots are filtered as values; they never terminate
the exact descriptor slot traversal or hide a later projected route. -/
theorem sparse_route_slot_filter_preserves_later_route
    (before after : List (Option ProjectionRoute))
    (route : ProjectionRoute) :
    (before ++ none :: some route :: after).filterMap id =
      before.filterMap id ++ route :: after.filterMap id := by
  simp

structure DecodedRollRow where
  scope : Nat
  eventStart : Nat
  eventLength : Nat
  deriving Repr, DecidableEq

def RustDescriptorImage.decodeRollRow?
    (image : RustDescriptorImage) (slot : Nat) : Option DecodedRollRow := do
  if slot < image.rollScopeCount then pure () else none
  let offset := image.roleRollScopeOffset + slot * productionRoleRollScopeStride
  let scope ← readU16LE? image.roleBytes offset
  if scope < productionScopeCapacity then pure () else none
  let eventRangeRaw ← readU32LE? image.roleBytes (offset + 2)
  let (start, length) ← decodePackedLaneRange? eventRangeRaw image.eventCount
  if 0 < length then pure { scope, eventStart := start, eventLength := length } else none

def RustDescriptorImage.decodeRollRows?
    (image : RustDescriptorImage) : Option (List DecodedRollRow) :=
  (List.range image.rollScopeCount).mapM image.decodeRollRow?

def RustDescriptorImage.decodeRolls?
    (image : RustDescriptorImage) : Option (List ProjectionRoll) := do
  let rows ← image.decodeRollRows?
  pure (normalizeProjectionRolls (rows.map fun row => {
    events := eventRange row.eventStart row.eventLength
  }))

end Hibana
