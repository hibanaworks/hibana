import Hibana.SourceOccurrenceTrace

open Hibana Hibana.EndpointDelegation
set_option maxRecDepth 100000

structure Attempt where
  commands : List Nat
  /-- 0=accepted, 1=pending, 2=Endpoint error, 3=no owned Rust value to call. -/
  outcome : Nat
  runtime : String

structure Probe where
  operation : Nat
  actor : Nat
  deriving BEq, Inhabited

structure ConformanceCase where
  name : String
  choreo : Choreo
  images : List RustDescriptorImage
  probes : List Probe
  attempts : List Attempt

inductive Oracle where
  | source
  | serialized
  | unguarded
  | unprojected
  | unheld
  deriving BEq

def checkedPlans (source expanded : Source) (unfolding : SourceUnfolding source expanded) :
    List (SourceOccurrenceCut source) :=
  match originalAt : normalize source, expandedAt : normalize expanded with
  | some original, some normalized =>
    if same : normalized.issuance = original.issuance then
      normalized.paths.attach.map fun path => {
        original, originalAccepted := originalAt, expanded, unfolding,
        normalized, accepted := expandedAt, sameIssuance := same,
        path := path.val, selected := path.property, observed := fun _ => false }
    else []
  | _, _ => []

/-- Only bounded instances of the established source unfolding are used.
The serialized mutant deliberately omits the interleaved continuation case. -/
def boundedPlans (source : Source) (oracle : Oracle) : List (SourceOccurrenceCut source) :=
  match source with
  | .seq (.roll body) tail =>
    let first := SourceUnfolding.seq (.rollOnce (.oneCopy body)) (.oneCopy tail)
    let before := SourceUnfolding.seq
      (.rollMore (.rollOnce (.oneCopy body)) (.oneCopy body)) (.oneCopy tail)
    let after := SourceUnfolding.resumeAfterContinuation first (.oneCopy body)
    let alongside := SourceUnfolding.resumeAlongsideContinuation
      (.rollOnce (.oneCopy body)) (.rollOnce (.oneCopy body)) (.oneCopy tail)
    checkedPlans _ _ first ++ checkedPlans _ _ before ++ checkedPlans _ _ after ++
      if oracle == .serialized then [] else checkedPlans _ _ alongside
  | .par (.roll body) tail =>
    checkedPlans _ _ (.par (.rollOnce (.oneCopy body)) (.oneCopy tail)) ++
      checkedPlans _ _ (.par (.rollMore (.rollOnce (.oneCopy body)) (.oneCopy body)) (.oneCopy tail))
  | _ => []

/-- Bind both physical holders' immutable rows to the original logical
message. Delegated roles retain their logical wire identity; the accepted
source unfolding determines which actor's image must contain each half. -/
def channelFor (plan : SourceOccurrenceCut source) (images : List RustDescriptorImage)
    (occurrence : Nat) : Option (Nat × Nat × Nat) := do
  let (origin, message) ← (plan.unfolding.occurrences 0)[occurrence]?
  let (_, binding) ← plan.path.bindings.find? (fun entry => entry.1 == occurrence)
  let image ← resolveUnique? (images.filter (fun image => image.role == binding.senderActor))
  let row ← resolveUnique? ((List.range image.eventCount).filter fun row =>
    image.eventEffIndex? row == some origin &&
      image.eventView? row == some message.sender.role)
  let lane ← image.eventLane? row
  let receiver ← resolveUnique? (images.filter (fun image => image.role == binding.receiverActor))
  let incoming ← resolveUnique? ((List.range receiver.eventCount).filter fun row =>
    receiver.eventEffIndex? row == some origin &&
      receiver.eventView? row == some message.receiver.role)
  let receiveLane ← receiver.eventLane? incoming
  if lane != receiveLane then none else
  pure (message.sender.role, message.receiver.role, lane)

def indexedPlans (plans : List (SourceOccurrenceCut source)) (images : List RustDescriptorImage) :
    Option (List (SourceOccurrenceCut source × Array (Nat × Nat × Nat))) :=
  plans.mapM fun plan => do
    let channels ← (List.range plan.expanded.eventCount).mapM (channelFor plan images)
    pure (plan, channels.toArray)

/-- These channels are immutable translations of the accepted source and
descriptor rows, computed once. No observed admission result is cached. -/
def atFifoHead (channels : Array (Nat × Nat × Nat))
    (points : List SourcePoint) (occurrence : Nat) : Bool :=
  match channels[occurrence]? with
  | none => false
  | some channel =>
    points.find? (fun point => match point with
      | .accepted event => channels[event]? == some channel &&
          !points.contains (.committed ⟨event, .receive⟩)
      | _ => false) == some (.accepted occurrence)

