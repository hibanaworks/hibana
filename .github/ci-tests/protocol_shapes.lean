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

theorem shapeLookupMappedRows (keys : List SourcePoint)
    (value : SourcePoint → List SourcePoint) (point : SourcePoint) :
    (keys.map fun key => (key, value key)).lookup point =
      if keys.contains point then some (value point) else none := by
  induction keys with
  | nil => rfl
  | cons key rest ih =>
    cases equal : point == key with
    | false =>
      have different : point ≠ key := by
        intro same
        subst key
        simp at equal
      simp [List.lookup_cons, equal, ih, different]
    | true =>
      have same : point = key := eq_of_beq equal
      subst key
      simp

def shapeDependencyRows (plan : SourceOccurrenceCut source) :
    List (SourcePoint × List SourcePoint) :=
  let domain := sourceVisitPoints plan.path.bindings
  domain.map fun point =>
    (point, domain.filter fun prior =>
      sourcePointDepends plan.expanded plan.path.bindings prior point)

def shapeRowsReady (rows : List (SourcePoint × List SourcePoint))
    (points : List SourcePoint) (point : SourcePoint) : Bool :=
  match rows.lookup point with
  | none => false
  | some required => !points.contains point && required.all points.contains

theorem shapeRowsReady_matches_source_readiness
    (plan : SourceOccurrenceCut source) (points : List SourcePoint) (point : SourcePoint) :
    shapeRowsReady (shapeDependencyRows plan) points point =
      (occurrenceCut plan points).ready point := by
  by_cases present : point ∈ sourceVisitPoints plan.path.bindings <;>
    simp [shapeRowsReady, shapeDependencyRows, shapeLookupMappedRows, present,
      SourceOccurrenceCut.ready, occurrenceCut]

structure ShapePlan (source : Source) where
  plan : SourceOccurrenceCut source
  dependencies : List (SourcePoint × List SourcePoint)
  dependencies_eq : dependencies = shapeDependencyRows plan

def ShapePlan.of (plan : SourceOccurrenceCut source) : ShapePlan source :=
  ⟨plan, shapeDependencyRows plan, rfl⟩

def ShapePlan.ready (prepared : ShapePlan source) (points : List SourcePoint)
    (point : SourcePoint) : Bool :=
  shapeRowsReady prepared.dependencies points point

theorem ShapePlan.ready_matches_source (prepared : ShapePlan source)
    (points : List SourcePoint) (point : SourcePoint) :
    prepared.ready points point = (occurrenceCut prepared.plan points).ready point := by
  unfold ShapePlan.ready
  rw [prepared.dependencies_eq, shapeRowsReady_matches_source_readiness]

#print axioms shapeLookupMappedRows
#print axioms shapeRowsReady_matches_source_readiness
#print axioms ShapePlan.ready_matches_source

def shapeAdvance (prepared : ShapePlan source) (points : List SourcePoint)
    (label : Nat) (receive : Bool) : List (List SourcePoint) :=
  prepared.plan.path.bindings.filterMap fun (event, binding) => do
    if binding.message.label != label then none else do
      let isLocal := binding.message.sender == binding.message.receiver
      if isLocal && receive then none else do
        let point := if isLocal then .committed ⟨event, .local⟩
          else if receive then .committed ⟨event, .receive⟩ else .accepted event
        if !prepared.ready points point then none else do
          let points := points ++ [point]
          if isLocal || receive then some points else do
            let publication := SourcePoint.committed ⟨event, .send⟩
            if prepared.ready points publication then some (points ++ [publication]) else none

def shapeStates (source : Source) (plans : List (ShapePlan source))
    (word : List Nat) : List (ShapePlan source × List SourcePoint) :=
  word.foldl (fun states command => states.flatMap fun (plan, points) =>
    (shapeAdvance plan points (command / 2) (command % 2 == 1)).map (plan, ·))
    (plans.map (·, []))

