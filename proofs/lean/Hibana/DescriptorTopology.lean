import Hibana.DescriptorImage

namespace Hibana

private structure DecodedRoleEventRow where
  effIndex : Nat
  dependencyRow : Nat
  conflictRow : Nat
  scope : Nat
  frameLabel : Nat
  flags : Nat

private structure DecodedDependencyRow where
  start : Nat
  stop : Nat
  parallelScope : Nat
  conflict : Nat
  deriving Repr, DecidableEq

private def canonicalRoleAtoms
    (source : CanonicalProgramSource) (role : Nat) : List DecodedProgramAtom :=
  (enumerateProgramAtoms 0 source.atoms).filter fun atom =>
    (atom.localAction? role).isSome

private def canonicalLocalEventRange
    (atoms : List DecodedProgramAtom)
    (startEff stopEff : Nat) : Nat × Nat :=
  let positions := (List.range atoms.length).filter fun index =>
    match atoms[index]? with
    | some atom => startEff ≤ atom.effIndex ∧ atom.effIndex < stopEff
    | none => false
  match positions.head? with
  | none => (0, 0)
  | some start => (start, positions.length)

private def parallelExitScan : List DecodedScopeMarker -> Nat -> Option Nat
  | [], _ => none
  | marker :: rest, depth =>
      if marker.tag % 4 = 0 then
        parallelExitScan rest (depth + 1)
      else if marker.tag % 4 = 2 then
        if depth = 0 then some marker.offset
        else parallelExitScan rest (depth - 1)
      else
        parallelExitScan rest depth

private def parallelExitForEnter?
    (markers : List DecodedScopeMarker) (enterIndex : Nat) : Option Nat :=
  parallelExitScan (markers.drop (enterIndex + 1)) 0

private def parentParallelEndScan
    (markers : List DecodedScopeMarker) : List Nat -> Nat -> Option Nat
  | [], _ => none
  | index :: rest, depth =>
      match markers[index]? with
      | none => parentParallelEndScan markers rest depth
      | some marker =>
          if marker.tag % 4 = 2 then
            parentParallelEndScan markers rest (depth + 1)
          else if marker.tag % 4 = 0 then
            if depth = 0 then
              if marker.scope / 8192 = 2 then parallelExitForEnter? markers index
              else parentParallelEndScan markers rest depth
            else
              parentParallelEndScan markers rest (depth - 1)
          else
            parentParallelEndScan markers rest depth

private def nearestParentParallelEnd
    (markers : List DecodedScopeMarker)
    (enterIndex ownExit : Nat) : Nat :=
  match parentParallelEndScan markers (List.range enterIndex).reverse 0 with
  | some stop => stop
  | none => ownExit

private def canonicalScopeDependencyBounds?
    (source : CanonicalProgramSource) (scope : Nat) : Option (Nat × Nat) := do
  if scope / 8192 = 0 then
    routeBounds? source.markers scope
  else
    let index ← (List.range source.markers.length).find? fun index =>
      match source.markers[index]? with
      | some marker => marker.scope = scope && marker.tag % 4 = 0
      | none => false
    let marker ← source.markers[index]?
    pure (marker.offset, scopeSegmentEnd source.markers index source.atoms.length marker)

private def canonicalPackedDependencyConflict
    (source : CanonicalProgramSource) (scope : Nat) : Nat :=
  match canonicalScopeDependencyBounds? source scope with
  | none => 0
  | some target =>
      match canonicalEnclosingRouteArm? source scope target with
      | none => 0
      | some arm => arm.scope % 8192 * 4 + arm.arm + 2

private def localRangeContainsLane
    (atoms : List DecodedProgramAtom)
    (start stop lane : Nat) : Bool :=
  (List.range (stop - start)).any fun offset =>
    match atoms[start + offset]? with
    | some atom => atom.lane = lane
    | none => false

private def applyCanonicalDependency
    (atoms : List DecodedProgramAtom)
    (start stop parentParallelEnd : Nat)
    (dependency : DecodedDependencyRow)
    (dependencies : List (Option DecodedDependencyRow)) :
    List (Option DecodedDependencyRow) :=
  (List.range (atoms.length - stop)).foldl (fun current offset =>
    let step := stop + offset
    match atoms[step]? with
    | none => current
    | some atom =>
        let applies :=
          localRangeContainsLane atoms start stop atom.lane ||
            parentParallelEnd ≤ atom.effIndex
        let replaces := match current[step]? with
          | some (some prior) => prior.stop ≤ stop
          | some none | none => true
        if applies && replaces then current.set step (some dependency) else current
  ) dependencies

private def Choreo.canonicalRoleDependenciesByEvent
    (choreo : Choreo) (role : Nat) : List (Option DecodedDependencyRow) :=
  let source := canonicalProgramSource choreo
  let atoms := canonicalRoleAtoms source role
  (List.range source.markers.length).foldl (fun dependencies markerIndex =>
    match source.markers[markerIndex]? with
    | none => dependencies
    | some marker =>
        if marker.tag % 4 = 0 && marker.scope / 8192 = 2 then
          match parallelExitForEnter? source.markers markerIndex with
          | none => dependencies
          | some exitEff =>
              let localRange := canonicalLocalEventRange atoms marker.offset exitEff
              let start := localRange.1
              let stop := start + localRange.2
              if start < stop then
                let parentEnd := nearestParentParallelEnd
                  source.markers markerIndex exitEff
                let dependency := {
                  start
                  stop
                  parallelScope := marker.scope % 8192
                  conflict := canonicalPackedDependencyConflict source marker.scope
                }
                applyCanonicalDependency atoms start stop parentEnd dependency dependencies
              else
                dependencies
        else
          dependencies
  ) (List.replicate atoms.length none)

def Choreo.canonicalRoleEventDependencyRows
    (choreo : Choreo) (role : Nat) : List Nat :=
  compactConflictReferences 0 (choreo.canonicalRoleDependenciesByEvent role)

def Choreo.canonicalRoleDependencies
    (choreo : Choreo) (role : Nat) : List DecodedDependencyRow :=
  (choreo.canonicalRoleDependenciesByEvent role).filterMap id

private structure DecodedRouteArmRow where
  eventStart : Nat
  eventLength : Nat
  childSlot : Nat
  laneStepStart : Nat
  laneStepLength : Nat
  deriving Repr, DecidableEq

private structure DecodedRouteArmLaneStep where
  lane : Nat
  firstStep : Nat
  lastStep : Nat
  deriving Repr, DecidableEq

private structure PassiveChildSelection where
  scope : Nat
  span : Nat

private def canonicalRouteSlot? (scope : Nat) :
    List DecodedRouteResolver -> Nat -> Option Nat
  | [], _ => none
  | resolver :: rest, slot =>
      if resolver.scope = scope then some slot
      else canonicalRouteSlot? scope rest (slot + 1)

private def canonicalPassiveChildScope?
    (source : CanonicalProgramSource)
    (route arm armStart armStop : Nat) : Option Nat :=
  let selected := source.resolvers.foldl (fun current resolver =>
    let startsAtArm := source.markers.any fun marker =>
      marker.scope = resolver.scope && marker.tag % 4 = 0 && marker.offset = armStart
    if resolver.scope = route || !startsAtArm then current
    else
      match canonicalParentRouteArm? source resolver.scope,
          routeBounds? source.markers resolver.scope with
      | some parent, some bounds =>
          if parent.scope = route && parent.arm = arm &&
              armStart < bounds.2 && bounds.2 ≤ armStop then
            let candidate := { scope := resolver.scope, span := bounds.2 - armStart }
            match current with
            | none => some candidate
            | some prior => if prior.span < candidate.span then some candidate else current
          else
            current
      | _, _ => current) none
  selected.map PassiveChildSelection.scope

private structure CanonicalRouteArmSpec where
  eventStart : Nat
  eventLength : Nat
  childSlot : Nat

