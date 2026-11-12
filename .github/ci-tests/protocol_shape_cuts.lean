-- Appended after endpoint_conformance.lean. This is an evidence codec for
-- the existing public-cut/source checker, not another permission model.
namespace ProtocolCutEvidence
open Hibana Hibana.EndpointDelegation

deriving instance Lean.FromJson, Lean.ToJson for PublicOperationPhase, SessionFault, Side, RouteArm
deriving instance Lean.FromJson, Lean.ToJson for ActorStepObservation, ActorViewObservation,
  ActorProgressObservation, OperationSlotObservation, OperationBorrowObservation,
  PublicOperationCut, RustDescriptorImage

structure StateRecord where
  id : Nat
  cut : PublicOperationCut
  progress : ActorProgressObservation
  borrowReentry : Option (Nat × Nat × Nat)
  retainedRolls : List Nat
  creditedRolls : List Nat
  deriving Lean.FromJson, Lean.ToJson

def StateRecord.contentKey (state : StateRecord) : String :=
  (Lean.toJson { state with id := 0 }).compress

theorem StateRecord.contentKey_ignores_inventory_id (state : StateRecord) (id : Nat) :
    ({ state with id := id } : StateRecord).contentKey = state.contentKey := rfl

-- The derived model representation must reproduce every input value. This
-- rejects unknown or ignored fields recursively, without another field-name
-- inventory or hard-coded structure sizes. Object key order is immaterial.
def decode [Lean.FromJson α] [Lean.ToJson α] (value : Lean.Json) : Except String α := do
  let record : α ← Lean.fromJson? value
  unless value == Lean.toJson record do throw "observation contains unmodelled fields or values"
  pure record

-- Invert the model's existing component-zero source encoding. The caller
-- also checks ownershipSource? equality; no choreography is hand-written.
def sourceChoreography? : Source → Option Choreo
  | .send message => do
      if message.sender.component != 0 || message.receiver.component != 0 then none else pure ()
      let schema ← match message.payload with
        | .wire schema => if schema < 4294967296 then some schema else none
        | .endpoint key =>
            if key.component == 0 && key.role < 256 then some (4294967296 + key.role) else none
      pure (.send message.sender.role message.receiver.role message.label schema)
  | .seq left right => do return .seq (← sourceChoreography? left) (← sourceChoreography? right)
  | .par left right => do return .par (← sourceChoreography? left) (← sourceChoreography? right)
  | .route resolver left right => do
      let authority := match resolver with | none => RouteAuthority.intrinsic | some id => .dynamic id
      pure (.route authority (← sourceChoreography? left) (← sourceChoreography? right))
  | .roll body => do return .roll (← sourceChoreography? body)

structure Query where
  id : Nat
  state : Nat
  origin : Nat
  receive : Bool
  input : List Nat
  session : Nat
  commits : List (List Nat)
  deriving Lean.FromJson, Lean.ToJson

-- This fixture commits each send in the accepting poll. Reconstruct its FIFO
-- from actual commits, independently of which subsequent call Rust accepted.
def fixtureHead (frames : List (List Nat)) (view lane : Nat) : Option (List Nat) :=
  frames.find? (fun frame => frame[1]? == some view && frame[2]? == some lane)

theorem fixture_head_never_skips_an_earlier_matching_frame
    (first : List Nat) (later : List (List Nat)) (view lane : Nat)
    (target : first[1]? = some view) (direction : first[2]? = some lane) :
    fixtureHead (first :: later) view lane = some first := by
  simp [fixtureHead, target, direction]

theorem fixture_head_keeps_the_full_wire_label :
    fixtureHead [[0,1,1,300],[0,1,1,301]] 1 1 = some [0,1,1,300] := by decide

#print axioms fixture_head_never_skips_an_earlier_matching_frame
#print axioms fixture_head_keeps_the_full_wire_label

def fixtureFrames (images : List RustDescriptorImage) (session : Nat)
    (commits : List (List Nat)) : Except String (List (List Nat)) := do
  let mut frames : List (List Nat) := []
  for record in commits do
    let [kind, identity, observedSession, metadata] := record | throw "invalid call commit fields"
    unless observedSession == session && (kind == 514 || kind == 515) &&
        identity % 256 == (if kind == 514 then 1 else 2) do
      throw "invalid call commit identity"
    let some image := resolveUnique? (images.filter (·.role == identity / 256))
      | throw "call commit has no unique resident image"
    let source := metadata % 65536
    let lane := metadata / 65536 % 256
    let view := metadata / 16777216
    let some row := resolveUnique? ((List.range image.eventCount).filter fun row =>
        image.eventEffIndex? row == some source && image.eventLane? row == some lane &&
          image.eventView? row == some view && (image.eventAction? row).any (fun action =>
            match action with
            | .send .. => kind == 514
            | .recv .. => kind == 515
            | .local .. => true))
      | throw "call commit has no unique resident operation"
    let some action := image.eventAction? row | throw "invalid call operation"
    let some color := image.eventFrameLabel? row | throw "invalid call wire label"
    match action with
    | .local .. => pure ()
    | .send peer .. => frames := frames ++ [[view, peer, lane, color]]
    | .recv peer .. =>
        let (before, after) := frames.span (fun frame => frame[1]? != some view || frame[2]? != some lane)
        let frame :: rest := after | throw "call receive has no FIFO head"
        unless frame == [peer, view, lane, color] do throw "call receive has the wrong FIFO head"
        frames := before ++ rest
  pure frames