def shapePermits (states : List (ShapePlan source × List SourcePoint))
    (choice : Nat) (label : Nat) (receive : Bool) : Bool :=
  states.any fun (plan, points) =>
    shapeFreshChoices plan.plan points choice && !(shapeAdvance plan points label receive).isEmpty

def shapeRepetitions : Source → Nat
  | .send _ => 0
  | .seq left right | .par left right | .route _ left right =>
      shapeRepetitions left + shapeRepetitions right
  | .roll body => 1 + shapeRepetitions body

def shapeFiniteWords : Source → List (List Nat)
  | .send message => [if message.sender == message.receiver then [2 * message.label]
      else [2 * message.label, 2 * message.label + 1]]
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
def shapeShortPrefixes (source : Source) (plans : List (ShapePlan source))
    (resolvers : Nat) : List (List Nat) :=
  if shapeRepetitions source != 0 then [] else Id.run do
    let mut level := [[]]
    let mut found := []
    for _ in List.range 4 do
      let mut next := []
      for word in level do
        let states := shapeStates source plans word
        for (_, message) in source.messageEntries 0 do
          for receive in [false, true] do
            if (List.range (2 ^ resolvers)).any fun choice =>
                shapePermits states choice message.label receive then
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

def shapeGroupStep (attempt : ShapeAttempt)
    (groups : List (List Nat × List ShapeAttempt)) : List (List Nat × List ShapeAttempt) :=
  match groups with
  | [] => [(attempt.prefix, [attempt])]
  | (word, attempts) :: rest =>
      if attempt.prefix == word then (word, attempt :: attempts) :: rest
      else (attempt.prefix, [attempt]) :: groups

def shapeAttemptGroups (attempts : List ShapeAttempt) : List (List Nat × List ShapeAttempt) :=
  attempts.foldr shapeGroupStep []

theorem shapeGroupStep_keeps_every_attempt (attempt : ShapeAttempt)
    (groups : List (List Nat × List ShapeAttempt)) :
    (shapeGroupStep attempt groups).flatMap Prod.snd = attempt :: groups.flatMap Prod.snd := by
  cases groups with
  | nil => rfl
  | cons group rest =>
      rcases group with ⟨word, attempts⟩
      simp only [shapeGroupStep]
      split <;> simp

theorem shapeAttemptGroups_keeps_order_and_multiplicity (attempts : List ShapeAttempt) :
    (shapeAttemptGroups attempts).flatMap Prod.snd = attempts := by
  induction attempts with
  | nil => rfl
  | cons attempt rest ih =>
      change (shapeGroupStep attempt (shapeAttemptGroups rest)).flatMap Prod.snd = attempt :: rest
      rw [shapeGroupStep_keeps_every_attempt, ih]

#print axioms shapeGroupStep_keeps_every_attempt
#print axioms shapeAttemptGroups_keeps_order_and_multiplicity

-- The typed scripts execute exactly two visits at every roll site. This is
-- a finite witness in the existing source semantics, not a runtime counter.
def shapeTwoCopies : Source → Source
  | .send message => .send message
  | .seq left right => .seq (shapeTwoCopies left) (shapeTwoCopies right)
  | .par left right => .par (shapeTwoCopies left) (shapeTwoCopies right)
  | .route resolver left right => .route resolver (shapeTwoCopies left) (shapeTwoCopies right)
  | .roll body => .seq (shapeTwoCopies body) (shapeTwoCopies body)

def shapeTwoCopyUnfolding : (source : Source) → SourceUnfolding source (shapeTwoCopies source)
  | .send message => .send message
  | .seq left right => .seq (shapeTwoCopyUnfolding left) (shapeTwoCopyUnfolding right)
  | .par left right => .par (shapeTwoCopyUnfolding left) (shapeTwoCopyUnfolding right)
  | .route resolver left right => .route resolver (shapeTwoCopyUnfolding left) (shapeTwoCopyUnfolding right)
  | .roll body => .rollMore (.rollOnce (shapeTwoCopyUnfolding body)) (shapeTwoCopyUnfolding body)