private def canonicalRouteArmSpecsFrom
    (source : CanonicalProgramSource)
    (atoms : List DecodedProgramAtom) :
    List DecodedRouteResolver -> Nat -> List CanonicalRouteArmSpec
  | [], _ => []
  | resolver :: rest, slot =>
      match routeArmRanges? source.markers resolver.scope with
      | none => canonicalRouteArmSpecsFrom source atoms rest (slot + 1)
      | some (left, right) =>
          let makeSpec (arm : Nat) (bounds : Nat × Nat) :=
            let localRange := canonicalLocalEventRange atoms bounds.1 bounds.2
            let childSlot := match canonicalPassiveChildScope?
                source resolver.scope arm bounds.1 bounds.2 with
              | none => 65535
              | some childScope =>
                  match canonicalRouteSlot? childScope source.resolvers 0 with
                  | some childSlot => childSlot
                  | none => 65535
            {
              eventStart := localRange.1
              eventLength := localRange.2
              childSlot
            }
          makeSpec 0 left :: makeSpec 1 right ::
            canonicalRouteArmSpecsFrom source atoms rest (slot + 1)

private def updateRouteArmLaneStep
    (lane step : Nat) : List DecodedRouteArmLaneStep -> List DecodedRouteArmLaneStep
  | [] => [{ lane, firstStep := step, lastStep := step }]
  | current :: rest =>
      if current.lane = lane then { current with lastStep := step } :: rest
      else current :: updateRouteArmLaneStep lane step rest

private def canonicalRouteArmLaneSteps
    (atoms : List DecodedProgramAtom)
    (spec : CanonicalRouteArmSpec) : List DecodedRouteArmLaneStep :=
  (List.range spec.eventLength).foldl (fun rows offset =>
    let step := spec.eventStart + offset
    match atoms[step]? with
    | none => rows
    | some atom => updateRouteArmLaneStep atom.lane step rows
  ) []

private structure CanonicalRouteArmColumns where
  rows : List DecodedRouteArmRow
  laneSteps : List DecodedRouteArmLaneStep

private def compileCanonicalRouteArmColumns
    (atoms : List DecodedProgramAtom) :
    List CanonicalRouteArmSpec -> Nat -> CanonicalRouteArmColumns
  | [], _ => { rows := [], laneSteps := [] }
  | spec :: rest, laneStepStart =>
      let laneSteps := canonicalRouteArmLaneSteps atoms spec
      let tail := compileCanonicalRouteArmColumns
        atoms rest (laneStepStart + laneSteps.length)
      {
        rows := {
          eventStart := spec.eventStart
          eventLength := spec.eventLength
          childSlot := spec.childSlot
          laneStepStart
          laneStepLength := laneSteps.length
        } :: tail.rows
        laneSteps := laneSteps ++ tail.laneSteps
      }

private def Choreo.canonicalRoleRouteArmColumns
    (choreo : Choreo) (role : Nat) : CanonicalRouteArmColumns :=
  let source := canonicalProgramSource choreo
  let atoms := canonicalRoleAtoms source role
  let specs := canonicalRouteArmSpecsFrom source atoms source.resolvers 0
  compileCanonicalRouteArmColumns atoms specs 0

def Choreo.canonicalRoleRouteArms
    (choreo : Choreo) (role : Nat) : List DecodedRouteArmRow :=
  (choreo.canonicalRoleRouteArmColumns role).rows

def Choreo.canonicalRoleRouteArmLaneSteps
    (choreo : Choreo) (role : Nat) : List DecodedRouteArmLaneStep :=
  (choreo.canonicalRoleRouteArmColumns role).laneSteps

private structure ResidentRangeState where
  currentEff : Nat
  ranges : List (Nat × Nat)

private def appendNonemptyRange
    (ranges : List (Nat × Nat)) (range : Nat × Nat) : List (Nat × Nat) :=
  if range.2 = 0 then ranges else ranges ++ [range]

private def canonicalResidentRanges
    (source : CanonicalProgramSource)
    (atoms : List DecodedProgramAtom) : List (Nat × Nat) :=
  let state := (List.range source.markers.length).foldl (fun state markerIndex =>
    match source.markers[markerIndex]? with
    | none => state
    | some marker =>
        if marker.tag % 4 = 0 && marker.scope / 8192 = 2 then
          let exitEff := scopeSegmentEnd
            source.markers markerIndex source.atoms.length marker
          let before := canonicalLocalEventRange atoms state.currentEff marker.offset
          let parallelStart := max marker.offset state.currentEff
          let parallel := canonicalLocalEventRange atoms parallelStart exitEff
          {
            currentEff := max state.currentEff exitEff
            ranges := appendNonemptyRange
              (appendNonemptyRange state.ranges before) parallel
          }
        else
          state
  ) ({ currentEff := 0, ranges := [] } : ResidentRangeState)
  let withTail := appendNonemptyRange state.ranges
    (canonicalLocalEventRange atoms state.currentEff source.atoms.length)
  if withTail.isEmpty then
    appendNonemptyRange [] (canonicalLocalEventRange atoms 0 source.atoms.length)
  else
    withTail

private def canonicalResidentBoundariesFromRanges :
    List (Nat × Nat) -> List Nat
  | [] => []
  | first :: rest =>
      first.1 :: (first :: rest).map fun range => range.1 + range.2

private def lanesForLocalRange
    (atoms : List DecodedProgramAtom) (range : Nat × Nat) : List Nat :=
  (List.range range.2).filterMap fun offset =>
    (atoms[range.1 + offset]?).map DecodedProgramAtom.lane

private def laneBytesForLanes (lanes : List Nat) : List Nat :=
  let unique := lanes.eraseDups
  let maxLanePlusOne := unique.foldl (fun current lane => max current (lane + 1)) 0
  (List.range ((maxLanePlusOne + 7) / 8)).map fun byteIndex =>
    (unique.filter fun lane => lane / 8 = byteIndex).foldl
      (fun byte lane => byte + 2 ^ (lane % 8)) 0

private def laneBytesForLocalRange
    (atoms : List DecodedProgramAtom) (range : Nat × Nat) : List Nat :=
  laneBytesForLanes (lanesForLocalRange atoms range)

private def laneBytesForLocalRanges
    (atoms : List DecodedProgramAtom) (ranges : List (Nat × Nat)) : List Nat :=
  laneBytesForLanes (ranges.flatMap (lanesForLocalRange atoms))

private structure RouteDepthState where
  current : Nat
  maximum : Nat

private def canonicalMaxRouteCommitCount (source : CanonicalProgramSource) : Nat :=
  let depth := source.markers.foldl (fun depth marker =>
    if marker.scope / 8192 = 0 then
      if marker.tag % 4 = 0 then
        let current := depth.current + 1
        { current, maximum := max depth.maximum current }
      else if marker.tag % 4 = 2 then
        { depth with current := depth.current - 1 }
      else
        depth
    else
      depth
  ) ({ current := 0, maximum := 0 } : RouteDepthState)
  depth.maximum

private structure CanonicalRoleMetadata where
  roleCount : Nat
  activeLaneCount : Nat
  endpointLaneSlotCount : Nat
  logicalLaneCount : Nat
  maxRouteCommitCount : Nat
  firstActiveLane : Nat
  activeLaneStart : Nat
  activeLaneLength : Nat

