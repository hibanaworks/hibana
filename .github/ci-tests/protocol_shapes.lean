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

def shapePathSelected (source : Source) (choice : Nat) (path : NormalizedPath) : Bool :=
  (shapeResolvers source 0).all fun (scope, id) =>
    path.selections.all fun selection =>
      selection.scope != scope || selection.arm ==
        if choice / 2 ^ id % 2 == 0 then .left else .right

def shapeInitialPermits (source : Source) (plans : List (SourceOccurrenceCut source))
    (choice label : Nat) (receive : Bool) : Bool :=
  plans.any fun plan => shapePathSelected source choice plan.path &&
    plan.path.bindings.any fun (event, binding) =>
      binding.message.label == label && plan.ready (
        if receive then .committed ⟨event, .receive⟩ else .accepted event)

-- Reject omitted/duplicate probes independently of the observed outcome.
def shapeProbeKeys (source : Source) (resolvers : Nat) : List (Nat × Nat × Bool) :=
  (List.range (2 ^ resolvers)).flatMap fun choice =>
    (source.messageEntries 0).flatMap fun (_, message) =>
      [(choice, message.label, false), (choice, message.label, true)]

def shapeKeyLess (a b : Nat × Nat × Bool) : Bool :=
  a.1 < b.1 || (a.1 == b.1 && (a.2.1 < b.2.1 ||
    (a.2.1 == b.2.1 && !a.2.2 && b.2.2)))

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
    let plans := checkedPlans source source.oneCopy (.oneCopy source)
    unless !plans.isEmpty do throw (IO.userError s!"shape {input.name}: no accepted source plan")
    let output : ShapeOutput ← shapeRead (System.FilePath.mk outputPath / s!"{input.name}.json")
    let actualKeys := output.attempts.toList.map fun a => (a.choice, a.label, a.receive)
    unless input.name == output.name &&
        actualKeys.mergeSort shapeKeyLess == (shapeProbeKeys source input.resolvers).mergeSort shapeKeyLess do
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory")
    for attempt in output.attempts do
      let permitted := shapeInitialPermits source plans attempt.choice attempt.label attempt.receive
      checked := checked + 1
      if permitted != attempt.accepted then
        if runtimeOnly + modelOnly < 10 then
          IO.println s!"shape {input.name} choice={attempt.choice} label={attempt.label} receive={attempt.receive}: model={permitted} runtime={attempt.accepted}"
        if attempt.accepted then runtimeOnly := runtimeOnly + 1 else modelOnly := modelOnly + 1
  IO.println s!"Generated shapes/source: shapes={inputs.size} attempts={checked} runtime-only={runtimeOnly} model-only={modelOnly}"
  unless runtimeOnly == 0 && modelOnly == 0 do
    throw (IO.userError "generated Endpoint/source conformance failed")
  pure 0
