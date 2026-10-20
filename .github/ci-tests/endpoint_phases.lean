-- Concatenated after endpoint_conformance.lean. This interprets observations
-- of real calls and I/O, never controls the Rust application or the carrier.

structure PhaseBoundary where
  /-- 0=start, 1=accept, 2=deliver, 3=successful public completion. -/
  kind : Nat
  operation : Nat
  visit : Nat
  actor : Nat
  wire : List Nat
  witness : Nat
  deriving BEq

structure PhaseAttempt where
  commands : List Nat
  accepted : Bool
  finished : Bool
  events : List PhaseBoundary

structure PhaseState where
  points : List SourcePoint := []
  started : List (Nat × Nat) := []
  delivered : List (Nat × Nat) := []
  packets : List PhaseBoundary := []

inductive PhaseOracle where
  | source | atomic | actorwide
  deriving BEq

def phaseOccurrence? (plan : SourceOccurrenceCut source) (event : PhaseBoundary) :
    Option Nat :=
  (((plan.unfolding.occurrences 0).zipIdx.filter fun ((origin, _), _) =>
    origin == event.operation / 2)[event.visit]?).map Prod.snd

def phaseWireCheck (plan : SourceOccurrenceCut source) (images : List RustDescriptorImage)
    (session : Nat) (event : PhaseBoundary) (occurrence : Nat) : Option Bool := do
  let (_, message) ← (plan.unfolding.occurrences 0)[occurrence]?
  let (_, binding) ← plan.path.bindings.find? (fun entry => entry.1 == occurrence)
  let image ← resolveUnique? (images.filter (·.role == binding.senderActor))
  let row ← resolveUnique? ((List.range image.eventCount).filter fun row =>
    image.eventEffIndex? row == some (event.operation / 2) &&
      image.eventView? row == some message.sender.role)
  let (sender, receiver, lane) ← channelFor plan images occurrence
  let color ← image.eventFrameLabel? row
  let header := [session / 16777216 % 256, session / 65536 % 256,
    session / 256 % 256, session % 256, lane, sender, receiver, color]
  let payload := match message.payload with
    | .wire _ => true -- Canonical codec and value checks remain Rust obligations.
    | .endpoint key => event.wire.drop 8 ==
        [key.role, (event.operation / 2) / 256, (event.operation / 2) % 256]
  pure (event.wire.length ≥ 8 && event.wire.take 8 == header && payload)

def phaseStep (plan : SourceOccurrenceCut source) (images : List RustDescriptorImage)
    (channels : Array (Nat × Nat × Nat)) (session : Nat) (oracle : PhaseOracle)
    (state : PhaseState) (event : PhaseBoundary) : Option PhaseState := do
  let occurrence ← phaseOccurrence? plan event
  let (_, binding) ← plan.path.bindings.find? (fun entry => entry.1 == occurrence)
  let side := if event.operation % 2 == 0 then Side.send else Side.receive
  let actor := if side == .send then binding.senderActor else binding.receiverActor
  if event.actor != actor then none else pure ()
  let key := (event.operation, event.visit)
  let action : ActionId := ⟨occurrence, side⟩
  let cut := occurrenceCut plan state.points
  match event.kind with
  | 0 =>
      if state.started.contains key || !event.wire.isEmpty || event.witness != 0 then none
      else some { state with started := state.started ++ [key] }
  | 1 =>
      if side != .send || !state.started.contains key ||
          !cut.ready (.accepted occurrence) || !cut.resumptionReady (.accepted occurrence) ||
          !(phaseWireCheck plan images session event occurrence).getD false ||
          state.packets.any (fun previous => previous.witness == event.witness) then none
      else do
        if oracle == .actorwide && state.packets.any (fun previous =>
            previous.actor == event.actor &&
              (phaseOccurrence? plan previous).any fun earlier =>
                !state.points.contains (.committed ⟨earlier, .send⟩)) then none else pure ()
        pure { state with
          points := state.points ++ [.accepted occurrence]
          packets := state.packets ++ [event] }
  | 2 =>
      let sent ← resolveUnique? (state.packets.filter fun previous =>
        previous.operation / 2 == event.operation / 2 && previous.visit == event.visit)
      if side != .receive || !state.started.contains key || state.delivered.contains key ||
          sent.witness != event.witness || sent.wire != event.wire ||
          !atFifoHead channels state.points occurrence ||
          (oracle == .atomic && !state.points.contains (.committed ⟨occurrence, .send⟩)) then none
      else some { state with delivered := state.delivered ++ [key] }
  | 3 =>
      if !state.started.contains key || !event.wire.isEmpty || event.witness != 0 ||
          !cut.ready (.committed action) ||
          (side == .receive && (!state.delivered.contains key ||
            !cut.resumptionReady (.committed action))) then none
      else some { state with points := state.points ++ [.committed action] }
  | _ => none