private def Choreo.canonicalRoleMetadata
    (choreo : Choreo) (role : Nat) : CanonicalRoleMetadata :=
  let source := canonicalProgramSource choreo
  let roleCount := (enumerateProgramAtoms 0 source.atoms).foldl (fun count atom =>
    max count (max atom.sender atom.receiver + 1)) 0
  let atoms := canonicalRoleAtoms source role
  let activeLanes := (atoms.map DecodedProgramAtom.lane).eraseDups
  let activeLaneCount := activeLanes.length
  let endpointLaneSlotCount := activeLanes.foldl
    (fun count lane => max count (lane + 1)) 1
  let logicalLaneCount := endpointLaneSlotCount
  let firstActiveLane := match activeLanes with
    | [] => packedU16Absent
    | first :: rest => rest.foldl min first
  {
    roleCount
    activeLaneCount
    endpointLaneSlotCount
    logicalLaneCount
    maxRouteCommitCount := canonicalMaxRouteCommitCount source
    firstActiveLane
    activeLaneStart := 0
    activeLaneLength := (laneBytesForLanes activeLanes).length
  }

def Choreo.canonicalRoleCount (choreo : Choreo) (role : Nat) : Nat :=
  (choreo.canonicalRoleMetadata role).roleCount

def Choreo.canonicalActiveLaneCount (choreo : Choreo) (role : Nat) : Nat :=
  (choreo.canonicalRoleMetadata role).activeLaneCount

def Choreo.canonicalEndpointLaneSlotCount (choreo : Choreo) (role : Nat) : Nat :=
  (choreo.canonicalRoleMetadata role).endpointLaneSlotCount

def Choreo.canonicalLogicalLaneCount (choreo : Choreo) (role : Nat) : Nat :=
  (choreo.canonicalRoleMetadata role).logicalLaneCount

theorem Choreo.canonical_logical_lane_count_is_exact_endpoint_span
    (choreo : Choreo) (role : Nat) :
    choreo.canonicalLogicalLaneCount role =
      choreo.canonicalEndpointLaneSlotCount role := by
  rfl

def Choreo.canonicalRoleMaxRouteCommitCount (choreo : Choreo) (role : Nat) : Nat :=
  (choreo.canonicalRoleMetadata role).maxRouteCommitCount

def Choreo.canonicalFirstActiveLane (choreo : Choreo) (role : Nat) : Nat :=
  (choreo.canonicalRoleMetadata role).firstActiveLane

def Choreo.canonicalActiveLaneRange (choreo : Choreo) (role : Nat) : Nat × Nat :=
  let metadata := choreo.canonicalRoleMetadata role
  (metadata.activeLaneStart, metadata.activeLaneLength)

private def appendLaneBytes
    (current additions : List Nat) : List Nat × (Nat × Nat) :=
  if additions.isEmpty then
    (current, (0, 0))
  else
    (current ++ additions, (current.length, additions.length))

private structure CanonicalRouteLaneColumns where
  laneBits : List Nat
  armRows : List (Nat × Nat)
  offerRows : List (Nat × Nat)

private def compileCanonicalRouteLaneColumns
    (source : CanonicalProgramSource)
    (atoms : List DecodedProgramAtom) :
    List DecodedRouteResolver -> List Nat -> CanonicalRouteLaneColumns
  | [], laneBits => { laneBits, armRows := [], offerRows := [] }
  | resolver :: rest, laneBits =>
      match routeArmRanges? source.markers resolver.scope with
      | none => compileCanonicalRouteLaneColumns source atoms rest laneBits
      | some (leftBounds, rightBounds) =>
          let leftRange := canonicalLocalEventRange atoms leftBounds.1 leftBounds.2
          let rightRange := canonicalLocalEventRange atoms rightBounds.1 rightBounds.2
          let (withLeft, leftRow) := appendLaneBytes
            laneBits (laneBytesForLocalRange atoms leftRange)
          let (withRight, rightRow) := appendLaneBytes
            withLeft (laneBytesForLocalRange atoms rightRange)
          let offerBytes := laneBytesForLocalRanges atoms [leftRange, rightRange]
          let (withOffer, offerRow) := appendLaneBytes withRight offerBytes
          let tail := compileCanonicalRouteLaneColumns source atoms rest withOffer
          {
            laneBits := tail.laneBits
            armRows := leftRow :: rightRow :: tail.armRows
            offerRows := offerRow :: tail.offerRows
          }

private structure CanonicalRoleLaneColumns where
  residentBoundaries : List Nat
  laneBits : List Nat
  routeArmLaneRows : List (Nat × Nat)
  routeOfferLaneRows : List (Nat × Nat)

private def Choreo.canonicalRoleLaneColumns
    (choreo : Choreo) (role : Nat) : CanonicalRoleLaneColumns :=
  let source := canonicalProgramSource choreo
  let atoms := canonicalRoleAtoms source role
  let residentRanges := canonicalResidentRanges source atoms
  let activeBits := laneBytesForLocalRange atoms (0, atoms.length)
  let routes := compileCanonicalRouteLaneColumns
    source atoms source.resolvers activeBits
  {
    residentBoundaries := canonicalResidentBoundariesFromRanges residentRanges
    laneBits := routes.laneBits
    routeArmLaneRows := routes.armRows
    routeOfferLaneRows := routes.offerRows
  }

def Choreo.canonicalRoleResidentBoundaries
    (choreo : Choreo) (role : Nat) : List Nat :=
  (choreo.canonicalRoleLaneColumns role).residentBoundaries

def Choreo.canonicalRoleLaneBits
    (choreo : Choreo) (role : Nat) : List Nat :=
  (choreo.canonicalRoleLaneColumns role).laneBits

def Choreo.canonicalRoleRouteArmLaneRows
    (choreo : Choreo) (role : Nat) : List (Nat × Nat) :=
  (choreo.canonicalRoleLaneColumns role).routeArmLaneRows

def Choreo.canonicalRoleRouteOfferLaneRows
    (choreo : Choreo) (role : Nat) : List (Nat × Nat) :=
  (choreo.canonicalRoleLaneColumns role).routeOfferLaneRows

private def packedRouteConflictScope? (raw : Nat) : Option Nat :=
  if raw = packedU16Absent || raw = packedConflictReentryWithoutParent || 16384 ≤ raw then
    none
  else
    some ((raw % 8192) / 2)

private def canonicalNextRouteCommitConflict?
    (source : CanonicalProgramSource) (current : Nat) : Option Nat := do
  let scope ← packedRouteConflictScope? current
  let next := canonicalRouteScopeConflict source scope
  let _ ← packedRouteConflictScope? next
  pure next

private def canonicalRouteCommitChainFrom
    (source : CanonicalProgramSource) : Nat -> Nat -> List Nat
  | 0, _ => []
  | fuel + 1, current =>
      current :: match canonicalNextRouteCommitConflict? source current with
        | none => []
        | some next => canonicalRouteCommitChainFrom source fuel next

private def canonicalRouteCommitChain
    (source : CanonicalProgramSource) (scope arm : Nat) : List Nat :=
  (canonicalRouteCommitChainFrom
    source (source.resolvers.length + 1) (scope * 2 + arm)).reverse

private structure CanonicalRouteCommitColumns where
  ranges : List (Nat × Nat)
  rows : List Nat

private def compileCanonicalRouteCommitColumns
    (source : CanonicalProgramSource) :
    List DecodedRouteResolver -> Nat -> CanonicalRouteCommitColumns
  | [], _ => { ranges := [], rows := [] }
  | resolver :: rest, rowStart =>
      let left := canonicalRouteCommitChain source resolver.scope 0
      let right := canonicalRouteCommitChain source resolver.scope 1
      let rightStart := rowStart + left.length
      let tail := compileCanonicalRouteCommitColumns
        source rest (rightStart + right.length)
      {
        ranges := (rowStart, left.length) ::
          (rightStart, right.length) :: tail.ranges
        rows := left ++ right ++ tail.rows
      }

private def Choreo.canonicalRoleRouteCommitColumns
    (choreo : Choreo) : CanonicalRouteCommitColumns :=
  let source := canonicalProgramSource choreo
  compileCanonicalRouteCommitColumns source source.resolvers 0

def Choreo.canonicalRoleRouteCommitRanges
    (choreo : Choreo) : List (Nat × Nat) :=
  choreo.canonicalRoleRouteCommitColumns.ranges