def shapeScriptCount : Source → Nat
  | .send _ => 1
  | .seq left right => shapeScriptCount left * shapeScriptCount right
  | .par left right => 2 * shapeScriptCount left * shapeScriptCount right
  | .route _ left right => shapeScriptCount left + shapeScriptCount right
  | .roll body => shapeScriptCount body * shapeScriptCount body

def shapeChoiceKey (scope : Scope) (right : Bool) : String :=
  s!"{scope.start}:{scope.split}:{scope.stop}:{right}"

-- Normalization assigns every route, including routes in an unchosen arm.
-- A script chooses only the routes it enters. Keep all matching full plans.
def shapeActiveChoices (source : Source) (path : List Selection) (start : Nat) :
    Except String (List String) := do
  match source with
  | .send _ => pure []
  | .seq left right | .par left right =>
    pure ((← shapeActiveChoices left path start) ++
      (← shapeActiveChoices right path (start + left.eventCount)))
  | .route _ left right =>
    let split := start + left.eventCount
    let scope : Scope := ⟨start, split, split + right.eventCount⟩
    let some arm := chosen? path scope | throw "source path omitted a choice"
    let child ← match arm with
      | .left => shapeActiveChoices left path start
      | .right => shapeActiveChoices right path split
    pure (shapeChoiceKey scope (arm == .right) :: child)
  | .roll body => shapeActiveChoices body path start

-- Validate the immutable script against the source. Its rank checks that
-- every combination of choices, visit boundaries and parallel orders occurs.
def shapeScriptKey (source : Source) (script : Lean.Json) (start next : Nat) :
    Except String (List String × Nat × Nat) := do
  let .arr fields := script | throw "script node is not an array"
  unless !fields.isEmpty do throw "empty script node"
  let kind : Nat ← Lean.fromJson? fields[0]!
  match source with
  | .send message =>
    unless kind == 0 && fields.size == 3 do throw "invalid message script"
    let label : Nat ← Lean.fromJson? fields[1]!
    let occurrence : Nat ← Lean.fromJson? fields[2]!
    unless label == message.label && occurrence == next do throw "wrong script message or occurrence"
    pure ([], 0, next + 1)
  | .seq left right | .par left right =>
    let parallel := match source with | .par _ _ => true | _ => false
    unless kind == (if parallel then 2 else 1) && fields.size == (if parallel then 4 else 3) do
      throw "invalid sequential or parallel script"
    let reverse : Bool ← if parallel then Lean.fromJson? fields[3]! else pure false
    let (first, firstRank, next) ← shapeScriptKey left fields[1]! start next
    let (second, secondRank, next) ← shapeScriptKey right fields[2]!
      (start + (shapeTwoCopies left).eventCount) next
    let rank := firstRank * shapeScriptCount right + secondRank
    pure (first ++ second, if parallel then 2 * rank + (if reverse then 1 else 0) else rank, next)
  | .route _ left right =>
    unless kind == 3 && fields.size == 3 do throw "invalid choice script"
    let chooseRight : Bool ← Lean.fromJson? fields[2]!
    let split := start + (shapeTwoCopies left).eventCount
    let scope : Scope := ⟨start, split, split + (shapeTwoCopies right).eventCount⟩
    let (choices, rank, next) ← if chooseRight then
      shapeScriptKey right fields[1]! split next
    else shapeScriptKey left fields[1]! start next
    pure (shapeChoiceKey scope chooseRight :: choices,
      if chooseRight then shapeScriptCount left + rank else rank, next)
  | .roll body =>
    unless kind == 4 && fields.size == 3 do throw "invalid repeated script"
    let (first, firstRank, next) ← shapeScriptKey body fields[1]! start next
    let (second, secondRank, next) ← shapeScriptKey body fields[2]!
      (start + (shapeTwoCopies body).eventCount) next
    pure (first ++ second, firstRank * shapeScriptCount body + secondRank, next)

structure ShapeScriptHeader where
  name : Nat
  scripts : Nat
  deriving Lean.FromJson

