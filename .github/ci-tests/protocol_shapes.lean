-- Appended to endpoint_conformance.lean. This uses the established occurrence
-- semantics, not the Rust generator's first-event calculation.
structure ShapeNode where
  kind : Nat
  left : Nat := 0
  right : Nat := 0
  sender : Nat := 0
  receiver : Nat := 0
  label : Nat := 0
  owned : Bool := false
  resolver : Nat := 0
  deriving Lean.FromJson

structure ShapeInput where
  name : Nat
  nodes : Array ShapeNode
  resolvers : Nat
  deriving Lean.FromJson

structure ShapeAttempt where
  choice : Nat
  «prefix» : List Nat
  label : Nat
  receive : Bool
  accepted : Bool
  deriving Lean.FromJson

structure ShapeOutput where
  name : Nat
  attempts : Array ShapeAttempt
  deriving Lean.FromJson

def parseShape (input : ShapeInput) : Except String Source := do
  let mut sources : Array Source := #[]
  for node in input.nodes do
    let source ← if node.kind == 0 then
      pure (.send ⟨⟨0, node.sender⟩, ⟨0, node.receiver⟩, node.label,
        if node.owned then .endpoint ⟨0, 3⟩ else .wire 0⟩)
    else do
      let some left := sources[node.left]? | throw "invalid left child"
      match node.kind with
      | 5 => pure (.roll left)
      | 1 | 2 | 3 | 4 =>
        let some right := sources[node.right]? | throw "invalid right child"
        pure (match node.kind with
          | 1 => .seq left right
          | 2 => .par left right
          | 3 => .route none left right
          | _ => .route (some node.resolver) left right)
      | _ => throw "unknown shape node"
    sources := sources.push source
  let some source := sources.back? | throw "empty shape"
  pure source

def shapeResolvers : Source → Nat → List (Scope × Nat)
  | .send _, _ => []
  | .seq left right, start | .par left right, start =>
      shapeResolvers left start ++ shapeResolvers right (start + left.eventCount)
  | .route resolver left right, start =>
      let own := resolver.toList.map fun id =>
        (⟨start, start + left.eventCount, start + left.eventCount + right.eventCount⟩, id)
      own ++ shapeResolvers left start ++ shapeResolvers right (start + left.eventCount)
  | .roll body, start => shapeResolvers body start