def Choreo.canonicalRoleRouteCommitRows
    (choreo : Choreo) : List Nat :=
  choreo.canonicalRoleRouteCommitColumns.rows

private def firstCanonicalEnterForScope
    (markers : List DecodedScopeMarker) (index : Nat) (marker : DecodedScopeMarker) : Bool :=
  marker.tag % 4 = 0 && !(markers.take index).any fun prior =>
    prior.scope = marker.scope && prior.tag % 4 = 0

def Choreo.canonicalRoleRollRows
    (choreo : Choreo) (role : Nat) : List DecodedRollRow :=
  let source := canonicalProgramSource choreo
  let atoms := canonicalRoleAtoms source role
  (List.range source.markers.length).filterMap fun index => do
    let marker ← source.markers[index]?
    if marker.scope / 8192 = 1 && firstCanonicalEnterForScope source.markers index marker then
      let stop := scopeSegmentEnd source.markers index source.atoms.length marker
      let range := canonicalLocalEventRange atoms marker.offset stop
      if 0 < range.2 then
        some {
          scope := marker.scope % 8192
          eventStart := range.1
          eventLength := range.2
        }
      else
        none
    else
      none

private def rollRowsStructurallyCoherent
    (rows : List DecodedRollRow) : Bool :=
  decide (rows.Pairwise fun left right =>
    left.scope ≠ right.scope ∧
      (left.eventStart + left.eventLength ≤ right.eventStart ∨
       right.eventStart + right.eventLength ≤ left.eventStart ∨
       (left.eventStart ≤ right.eventStart ∧
        right.eventStart + right.eventLength ≤ left.eventStart + left.eventLength) ∨
       (right.eventStart ≤ left.eventStart ∧
        left.eventStart + left.eventLength ≤ right.eventStart + right.eventLength)))

/-- Resident iteration rows have unique scope identities and the laminar range
shape required by runtime nesting. Equal projected ranges are permitted and use
the source-preorder scope ordinal as their erased nesting order. -/
def RustDescriptorImage.rollColumnsCoherent
    (image : RustDescriptorImage) : Bool :=
  match image.decodeRollRows? with
  | some rows => rollRowsStructurallyCoherent rows
  | none => false

private def validPackedScope (raw : Nat) : Bool :=
  raw = packedU16Absent ||
    (raw < 3 * productionScopeCapacity &&
      raw % productionScopeCapacity < productionScopeCapacity)

private def validPackedConflict (raw : Nat) : Bool :=
  raw = packedU16Absent ||
    raw = packedConflictReentryWithoutParent ||
    (raw < 32768 && (raw % 16384) / 2 < productionScopeCapacity)

private def validRouteArmConflict (raw : Nat) : Bool :=
  raw ≠ packedU16Absent &&
    raw ≠ packedConflictReentryWithoutParent &&
    validPackedConflict raw

private def validPackedDependencyConflict (raw : Nat) : Bool :=
  let tag := raw % 4
  let route := raw / 4
  raw < 32768 &&
    if tag < 2 then route = 0 else route < productionScopeCapacity

private def RustDescriptorImage.decodeRoleEventRow?
    (image : RustDescriptorImage) (row : Nat) : Option DecodedRoleEventRow := do
  if row < image.eventCount then pure () else none
  let offset := row * productionRoleEventStride
  let effIndex ← readU16LE? image.roleBytes offset
  let dependencyRow ← readU16LE? image.roleBytes (offset + 2)
  let conflictRow ← readU16LE? image.roleBytes (offset + 4)
  let scope ← readU16LE? image.roleBytes (offset + 6)
  let frameLabel ← readByte? image.roleBytes (offset + 8)
  let flags ← readByte? image.roleBytes (offset + 9)
  if effIndex < productionEventIdentityCapacity ∧
      (dependencyRow = packedU16Absent ∨ dependencyRow < image.dependencyRowCount) ∧
      (conflictRow = packedU16Absent ∨ conflictRow < image.conflictRowCount) ∧
      validPackedScope scope = true ∧
      flags < 2 then
    pure { effIndex, dependencyRow, conflictRow, scope, frameLabel, flags }
  else
    none

private def RustDescriptorImage.decodeRoleEventRows?
    (image : RustDescriptorImage) : Option (List DecodedRoleEventRow) :=
  (List.range image.eventCount).mapM image.decodeRoleEventRow?

/-- The resident lane byte for every local event must fit the exact logical
lane span used by the Rust decoder. Agreement with a program atom alone is not
enough when descriptor metadata is malformed. -/
def RustDescriptorImage.eventLaneColumnCoherent
    (image : RustDescriptorImage) : Bool :=
  match (List.range image.eventCount).mapM image.eventLane? with
  | some lanes => lanes.all (· < image.logicalLaneCount)
  | none => false

def RustDescriptorImage.decodeEventDependencyRows?
    (image : RustDescriptorImage) : Option (List Nat) :=
  image.decodeRoleEventRows?.map fun rows =>
    rows.map DecodedRoleEventRow.dependencyRow

private def RustDescriptorImage.decodeDependencyRow?
    (image : RustDescriptorImage) (row : Nat) : Option DecodedDependencyRow := do
  if row < image.dependencyRowCount then pure () else none
  let offset := image.roleDependencyOffset + row * productionRoleDependencyStride
  let start ← readU16LE? image.roleBytes offset
  let stop ← readU16LE? image.roleBytes (offset + 2)
  let parallelScope ← readU16LE? image.roleBytes (offset + 4)
  let conflict ← readU16LE? image.roleBytes (offset + 6)
  if start < stop ∧
      stop < productionLocalStepCapacity ∧
      stop ≤ image.eventCount ∧
      parallelScope < productionScopeCapacity ∧
      validPackedDependencyConflict conflict = true then
    pure { start, stop, parallelScope, conflict }
  else
    none

def RustDescriptorImage.decodeDependencyRows?
    (image : RustDescriptorImage) : Option (List DecodedDependencyRow) :=
  (List.range image.dependencyRowCount).mapM image.decodeDependencyRow?

private def RustDescriptorImage.conflictRow?
    (image : RustDescriptorImage) (row : Nat) : Option Nat := do
  if row < image.conflictRowCount then pure () else none
  let raw ← readU16LE? image.roleBytes
    (image.roleConflictOffset + row * productionRoleConflictStride)
  if validRouteArmConflict raw then some raw else none

private def RustDescriptorImage.routeScopeConflictRow?
    (image : RustDescriptorImage) (row : Nat) : Option Nat := do
  if row < image.routeScopeCount then pure () else none
  let raw ← readU16LE? image.roleBytes
    (image.roleRouteScopeConflictOffset + row * productionRoleConflictStride)
  if validPackedConflict raw then some raw else none

private def RustDescriptorImage.routeCommitRow?
    (image : RustDescriptorImage) (row : Nat) : Option Nat := do
  if row < image.routeCommitRowCount then pure () else none
  let raw ← readU16LE? image.roleBytes
    (image.roleRouteCommitRowOffset + row * productionRoleConflictStride)
  if validRouteArmConflict raw then some raw else none

def RustDescriptorImage.decodeRouteCommitRows?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.routeCommitRowCount).mapM image.routeCommitRow?

private def RustDescriptorImage.decodeRouteArmRow?
    (image : RustDescriptorImage) (row laneStepStart : Nat) :
    Option DecodedRouteArmRow := do
  if row < image.routeScopeCount * 2 then pure () else none
  let offset := image.roleRouteArmOffset + row * productionRoleRouteArmStride
  let eventRangeRaw ← readU32LE? image.roleBytes offset
  let laneStepLenAndChild ← readU32LE? image.roleBytes (offset + 4)
  let decoded ← decodePackedRouteArm?
    eventRangeRaw laneStepLenAndChild image.eventCount
  if laneStepStart + decoded.laneStepLength ≤ image.routeArmLaneStepCount ∧
      (decoded.childSlot = 65535 ∨
        (row / 2 < decoded.childSlot ∧ decoded.childSlot < image.routeScopeCount)) then
    pure {
      eventStart := decoded.eventStart
      eventLength := decoded.eventLength
      childSlot := decoded.childSlot
      laneStepStart
      laneStepLength := decoded.laneStepLength
    }
  else
    none