structure ShapeCompletedScript where
  script : Nat
  reversePoll : Bool
  completed : Array (Array Lean.Json)
  probes : Array Lean.Json
  deriving Lean.FromJson

-- Source witnesses for injected early sends. Crossing a choice on the receive
-- side, or crossing a roll boundary on either side, would make this inference
-- invalid: the receive could be absent or belong to another visit.
def shapeRequiredMessages : Source → List Message
  | .send message => [message]
  | .seq a b | .par a b => shapeRequiredMessages a ++ shapeRequiredMessages b
  | .route _ _ _ | .roll _ => []

def shapeMessagesWithoutRoll : Source → List Message
  | .send message => [message]
  | .seq a b | .par a b | .route _ a b =>
    shapeMessagesWithoutRoll a ++ shapeMessagesWithoutRoll b
  | .roll _ => []

def shapeAllMessages : Source → List Message
  | .send message => [message]
  | .seq a b | .par a b | .route _ a b => shapeAllMessages a ++ shapeAllMessages b
  | .roll body => shapeAllMessages body

def shapeCausalPairs : Source → List (Message × Message)
  | .send _ => []
  | .seq a b =>
    ((shapeRequiredMessages a).flatMap fun before =>
      (shapeMessagesWithoutRoll b).map (before, ·)) ++ shapeCausalPairs a ++ shapeCausalPairs b
  | .par a b | .route _ a b => shapeCausalPairs a ++ shapeCausalPairs b
  | .roll body => shapeCausalPairs body

def shapeCausalWitness (source : Source) (before target role : Nat) : Bool :=
  (shapeAllMessages source).countP (·.label == before) == 1 &&
  (shapeAllMessages source).countP (·.label == target) == 1 &&
  (shapeCausalPairs source).any fun (a, b) =>
    a.label == before && b.label == target && a.sender != a.receiver &&
    a.receiver == b.sender && a.receiver.role == role &&
    (match a.payload, b.payload with | .wire _, .wire _ => true | _, _ => false)

def shapeCheckLocalWitnesses (source : Source) (probes : Array Lean.Json) : Except String Unit := do
  for probe in probes do
    let kind : String ← probe.getObjValAs? String "kind"
    if kind == "causal" then
      let before : Nat ← probe.getObjValAs? Nat "before"
      let target : Nat ← probe.getObjValAs? Nat "label"
      let role : Nat ← probe.getObjValAs? Nat "role"
      let accepted : Bool ← probe.getObjValAs? Bool "accepted"
      unless !accepted && shapeCausalWitness source before target role do
        throw "invalid causal rejection witness"
    else unless kind == "borrow" do throw "unknown local rejection witness"

def shapeReadLine [Lean.FromJson α] (file : IO.FS.Handle) : IO α := do
  let line ← file.getLine
  unless !line.isEmpty do throw (IO.userError "missing completed script record")
  match Lean.Json.parse line >>= Lean.fromJson? with
  | .ok value => pure value
  | .error error => throw (IO.userError s!"invalid completed script JSON: {error}")

def shapeCompletedAdvance (prepared : ShapePlan source) (points : List SourcePoint)
    (actor role label : Nat) (receive : Bool) : List (List SourcePoint) :=
  -- A RouteBranch consumes a previewed self-role step with recv; both public
  -- local-operation forms denote the same local source point.
  let localStep := prepared.plan.path.bindings.any fun (_, binding) =>
    binding.message.label == label && binding.message.sender == binding.message.receiver
  (shapeAdvance prepared points label (receive && !localStep)).filter fun next =>
    match next.getLast? with
    | some (.committed action) =>
      prepared.plan.path.bindings.any fun (event, binding) => event == action.event &&
        actor == (if receive then binding.receiverActor else binding.senderActor) &&
        role == if receive then binding.message.receiver.role else binding.message.sender.role
    | _ => false