def fixtureReady (image : RustDescriptorImage) (view : Nat) (frames : List (List Nat))
    (receive : Bool) (candidates : List Nat) : Except String Bool := do
  let mut ready := false
  for candidate in candidates do
    let some action := image.eventAction? candidate | throw "source candidate has no operation"
    let some actualView := image.eventView? candidate | throw "source candidate has no view"
    let some lane := image.eventLane? candidate | throw "source candidate has no lane"
    let some color := image.eventFrameLabel? candidate | throw "source candidate has no wire label"
    unless actualView == view do throw "source candidate has a foreign view"
    match action with
    | .send .. =>
        if receive then throw "source send candidate has the wrong direction"
        ready := true
    | .local .. => ready := true
    | .recv peer .. =>
        unless receive do throw "source receive candidate has the wrong direction"
        ready := ready || fixtureHead frames view lane ==
          some [peer, view, lane, color]
  pure ready

-- A retained receive preview may only describe a prospective repetition. It
-- does not assert that its packet arrived. The commit-derived FIFO above is
-- the authority for receive availability, including frames held by a preview
-- which have not yet produced a receive commit.

def prepare (choreo : Choreo) (images : List RustDescriptorImage)
    (states : Array StateRecord) (preview : Bool := false) :
    Except String (Array (Nat → Bool → List Nat → Option (List Nat))) := do
  let mut stateQueries : Array (Nat → Bool → List Nat → Option (List Nat)) := #[]
  let mut prepared : Std.HashMap String (Nat → Bool → List Nat → Option (List Nat)) := {}
  for index in [:states.size] do
    let some state := states[index]? | throw "missing operation state"
    unless state.id == index do throw "missing, duplicated or reordered operation state"
    -- Replays of different contracts can measure the identical passive cut.
    -- Reuse only its complete, losslessly decoded content, never its outcome
    -- or a subset of progress. Every original state ID and question remains.
    let key := state.contentKey
    if let some query := prepared.get? key then
      stateQueries := stateQueries.push query
      continue
    let some image := resolveUnique? (images.filter fun image => image.role == state.progress.actor)
      | throw "state has no unique actual resident image"
    let some query := state.cut.sourceQueries? image choreo state.progress
      state.retainedRolls state.creditedRolls state.borrowReentry preview
      | throw s!"source query has invalid descriptor or cut evidence: state={state.id} actor={state.progress.actor} cut={state.cut.matches image} previews={state.cut.previewsMatch image} progress={state.progress.valid} order={image.actorOrderCheck choreo} rolls={image.actorRollCheck choreo} routes={image.actorRouteCheck choreo}"
    stateQueries := stateQueries.push query
    prepared := prepared.insert key query
  pure stateQueries

def evaluate (choreo : Choreo) (images : List RustDescriptorImage)
    (states : Array StateRecord) (queries : Array Query) : Except String (Array (Nat × List Nat)) := do
  let stateQueries ← prepare choreo images states
  let mut answers := #[]
  let stateKeys := states.map StateRecord.contentKey
  let mut cache : Std.HashMap (String × Nat × Bool × List Nat) (List Nat) := {}
  for index in [:queries.size] do
    let some query := queries[index]? | throw "missing source query"
    unless query.id == index do throw "missing, duplicated or reordered source query"
    let some evaluateState := stateQueries[query.state]? | throw "query names a missing operation state"
    let some stateKey := stateKeys[query.state]? | throw "query names a missing operation state"
    let key := (stateKey, query.origin, query.receive, query.input)
    let candidates ← match cache.get? key with
      | some candidates => pure candidates
      | none =>
          let some candidates := evaluateState query.origin query.receive query.input
            | throw "source query has invalid descriptor or cut evidence"
          pure candidates
    cache := cache.insert key candidates
    answers := answers.push (query.id, candidates)
  pure answers

end ProtocolCutEvidence