private def RustDescriptorImage.decodeRouteArmRowsFrom?
    (image : RustDescriptorImage) : Nat → Nat → Nat → Option (List DecodedRouteArmRow)
  | 0, _, _ => some []
  | remaining + 1, row, laneStepStart => do
      let decoded ← image.decodeRouteArmRow? row laneStepStart
      let rest ← image.decodeRouteArmRowsFrom?
        remaining (row + 1) (laneStepStart + decoded.laneStepLength)
      pure (decoded :: rest)

def RustDescriptorImage.decodeRouteArmRows?
    (image : RustDescriptorImage) : Option (List DecodedRouteArmRow) :=
  image.decodeRouteArmRowsFrom? (image.routeScopeCount * 2) 0 0

private def RustDescriptorImage.decodedRouteMembershipsAt?
    (image : RustDescriptorImage)
    (eventId : Nat) : Option (List (Nat × Nat)) := do
  let scopes ← image.decodeRouteScopeRows?
  let arms ← image.decodeRouteArmRows?
  pure ((List.range arms.length).filterMap fun rowIndex =>
    match arms[rowIndex]?, scopes[rowIndex / 2]? with
    | some row, some scope =>
        if row.eventStart ≤ eventId && eventId < row.eventStart + row.eventLength then
          some (scope, rowIndex % 2)
        else
          none
    | _, _ => none)

private def RustDescriptorImage.eventConflictMembership?
    (image : RustDescriptorImage)
    (eventId : Nat) : Option (Option (Nat × Nat)) := do
  let event ← image.decodeRoleEventRow? eventId
  if event.conflictRow = packedU16Absent then
    some none
  else
    let raw ← image.conflictRow? event.conflictRow
    some (some ((raw % 16384) / 2, raw % 2))

private def packedConflictMembership? (raw : Nat) : Option (Option (Nat × Nat)) :=
  if raw = packedU16Absent || raw = packedConflictReentryWithoutParent then
    some none
  else if validRouteArmConflict raw then
    some (some ((raw % 16384) / 2, raw % 2))
  else
    none

private def RustDescriptorImage.routeParentMembership?
    (image : RustDescriptorImage) (scope : Nat) : Option (Option (Nat × Nat)) := do
  let scopes ← image.decodeRouteScopeRows?
  let slot ← (List.range scopes.length).find? fun index =>
    scopes[index]? = some scope
  let raw ← image.routeScopeConflictRow? slot
  packedConflictMembership? raw

/-- Every passive route child must name a distinct later scope whose recorded
parent authority is exactly the route row and arm that owns the child pointer.
This is the byte-level counterpart of
`RoleLaneImage::passive_arm_child_ordinal_by_slot`; accepting less here would
make the Lean checker accept descriptor images rejected by the Rust runtime. -/
def RustDescriptorImage.passiveChildColumnsCoherent
    (image : RustDescriptorImage) : Bool :=
  match image.decodeRouteScopeRows?, image.decodeRouteArmRows? with
  | some scopes, some arms =>
      (List.range arms.length).all fun rowIndex =>
        match arms[rowIndex]?, scopes[rowIndex / 2]? with
        | some row, some parentScope =>
            if row.childSlot = packedU16Absent then
              true
            else
              match scopes[row.childSlot]?,
                  image.routeScopeConflictRow? row.childSlot with
              | some childScope, some childConflict =>
                  childScope != parentScope &&
                    packedConflictMembership? childConflict =
                      some (some (parentScope, rowIndex % 2))
              | _, _ => false
        | _, _ => false
  | _, _ => false

private def RustDescriptorImage.runtimeRouteParentMembership?
    (image : RustDescriptorImage) (scope : Nat) : Option (Option (Nat × Nat)) := do
  let parent ← image.routeParentMembership? scope
  match parent with
  | some (parentScope, arm) =>
      if parentScope = scope then some none else some (some (parentScope, arm))
  | none => some none

private def RustDescriptorImage.routeCommitChainMatches
    (image : RustDescriptorImage) : List Nat -> (Nat × Nat) -> Bool
  | [], _ => false
  | current :: rest, expected =>
      match packedConflictMembership? current with
      | some (some membership) =>
          membership = expected &&
            match image.runtimeRouteParentMembership? membership.1, rest with
            | some none, [] => true
            | some (some parent), _ :: _ =>
                image.routeCommitChainMatches rest parent
            | _, _ => false
      | _ => false

private def RustDescriptorImage.routeScopeIsAncestorOrSelf
    (image : RustDescriptorImage) (ancestor descendant : Nat) : Nat → Bool
  | 0 => ancestor = descendant
  | fuel + 1 =>
      if ancestor = descendant then
        true
      else
        match image.routeParentMembership? descendant with
        | some (some (parent, _)) =>
            image.routeScopeIsAncestorOrSelf ancestor parent fuel
        | some none | none => false

/-- The event conflict column and route-arm range column must identify the same
innermost route membership for every resident event. The exact conflict key is
the operation authority; every containing route range must be that scope or an
ancestor recovered from the route-parent column. No range-length heuristic may
silently override the exact key. -/
def RustDescriptorImage.routeArmColumnsCoherent
    (image : RustDescriptorImage) : Bool :=
  (List.range image.eventCount).all fun eventId =>
    match image.eventConflictMembership? eventId,
        image.decodedRouteMembershipsAt? eventId with
    | some none, some memberships => memberships.isEmpty
    | some (some membership), some memberships =>
        memberships.contains membership &&
        memberships.all fun candidate =>
          image.routeScopeIsAncestorOrSelf
            candidate.1 membership.1 (image.routeScopeCount + 1)
    | _, _ => false

def RustDescriptorImage.decodeResidentBoundaries?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.residentBoundaryCount).mapM fun row =>
    readU16LE? image.roleBytes
      (image.roleResidentBoundaryOffset + row * productionRoleU16Stride)

def RustDescriptorImage.decodeLaneBits?
    (image : RustDescriptorImage) : Option (List Nat) :=
  (List.range image.laneBitCount).mapM fun row =>
    readByte? image.roleBytes (image.roleLaneBitOffset + row)

private def laneBitPresent (bytes : List Nat) (lane : Nat) : Bool :=
  match bytes[lane / 8]? with
  | none => false
  | some byte => (byte / (2 ^ (lane % 8))) % 2 == 1

private def activeLanesFromBytes (bytes : List Nat) : List Nat :=
  (List.range (bytes.length * 8)).filter (laneBitPresent bytes)

/-- The active bitmap is the sole authority for active-lane count, first lane,
and the exact logical span. Empty and trailing-zero aliases are rejected, so
metadata cannot independently resize endpoint storage. -/
def RustDescriptorImage.activeLaneMetadataCoherent
    (image : RustDescriptorImage) : Bool :=
  match image.decodeLaneBits? with
  | none => false
  | some laneBits =>
      let activeBytes :=
        (laneBits.drop image.activeLaneStart).take image.activeLaneLength
      let activeLanes := activeLanesFromBytes activeBytes
      let expectedLogical := match activeLanes.getLast? with
        | none => 1
        | some lane => lane + 1
      let expectedFirst := match activeLanes.head? with
        | none => packedU16Absent
        | some lane => lane
      let expectedByteLength :=
        if activeLanes.isEmpty then 0 else (expectedLogical + 7) / 8
      decide (
        image.activeLaneStart = 0 ∧
        image.activeLaneStart + image.activeLaneLength ≤ laneBits.length ∧
        image.activeLaneLength = expectedByteLength ∧
        image.activeLaneCount = activeLanes.length ∧
        image.firstActiveLane = expectedFirst ∧
        image.logicalLaneCount = expectedLogical ∧
        image.endpointLaneSlotCount = expectedLogical)