def shapeCheckCompleted (source : Source) (name : Nat) (outputPath : String) : IO (Nat × Nat) := do
  let count := shapeScriptCount source
  let root := System.FilePath.mk outputPath / ".."
  let scripts ← IO.FS.Handle.mk (root / "scripts" / s!"{name}.jsonl") .read
  let observations ← IO.FS.Handle.mk (root / "scope-observations" / s!"{name}.jsonl") .read
  let header : ShapeScriptHeader ← shapeReadLine observations
  unless header.name == name && header.scripts == count do
    throw (IO.userError s!"shape {name}: wrong completed script inventory")
  let mut plans : Std.HashMap (List String) (List (SourceOccurrenceCut source)) := {}
  for plan in checkedPlans source (shapeTwoCopies source) (shapeTwoCopyUnfolding source) do
    let key ← match shapeActiveChoices plan.expanded plan.path.selections 0 with
      | .ok choices => pure (choices.mergeSort (· ≤ ·))
      | .error error => throw (IO.userError error)
    plans := plans.insert key (plan :: ((plans.get? key).getD []))
  unless !plans.isEmpty do throw (IO.userError "no accepted completed source plan")
  let mut prepared : Option (List String × List (ShapePlan source)) := none
  let mut calls := 0
  for index in [:count] do
    let script : Lean.Json ← shapeReadLine scripts
    let (choices, rank, messages) ← match shapeScriptKey source script 0 0 with
      | .ok value => pure value
      | .error error => throw (IO.userError s!"shape {name} script {index}: {error}")
    unless rank == index do
      throw (IO.userError s!"shape {name}: missing or duplicate completed script")
    let key := choices.mergeSort (· ≤ ·)
    let some matching := plans.get? key
      | throw (IO.userError s!"shape {name} script {index}: script has no accepted source path; choices={choices}; source={(plans.toList.take 4).map Prod.fst}")
    unless matching.all (fun plan => plan.path.bindings.length == messages) do
      throw (IO.userError s!"shape {name}: script and source path have different messages")
    -- Retain derived rows for one source-path group and its two schedules.
    let ready := match prepared with
      | some (previous, ready) => if previous == key then ready else matching.map ShapePlan.of
      | none => matching.map ShapePlan.of
    prepared := some (key, ready)
    for reversePoll in [false, true] do
      let observed : ShapeCompletedScript ← shapeReadLine observations
      unless observed.script == index && observed.reversePoll == reversePoll do
        throw (IO.userError s!"shape {name}: missing or reordered completed execution")
      match shapeCheckLocalWitnesses source observed.probes with
      | .ok () => pure ()
      | .error error => throw (IO.userError s!"shape {name} script {index}: {error}")
      let mut states := ready.map (fun plan => (plan, ([] : List SourcePoint)))
      for call in observed.completed do
        unless call.size == 4 do throw (IO.userError "invalid completed call fields")
        let decoded : Except String (Nat × Nat × Nat × Bool) := do
          pure (← Lean.fromJson? call[0]!, ← Lean.fromJson? call[1]!, ← Lean.fromJson? call[2]!, ← Lean.fromJson? call[3]!)
        let (actor, role, label, receive) ← match decoded with
          | .ok value => pure value
          | .error error => throw (IO.userError s!"invalid completed call: {error}")
        unless actor < 256 && role < 256 && label < 256 do throw (IO.userError "invalid completed call domain")
        states := states.flatMap fun (plan, points) =>
          (shapeCompletedAdvance plan points actor role label receive).map (plan, ·)
        unless !states.isEmpty do
          throw (IO.userError s!"shape {name} script {index}: invalid completed trace")
        calls := calls + 1
      unless states.any (fun (plan, points) => (sourceVisitPoints plan.plan.path.bindings).all points.contains) do
        throw (IO.userError s!"shape {name} script {index}: incomplete completed trace")
  unless (← scripts.getLine).isEmpty && (← observations.getLine).isEmpty do
    throw (IO.userError s!"shape {name}: extra completed script record")
  pure (2 * count, calls)