def phaseRun (plan : SourceOccurrenceCut source) (images : List RustDescriptorImage)
    (channels : Array (Nat × Nat × Nat)) (oracle : PhaseOracle)
    (events : List PhaseBoundary) : Option PhaseState :=
  events.foldlM (phaseStep plan images channels 9076 oracle) {}

def validIoWord (commands : List Nat) : Bool :=
  (commands.foldlM (fun seen command =>
    if command ≥ 4 || seen.contains command ||
        (command % 2 == 1 && !seen.contains (command - 1)) then none
    else some (seen ++ [command])) ([] : List Nat)).isSome

def gatedWord (events : List PhaseBoundary) : List Nat :=
  events.filterMap fun event =>
    if event.visit != 0 || (event.kind != 1 && event.kind != 3) ||
        event.operation % 2 != 0 then none
    else if event.operation == 0 then some (if event.kind == 1 then 0 else 1)
    else if event.operation == 6 then some (if event.kind == 1 then 2 else 3)
    else none

def phasePrefixes (attempts : List PhaseAttempt) : List (List Nat) :=
  ([] :: attempts.filterMap (fun attempt =>
    if attempt.accepted && attempt.commands.length < 4 then some attempt.commands else none)).eraseDups

def phaseEnumerationComplete (attempts : List PhaseAttempt) : Bool :=
  let prefixes := phasePrefixes attempts
  attempts.all (fun attempt => !attempt.commands.isEmpty && attempt.commands.length ≤ 4 &&
    prefixes.contains attempt.commands.dropLast) &&
  prefixes.all fun priorCommands =>
    ((attempts.filter (fun attempt => attempt.commands.dropLast == priorCommands)).map fun attempt =>
      attempt.commands.getLast!).mergeSort (· ≤ ·) == [0, 1, 2, 3]

def checkPhases (choreo : Choreo) (images : List RustDescriptorImage)
    (attempts : List PhaseAttempt) : IO Unit := do
  let some source := choreo.ownershipSource?
    | throw (IO.userError "missing phased source")
  let plans := boundedPlans source .source
  let some indexed := indexedPlans plans images
    | throw (IO.userError "unmatched phased direction")
  let actors := (plans.flatMap fun plan => plan.path.bindings.flatMap fun (_, binding) =>
    [binding.senderActor, binding.receiverActor]).eraseDups.mergeSort (· ≤ ·)
  unless !plans.isEmpty && (images.map (·.role)).mergeSort (· ≤ ·) == actors &&
      images.all (fun image => image.actorOrderCheck choreo && image.actorRollCheck choreo) do
    throw (IO.userError "phased descriptor/source mismatch")
  unless phaseEnumerationComplete attempts do
    throw (IO.userError "missing/duplicate phased probes")
  let some first := attempts.head? | throw (IO.userError "empty phased observations")
  if phaseEnumerationComplete attempts.tail || phaseEnumerationComplete (first :: attempts) then
    throw (IO.userError "phased enumeration controls failed")
  let permits := fun oracle events => indexed.any fun (plan, channels) =>
    (phaseRun plan images channels oracle events).isSome
  let mut complete := 0
  let mut atomic := 0
  let mut actorwide := 0
  for attempt in attempts do
    unless attempt.accepted == validIoWord attempt.commands &&
        attempt.finished == (attempt.accepted && attempt.commands.length == 4) &&
        gatedWord attempt.events == (if attempt.accepted then attempt.commands else
          attempt.commands.dropLast) && permits .source attempt.events do
      throw (IO.userError s!"phased mismatch commands={attempt.commands} accepted={attempt.accepted} finished={attempt.finished}")
    if !permits .atomic attempt.events then atomic := atomic + 1
    if !permits .actorwide attempt.events then actorwide := actorwide + 1
    if attempt.finished then
      complete := complete + 1
      unless indexed.any (fun (plan, channels) => match phaseRun plan images channels .source attempt.events with
          | none => false
          | some state => (sourceVisitPoints plan.path.bindings).all state.points.contains) do
        throw (IO.userError "completed app lacks completed source word")
      let some accepted := attempt.events.find? (·.kind == 1)
        | throw (IO.userError "finished without physical acceptance")
      let without := attempt.events.filter (fun event => event != accepted)
      let duplicate := attempt.events.flatMap fun event => if event == accepted then [event, event] else [event]
      if permits .source without || permits .source duplicate then
        throw (IO.userError "phased carrier-history mutation was accepted")
  unless complete > 0 && atomic > 0 && actorwide > 0 do
    throw (IO.userError "phased controls did not exercise accepted-but-unpublished overlap")
  IO.println s!"Endpoint phased conformance: attempts={attempts.length} complete={complete} atomic-mutant={atomic} actorwide-mutant={actorwide} missing/duplicate-controls=pass"