private def residentBoundariesValid
    (eventCount : Nat) (boundaries : List Nat) : Bool :=
  if boundaries.isEmpty then true
  else decide (
    2 ≤ boundaries.length ∧
    boundaries.head? = some 0 ∧
    boundaries.getLast? = some eventCount ∧
    boundaries.Pairwise (· < ·))

private def RustDescriptorImage.decodeLaneRangeAt?
    (image : RustDescriptorImage)
    (offset row count capacity : Nat)
    (nonempty : Bool) : Option (Nat × Nat) := do
  if row < count then pure () else none
  let raw ← readU32LE? image.roleBytes
    (offset + row * productionRoleLaneRangeStride)
  let (start, length) ← decodePackedLaneRange? raw capacity
  if (length = 0 → start = 0) ∧ (!nonempty || 0 < length) then
    some (start, length)
  else
    none

def RustDescriptorImage.decodeRouteArmLaneRows?
    (image : RustDescriptorImage) : Option (List (Nat × Nat)) :=
  (List.range (image.routeScopeCount * 2)).mapM fun row =>
    image.decodeLaneRangeAt? image.roleRouteArmLaneRowOffset row
      (image.routeScopeCount * 2) image.laneBitCount false

def RustDescriptorImage.decodeRouteOfferLaneRows?
    (image : RustDescriptorImage) : Option (List (Nat × Nat)) :=
  (List.range image.routeScopeCount).mapM fun row =>
    image.decodeLaneRangeAt? image.roleRouteOfferLaneRowOffset row
      image.routeScopeCount image.laneBitCount false

def RustDescriptorImage.decodeRouteCommitRanges?
    (image : RustDescriptorImage) : Option (List (Nat × Nat)) :=
  (List.range (image.routeScopeCount * 2)).mapM fun row =>
    image.decodeLaneRangeAt? image.roleRouteCommitRangeOffset row
      (image.routeScopeCount * 2) image.routeCommitRowCount true

/-- Every route-commit range is the exact ancestor-first chain consumed by the
Rust runtime: reversing the bytes starts at the selected route arm, follows the
single parent column, and ends exactly at a root. Truncation, duplication,
foreign arms, and rows before the root are rejected. -/
def RustDescriptorImage.routeCommitColumnsCoherent
    (image : RustDescriptorImage) : Bool :=
  match image.decodeRouteScopeRows?, image.decodeRouteCommitRanges?,
      image.decodeRouteCommitRows? with
  | some scopes, some ranges, some rows =>
      (List.range ranges.length).all fun rowIndex =>
        match scopes[rowIndex / 2]?, ranges[rowIndex]? with
        | some scope, some (start, length) =>
            image.routeCommitChainMatches
              ((rows.drop start).take length).reverse
              (scope, rowIndex % 2)
        | _, _ => false
  | _, _, _ => false

def RustDescriptorImage.routeCommitCapacityExact
    (image : RustDescriptorImage) : Bool :=
  match image.decodeRouteCommitRanges? with
  | none => false
  | some [] =>
      image.routeCommitRowCount = 0 && image.maxRouteCommitCount = 0
  | some ranges@(_ :: _) =>
      decide (image.maxRouteCommitCount =
        ranges.foldl (fun largest range => max largest range.2) 0)

/-- Empty route authority has one representation: no rows and no retained
builder capacity. This matches the production constructor and prevents metadata
from reserving runtime state for a route that does not exist. -/
theorem route_commit_capacity_empty_is_exact
    {image : RustDescriptorImage}
    (decoded : image.decodeRouteCommitRanges? = some [])
    (accepted : image.routeCommitCapacityExact = true) :
    image.routeCommitRowCount = 0 ∧ image.maxRouteCommitCount = 0 := by
  simp [RustDescriptorImage.routeCommitCapacityExact, decoded] at accepted
  exact accepted

private def rangePartitionFrom
    (expected capacity : Nat) : List (Nat × Nat) -> Bool
  | [] => expected = capacity
  | (start, length) :: rest =>
      start = expected && rangePartitionFrom (start + length) capacity rest

private def RustDescriptorImage.decodeRouteArmLaneStep?
    (image : RustDescriptorImage) (row : Nat) : Option DecodedRouteArmLaneStep := do
  if row < image.routeArmLaneStepCount then pure () else none
  let offset := image.roleRouteArmLaneStepOffset +
    row * productionRoleRouteArmLaneStepStride
  let lane ← readByte? image.roleBytes offset
  let firstStep ← readU16LE? image.roleBytes (offset + 1)
  let lastStep ← readU16LE? image.roleBytes (offset + 3)
  if lane < image.logicalLaneCount ∧
      firstStep ≤ lastStep ∧
      lastStep < image.eventCount then
    pure { lane, firstStep, lastStep }
  else
    none

def RustDescriptorImage.decodeRouteArmLaneSteps?
    (image : RustDescriptorImage) : Option (List DecodedRouteArmLaneStep) :=
  (List.range image.routeArmLaneStepCount).mapM image.decodeRouteArmLaneStep?

private def laneBitmapBytes
    (laneBits : List Nat) (row : Nat × Nat) : List Nat :=
  (laneBits.drop row.1).take row.2

private def laneBitmapRowCanonical
    (laneBits activeBytes activeLanes : List Nat)
    (row : Nat × Nat) : Bool :=
  let rowBytes := laneBitmapBytes laneBits row
  if row.2 = 0 then
    decide (row.1 = 0)
  else
    decide (
      row.1 + row.2 ≤ laneBits.length ∧
      row.2 ≤ activeBytes.length ∧
      rowBytes.getLast? ≠ some 0) &&
    (activeLanesFromBytes rowBytes).all activeLanes.contains

private def laneBitmapMatchesStepLanes
    (logicalLaneCount : Nat)
    (laneBits : List Nat)
    (row : Nat × Nat)
    (steps : List DecodedRouteArmLaneStep) : Bool :=
  let rowBytes := laneBitmapBytes laneBits row
  let stepLanes := steps.map DecodedRouteArmLaneStep.lane
  (List.range logicalLaneCount).all fun lane =>
    laneBitPresent rowBytes lane == stepLanes.contains lane

private def routeArmEventIndices (arm : DecodedRouteArmRow) : List Nat :=
  (List.range arm.eventLength).map (arm.eventStart + ·)

private def routeArmLaneEventIndices
    (eventLanes : List Nat)
    (arm : DecodedRouteArmRow)
    (lane : Nat) : List Nat :=
  (routeArmEventIndices arm).filter fun eventId => eventLanes[eventId]? = some lane

private def routeArmLaneStepExact
    (eventLanes : List Nat)
    (arm : DecodedRouteArmRow)
    (step : DecodedRouteArmLaneStep) : Bool :=
  let indices := routeArmLaneEventIndices eventLanes arm step.lane
  indices.head? = some step.firstStep && indices.getLast? = some step.lastStep

private def routeArmLaneRelationsExactFor
    (eventLanes : List Nat)
    (steps : List DecodedRouteArmLaneStep)
    (arm : DecodedRouteArmRow) : Bool :=
  let armSteps := (steps.drop arm.laneStepStart).take arm.laneStepLength
  let stepLanes := armSteps.map DecodedRouteArmLaneStep.lane
  decide stepLanes.Nodup &&
    armSteps.all (routeArmLaneStepExact eventLanes arm) &&
      (routeArmEventIndices arm).all fun eventId =>
        match eventLanes[eventId]? with
        | some lane => stepLanes.contains lane
        | none => false