def main (args : List String) : IO UInt32 := do
  let [inputPath, outputPath] := args
    | throw (IO.userError "usage: protocol-shapes INPUT.json OBSERVATION_DIRECTORY")
  let inputs : Array ShapeInput ← shapeRead inputPath
  unless !inputs.isEmpty do throw (IO.userError "empty shape inventory")
  let expectedNames := inputs.toList.map (fun input => s!"{input.name}.json")
  let actualNames := (← System.FilePath.readDir outputPath).toList.map (·.fileName)
  unless expectedNames.mergeSort (· ≤ ·) == actualNames.mergeSort (· ≤ ·) do
    throw (IO.userError "missing, duplicate or unknown shape observation")
  let expectedScripts := inputs.toList.map (fun input => s!"{input.name}.jsonl")
  for directory in ["scripts", "scope-observations"] do
    let observed := (← System.FilePath.readDir
      (System.FilePath.mk outputPath / ".." / directory)).toList.map (·.fileName)
    unless expectedScripts.mergeSort (· ≤ ·) == observed.mergeSort (· ≤ ·) do
      throw (IO.userError "missing, duplicate or unknown completed script file")
  let mut checked := 0
  let mut continuations := 0
  let mut runtimeOnly := 0
  let mut modelOnly := 0
  let mut completedRuns := 0
  let mut completedCalls := 0
  for input in inputs do
    let source ← match parseShape input with
      | .ok source => pure source
      | .error error => throw (IO.userError s!"shape {input.name}: {error}")
    let some _ := normalize source
      | throw (IO.userError s!"shape {input.name}: source ownership rejected")
    unless ((shapeResolvers source 0).map Prod.snd).mergeSort (· ≤ ·) == List.range input.resolvers do
      throw (IO.userError s!"shape {input.name}: missing or duplicate resolver assignments")
    let plans := (shapePlans source).map ShapePlan.of
    unless !plans.isEmpty do throw (IO.userError s!"shape {input.name}: no accepted source plan")
    let prefixes := (shapePrefixes source ++ shapeShortPrefixes source plans input.resolvers).eraseDups
    unless prefixes.all (fun word => !(shapeStates source plans word).isEmpty) do
      throw (IO.userError s!"shape {input.name}: source prefix has no accepted occurrence execution")
    let output : ShapeOutput ← shapeRead (System.FilePath.mk outputPath / s!"{input.name}.json")
    let actualKeys := output.attempts.toList.map fun a => (a.choice, a.prefix, a.label, a.receive)
    unless input.name == output.name &&
        actualKeys.mergeSort shapeKeyLess == (shapeProbeKeys source prefixes input.resolvers).mergeSort shapeKeyLess do
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory")
    for (word, attempts) in shapeAttemptGroups output.attempts.toList do
      let states := shapeStates source plans word
      for attempt in attempts do
        unless attempt.prefix == word do throw (IO.userError "inconsistent prefix group")
        let permitted := shapePermits states attempt.choice attempt.label attempt.receive
        checked := checked + 1
        if !attempt.prefix.isEmpty then continuations := continuations + 1
        if permitted != attempt.accepted then
          if runtimeOnly + modelOnly < 10 then
            IO.println s!"shape {input.name} choice={attempt.choice} prefix={attempt.prefix} label={attempt.label} receive={attempt.receive}: model={permitted} runtime={attempt.accepted}"
          if attempt.accepted then runtimeOnly := runtimeOnly + 1 else modelOnly := modelOnly + 1
    unless runtimeOnly == 0 && modelOnly == 0 do
      throw (IO.userError s!"generated Endpoint/source conformance failed: runtime-only={runtimeOnly} model-only={modelOnly}")
    let (runs, calls) ← shapeCheckCompleted source input.name outputPath
    completedRuns := completedRuns + runs
    completedCalls := completedCalls + calls
  IO.println s!"Completed shape scripts/source: shapes={inputs.size} runs={completedRuns} calls={completedCalls} all accepted"
  IO.println s!"Generated shapes/source: shapes={inputs.size} attempts={checked} initial={checked-continuations} continuation={continuations} runtime-only={runtimeOnly} model-only={modelOnly}"
  pure 0
