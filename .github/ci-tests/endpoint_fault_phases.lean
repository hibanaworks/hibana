-- Bounded, single-fault extension of the same phased source/FIFO oracle.
-- State here is erased test interpretation, never runtime authority.

structure FaultPhaseAttempt where
  before : List Nat
  after : List Nat
  abort : Bool
  accepted : Bool
  actors : List Nat
  events : List PhaseBoundary
  deriving Lean.FromJson

structure FaultPhaseState where
  base : PhaseState := {}
  closed : Option (Nat × Nat × Nat) := none
  isolated : List Nat := []
  faults : List Nat := []
  errors : List (Nat × Nat) := []
  grants : List Nat := []

inductive FaultPhaseOracle where
  | source | closeDrops | abortDrains | broadcast
  deriving BEq

def faultGateCommand? (event : PhaseBoundary) : Option Nat :=
  if (event.kind != 1 && event.kind != 3) || event.operation % 2 != 0 then none else
    (gatedCalls.zipIdx.find? (fun (call, _) => call == (event.operation, event.visit))).map
      fun (_, index) => 2 * index + if event.kind == 1 then 0 else 1

def faultCommandReady (plan : SourceOccurrenceCut source)
    (channels : Array (Nat × Nat × Nat)) (state : FaultPhaseState) (command : Nat) : Bool := Id.run do
  let some point := gatePoint? plan command | return false
  let some (operation, visit) := gatedCalls[command / 2]? | return false
  let some occurrence := phaseOccurrence? plan ⟨0, operation, visit, 0, [], 0⟩ | return false
  let some actor := (action? plan.path.bindings ⟨occurrence, .send⟩).map (·.actor) | return false
  return state.grants.isEmpty && !state.faults.contains actor &&
    !state.errors.contains (operation, visit) && phasePointReady plan channels state.base.points point

def faultReceiveCanFail (plan : SourceOccurrenceCut source)
    (channels : Array (Nat × Nat × Nat)) (state : FaultPhaseState) (occurrence : Nat) : Bool :=
  state.closed.isSome && channels[occurrence]? == state.closed &&
    !state.base.points.contains (.committed ⟨occurrence, .receive⟩) &&
    !(state.base.packets.any fun packet =>
      (phaseOccurrence? plan packet).any fun sent => channels[sent]? == state.closed &&
        !state.isolated.contains sent && !state.base.delivered.contains (packet.operation + 1, packet.visit)) &&
    (sourceVisitPoints plan.path.bindings).all fun prior =>
      !sourcePointDepends plan.expanded plan.path.bindings prior (.committed ⟨occurrence, .receive⟩) ||
        prior == .accepted occurrence || state.base.points.contains prior