/-- Every lane relation names the exact first and last event of that lane in
its own arm, and every arm event is covered by one unique relation. -/
def RustDescriptorImage.routeArmLaneRelationsExact
    (image : RustDescriptorImage) : Bool :=
  match (List.range image.eventCount).mapM image.eventLane?,
      image.decodeRouteArmRows?, image.decodeRouteArmLaneSteps? with
  | some eventLanes, some arms, some steps =>
      rangePartitionFrom 0 image.routeArmLaneStepCount
          (arms.map fun arm => (arm.laneStepStart, arm.laneStepLength)) &&
        arms.all (routeArmLaneRelationsExactFor eventLanes steps)
  | _, _, _ => false

private def armLaneColumnsCoherent
    (logicalLaneCount : Nat)
    (laneBits activeBytes activeLanes : List Nat)
    (steps : List DecodedRouteArmLaneStep) :
    List (Nat × Nat) → List DecodedRouteArmRow → Bool
  | [], [] => true
  | row :: rows, arm :: arms =>
      laneBitmapRowCanonical laneBits activeBytes activeLanes row &&
      laneBitmapMatchesStepLanes logicalLaneCount laneBits row
        ((steps.drop arm.laneStepStart).take arm.laneStepLength) &&
      armLaneColumnsCoherent logicalLaneCount laneBits activeBytes activeLanes
        steps rows arms
  | _, _ => false

private def laneBitmapUnionExact
    (logicalLaneCount : Nat)
    (laneBits : List Nat)
    (left right offer : Nat × Nat) : Bool :=
  let leftBytes := laneBitmapBytes laneBits left
  let rightBytes := laneBitmapBytes laneBits right
  let offerBytes := laneBitmapBytes laneBits offer
  (List.range logicalLaneCount).all fun lane =>
    laneBitPresent offerBytes lane ==
      (laneBitPresent leftBytes lane || laneBitPresent rightBytes lane)

private def offerLaneColumnsCoherent
    (logicalLaneCount : Nat)
    (laneBits activeBytes activeLanes : List Nat) :
    List (Nat × Nat) → List (Nat × Nat) → Bool
  | left :: right :: armRows, offer :: offers =>
      laneBitmapRowCanonical laneBits activeBytes activeLanes offer &&
      laneBitmapUnionExact logicalLaneCount laneBits left right offer &&
      offerLaneColumnsCoherent logicalLaneCount laneBits activeBytes activeLanes
        armRows offers
  | [], [] => true
  | _, _ => false

private def optionalRangePartitionFrom
    (expected capacity : Nat) : List (Nat × Nat) → Bool
  | [] => expected = capacity
  | (start, length) :: rest =>
      if length = 0 then
        start = 0 && optionalRangePartitionFrom expected capacity rest
      else
        start = expected &&
          optionalRangePartitionFrom (start + length) capacity rest

private def routeLaneRowsInEmissionOrder :
    List (Nat × Nat) → List (Nat × Nat) → Option (List (Nat × Nat))
  | left :: right :: armRows, offer :: offers => do
      let rest ← routeLaneRowsInEmissionOrder armRows offers
      pure (left :: right :: offer :: rest)
  | [], [] => some []
  | _, _ => none

private def RustDescriptorImage.residentEventLanesMatchActive
    (image : RustDescriptorImage) : Bool :=
  image.eventLaneColumnCoherent &&
  match image.decodeLaneBits?,
      (List.range image.eventCount).mapM image.eventLane? with
  | some laneBits, some eventLanes =>
      let activeBytes :=
        (laneBits.drop image.activeLaneStart).take image.activeLaneLength
      (List.range image.logicalLaneCount).all fun lane =>
        laneBitPresent activeBytes lane == eventLanes.contains lane
  | _, _ => false

/-- Every lane column carries one exact fact. Active lanes own the prefix;
route rows follow in `(left, right, union)` order; arm bitmaps equal their
lane-step lanes; and no bitmap or lane-step bytes remain unowned. -/
def RustDescriptorImage.laneColumnsCoherent
    (image : RustDescriptorImage) : Bool :=
  image.activeLaneMetadataCoherent &&
  image.residentEventLanesMatchActive &&
  image.routeArmLaneRelationsExact &&
  match image.decodeLaneBits?, image.decodeRouteArmLaneRows?,
      image.decodeRouteOfferLaneRows?, image.decodeRouteArmRows?,
      image.decodeRouteArmLaneSteps? with
  | some laneBits, some armRows, some offerRows, some arms, some steps =>
      let activeBytes :=
        (laneBits.drop image.activeLaneStart).take image.activeLaneLength
      let activeLanes := activeLanesFromBytes activeBytes
      match routeLaneRowsInEmissionOrder armRows offerRows with
      | some emittedRows =>
          optionalRangePartitionFrom
            (image.activeLaneStart + image.activeLaneLength)
            laneBits.length emittedRows &&
          armLaneColumnsCoherent image.logicalLaneCount laneBits activeBytes
            activeLanes steps armRows arms &&
          offerLaneColumnsCoherent image.logicalLaneCount laneBits activeBytes
            activeLanes armRows offerRows
      | none => false
  | _, _, _, _, _ => false

theorem lane_columns_coherent_binds_active_lane_metadata
    {image : RustDescriptorImage}
    (accepted : image.laneColumnsCoherent = true) :
    image.activeLaneMetadataCoherent = true := by
  unfold RustDescriptorImage.laneColumnsCoherent at accepted
  simp only [Bool.and_eq_true] at accepted
  exact accepted.1.1.1

theorem lane_columns_coherent_binds_event_lane_column
    {image : RustDescriptorImage}
    (accepted : image.laneColumnsCoherent = true) :
    image.eventLaneColumnCoherent = true := by
  unfold RustDescriptorImage.laneColumnsCoherent at accepted
  simp only [Bool.and_eq_true] at accepted
  have eventLanes := accepted.1.1.2
  unfold RustDescriptorImage.residentEventLanesMatchActive at eventLanes
  simp only [Bool.and_eq_true] at eventLanes
  exact eventLanes.1

theorem lane_columns_coherent_binds_exact_route_arm_lane_relations
    {image : RustDescriptorImage}
    (accepted : image.laneColumnsCoherent = true) :
    image.routeArmLaneRelationsExact = true := by
  unfold RustDescriptorImage.laneColumnsCoherent at accepted
  simp only [Bool.and_eq_true] at accepted
  exact accepted.1.2

private def RustDescriptorImage.roleRuntimeColumnsCheck
    (image : RustDescriptorImage) : Bool :=
  match image.decodeRoleEventRows?,
      image.decodeDependencyRows?,
      (List.range image.conflictRowCount).mapM image.conflictRow?,
      (List.range image.routeScopeCount).mapM image.routeScopeConflictRow?,
      image.decodeRouteArmRows?,
      image.decodeResidentBoundaries?,
      (List.range (image.routeScopeCount * 2)).mapM fun row =>
        image.decodeLaneRangeAt? image.roleRouteArmLaneRowOffset row
          (image.routeScopeCount * 2) image.laneBitCount false,
      (List.range image.routeScopeCount).mapM fun row =>
        image.decodeLaneRangeAt? image.roleRouteOfferLaneRowOffset row
          image.routeScopeCount image.laneBitCount false,
      (List.range image.routeArmLaneStepCount).mapM image.decodeRouteArmLaneStep?,
      (List.range (image.routeScopeCount * 2)).mapM fun row =>
        image.decodeLaneRangeAt? image.roleRouteCommitRangeOffset row
          (image.routeScopeCount * 2) image.routeCommitRowCount true,
      (List.range image.routeCommitRowCount).mapM image.routeCommitRow? with
  | some events, some _, some _, some _, some _, some boundaries,
      some _, some _, some _, some commitRanges, some _ =>
      events.all (fun event =>
        (event.dependencyRow = packedU16Absent ||
          (image.decodeDependencyRow? event.dependencyRow).isSome) &&
        (event.conflictRow = packedU16Absent ||
          (image.conflictRow? event.conflictRow).isSome)) &&
      residentBoundariesValid image.eventCount boundaries &&
      image.laneColumnsCoherent &&
      rangePartitionFrom 0 image.routeCommitRowCount commitRanges &&
      image.routeArmColumnsCoherent &&
      image.passiveChildColumnsCoherent &&
      image.routeCommitColumnsCoherent &&
      image.routeCommitCapacityExact &&
      image.rollColumnsCoherent
  | _, _, _, _, _, _, _, _, _, _, _ => false

