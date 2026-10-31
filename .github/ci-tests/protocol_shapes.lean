-- Appended to endpoint_conformance.lean. This uses the established occurrence
-- semantics, not the Rust generator's first-event calculation.
structure ShapeNode where
  kind : Nat
  left : Nat := 0
  right : Nat := 0
  sender : Nat := 0
  receiver : Nat := 0
  event : Nat
  label : Nat := 0
  owned : Bool := false
  resolver : Nat := 0
  deriving Lean.FromJson

structure ShapeInput where
  name : Nat
  nodes : Array ShapeNode
  resolvers : Nat
  parallelCoverage : Bool := false
  deriving Lean.FromJson

structure ShapeAttempt where
  choice : List Nat
  «prefix» : List Nat
  event : Nat
  receive : Bool
  accepted : Bool
  deriving Lean.FromJson

structure ShapeOutput where
  name : Nat
  attempts : Array ShapeAttempt
  deriving Lean.FromJson

def parseShape (input : ShapeInput) : Except String Source := do
  let mut sources : Array Source := #[]
  let mut event := 0
  for node in input.nodes do
    let source ← if node.kind == 0 then
      unless node.event == event do throw "source events must be distinct consecutive positions"
      event := event + 1
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

-- A choice executes one arm. Sequence and parallel composition retain the
-- combinations of their children's decisions. Unselected arms add no inputs.
def shapePathChoices : Source → List (List Nat)
  | .send _ => [[]]
  | .seq left right | .par left right =>
      (shapePathChoices left).flatMap fun a =>
        (shapePathChoices right).map fun b => (a ++ b).mergeSort (· ≤ ·)
  | .route none left right => shapePathChoices left ++ shapePathChoices right
  | .route (some id) left right =>
      shapePathChoices left ++ (shapePathChoices right).map (id :: ·)
  | .roll body => shapePathChoices body

-- Include each source path, then vary each resolver once. This tests a wrong
-- callback or an unrelated choice affecting admission without a 2^N table.
def shapeChoicePatterns (source : Source) : List (List Nat) :=
  let paths := ((shapePathChoices source).map (·.mergeSort (· ≤ ·))).eraseDups
  let ids := (shapeResolvers source 0).map Prod.snd
  (paths ++ paths.flatMap fun path => ids.map fun id =>
      if path.contains id then path.filter (· != id)
      else (id :: path).mergeSort (· ≤ ·)).eraseDups.mergeSort fun a b => decide (a ≤ b)

def shapeFreshChoices (plan : SourceOccurrenceCut source) (points : List SourcePoint)
    (choice : List Nat) : Bool :=
  (shapeResolvers plan.expanded 0).all fun (scope, id) =>
    points.any (fun point =>
      let event := match point with
        | .accepted event => event
        | .committed action => action.event
      scope.start ≤ event && event < scope.stop) ||
    plan.path.selections.all fun selection =>
      selection.scope != scope || selection.arm ==
        if choice.contains id then .right else .left

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

def shapeMessage? (source : Source) (event : Nat) : Option Message :=
  ((source.messageEntries 0).find? (fun entry => entry.1 == event)).map Prod.snd

-- A public call selects a logical role, direction and message contract.
-- Its alias in a generated script does not select a source occurrence.
def shapeMatchesRequest (expected actual : Message) (receive : Bool) : Bool :=
  decide (expected.label = actual.label ∧ expected.payload = actual.payload ∧
    (if receive then expected.receiver = actual.receiver else expected.sender = actual.sender))

theorem shapeRequest_matches_its_exact_public_contract
    (expected actual : Message) (receive : Bool) :
    shapeMatchesRequest expected actual receive = true ↔
      expected.label = actual.label ∧ expected.payload = actual.payload ∧
        (if receive then expected.receiver = actual.receiver else expected.sender = actual.sender) := by
  simp [shapeMatchesRequest]

theorem a_send_alias_does_not_select_the_remote_peer
    (message : Message) (peer : EndpointKey) :
    shapeMatchesRequest message { message with receiver := peer } false = true := by
  simp [shapeMatchesRequest]

theorem a_receive_alias_does_not_select_the_remote_peer
    (message : Message) (peer : EndpointKey) :
    shapeMatchesRequest message { message with sender := peer } true = true := by
  simp [shapeMatchesRequest]

theorem equal_labels_do_not_erase_the_sending_role
    (expected actual : Message) (different : expected.sender ≠ actual.sender) :
    shapeMatchesRequest expected actual false = false := by
  simp [shapeMatchesRequest, different]

