-- Appended to endpoint_conformance.lean before nested_roll_admission.lean.
namespace ResumedRollAdmission

abbrev Expansion (source : Source) := Σ expanded : Source, SourceUnfolding source expanded

def message (sender receiver label : Nat) : Source :=
  .send ⟨⟨0, sender⟩, ⟨0, receiver⟩, label, .wire 0⟩

def exchange (delegated : Bool) : Source :=
  if delegated then
    .seq (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 232, .endpoint ⟨0, 3⟩⟩)
      (.seq (message 3 2 235) (.seq (message 2 3 236)
        (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 233, .endpoint ⟨0, 3⟩⟩)))
  else .seq (message 0 1 232) (message 1 0 233)

def continuation (delegated : Bool) : Source :=
  .seq (.roll (exchange delegated)) (message 0 1 237)

def body (delegated : Bool) : Source :=
  .seq (message 0 1 231) (continuation delegated)

def family (delegated : Bool) : Source := .roll (body delegated)

-- The count is one less than the number of complete exchanges.
def copies (source : Source) : Nat → Expansion (.roll source)
  | 0 => ⟨source.oneCopy, .rollOnce (.oneCopy source)⟩
  | count + 1 =>
      let earlier := copies source count
      ⟨.seq earlier.1 source.oneCopy, .rollMore earlier.2 (.oneCopy source)⟩

def after (delegated : Bool) (earlier : Expansion (continuation delegated)) :
    Nat → Expansion (continuation delegated)
  | 0 => earlier
  | count + 1 =>
      let prior := after delegated earlier count
      ⟨.seq prior.1 (exchange delegated).oneCopy,
        .resumeAfterContinuation prior.2 (.oneCopy (exchange delegated))⟩

-- Weight counts complete Prefix/Exchange/Tail operations, independently of
-- the number of wire messages in an ordinary or delegated exchange.
def bodies (delegated : Bool) (budget : Nat) : List (Nat × Expansion (body delegated)) :=
  (List.range budget).flatMap fun before =>
    let weight := before + 3
    if weight > budget then [] else
    let first := copies (exchange delegated) before
    let withTail : Expansion (continuation delegated) :=
      ⟨.seq first.1 (message 0 1 237).oneCopy, .seq first.2 (.oneCopy _)⟩
    let parts := (weight, withTail) ::
      (List.range (budget - weight)).flatMap fun more =>
        let next := copies (exchange delegated) more
        [(weight + more + 1, after delegated withTail (more + 1)),
          (weight + more + 1,
            ⟨.seq first.1 (.par next.1 (message 0 1 237).oneCopy),
              .resumeAlongsideContinuation first.2 next.2 (.oneCopy _)⟩)]
    parts.map fun (cost, part) =>
      (cost, ⟨.seq (message 0 1 231).oneCopy part.1, .seq (.oneCopy _) part.2⟩)

def outer (delegated : Bool) (parts : List (Nat × Expansion (body delegated))) :
    Nat → Nat → List (Expansion (family delegated))
  | 0, _ => []
  | fuel + 1, budget => parts.flatMap fun (cost, part) =>
      if cost > budget then [] else
      ⟨part.1, .rollOnce part.2⟩ ::
        (outer delegated parts fuel (budget - cost)).map fun earlier =>
          ⟨.seq earlier.1 part.1, .rollMore earlier.2 part.2⟩

abbrev State (delegated : Bool) := SourceOccurrenceCut (family delegated) × List SourcePoint

def labels (delegated : Bool) (letter : Nat) : List Nat :=
  if letter == 0 then [231] else if letter == 2 then [237] else
    if delegated then [232, 235, 236, 233] else [232, 233]

