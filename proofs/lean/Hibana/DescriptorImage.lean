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

def productionScopeCapacity : Nat := 8192

def productionPackedU16Capacity : Nat := 65536

def productionLocalStepCapacity : Nat := productionPackedU16Capacity

def productionResolverCapacity : Nat := productionPackedU16Capacity

def productionRouteResolverDynamicScopeBit : Nat := 32768

/-- Capacity facts emitted by a well-formed program image. Every structured
scope owns at least an enter/exit marker pair, and a route commit chain has at
most one row beyond the number of active structured scopes. -/
structure ProgramCapacityWitness where
  scopeCount : Nat
  scopeMarkerCount : Nat
  routeCommitCount : Nat
  programByteLength : Nat
  scopeMarkersCoverScopes : scopeCount * 2 ≤ scopeMarkerCount
  routeCommitsCoveredByScopes : routeCommitCount ≤ scopeCount + 1
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
  have routeCovered : witness.routeCommitCount ≤ witness.scopeCount + 1 :=
    witness.routeCommitsCoveredByScopes
  constructor
  · dsimp [productionScopeCapacity]
    omega
  · dsimp [productionLocalStepCapacity, productionPackedU16Capacity]
    omega

def routeCommit257CapacityWitness : ProgramCapacityWitness := {
  scopeCount := 256
  scopeMarkerCount := 1024
  routeCommitCount := 257
  programByteLength := 5120
  scopeMarkersCoverScopes := by decide
  routeCommitsCoveredByScopes := by decide
  scopeMarkerBytesFit := by decide
  descriptorFits := by decide
}

theorem route_commit_count_257_fits_the_descriptor_derived_domain :
    routeCommit257CapacityWitness.routeCommitCount = 257 ∧
      routeCommit257CapacityWitness.programByteLength < productionDescriptorByteCapacity := by
  decide

def productionRouteTraversalBound (routeScopeCount : Nat) : Nat :=
  routeScopeCount + 1

theorem production_route_traversal_bound_covers_every_acyclic_parent_chain
    (routeScopeCount chainLength : Nat)
    (chainCoveredByScopes : chainLength ≤ routeScopeCount) :
    chainLength < productionRouteTraversalBound routeScopeCount := by
  simp only [productionRouteTraversalBound]
  omega

theorem production_route_traversal_has_no_former_256_step_limit :
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

/-- Runtime offer entries are keyed by the active lane that owns them. The
production kernel may merge several lanes into one entry, but it cannot create
an entry without an owning active lane. -/
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

theorem production_frontier_capacity_has_no_former_eight_entry_limit :
    productionFrontierCapacity 9 = 9 ∧
      productionFrontierCapacity 256 = productionLaneCapacity := by
  decide

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
`EffList::push_scope_marker_outer_enter_reentry_mut`. -/
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
  ((canonicalProgramSource choreo).atoms[index]?).map ProgramAtomBody.frameLabel |>.getD 256

def Choreo.canonicalRoleFrameLabels (choreo : Choreo) (role : Nat) : List Nat :=
  (canonicalProgramSource choreo).atoms.filterMap fun atom =>
    if (Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema).isSome then
      some atom.frameLabel
    else none

structure ScopeSelection where
  scope : Nat
  start : Nat
  equalStartSpan : Option Nat

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
    (scope start span : Nat) : Option ScopeSelection :=
  match current with
  | none => some { scope, start, equalStartSpan := none }
  | some selected =>
      if selected.start < start then
        some { scope, start, equalStartSpan := none }
      else if selected.start = start then
        match selected.equalStartSpan with
        | none => some { scope, start, equalStartSpan := some span }
        | some best =>
            if span < best then some { scope, start, equalStartSpan := some span }
            else current
      else
        current

def canonicalScopeAt
    (markers : List DecodedScopeMarker) (atomCount effIndex : Nat) : Nat :=
  let selected := (List.range markers.length).foldl (fun current index =>
    match markers[index]? with
    | none => current
    | some marker =>
        if marker.offset ≤ effIndex && marker.tag % 4 = 0 then
          let stop := scopeSegmentEnd markers index atomCount marker
          if effIndex < stop then
            selectInnermostScope current marker.scope marker.offset (stop - marker.offset)
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
          (candidateSpan = selectedSpan && selected.start < candidate.start) then
        some candidate
      else
        current

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
  atoms.find? fun candidate => candidate.effIndex = effIndex

def RustDescriptorImage.eventAction?
    (image : RustDescriptorImage) (eventId : Nat) : Option LocalAction := do
  if image.role < image.roleCount then pure () else none
  let atom ← image.eventAtom? eventId
  let lane ← image.eventLane? eventId
  if lane = atom.lane then atom.localAction? image.role else none

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
      (eventLength = 0 → encodedLaneStepLength = 0) then
    pure {
      eventStart
      eventLength
      laneStepLength
      childSlot
    }
  else
    none

theorem packed_route_arm_crosses_former_twelve_bit_boundary :
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