def faultPhaseStep (plan : SourceOccurrenceCut source) (images : List RustDescriptorImage)
    (channels : Array (Nat × Nat × Nat)) (oracle : FaultPhaseOracle)
    (state : FaultPhaseState) (event : PhaseBoundary) : Option FaultPhaseState := do
  if event.kind == 4 || event.kind == 5 then
    let channel ← channels[0]?
    let (sender, receiver, lane) := channel
    -- The concrete injector closes a whole port. Verify that this fixed source
    -- has just one logical incoming direction on it, rather than assuming it.
    if state.closed.isSome || event.operation != 0 || event.visit != 0 || event.actor != 0 ||
        event.witness != 0 || event.wire != [sender, receiver, lane] ||
        !(channels.toList.all fun (otherSender, otherReceiver, otherLane) =>
          otherReceiver != receiver || otherLane != lane || otherSender == sender) then none else
      let discard := if oracle == .closeDrops then true else
        if oracle == .abortDrains then false else event.kind == 5
      let isolated := if discard then state.base.packets.filterMap fun packet => do
        let occurrence ← phaseOccurrence? plan packet
        if channels[occurrence]? == some channel &&
            !state.base.delivered.contains (packet.operation + 1, packet.visit) then some occurrence else none
        else []
      some { state with closed := some channel, isolated }
  else if event.kind == 8 then
    if event.actor != 0 || event.visit != 0 || !event.wire.isEmpty || event.witness != 0 ||
        !faultCommandReady plan channels state event.operation then none else
      some { state with grants := [event.operation] }
  else
    let occurrence ← phaseOccurrence? plan event
    let (_, binding) ← plan.path.bindings.find? (fun entry => entry.1 == occurrence)
    let side := if event.operation % 2 == 0 then Side.send else Side.receive
    let actor := if side == .send then binding.senderActor else binding.receiverActor
    let key := (event.operation, event.visit)
    if event.actor != actor then none else pure ()
    if event.kind == 6 then
      if !state.base.started.contains key || state.faults.contains actor ||
          !event.wire.isEmpty || event.witness != 0 then none else pure ()
      if side == .send then
        let command ← (gatedCalls.zipIdx.find? (fun (call, _) => call == key)).map (fun (_, i) => 2 * i)
        if state.grants != [command] || state.closed.isNone || channels[occurrence]? != state.closed ||
            !phasePointReady plan channels state.base.points (.accepted occurrence) then none else pure ()
      else if !faultReceiveCanFail plan channels state occurrence then none else pure ()
      some { state with
        faults := if oracle == .broadcast then [0, 1, 2] else actor :: state.faults
        grants := if side == .send then [] else state.grants }
    else if event.kind == 7 then
      if !state.base.started.contains key || !state.faults.contains actor || state.errors.contains key ||
          !event.wire.isEmpty || event.witness != 0 ||
          state.base.points.contains (.committed ⟨occurrence, side⟩) then none else
        some { state with errors := key :: state.errors, grants := state.grants.filter fun command =>
          !((gatePoint? plan command).bind (fun point => match point with
            | .accepted event | .committed ⟨event, .send⟩ =>
                (action? plan.path.bindings ⟨event, .send⟩).map (·.actor)
            | _ => none)).any (· == actor) }
    else
      if event.kind != 0 && state.faults.contains actor then none else pure ()
      if event.kind == 1 && state.closed.isSome && channels[occurrence]? == state.closed then none else pure ()
      if event.kind == 2 && state.isolated.contains occurrence then none else pure ()
      let grant ← match faultGateCommand? event with
        | none => some state.grants
        | some command => if state.grants == [command] then some [] else none
      let base ← phaseStep plan images channels 9076 .source state.base event
      some { state with base, grants := grant }

def faultPhaseRun (plan : SourceOccurrenceCut source) (images : List RustDescriptorImage)
    (channels : Array (Nat × Nat × Nat)) (oracle : FaultPhaseOracle)
    (events : List PhaseBoundary) : Option FaultPhaseState :=
  events.foldlM (faultPhaseStep plan images channels oracle) {}

def phasePointActor? (plan : SourceOccurrenceCut source) (point : SourcePoint) : Option Nat :=
  match point with
  | .accepted event => (action? plan.path.bindings ⟨event, .send⟩).map (·.actor)
  | .committed action => (action? plan.path.bindings action).map (·.actor)

/-- All non-gated work in these localsides is driven to quiescence by Tokio.
This rejects omitting an enabled automatic operation or a ready EOF, rather
than predicting availability from an incomplete observed trace alone. -/
def faultPhaseQuiescent (plan : SourceOccurrenceCut source) (channels : Array (Nat × Nat × Nat))
    (state : FaultPhaseState) (actors : List Nat) : Bool :=
  state.grants.isEmpty && actors.length == 3 &&
    (state.base.started.all fun (operation, visit) => Id.run do
      let some occurrence := phaseOccurrence? plan ⟨0, operation, visit, 0, [], 0⟩ | return false
      let action : ActionId := ⟨occurrence, if operation % 2 == 0 then .send else .receive⟩
      let some actor := (action? plan.path.bindings action).map (·.actor) | return false
      return !state.faults.contains actor || state.errors.contains (operation, visit) ||
        state.base.points.contains (.committed action)) &&
    ((List.range 3).all fun actor =>
      let done := (sourceVisitPoints plan.path.bindings).all fun point =>
        phasePointActor? plan point != some actor || state.base.points.contains point
      actors[actor]? == some (if state.faults.contains actor then 2 else if done then 1 else 0)) &&
    ((sourceVisitPoints plan.path.bindings).all fun point =>
      match phasePointActor? plan point with
      | none => false
      | some actor =>
        state.faults.contains actor ||
          ((ioCommands.filterMap (gatePoint? plan)).contains point ||
            !phasePointReady plan channels state.base.points point) &&
          (match point with
           | .committed ⟨event, .receive⟩ => !faultReceiveCanFail plan channels state event
           | _ => true))

def faultAttemptKey (attempt : FaultPhaseAttempt) : List Nat :=
  attempt.before ++ [if attempt.abort then 9 else 8] ++ attempt.after