def complete (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (event : Nat) : Option (List SourcePoint) :=
  [.accepted event, .committed ⟨event, .send⟩, .committed ⟨event, .receive⟩].foldlM
    (fun points point =>
      if (occurrenceCut plan points).ready point &&
          (occurrenceCut plan points).resumptionReady point
      then some (points ++ [point]) else none) points

def advance (delegated : Bool) (states : List (State delegated)) (letter : Nat) :
    List (State delegated) :=
  states.flatMap fun (plan, points) => plan.path.bindings.filterMap fun (event, binding) => do
    let word := labels delegated letter
    if binding.message.label != word.head! then none else do
      let points ← word.zipIdx.foldlM (fun points (label, offset) => do
        let (_, binding) ← plan.path.bindings.find? (fun pair => pair.1 == event + offset)
        if binding.message.label != label then none else complete plan points (event + offset)) points
      pure (plan, points)

structure Query where
  delegated : Bool
  «prefix» : List Nat
  probe : Nat
  permitted : Bool
  deriving BEq, Inhabited

def referenceFor (delegated : Bool) (bound : Nat) : List Query × Nat := Id.run do
  -- A queried prefix can complete the current body and its outer visit with
  -- at most three additional complete operations.
  let budget := bound + 3
  let plans := (outer delegated (bodies delegated budget) budget budget).flatMap fun plan =>
    checkedPlans (family delegated) plan.1 plan.2
  if plans.isEmpty then panic! "no resumed-roll source plans"
  let mut level : List (List Nat × List (State delegated)) := [([], plans.map (·, []))]
  let mut queries := []
  for depth in List.range (bound + 1) do
    let mut next := []
    for (word, states) in level do
      for letter in [0, 1, 2] do
        let permitted := states.any fun (plan, points) => plan.path.bindings.any fun (event, binding) =>
          binding.message.label == (labels delegated letter).head! &&
            (occurrenceCut plan points).ready (.accepted event) &&
            (occurrenceCut plan points).resumptionReady (.accepted event)
        queries := queries ++ [⟨delegated, word, letter, permitted⟩]
        if depth < bound then
          let advanced := advance delegated states letter
          if !advanced.isEmpty then next := next ++ [(word ++ [letter], advanced)]
    level := next
  return (queries, plans.length)

def encode (queries : List Query) : ByteArray := Id.run do
  let mut bytes := ByteArray.mk #[72, 78, 69, 1]
  for shift in [24, 16, 8, 0] do
    bytes := bytes.push (UInt8.ofNat (queries.length / 2 ^ shift % 256))
  for query in queries do
    bytes := bytes.push (if query.delegated then 1 else 0)
    bytes := bytes.push (UInt8.ofNat query.prefix.length)
    for letter in query.prefix do bytes := bytes.push (UInt8.ofNat letter)
    bytes := bytes.push (UInt8.ofNat query.probe)
    bytes := bytes.push (if query.permitted then 1 else 0)
  return bytes

def decodeRows : Nat → List Nat → Except String (List Query)
  | 0, [] => pure []
  | 0, _ => throw "surplus resumed-roll queries"
  | count + 1, delegated :: size :: bytes => do
      let word := bytes.take size
      let probe :: result :: tail := bytes.drop size | throw "truncated resumed-roll query"
      if delegated > 1 || size > 7 || word.length != size || !word.all (· < 3) ||
          probe > 2 || result > 1 then throw "invalid resumed-roll query"
      let rest ← decodeRows count tail
      pure (⟨delegated == 1, word, probe, result == 1⟩ :: rest)
  | _ + 1, _ => throw "missing resumed-roll query"

def decode (bytes : ByteArray) : Except String (List Query) := do
  let 72 :: 78 :: 69 :: 1 :: a :: b :: c :: d :: rest := bytes.toList.map UInt8.toNat
    | throw "invalid resumed-roll header"
  let count := a * 16777216 + b * 65536 + c * 256 + d
  if count > rest.length / 4 then throw "invalid resumed-roll query count"
  decodeRows count rest

def write (path : String) : IO Unit := do
  let (ordinary, ordinaryPlans) := referenceFor false 7
  let (delegated, delegatedPlans) := referenceFor true 7
  let queries := ordinary ++ delegated
  let late := [0, 1, 2, 1]
  for resource in [false, true] do
    unless queries.any (fun query => query.delegated == resource &&
        query.prefix == late && query.probe == 1 && query.permitted) do
      throw (IO.userError "missing late inner-roll continuation")
  let bytes := encode queries
  let actual ← match decode bytes with
    | .ok actual => pure actual
    | .error error => throw (IO.userError error)
  unless actual == queries do throw (IO.userError "resumed-roll codec mismatch")
  let some first := actual.head? | throw (IO.userError "empty resumed-roll inventory")
  for invalid in [actual.tail!, first :: actual,
      {first with permitted := !first.permitted} :: actual.tail!] do
    if invalid == queries then throw (IO.userError "invalid resumed-roll inventory accepted")
  for invalid in [bytes.push 0, bytes.extract 0 (bytes.size - 1)] do
    if (decode invalid).isOk then throw (IO.userError "invalid resumed-roll bytes accepted")
  IO.FS.writeBinFile path bytes
  IO.println s!"Resumed-roll source cases: ordinary-plans={ordinaryPlans} delegated-plans={delegatedPlans} queries={queries.length} roundtrip=pass controls=pass"

end ResumedRollAdmission