def nextPoints (plan : SourceOccurrenceCut source) (channels : Array (Nat × Nat × Nat)) (oracle : Oracle)
    (points : List SourcePoint) (probe : Probe) : List (List SourcePoint) :=
  (plan.unfolding.occurrences 0).zipIdx.filterMap fun ((origin, _), occurrence) =>
    if origin != probe.operation / 2 then none else
    let side := if probe.operation % 2 == 0 then Side.send else Side.receive
    let binding := plan.path.bindings.find? (fun entry => entry.1 == occurrence)
    let caller := binding.map fun (_, binding) =>
      if side == .send then binding.senderActor else binding.receiverActor
    if oracle != .unheld && caller != some probe.actor then none else
    let action : ActionId := ⟨occurrence, side⟩
    let point := action.entryPoint
    let ready := if oracle == .unguarded then
      (sourceVisitPoints plan.path.bindings).contains point && !points.contains point
      else (occurrenceCut plan points).ready point
    if !ready || (oracle != .unguarded && oracle != .unprojected &&
        !(occurrenceCut plan points).resumptionReady point) ||
        (side == .receive && oracle != .unguarded &&
        !atFifoHead channels points occurrence) then none else
    let accepted := points ++ [point]
    if side == .receive then some accepted else
      let publication := SourcePoint.committed action
      if oracle == .unguarded || (occurrenceCut plan accepted).ready publication then
        some (accepted ++ [publication])
      else none

def permits (plans : List (SourceOccurrenceCut source × Array (Nat × Nat × Nat)))
    (oracle : Oracle) (probes : List Probe) (commands : List Nat) : Bool :=
  plans.any fun (plan, channels) =>
    !(commands.foldl (fun states command =>
      match probes[command]? with
      | none => []
      | some probe => states.flatMap fun points => nextPoints plan channels oracle points probe)
      [[]]).isEmpty

/-- All possible holders, including initial issuance before the first transfer.
This comes from accepted source paths, not observed successes or the probe table.
A holder remains a candidate for its view's operations even when that actor has
no legitimate row for the particular operation. -/
def possibleHolders (plans : List (SourceOccurrenceCut source)) (key : EndpointKey) : List Nat :=
  (plans.flatMap fun plan =>
    let initial := (issuer? plan.original.issuance key).getD key.role
    initial :: plan.path.bindings.flatMap fun (_, binding) =>
      (if binding.message.sender == key then [binding.senderActor] else []) ++
      (if binding.message.receiver == key then [binding.receiverActor] else []) ++
      (if binding.message.payload == .endpoint key then
        [binding.senderActor, binding.receiverActor] else [])).eraseDups

def expectedProbes (plans : List (SourceOccurrenceCut source)) : List Probe :=
  (source.messageEntries 0).flatMap fun (origin, message) =>
    ((possibleHolders plans message.sender).map fun actor => ⟨2 * origin, actor⟩) ++
    ((possibleHolders plans message.receiver).map fun actor => ⟨2 * origin + 1, actor⟩)

def probeInventory (probes : List Probe) : List (Nat × Nat) :=
  (probes.map fun probe => (probe.operation, probe.actor)).mergeSort fun left right =>
    left.1 < right.1 || (left.1 == right.1 && left.2 ≤ right.2)

def completeProbeInventory (plans : List (SourceOccurrenceCut source))
    (probes : List Probe) : Bool :=
  probeInventory probes == probeInventory (expectedProbes plans)

/-- Derive occurrence bounds from the accepted expansions, independently of
the Rust enumeration loop. Every successful bounded prefix must probe every
remaining local operation exactly once. Missing probes cannot make the
conformance result green. -/
def eligibleCommands (plans : List (SourceOccurrenceCut source))
    (probes : List Probe) (commands : List Nat) : List Nat :=
  (List.range probes.length).filter fun command =>
    let probe := probes[command]!
    let capacity := plans.foldl (fun largest plan => max largest
      ((plan.unfolding.occurrences 0).filter fun entry =>
        entry.1 == probe.operation / 2).length) 0
    (commands.filter fun prior => (probes[prior]?.map (·.operation)) ==
      some probe.operation).length < capacity

def testedPrefixes (sample : ConformanceCase) (depth : Nat) : List (List Nat) :=
  ([] :: (sample.attempts.filterMap fun attempt =>
    if attempt.outcome == 0 && attempt.commands.length < depth then
      some attempt.commands else none)).eraseDups

def enumerationComplete (sample : ConformanceCase)
    (plans : List (SourceOccurrenceCut source)) (depth : Nat) : Bool :=
  let prefixes := testedPrefixes sample depth
  sample.attempts.all (fun attempt => !attempt.commands.isEmpty &&
    attempt.commands.length ≤ depth && prefixes.contains attempt.commands.dropLast &&
    attempt.outcome ≤ 3) &&
  prefixes.all fun priorCommands =>
    let candidates := sample.attempts.filter fun attempt => attempt.commands.dropLast == priorCommands
    (candidates.map fun attempt => attempt.commands.getLast!).mergeSort (· ≤ ·) ==
      eligibleCommands plans sample.probes priorCommands