def faultPhaseEnumerationComplete (normal : List PhaseAttempt) (attempts : List FaultPhaseAttempt) : Bool :=
  let before := ([] :: normal.filterMap (fun attempt => if attempt.accepted then some attempt.commands else none)).eraseDups
  let roots := before.flatMap fun commands => [commands ++ [8], commands ++ [9]]
  let children := attempts.flatMap fun attempt =>
    if attempt.accepted && attempt.before.length + attempt.after.length < ioCommands.length then
      ioCommands.map fun command => faultAttemptKey attempt ++ [command] else []
  (attempts.map faultAttemptKey).mergeSort (fun a b => compare a b != .gt) ==
    (roots ++ children).mergeSort (fun a b => compare a b != .gt)

def checkFaultPhases (choreo : Choreo) (images : List RustDescriptorImage)
    (normal : List PhaseAttempt) (attempts : List FaultPhaseAttempt) : IO Unit := do
  unless phaseEnumerationComplete normal && faultPhaseEnumerationComplete normal attempts do
    throw (IO.userError "missing/duplicate fault-phased probes")
  let some first := attempts.head? | throw (IO.userError "empty fault observations")
  if faultPhaseEnumerationComplete normal attempts.tail || faultPhaseEnumerationComplete normal (first :: attempts) then
    throw (IO.userError "fault-phased enumeration controls failed")
  let some source := choreo.ownershipSource? | throw (IO.userError "missing fault source")
  let some indexed := indexedPlans (boundedPlans source .source) images
    | throw (IO.userError "unmatched fault direction")
  let mut closeDrops := 0
  let mut abortDrains := 0
  let mut broadcast := 0
  let mut roots := 0
  let mut failureControls := false
  for attempt in attempts do
    let released := attempt.events.filterMap fun event => if event.kind == 8 then some event.operation else none
    let expected := attempt.before ++ (if attempt.accepted then attempt.after else attempt.after.dropLast)
    let closures := attempt.events.filter fun event => event.kind == 4 || event.kind == 5
    let beforeClose := (attempt.events.takeWhile fun event => event.kind != 4 && event.kind != 5).filterMap
      fun event => if event.kind == 8 then some event.operation else none
    unless released == expected && beforeClose == attempt.before &&
        closures.length == 1 && (closures.head?.map (·.kind)) == some (if attempt.abort then 5 else 4) &&
        (!attempt.after.isEmpty || attempt.accepted) do
      throw (IO.userError s!"malformed fault commands {faultAttemptKey attempt}")
    let states := indexed.filterMap fun (plan, channels) => do
      let state ← faultPhaseRun plan images channels .source attempt.events
      if !faultPhaseQuiescent plan channels state attempt.actors then none else some (plan, channels, state)
    unless !states.isEmpty && (attempt.accepted ||
        (attempt.after.getLast?.any fun candidate => !(states.any fun (plan, channels, state) =>
          faultCommandReady plan channels state candidate))) do
      throw (IO.userError s!"fault phased mismatch commands={faultAttemptKey attempt} accepted={attempt.accepted} actors={attempt.actors}")
    let permits := fun oracle => indexed.any fun (plan, channels) =>
      (faultPhaseRun plan images channels oracle attempt.events).any fun state =>
        faultPhaseQuiescent plan channels state attempt.actors
    if !failureControls then
      if let some failure := attempt.events.find? (·.kind == 7) then
        let missing := attempt.events.filter (· != failure)
        let duplicate := attempt.events.flatMap fun event => if event == failure then [event, event] else [event]
        let accepts := fun events => indexed.any fun (plan, channels) =>
          (faultPhaseRun plan images channels .source events).any fun state =>
            faultPhaseQuiescent plan channels state attempt.actors
        if accepts missing || accepts duplicate then
          throw (IO.userError "missing/duplicate public failure observation was accepted")
        failureControls := true
    if !permits .closeDrops then closeDrops := closeDrops + 1
    if !permits .abortDrains then abortDrains := abortDrains + 1
    if !permits .broadcast then broadcast := broadcast + 1
    if attempt.after.isEmpty then roots := roots + 1
  unless roots > 0 && closeDrops > 0 && abortDrains > 0 && broadcast > 0 && failureControls do
    throw (IO.userError "fault phased mutants did not exercise closure/isolation/local faults")
  IO.println s!"Endpoint fault phased conformance: attempts={attempts.length} injections={roots} close-drops-mutant={closeDrops} abort-drains-mutant={abortDrains} broadcast-mutant={broadcast} missing/duplicate-controls=pass"