theorem role_runtime_columns_check_binds_route_arm_columns
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.routeArmColumnsCoherent = true := by
  unfold RustDescriptorImage.roleRuntimeColumnsCheck at accepted
  split at accepted <;> simp_all

theorem role_runtime_columns_check_binds_lane_columns
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.laneColumnsCoherent = true := by
  unfold RustDescriptorImage.roleRuntimeColumnsCheck at accepted
  split at accepted <;> simp_all

theorem role_runtime_columns_check_binds_event_lane_column
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.eventLaneColumnCoherent = true := by
  exact lane_columns_coherent_binds_event_lane_column
    (role_runtime_columns_check_binds_lane_columns accepted)

theorem role_runtime_columns_check_binds_active_lane_metadata
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.activeLaneMetadataCoherent = true := by
  exact lane_columns_coherent_binds_active_lane_metadata
    (role_runtime_columns_check_binds_lane_columns accepted)

theorem role_runtime_columns_check_binds_exact_route_arm_lane_relations
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.routeArmLaneRelationsExact = true := by
  exact lane_columns_coherent_binds_exact_route_arm_lane_relations
    (role_runtime_columns_check_binds_lane_columns accepted)

theorem role_runtime_columns_check_binds_passive_child_columns
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.passiveChildColumnsCoherent = true := by
  unfold RustDescriptorImage.roleRuntimeColumnsCheck at accepted
  split at accepted <;> simp_all

theorem role_runtime_columns_check_binds_route_commit_columns
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.routeCommitColumnsCoherent = true := by
  unfold RustDescriptorImage.roleRuntimeColumnsCheck at accepted
  split at accepted <;> simp_all

theorem role_runtime_columns_check_binds_exact_route_commit_capacity
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.routeCommitCapacityExact = true := by
  unfold RustDescriptorImage.roleRuntimeColumnsCheck at accepted
  split at accepted <;> simp_all

theorem role_runtime_columns_check_binds_roll_columns
    {image : RustDescriptorImage}
    (accepted : image.roleRuntimeColumnsCheck = true) :
    image.rollColumnsCoherent = true := by
  unfold RustDescriptorImage.roleRuntimeColumnsCheck at accepted
  split at accepted <;> simp_all

def RustDescriptorImage.decodeProjectionShape?
    (image : RustDescriptorImage) : Option ProjectionShape := do
  let actions ← image.decodeActions?
  let routes ← image.decodeRoutes?
  let rolls ← image.decodeRolls?
  pure {
    events := actions.map fun action => { action }
    rolls
    routes
  }

private theorem projection_event_action_round_trip (actions : List LocalAction) :
    (actions.map fun action => ({ action } : ProjectionEvent)).map
      ProjectionEvent.action = actions := by
  induction actions with
  | nil => rfl
  | cons head tail inductionHypothesis =>
      simp only [List.map_cons]
      rw [inductionHypothesis]

theorem decoded_projection_shape_binds_action_column
    {image : RustDescriptorImage} {shape : ProjectionShape}
    (decoded : image.decodeProjectionShape? = some shape) :
    image.decodeActions? = some (shape.events.map ProjectionEvent.action) := by
  unfold RustDescriptorImage.decodeProjectionShape? at decoded
  cases actionsCase : image.decodeActions? with
  | none => simp [actionsCase] at decoded
  | some actions =>
      cases routesCase : image.decodeRoutes? with
      | none => simp [actionsCase, routesCase] at decoded
      | some routes =>
          cases rollsCase : image.decodeRolls? with
          | none => simp [actionsCase, routesCase, rollsCase] at decoded
          | some rolls =>
              have shapeEq :
                  ({
                    events := actions.map fun action => { action }
                    rolls
                    routes
                  } : ProjectionShape) = shape := by
                exact Option.some.inj (by
                  simpa [actionsCase, routesCase, rollsCase] using decoded)
              subst shape
              exact congrArg some (projection_event_action_round_trip actions).symm

def RustDescriptorImage.atomEffIndicesStrict
    (image : RustDescriptorImage) : Bool :=
  match image.decodeAtoms? with
  | none => false
  | some atoms =>
      decide ((atoms.map DecodedProgramAtom.effIndex).Pairwise
        (fun left right => left < right))

private def decodedResolverScopesUnique (image : RustDescriptorImage) : Bool :=
  match image.decodeRouteResolvers? with
  | none => false
  | some resolvers => decide (resolvers.map DecodedRouteResolver.scope).Nodup

private def decodedRouteScopesUnique (image : RustDescriptorImage) : Bool :=
  match (List.range image.routeScopeCount).mapM image.decodeRouteScope? with
  | none => false
  | some scopes => decide scopes.Nodup

def RustDescriptorImage.check (image : RustDescriptorImage) : Bool :=
  decide (
    0 < image.roleCount ∧
    image.roleCount ≤ 256 ∧
    image.role < image.roleCount ∧
    0 < image.logicalLaneCount ∧
    image.logicalLaneCount ≤ 256 ∧
    image.routeParticipantCount ≤ packedU16Absent ∧
    (image.routeResolverCount = 0 ↔ image.routeParticipantCount = 0) ∧
    image.programBytes.length = image.programBlobLen ∧
    image.roleBytes.length = image.roleBlobLen ∧
    image.programBytes.all (· < 256) ∧
    image.roleBytes.all (· < 256)) &&
  image.atomEffIndicesStrict &&
  decodedResolverScopesUnique image &&
  decodedRouteScopesUnique image &&
  image.decodeScopeMarkers?.isSome &&
  image.roleRuntimeColumnsCheck &&
  image.decodeProjectionShape?.isSome

theorem checked_descriptor_binds_route_arm_columns
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.routeArmColumnsCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_route_arm_columns accepted.1.2

theorem checked_descriptor_binds_event_lane_column
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.eventLaneColumnCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_event_lane_column accepted.1.2

theorem checked_descriptor_binds_lane_columns
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.laneColumnsCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_lane_columns accepted.1.2

theorem checked_descriptor_binds_active_lane_metadata
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.activeLaneMetadataCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_active_lane_metadata accepted.1.2

theorem checked_descriptor_binds_exact_route_arm_lane_relations
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.routeArmLaneRelationsExact = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_exact_route_arm_lane_relations accepted.1.2

theorem checked_descriptor_binds_strict_atom_order
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.atomEffIndicesStrict = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact accepted.1.1.1.1.1.2

theorem checked_descriptor_binds_passive_child_columns
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.passiveChildColumnsCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_passive_child_columns accepted.1.2

theorem checked_descriptor_binds_route_commit_columns
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.routeCommitColumnsCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_route_commit_columns accepted.1.2

theorem checked_descriptor_binds_exact_route_commit_capacity
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.routeCommitCapacityExact = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_exact_route_commit_capacity accepted.1.2

theorem checked_descriptor_binds_roll_columns
    {image : RustDescriptorImage}
    (accepted : image.check = true) :
    image.rollColumnsCoherent = true := by
  unfold RustDescriptorImage.check at accepted
  simp only [Bool.and_eq_true] at accepted
  exact role_runtime_columns_check_binds_roll_columns accepted.1.2

end Hibana