#print axioms shapeRequest_matches_its_exact_public_contract
#print axioms a_send_alias_does_not_select_the_remote_peer
#print axioms a_receive_alias_does_not_select_the_remote_peer
#print axioms equal_labels_do_not_erase_the_sending_role

def shapeAdvance (prepared : ShapePlan source) (points : List SourcePoint)
    (request : Nat) (receive : Bool) : List (List SourcePoint) := Id.run do
  let some expected := shapeMessage? source request | return []
  return prepared.plan.path.bindings.filterMap fun (event, binding) => do
    if !shapeMatchesRequest expected binding.message receive then none else do
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
    (choice : List Nat) (label : Nat) (receive : Bool) : Bool :=
  states.any fun (plan, points) =>
    shapeFreshChoices plan.plan points choice && !(shapeAdvance plan points label receive).isEmpty

def shapeRepetitions : Source → Nat
  | .send _ => 0
  | .seq left right | .par left right | .route _ left right =>
      shapeRepetitions left + shapeRepetitions right
  | .roll body => 1 + shapeRepetitions body

def shapeFiniteWords (source : Source) (start : Nat := 0) : List (List Nat) :=
  match source with
  | .send message => [if message.sender == message.receiver then [2 * start]
      else [2 * start, 2 * start + 1]]
  | .seq left right => (shapeFiniteWords left start).flatMap fun first =>
      (shapeFiniteWords right (start + left.eventCount)).map (first ++ ·)
  | .par left right => (shapeFiniteWords left start).flatMap fun first =>
      (shapeFiniteWords right (start + left.eventCount)).flatMap fun second =>
        [first ++ second, second ++ first]
  | .route _ left right => shapeFiniteWords left start ++
      shapeFiniteWords right (start + left.eventCount)
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
    (choices : List (List Nat)) : List (List Nat) :=
  if shapeRepetitions source != 0 then [] else Id.run do
    let mut level := [[]]
    let mut found := []
    for _ in List.range 4 do
      let mut next := []
      for word in level do
        let states := shapeStates source plans word
        for (event, _) in source.messageEntries 0 do
          for receive in [false, true] do
            if choices.any fun choice =>
                shapePermits states choice event receive then
              next := (word ++ [2 * event + if receive then 1 else 0]) :: next
      level := next.eraseDups
      found := found ++ level
    return found

-- Reject omitted/duplicate probes independently of the observed outcome.
def shapeProbeKeys (source : Source)
    (prefixes : List (List Nat)) (choices : List (List Nat)) : List (List Nat × List Nat × Nat × Bool) :=
  choices.flatMap fun choice =>
    prefixes.flatMap fun word =>
      (source.messageEntries 0).flatMap fun (event, _) =>
        [(choice, word, event, false), (choice, word, event, true)]

private def sameContractSequence : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 7, .wire 0⟩)
  (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 7, .wire 0⟩)

theorem an_alias_for_a_later_send_can_issue_the_first_source_operation :
    shapePermits (shapeStates sameContractSequence
      ((shapePlans sameContractSequence).map ShapePlan.of) []) [] 1 false = true := by decide

theorem repeated_aliases_consume_both_occurrences_and_cannot_replay_them :
    let states := shapeStates sameContractSequence
      ((shapePlans sameContractSequence).map ShapePlan.of) [2, 2]
    !states.isEmpty ∧ shapePermits states [] 0 false = false := by decide

#print axioms an_alias_for_a_later_send_can_issue_the_first_source_operation
#print axioms repeated_aliases_consume_both_occurrences_and_cannot_replay_them