def shapeFreshChoices (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (choice : Nat) : Bool :=
  (shapeResolvers plan.expanded 0).all fun (scope, id) =>
    points.any (fun point =>
      let event := match point with
        | .accepted event => event
        | .committed action => action.event
      scope.start ≤ event && event < scope.stop) ||
    plan.path.selections.all fun selection =>
      selection.scope != scope || selection.arm ==
        if choice / 2 ^ id % 2 == 0 then .left else .right

def shapeAdvance (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (label : Nat) (receive : Bool) : List (List SourcePoint) :=
  plan.path.bindings.filterMap fun (event, binding) => do
    if binding.message.label != label then none else do
      let point := if receive then .committed ⟨event, .receive⟩ else .accepted event
      if !(occurrenceCut plan points).ready point then none else do
        let points := points ++ [point]
        if receive then some points else do
          let publication := SourcePoint.committed ⟨event, .send⟩
          if (occurrenceCut plan points).ready publication then some (points ++ [publication]) else none

def shapeStates (source : Source) (plans : List (SourceOccurrenceCut source))
    (word : List Nat) : List (SourceOccurrenceCut source × List SourcePoint) :=
  word.foldl (fun states command => states.flatMap fun (plan, points) =>
    (shapeAdvance plan points (command / 2) (command % 2 == 1)).map (plan, ·))
    (plans.map (·, []))

def shapePermitsAfter (source : Source) (plans : List (SourceOccurrenceCut source))
    (choice : Nat) (word : List Nat) (label : Nat) (receive : Bool) : Bool :=
  (shapeStates source plans word).any fun (plan, points) =>
    shapeFreshChoices plan points choice && !(shapeAdvance plan points label receive).isEmpty

def shapeRepetitions : Source → Nat
  | .send _ => 0
  | .seq left right | .par left right | .route _ left right =>
      shapeRepetitions left + shapeRepetitions right
  | .roll body => 1 + shapeRepetitions body

def shapeFiniteWords : Source → List (List Nat)
  | .send message => [[2 * message.label, 2 * message.label + 1]]
  | .seq left right => (shapeFiniteWords left).flatMap fun first =>
      (shapeFiniteWords right).map (first ++ ·)
  | .par left right => (shapeFiniteWords left).flatMap fun first =>
      (shapeFiniteWords right).flatMap fun second => [first ++ second, second ++ first]
  | .route _ left right => shapeFiniteWords left ++ shapeFiniteWords right
  | .roll _ => []

def shapePlans : (source : Source) → List (SourceOccurrenceCut source)
  | .roll body =>
      if shapeRepetitions body == 0 then
        checkedPlans (.roll body) _ (.rollMore
          (.rollMore (.rollOnce (.oneCopy body)) (.oneCopy body)) (.oneCopy body))
      else checkedPlans (Source.roll body) (Source.roll body).oneCopy (.oneCopy (Source.roll body))
  | source => checkedPlans source source.oneCopy (.oneCopy source)

-- Two completed body visits require a third copy for the next-call probes.
-- Other repeating sources need continuation-aware occurrence unfoldings.
def shapePrefixes (source : Source) : List (List Nat) :=
  let words := match source with
    | .roll body =>
        if shapeRepetitions body == 0 then
          (shapeFiniteWords body).flatMap fun first =>
            (shapeFiniteWords body).map (first ++ ·)
        else []
    | _ => if shapeRepetitions source == 0 then shapeFiniteWords source else []
  ([] :: words.flatMap fun word =>
      (List.range (word.length + 1)).map (word.take ·)).eraseDups

-- Exhaust every order of the first four public operations for finite sources.
-- The complete-trace prefixes above also reach deeper positions in each tree.
def shapeShortPrefixes (source : Source) (plans : List (SourceOccurrenceCut source))
    (resolvers : Nat) : List (List Nat) :=
  if shapeRepetitions source != 0 then [] else Id.run do
    let mut level := [[]]
    let mut found := []
    for _ in List.range 4 do
      let mut next := []
      for word in level do
        for (_, message) in source.messageEntries 0 do
          for receive in [false, true] do
            if (List.range (2 ^ resolvers)).any fun choice =>
                shapePermitsAfter source plans choice word message.label receive then
              next := (word ++ [2 * message.label + if receive then 1 else 0]) :: next
      level := next.eraseDups
      found := found ++ level
    return found

-- Reject omitted/duplicate probes independently of the observed outcome.
def shapeProbeKeys (source : Source)
    (prefixes : List (List Nat)) (resolvers : Nat) : List (Nat × List Nat × Nat × Bool) :=
  (List.range (2 ^ resolvers)).flatMap fun choice =>
    prefixes.flatMap fun word =>
      (source.messageEntries 0).flatMap fun (_, message) =>
        [(choice, word, message.label, false), (choice, word, message.label, true)]

def shapeKeyLess (a b : Nat × List Nat × Nat × Bool) : Bool :=
  a.1 < b.1 || (a.1 == b.1 && (decide (a.2.1 < b.2.1) ||
    (a.2.1 == b.2.1 && (a.2.2.1 < b.2.2.1 ||
      (a.2.2.1 == b.2.2.1 && !a.2.2.2 && b.2.2.2)))))

def shapeRead [Lean.FromJson α] (path : System.FilePath) : IO α := do
  let raw ← IO.FS.readFile path
  match Lean.Json.parse raw >>= Lean.fromJson? with
  | .ok value => pure value
  | .error error => throw (IO.userError s!"invalid shape JSON in {path}: {error}")

def main (args : List String) : IO UInt32 := do
  let [inputPath, outputPath] := args
    | throw (IO.userError "usage: protocol-shapes INPUT.json OBSERVATION_DIRECTORY")
  let inputs : Array ShapeInput ← shapeRead inputPath
  unless !inputs.isEmpty do throw (IO.userError "empty shape inventory")
  let expectedNames := inputs.toList.map (fun input => s!"{input.name}.json")
  let actualNames := (← System.FilePath.readDir outputPath).toList.map (·.fileName)
  unless expectedNames.mergeSort (· ≤ ·) == actualNames.mergeSort (· ≤ ·) do
    throw (IO.userError "missing, duplicate or unknown shape observation")
  let mut checked := 0
  let mut continuations := 0
  let mut runtimeOnly := 0
  let mut modelOnly := 0
  for input in inputs do
    let source ← match parseShape input with
      | .ok source => pure source
      | .error error => throw (IO.userError s!"shape {input.name}: {error}")
    let some _ := normalize source
      | throw (IO.userError s!"shape {input.name}: source ownership rejected")
    unless ((shapeResolvers source 0).map Prod.snd).mergeSort (· ≤ ·) == List.range input.resolvers do
      throw (IO.userError s!"shape {input.name}: missing or duplicate resolver assignments")
    let plans := shapePlans source
    unless !plans.isEmpty do throw (IO.userError s!"shape {input.name}: no accepted source plan")
    let prefixes := (shapePrefixes source ++ shapeShortPrefixes source plans input.resolvers).eraseDups
    unless prefixes.all (fun word => !(shapeStates source plans word).isEmpty) do
      throw (IO.userError s!"shape {input.name}: source prefix has no accepted occurrence execution")
    let output : ShapeOutput ← shapeRead (System.FilePath.mk outputPath / s!"{input.name}.json")
    let actualKeys := output.attempts.toList.map fun a => (a.choice, a.prefix, a.label, a.receive)
    unless input.name == output.name &&
        actualKeys.mergeSort shapeKeyLess == (shapeProbeKeys source prefixes input.resolvers).mergeSort shapeKeyLess do
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory")
    for attempt in output.attempts do
      let permitted := shapePermitsAfter source plans attempt.choice attempt.prefix attempt.label attempt.receive
      checked := checked + 1
      if !attempt.prefix.isEmpty then continuations := continuations + 1
      if permitted != attempt.accepted then
        if runtimeOnly + modelOnly < 10 then
          IO.println s!"shape {input.name} choice={attempt.choice} prefix={attempt.prefix} label={attempt.label} receive={attempt.receive}: model={permitted} runtime={attempt.accepted}"
        if attempt.accepted then runtimeOnly := runtimeOnly + 1 else modelOnly := modelOnly + 1
  IO.println s!"Generated shapes/source: shapes={inputs.size} attempts={checked} initial={checked-continuations} continuation={continuations} runtime-only={runtimeOnly} model-only={modelOnly}"
  unless runtimeOnly == 0 && modelOnly == 0 do
    throw (IO.userError "generated Endpoint/source conformance failed")
  pure 0