def checkCases (cases : List ConformanceCase) : IO Unit := do
  let mut runtimeOnly := 0
  let mut modelOnly := 0
  let mut stalled := 0
  let mut tooStrict := 0
  let mut tooLoose := 0
  let mut missingProjection := 0
  let mut missingHolder := 0
  let mut checked := 0
  let mut cuts := 0
  for sample in cases do
    let some source := sample.choreo.ownershipSource?
      | throw (IO.userError s!"missing source: {sample.name}")
    let plans := boundedPlans source .source
    unless !plans.isEmpty do throw (IO.userError s!"no accepted bounded plans: {sample.name}")
    unless completeProbeInventory plans sample.probes do
      throw (IO.userError s!"missing or duplicate holder probes: {sample.name}")
    let some firstProbe := sample.probes.head?
      | throw (IO.userError s!"empty probe inventory: {sample.name}")
    if completeProbeInventory plans sample.probes.tail ||
        completeProbeInventory plans (firstProbe :: sample.probes) then
      throw (IO.userError "holder inventory controls missed missing/duplicate callers")
    let actors := (plans.flatMap fun plan => plan.path.bindings.flatMap fun (_, binding) =>
      [binding.senderActor, binding.receiverActor]).eraseDups.mergeSort (· ≤ ·)
    unless (sample.images.map (·.role)).mergeSort (· ≤ ·) == actors &&
        sample.images.all (fun image =>
          image.actorOrderCheck sample.choreo && image.actorRollCheck sample.choreo) do
      throw (IO.userError s!"descriptor/source mismatch: {sample.name}")
    let some indexed := indexedPlans plans sample.images
      | throw (IO.userError s!"unmatched source direction: {sample.name}")
    let some serial := indexedPlans (boundedPlans source .serialized) sample.images
      | throw (IO.userError s!"unmatched serialized direction: {sample.name}")
    let depth := plans.foldl (fun largest plan => max largest
      (sourceVisitActions plan.path.bindings).length) 0
    unless enumerationComplete sample plans depth do
      throw (IO.userError s!"incomplete or duplicate probes: {sample.name}")
    let some head := sample.attempts.head?
      | throw (IO.userError s!"empty observation: {sample.name}")
    if enumerationComplete { sample with attempts := sample.attempts.drop 1 } plans depth ||
        enumerationComplete { sample with attempts := head :: sample.attempts } plans depth then
      throw (IO.userError "enumeration controls failed to detect missing/duplicate probes")
    for priorCommands in testedPrefixes sample depth do
      let candidates := sample.attempts.filter fun attempt => attempt.commands.dropLast == priorCommands
      let ready := candidates.filter fun attempt =>
        permits indexed .source sample.probes attempt.commands
      cuts := cuts + 1
      if !ready.isEmpty && !ready.any (fun attempt => attempt.outcome == 0) then
        stalled := stalled + 1
        IO.println s!"stalled {sample.name}: prefix={priorCommands} ready={ready.map (·.commands.getLast!)}"
    for attempt in sample.attempts do
      let expected := permits indexed .source sample.probes attempt.commands
      let accepted := attempt.outcome == 0
      checked := checked + 1
      if expected != accepted then
        if runtimeOnly + modelOnly < 20 then
          IO.println s!"mismatch {sample.name}: commands={attempt.commands} model={expected} runtime={attempt.runtime}"
        if accepted then runtimeOnly := runtimeOnly + 1 else modelOnly := modelOnly + 1
      if accepted && !permits serial .serialized sample.probes attempt.commands then
        tooStrict := tooStrict + 1
      if !accepted && permits indexed .unguarded sample.probes attempt.commands then
        tooLoose := tooLoose + 1
      if !accepted && permits indexed .unprojected sample.probes attempt.commands then
        missingProjection := missingProjection + 1
      if !accepted && permits indexed .unheld sample.probes attempt.commands then
        missingHolder := missingHolder + 1
  IO.println s!"Endpoint/source conformance: attempts={checked} cuts={cuts} runtime-only={runtimeOnly} model-only={modelOnly} stalled={stalled} serialized-mutant={tooStrict} unguarded-mutant={tooLoose} unprojected-mutant={missingProjection} unheld-mutant={missingHolder}"
  unless tooStrict > 0 && tooLoose > 0 && missingProjection > 0 && missingHolder > 0 do
    throw (IO.userError "conformance controls did not exercise both mismatch directions")
  unless runtimeOnly == 0 && modelOnly == 0 && stalled == 0 do
    throw (IO.userError "Endpoint/source conformance failed")