def shapeKeyLess (a b : List Nat × List Nat × Nat × Bool) : Bool :=
  decide (a.1 < b.1) || (a.1 == b.1 && (decide (a.2.1 < b.2.1) ||
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
def shapeScriptKey (source : Source) (script : Lean.Json) (start origin next : Nat) :
    Except String (List String × Nat × Nat) := do
  let .arr fields := script | throw "script node is not an array"
  unless !fields.isEmpty do throw "empty script node"
  let kind : Nat ← Lean.fromJson? fields[0]!
  match source with
  | .send _ =>
    unless kind == 0 && fields.size == 3 do throw "invalid message script"
    let label : Nat ← Lean.fromJson? fields[1]!
    let occurrence : Nat ← Lean.fromJson? fields[2]!
    unless label == origin && occurrence == next do throw "wrong script message or occurrence"
    pure ([], 0, next + 1)
  | .seq left right | .par left right =>
    let parallel := match source with | .par _ _ => true | _ => false
    unless kind == (if parallel then 2 else 1) && fields.size == (if parallel then 4 else 3) do
      throw "invalid sequential or parallel script"
    let reverse : Bool ← if parallel then Lean.fromJson? fields[3]! else pure false
    let (first, firstRank, next) ← shapeScriptKey left fields[1]! start origin next
    let (second, secondRank, next) ← shapeScriptKey right fields[2]!
      (start + (shapeTwoCopies left).eventCount) (origin + left.eventCount) next
    let rank := firstRank * shapeScriptCount right + secondRank
    pure (first ++ second, if parallel then 2 * rank + (if reverse then 1 else 0) else rank, next)
  | .route _ left right =>
    unless kind == 3 && fields.size == 3 do throw "invalid choice script"
    let chooseRight : Bool ← Lean.fromJson? fields[2]!
    let split := start + (shapeTwoCopies left).eventCount
    let scope : Scope := ⟨start, split, split + (shapeTwoCopies right).eventCount⟩
    let (choices, rank, next) ← if chooseRight then
      shapeScriptKey right fields[1]! split (origin + left.eventCount) next
    else shapeScriptKey left fields[1]! start origin next
    pure (shapeChoiceKey scope chooseRight :: choices,
      if chooseRight then shapeScriptCount left + rank else rank, next)
  | .roll body =>
    unless kind == 4 && fields.size == 3 do throw "invalid repeated script"
    let (first, firstRank, next) ← shapeScriptKey body fields[1]! start origin next
    let (second, secondRank, next) ← shapeScriptKey body fields[2]!
      (start + (shapeTwoCopies body).eventCount) origin next
    pure (first ++ second, firstRank * shapeScriptCount body + secondRank, next)

structure ShapeScriptHeader where
  name : Nat
  scripts : Nat
  deriving Lean.FromJson

-- The script parser has already validated each constructor and occurrence.
-- Keep one poll-order value for every active par, in source preorder.
def shapeScriptParallelOrders (source : Source) (script : Lean.Json) :
    Except String (List Bool) := do
  let .arr fields := script | throw "invalid parallel script"
  match source with
  | .send _ => pure []
  | .seq a b => pure ((← shapeScriptParallelOrders a fields[1]!) ++
      (← shapeScriptParallelOrders b fields[2]!))
  | .roll body => pure ((← shapeScriptParallelOrders body fields[1]!) ++
      (← shapeScriptParallelOrders body fields[2]!))
  | .par a b =>
    let reverse : Bool ← Lean.fromJson? fields[3]!
    pure (reverse :: ((← shapeScriptParallelOrders a fields[1]!) ++
      (← shapeScriptParallelOrders b fields[2]!)))
  | .route _ a b =>
    let right : Bool ← Lean.fromJson? fields[2]!
    if right then shapeScriptParallelOrders b fields[1]!
    else shapeScriptParallelOrders a fields[1]!

-- Derive the required sites from a normalized source path, independently of
-- the script's list of poll orders. The expanded source has no roll nodes.
def shapeActiveParallelCount (source : Source) (path : List Selection) (start : Nat) :
    Except String Nat := do
  match source with
  | .send _ => pure 0
  | .seq left right | .par left right =>
    let own := match source with | .par _ _ => 1 | _ => 0
    pure (own + (← shapeActiveParallelCount left path start) +
      (← shapeActiveParallelCount right path (start + left.eventCount)))
  | .route _ left right =>
    let split := start + left.eventCount
    let some arm := chosen? path ⟨start, split, split + right.eventCount⟩
      | throw "source path omitted a parallel-site guard"
    match arm with
    | .left => shapeActiveParallelCount left path start
    | .right => shapeActiveParallelCount right path split
  | .roll _ => throw "parallel-site source was not unfolded"

def shapeParallelPairsCovered (width : Nat) (rows : List (List Bool)) : Bool :=
  !rows.isEmpty && rows.all (fun row => row.length == width) &&
  (List.range width).all fun a =>
    [false, true].all (fun x => rows.any fun row => row[a]? == some x) &&
    (List.range width).all fun b => b ≤ a ||
      [false, true].all fun x => [false, true].all fun y =>
        rows.any fun row => row[a]? == some x && row[b]? == some y

structure ShapeCompletedScript where
  script : Nat
  reversePoll : Bool
  completed : Array (Array Lean.Json)
  probes : Array Lean.Json
  deriving Lean.FromJson

-- Source witnesses for injected early sends. Crossing a choice on the receive
-- side, or crossing a roll boundary on either side, would make this inference
-- invalid: the receive could be absent or belong to another visit.
def shapeRequiredMessages (source : Source) (start : Nat) : List (Nat × Message) :=
  match source with
  | .send message => [(start, message)]
  | .seq a b | .par a b => shapeRequiredMessages a start ++
      shapeRequiredMessages b (start + a.eventCount)
  | .route _ _ _ | .roll _ => []

def shapeMessagesWithoutRoll (source : Source) (start : Nat) : List (Nat × Message) :=
  match source with
  | .send message => [(start, message)]
  | .seq a b | .par a b | .route _ a b =>
      shapeMessagesWithoutRoll a start ++ shapeMessagesWithoutRoll b (start + a.eventCount)
  | .roll _ => []

def shapeCausalPairs (source : Source) (start : Nat) : List ((Nat × Message) × (Nat × Message)) :=
  match source with
  | .send _ => []
  | .seq a b =>
    ((shapeRequiredMessages a start).flatMap fun before =>
      (shapeMessagesWithoutRoll b (start + a.eventCount)).map (before, ·)) ++
        shapeCausalPairs a start ++ shapeCausalPairs b (start + a.eventCount)
  | .par a b | .route _ a b => shapeCausalPairs a start ++
      shapeCausalPairs b (start + a.eventCount)
  | .roll body => shapeCausalPairs body start

def shapeCausalWitness (source : Source) (before target role : Nat) : Bool :=
  (shapeCausalPairs source 0).any fun ((first, a), (second, b)) =>
    first == before && second == target && a.sender != a.receiver &&
    a.receiver == b.sender && a.receiver.role == role &&
    (source.messageEntries 0).all (fun (event, other) =>
      event == target || !shapeMatchesRequest b other false) &&
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
  let localStep := match shapeMessage? source label with
    | some message => message.sender == message.receiver
    | none => false
  (shapeAdvance prepared points label (receive && !localStep)).filter fun next =>
    match next.getLast? with
    | some (.committed action) =>
      prepared.plan.path.bindings.any fun (event, binding) => event == action.event &&
        actor == (if receive then binding.receiverActor else binding.senderActor) &&
        role == if receive then binding.message.receiver.role else binding.message.sender.role
    | _ => false

def shapeCheckCompleted (source : Source) (name : Nat) (parallelCoverage : Bool)
    (outputPath : String) : IO (Nat × Nat) := do
  let exhaustiveCount := shapeScriptCount source
  let root := System.FilePath.mk outputPath / ".."
  let scripts ← IO.FS.Handle.mk (root / "scripts" / s!"{name}.jsonl") .read
  let observations ← IO.FS.Handle.mk (root / "scope-observations" / s!"{name}.jsonl") .read
  let header : ShapeScriptHeader ← shapeReadLine observations
  let count := header.scripts
  unless header.name == name && count > 0 && count ≤ exhaustiveCount &&
      (parallelCoverage || count == exhaustiveCount) do
    throw (IO.userError s!"shape {name}: wrong completed script inventory")
  let mut plans : Std.HashMap (List String) (List (SourceOccurrenceCut source)) := {}
  for plan in checkedPlans source (shapeTwoCopies source) (shapeTwoCopyUnfolding source) do
    let key ← match shapeActiveChoices plan.expanded plan.path.selections 0 with
      | .ok choices => pure (choices.mergeSort (· ≤ ·))
      | .error error => throw (IO.userError error)
    plans := plans.insert key (plan :: ((plans.get? key).getD []))
  unless !plans.isEmpty do throw (IO.userError "no accepted completed source plan")
  let mut prepared : Option (List String × List (ShapePlan source)) := none
  let mut parallelRows : Std.HashMap (List String) (List (List Bool)) := {}
  let mut previousRank : Option Nat := none
  let mut calls := 0
  for index in [:count] do
    let script : Lean.Json ← shapeReadLine scripts
    let (choices, rank, messages) ← match shapeScriptKey source script 0 0 0 with
      | .ok value => pure value
      | .error error => throw (IO.userError s!"shape {name} script {index}: {error}")
    unless (parallelCoverage || rank == index) &&
        previousRank.all (fun previous => previous < rank) do
      throw (IO.userError s!"shape {name}: missing or duplicate completed script")
    previousRank := some rank
    let key := choices.mergeSort (· ≤ ·)
    let some matching := plans.get? key
      | throw (IO.userError s!"shape {name} script {index}: script has no accepted source path; choices={choices}; source={(plans.toList.take 4).map Prod.fst}")
    unless matching.all (fun plan => plan.path.bindings.length == messages) do
      throw (IO.userError s!"shape {name}: script and source path have different messages")
    if parallelCoverage then
      let orders ← match shapeScriptParallelOrders source script with
        | .ok values => pure values
        | .error error => throw (IO.userError error)
      parallelRows := parallelRows.insert key (orders :: (parallelRows.get? key).getD [])
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
  if parallelCoverage then
    for (key, matching) in plans.toList do
      let some rows := parallelRows.get? key
        | throw (IO.userError s!"shape {name}: missing source choice path")
      for plan in matching do
        let width ← match shapeActiveParallelCount plan.expanded plan.path.selections 0 with
          | .ok value => pure value
          | .error error => throw (IO.userError error)
        unless shapeParallelPairsCovered width rows do
          throw (IO.userError s!"shape {name}: missing parallel pair coverage")
  pure (2 * count, calls)

def main (args : List String) : IO UInt32 := do
  let (inputPath, outputPath, weakenRoot) ← match args with
    | [input, output] => pure (input, output, false)
    | [input, output, "--mutant-root-seq-as-par"] => pure (input, output, true)
    | _ => throw (IO.userError "usage: protocol-shapes INPUT.json OBSERVATION_DIRECTORY [--mutant-root-seq-as-par]")
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
    let choices := shapeChoicePatterns source
    let prefixes := (shapePrefixes source ++ shapeShortPrefixes source plans choices).eraseDups
    unless prefixes.all (fun word => !(shapeStates source plans word).isEmpty) do
      throw (IO.userError s!"shape {input.name}: source prefix has no accepted occurrence execution")
    let output : ShapeOutput ← shapeRead (System.FilePath.mk outputPath / s!"{input.name}.json")
    let actualKeys := output.attempts.toList.map fun a => (a.choice, a.prefix, a.event, a.receive)
    unless input.name == output.name &&
        actualKeys.mergeSort shapeKeyLess == (shapeProbeKeys source prefixes choices).mergeSort shapeKeyLess do
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory")
    for (word, attempts) in shapeAttemptGroups output.attempts.toList do
      let states := shapeStates source plans word
      -- Mutation control: preserve the exact original probe inventory, then
      -- compare with a source that omits only its outer sequential dependency.
      let permits : ShapeAttempt → Bool := if weakenRoot then
        let altered := match source with | .seq a b => .par a b | _ => source
        let alternate := shapeStates altered ((shapePlans altered).map ShapePlan.of) word
        fun attempt => shapePermits alternate attempt.choice attempt.event attempt.receive
      else fun attempt => shapePermits states attempt.choice attempt.event attempt.receive
      for attempt in attempts do
        unless attempt.prefix == word do throw (IO.userError "inconsistent prefix group")
        let permitted := permits attempt
        checked := checked + 1
        if !attempt.prefix.isEmpty then continuations := continuations + 1
        if permitted != attempt.accepted then
          if runtimeOnly + modelOnly < 10 then
            IO.println s!"shape {input.name} choice={attempt.choice} prefix={attempt.prefix} event={attempt.event} receive={attempt.receive}: model={permitted} runtime={attempt.accepted}"
          if attempt.accepted then runtimeOnly := runtimeOnly + 1 else modelOnly := modelOnly + 1
    unless runtimeOnly == 0 && modelOnly == 0 do
      throw (IO.userError s!"generated Endpoint/source conformance failed: runtime-only={runtimeOnly} model-only={modelOnly}")
    let (runs, calls) ← shapeCheckCompleted source input.name input.parallelCoverage outputPath
    completedRuns := completedRuns + runs
    completedCalls := completedCalls + calls
  IO.println s!"Completed shape scripts/source: shapes={inputs.size} runs={completedRuns} calls={completedCalls} all accepted"
  IO.println s!"Generated shapes/source: shapes={inputs.size} attempts={checked} initial={checked-continuations} continuation={continuations} runtime-only={runtimeOnly} model-only={modelOnly}"
  pure 0
