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
  prefixChoices : List (List Nat)
  event : Nat
  receive : Bool
  accepted : Bool
  parked : List (Nat × Bool)
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

def shapePointEvent : SourcePoint → Nat
  | .accepted event => event
  | .committed action => action.event

def shapeScopeEntered (scope : Scope) (points : List SourcePoint) : Bool :=
  points.any fun point =>
    let event := shapePointEvent point
    scope.start ≤ event && event < scope.stop

-- A retained body inserted inside a continuation still belongs to its original
-- source scope. Its position in the expanded tree cannot choose the surrounding
-- continuation's resolver before a real operation of that scope is admitted.
def shapeResolverEnteredFrom (originals : List (Scope × Nat))
    (occurrences : List (Nat × Message)) (scope : Scope) (id : Nat)
    (points : List SourcePoint) : Bool :=
  shapeScopeEntered scope (points.filter fun point =>
    match occurrences[shapePointEvent point]? with
    | none => false
    | some (origin, _) =>
        originals.any fun (original, resolver) =>
          resolver == id && original.start ≤ origin && origin < original.stop)

def shapeResolverEntered (plan : SourceOccurrenceCut source) (scope : Scope) (id : Nat)
    (points : List SourcePoint) : Bool :=
  shapeResolverEnteredFrom (shapeResolvers source 0) (plan.unfolding.occurrences 0)
    scope id points

theorem an_entered_resolver_has_an_operation_in_its_original_source_scope
    (plan : SourceOccurrenceCut source) (scope : Scope) (id : Nat) (points : List SourcePoint)
    (entered : shapeResolverEntered plan scope id points = true) :
    ∃ point ∈ points, ∃ (origin : Nat) (message : Message) (original : Scope),
      (plan.unfolding.occurrences 0)[shapePointEvent point]? = some (origin, message) ∧
      (original, id) ∈ shapeResolvers source 0 ∧
      original.start ≤ origin ∧ origin < original.stop := by
  unfold shapeResolverEntered shapeResolverEnteredFrom shapeScopeEntered at entered
  obtain ⟨point, present, _⟩ := List.any_eq_true.mp entered
  obtain ⟨inPoints, selected⟩ := List.mem_filter.mp present
  cases originAt : (plan.unfolding.occurrences 0)[shapePointEvent point]? with
  | none => simp only [originAt, Bool.false_eq_true] at selected
  | some entry =>
    rcases entry with ⟨origin, message⟩
    simp only [originAt] at selected
    obtain ⟨⟨original, resolver⟩, inSource, fields⟩ := List.any_eq_true.mp selected
    have upper := Bool.and_eq_true_iff.mp fields
    have lower := Bool.and_eq_true_iff.mp upper.1
    have same : resolver = id := beq_iff_eq.mp lower.1
    subst resolver
    exact ⟨point, inPoints, origin, message, original, originAt, inSource,
      of_decide_eq_true lower.2, of_decide_eq_true upper.2⟩

#print axioms an_entered_resolver_has_an_operation_in_its_original_source_scope

-- Inputs choose only scopes entered by this call. Future visits stay free;
-- a scope already entered keeps the selection in this same source plan.
def shapeEntryChoicesFrom (originals resolvers : List (Scope × Nat))
    (occurrences : List (Nat × Message)) (selections : List Selection)
    (before after : List SourcePoint) (choice : List Nat) : Bool :=
  resolvers.all fun (scope, id) =>
    shapeResolverEnteredFrom originals occurrences scope id before ||
    !shapeResolverEnteredFrom originals occurrences scope id after ||
    selections.all fun selection =>
      selection.scope != scope || selection.arm ==
        if choice.contains id then .right else .left

def shapeEntryChoices (plan : SourceOccurrenceCut source)
    (before after : List SourcePoint) (choice : List Nat) : Bool :=
  shapeEntryChoicesFrom (shapeResolvers source 0) (shapeResolvers plan.expanded 0)
    (plan.unfolding.occurrences 0) plan.path.selections before after choice

theorem a_freshly_entered_resolver_uses_its_recorded_input
    (plan : SourceOccurrenceCut source) (before after : List SourcePoint)
    (choice : List Nat) (scope : Scope) (id : Nat) (selection : Selection)
    (checked : shapeEntryChoices plan before after choice = true)
    (present : (scope, id) ∈ shapeResolvers plan.expanded 0)
    (fresh : shapeResolverEntered plan scope id before = false)
    (entered : shapeResolverEntered plan scope id after = true)
    (selected : selection ∈ plan.path.selections) (same : selection.scope = scope) :
    (selection.arm == if choice.contains id then .right else .left) = true := by
  unfold shapeEntryChoices shapeEntryChoicesFrom at checked
  have scopeChecked := List.all_eq_true.mp checked (scope, id) present
  change (shapeResolverEntered plan scope id before ||
    !shapeResolverEntered plan scope id after || _) = true at scopeChecked
  simp only [fresh, entered, Bool.not_true, Bool.false_or] at scopeChecked
  have arm := List.all_eq_true.mp scopeChecked selection selected
  have reflexive : (scope != scope) = false := by
    cases scope
    simp [bne, BEq.beq, instBEqScope.beq]
  simpa only [same, reflexive, Bool.false_or] using arm

#print axioms a_freshly_entered_resolver_uses_its_recorded_input

theorem shapeLookupMappedRows (keys : List SourcePoint)
    (value : SourcePoint → α) (point : SourcePoint) :
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

def shapeResumptionRows (plan : SourceOccurrenceCut source) :
    List (SourcePoint × List ResumptionQuery) :=
  (sourceVisitPoints plan.path.bindings).map fun point => (point, plan.prepareResumption point)

def shapeResumptionsReady (rows : List (SourcePoint × List ResumptionQuery))
    (points : List SourcePoint) (point : SourcePoint) : Bool :=
  match rows.lookup point with
  | none => false
  | some checks => checks.all (fun query => query.check points.contains)

def shapeOriginalActorsMatch (plan : SourceOccurrenceCut source) : Bool :=
  plan.path.bindings.all fun (event, binding) =>
    match (plan.unfolding.occurrences 0)[event]? with
    | none => false
    | some (origin, message) => decide (binding.message = message) &&
        plan.original.paths.any fun path =>
          path.bindings.any fun original => decide (original = (origin, binding))

structure ShapePlan (source : Source) where
  plan : SourceOccurrenceCut source
  dependencies : List (SourcePoint × List SourcePoint)
  dependencies_eq : dependencies = shapeDependencyRows plan
  resumptions : List (SourcePoint × List ResumptionQuery)
  resumptions_eq : resumptions = shapeResumptionRows plan
  entryChoices : List SourcePoint → List SourcePoint → List Nat → Bool
  entryChoices_eq : ∀ before after choice,
    entryChoices before after choice = shapeEntryChoices plan before after choice

def ShapePlan.of (plan : SourceOccurrenceCut source) : ShapePlan source :=
  let originals := shapeResolvers source 0
  let resolvers := shapeResolvers plan.expanded 0
  let occurrences := plan.unfolding.occurrences 0
  ⟨plan, shapeDependencyRows plan, rfl, shapeResumptionRows plan, rfl,
    shapeEntryChoicesFrom originals resolvers occurrences plan.path.selections,
    fun _ _ _ => rfl⟩

theorem ShapePlan.entryChoices_matches_source (prepared : ShapePlan source)
    (before after : List SourcePoint) (choice : List Nat) :
    prepared.entryChoices before after choice = shapeEntryChoices prepared.plan before after choice :=
  prepared.entryChoices_eq before after choice

def ShapePlan.ready (prepared : ShapePlan source) (points : List SourcePoint)
    (point : SourcePoint) : Bool :=
  shapeRowsReady prepared.dependencies points point &&
    shapeResumptionsReady prepared.resumptions points point

theorem ShapePlan.ready_matches_source (prepared : ShapePlan source)
    (points : List SourcePoint) (point : SourcePoint) :
    prepared.ready points point = ((occurrenceCut prepared.plan points).ready point &&
      (occurrenceCut prepared.plan points).resumptionReady point) := by
  unfold ShapePlan.ready
  rw [prepared.dependencies_eq, prepared.resumptions_eq]
  by_cases present : point ∈ sourceVisitPoints prepared.plan.path.bindings
  · rw [shapeRowsReady_matches_source_readiness]
    have observations : points.contains = (fun observed => decide (observed ∈ points)) := by
      funext observed
      exact List.contains_eq_mem observed points
    simp [shapeResumptionsReady, shapeResumptionRows, shapeLookupMappedRows, present,
      prepared_resumption_matches_source, occurrenceCut, observations]
  · simp [shapeRowsReady, shapeDependencyRows, shapeLookupMappedRows, present,
      SourceOccurrenceCut.ready, occurrenceCut]

#print axioms shapeLookupMappedRows
#print axioms shapeRowsReady_matches_source_readiness
#print axioms ShapePlan.ready_matches_source
#print axioms ShapePlan.entryChoices_matches_source

theorem shapeOriginalActorsMatch_checks_each_occurrence (plan : SourceOccurrenceCut source)
    (checked : shapeOriginalActorsMatch plan = true)
    (present : (event, binding) ∈ plan.path.bindings) :
    ∃ origin message, (plan.unfolding.occurrences 0)[event]? = some (origin, message) ∧
      binding.message = message ∧
      ∃ path ∈ plan.original.paths, (origin, binding) ∈ path.bindings := by
  have checkedAt := List.all_eq_true.mp checked (event, binding) present
  cases originAt : (plan.unfolding.occurrences 0)[event]? with
  | none => simp only [originAt, Bool.false_eq_true] at checkedAt
  | some entry =>
      obtain ⟨origin, message⟩ := entry
      have fields := Bool.and_eq_true_iff.mp (by simpa only [originAt] using checkedAt)
      obtain ⟨path, member, found⟩ := List.any_eq_true.mp fields.2
      obtain ⟨original, originalMember, same⟩ := List.any_eq_true.mp found
      have same : original = (origin, binding) := of_decide_eq_true same
      exact ⟨origin, message, rfl, of_decide_eq_true fields.1,
        path, member, same ▸ originalMember⟩

#print axioms shapeOriginalActorsMatch_checks_each_occurrence

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
    (shapeAdvance plan points label receive).any fun after =>
      plan.entryChoices points after choice

def shapeInputStep (states : List (ShapePlan source × List SourcePoint))
    (command : Nat) (choice : List Nat) : List (ShapePlan source × List SourcePoint) :=
  states.flatMap fun (plan, points) =>
    ((shapeAdvance plan points (command / 2) (command % 2 == 1)).filter fun after =>
      plan.entryChoices points after choice).map (plan, ·)

-- Resolver inputs belong to a call, not to the source alias used to name it.
-- Keep one coherent source-plan witness through all the recorded calls.
def shapeReplayStates (source : Source) (plans : List (ShapePlan source))
    (word : List Nat) (inputs : List (List Nat)) :
    Option (List (ShapePlan source × List SourcePoint)) :=
  if word.length != inputs.length then none else
    let states := (word.zip inputs).foldl (fun states (command, choice) =>
      shapeInputStep states command choice) (plans.map (·, []))
    if states.isEmpty then none else some states

def shapeRepetitions : Source → Nat
  | .send _ => 0
  | .seq left right | .par left right | .route _ left right =>
      shapeRepetitions left + shapeRepetitions right
  | .roll body => 1 + shapeRepetitions body

-- Completed scripts use two visits at each roll. A prefix may leave a roll
-- after its first visit, including a nested roll in just one outer visit.
-- Admission therefore uses every one/two-visit cut, not one fixed expansion.
-- Next-call probes additionally allow a third visit at one dynamic roll site.
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

def shapeRollUnfoldings (cuts : List (Σ expanded : Source, SourceUnfolding body expanded)) :
    List (Σ expanded : Source, SourceUnfolding (.roll body) expanded) :=
  cuts.map (fun cut => ⟨cut.1, .rollOnce cut.2⟩) ++
    cuts.flatMap fun first => cuts.map fun second =>
      ⟨.seq first.1 second.1, .rollMore (.rollOnce first.2) second.2⟩

def shapeBoundedUnfoldings :
    (source : Source) → List (Σ expanded : Source, SourceUnfolding source expanded)
  | .send message => [⟨.send message, .send message⟩]
  | .seq left right =>
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.seq first.1 second.1, .seq first.2 second.2⟩
  | .par left right =>
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.par first.1 second.1, .par first.2 second.2⟩
  | .route resolver left right =>
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.route resolver first.1 second.1, .route resolver first.2 second.2⟩
  | .roll (.par left right) =>
      shapeRollUnfoldings (shapeBoundedUnfoldings (.par left right)) ++
        (shapeRollUnfoldings (shapeBoundedUnfoldings left)).flatMap fun first =>
          (shapeRollUnfoldings (shapeBoundedUnfoldings right)).map fun second =>
            ⟨.par first.1 second.1, .rollParallel first.2 second.2⟩
  | .roll body => shapeRollUnfoldings (shapeBoundedUnfoldings body)

def shapeExtraUnfoldings :
    (source : Source) → List (Σ expanded : Source, SourceUnfolding source expanded)
  | .send _ => []
  | .seq left right =>
      ((shapeExtraUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.seq first.1 second.1, .seq first.2 second.2⟩) ++
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeExtraUnfoldings right).map fun second =>
          ⟨.seq first.1 second.1, .seq first.2 second.2⟩
  | .par left right =>
      ((shapeExtraUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.par first.1 second.1, .par first.2 second.2⟩) ++
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeExtraUnfoldings right).map fun second =>
          ⟨.par first.1 second.1, .par first.2 second.2⟩
  | .route resolver left right =>
      ((shapeExtraUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.route resolver first.1 second.1, .route resolver first.2 second.2⟩) ++
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeExtraUnfoldings right).map fun second =>
          ⟨.route resolver first.1 second.1, .route resolver first.2 second.2⟩
  | .roll body =>
      let cuts := shapeBoundedUnfoldings body
      let extra := shapeExtraUnfoldings body
      (extra.map fun cut => ⟨cut.1, .rollOnce cut.2⟩) ++
      (extra.flatMap fun first => cuts.map fun second =>
        ⟨.seq first.1 second.1, .rollMore (.rollOnce first.2) second.2⟩) ++
      (cuts.flatMap fun first => extra.map fun second =>
        ⟨.seq first.1 second.1, .rollMore (.rollOnce first.2) second.2⟩) ++
      cuts.flatMap fun first => cuts.flatMap fun second => cuts.map fun third =>
        ⟨.seq (.seq first.1 second.1) third.1,
          .rollMore (.rollMore (.rollOnce first.2) second.2) third.2⟩

structure ShapeRollFocus (source : Source) where
  context : RollContext
  body : Source
  source_eq : context.plug (.roll body) = source

-- Contexts retain the selected anchor of the last enclosing visit. The source
-- cut checks that certificate before a guarded resumption becomes a plan.
def shapeRollFocuses : (source : Source) → List (ShapeRollFocus source)
  | .send _ => []
  | .roll body => ⟨.hole, body, rfl⟩ :: ((shapeRollFocuses body).map fun focus =>
      ⟨.roll focus.context, focus.body, by simp only [RollContext.plug, focus.source_eq]⟩)
  | .seq left right =>
      ((shapeRollFocuses left).map fun focus =>
        ⟨.seqLeft focus.context right, focus.body,
          by simp only [RollContext.plug, focus.source_eq]⟩) ++
      (shapeRollFocuses right).map fun focus =>
        ⟨.seqRight left focus.context, focus.body,
          by simp only [RollContext.plug, focus.source_eq]⟩
  | .par left right =>
      ((shapeRollFocuses left).map fun focus =>
        ⟨.parLeft focus.context right, focus.body,
          by simp only [RollContext.plug, focus.source_eq]⟩) ++
      (shapeRollFocuses right).map fun focus =>
        ⟨.parRight left focus.context, focus.body,
          by simp only [RollContext.plug, focus.source_eq]⟩
  | .route resolver left right =>
      ((shapeRollFocuses left).map fun focus =>
        ⟨.routeLeft resolver focus.context right, focus.body,
          by simp only [RollContext.plug, focus.source_eq]⟩) ++
      (shapeRollFocuses right).map fun focus =>
        ⟨.routeRight resolver left focus.context, focus.body,
          by simp only [RollContext.plug, focus.source_eq]⟩

-- Every subtree is an insertion point, including a point between two messages
-- in one route arm. The equality retains the unmodified continuation around it.
def shapeContinuationCuts : (source : Source) →
    List (Σ context : RollContext, { focus : Source // context.plug focus = source })
  | .send message => [⟨.hole, .send message, rfl⟩]
  | .seq left right =>
      ⟨.hole, .seq left right, rfl⟩ ::
      ((shapeContinuationCuts left).map fun ⟨context, focus, equal⟩ =>
        ⟨.seqLeft context right, focus, by simp only [RollContext.plug, equal]⟩) ++
      (shapeContinuationCuts right).map fun ⟨context, focus, equal⟩ =>
        ⟨.seqRight left context, focus, by simp only [RollContext.plug, equal]⟩
  | .par left right =>
      ⟨.hole, .par left right, rfl⟩ ::
      ((shapeContinuationCuts left).map fun ⟨context, focus, equal⟩ =>
        ⟨.parLeft context right, focus, by simp only [RollContext.plug, equal]⟩) ++
      (shapeContinuationCuts right).map fun ⟨context, focus, equal⟩ =>
        ⟨.parRight left context, focus, by simp only [RollContext.plug, equal]⟩
  | .route resolver left right =>
      ⟨.hole, .route resolver left right, rfl⟩ ::
      ((shapeContinuationCuts left).map fun ⟨context, focus, equal⟩ =>
        ⟨.routeLeft resolver context right, focus, by simp only [RollContext.plug, equal]⟩) ++
      (shapeContinuationCuts right).map fun ⟨context, focus, equal⟩ =>
        ⟨.routeRight resolver left context, focus, by simp only [RollContext.plug, equal]⟩
  | .roll body =>
      ⟨.hole, .roll body, rfl⟩ ::
      (shapeContinuationCuts body).map fun ⟨context, focus, equal⟩ =>
        ⟨.roll context, focus, by simp only [RollContext.plug, equal]⟩

def shapeAlongsidePlans (focus : ShapeRollFocus left) (right : Source) :
    List (Σ expanded : Source, SourceUnfolding (.seq left right) expanded) :=
  match focus with
  | ⟨context, body, equal⟩ =>
      equal ▸ ((shapeBoundedUnfoldings (context.plug (.roll body))).flatMap fun earlier =>
        (shapeBoundedUnfoldings body).flatMap fun next =>
          (shapeBoundedUnfoldings right).flatMap fun tail =>
            (shapeContinuationCuts tail.1).flatMap fun ⟨insertion, part, same⟩ =>
              let retained : SourceUnfolding right (insertion.plug part) :=
                by simpa only [same] using tail.2
              [⟨.seq earlier.1 (insertion.plug (.par next.1 part)),
                .resumeAlongsideContinuation context insertion earlier.2 (.rollOnce next.2) retained⟩,
               ⟨.seq earlier.1 (insertion.plug (.seq next.1 part)),
                .resumeBeforeContinuation context insertion earlier.2 (.rollOnce next.2) retained⟩])

def shapeAfterPlans (focus : ShapeRollFocus left) (right : Source) :
    List (Σ expanded : Source, SourceUnfolding (.seq left right) expanded) :=
  match focus with
  | ⟨context, body, equal⟩ =>
      equal ▸ ((shapeBoundedUnfoldings (.seq (context.plug (.roll body)) right)).flatMap fun earlier =>
        (shapeBoundedUnfoldings body).map fun next =>
          ⟨.seq earlier.1 next.1,
            .resumeAfterContinuation context earlier.2 next.2⟩)

-- The next public call may revisit a roll after its continuation has run.
-- Use the established unfolding rule: a completed earlier copy releases both
-- the next copy and the continuation. Their relative order is unrestricted.
-- The extra visit occurs at one dynamic site; all other visits use the bounded
-- cuts above. Every candidate carries its SourceUnfolding witness.
def shapeElasticUnfoldings :
    (source : Source) → List (Σ expanded : Source, SourceUnfolding source expanded)
  | .send _ => []
  | .seq left right =>
      let inside : List (Σ expanded : Source, SourceUnfolding (.seq left right) expanded) :=
        ((shapeElasticUnfoldings left).flatMap fun first =>
          (shapeBoundedUnfoldings right).map fun second =>
            ⟨.seq first.1 second.1, .seq first.2 second.2⟩) ++
        (shapeBoundedUnfoldings left).flatMap fun first =>
          (shapeElasticUnfoldings right).map fun second =>
            ⟨.seq first.1 second.1, .seq first.2 second.2⟩
      inside ++ (shapeRollFocuses left).flatMap fun focus =>
        shapeAlongsidePlans focus right ++ shapeAfterPlans focus right
  | .par left right =>
      ((shapeElasticUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.par first.1 second.1, .par first.2 second.2⟩) ++
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeElasticUnfoldings right).map fun second =>
          ⟨.par first.1 second.1, .par first.2 second.2⟩
  | .route resolver left right =>
      ((shapeElasticUnfoldings left).flatMap fun first =>
        (shapeBoundedUnfoldings right).map fun second =>
          ⟨.route resolver first.1 second.1, .route resolver first.2 second.2⟩) ++
      (shapeBoundedUnfoldings left).flatMap fun first =>
        (shapeElasticUnfoldings right).map fun second =>
          ⟨.route resolver first.1 second.1, .route resolver first.2 second.2⟩
  | .roll body =>
      let bounded := shapeBoundedUnfoldings body
      let elastic := shapeElasticUnfoldings body
      (elastic.map fun cut => ⟨cut.1, .rollOnce cut.2⟩) ++
      (elastic.flatMap fun first => bounded.map fun second =>
        ⟨.seq first.1 second.1, .rollMore (.rollOnce first.2) second.2⟩) ++
      (bounded.flatMap fun first => elastic.map fun second =>
        ⟨.seq first.1 second.1, .rollMore (.rollOnce first.2) second.2⟩)

def shapePlans : (source : Source) → List (SourceOccurrenceCut source)
  | source =>
      let cuts := shapeBoundedUnfoldings source ++ shapeExtraUnfoldings source ++
        shapeElasticUnfoldings source
      -- Check every operation in the selected expansion. An incorrectly reassigned
      -- resource operation could disappear from this actor's prerequisites,
      -- incorrectly releasing its following ordinary send.
      (cuts.flatMap fun cut => checkedPlans source cut.1 cut.2).filter shapeOriginalActorsMatch

private def disjointReturnedBranch : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .endpoint ⟨0, 3⟩⟩)
  (.seq (.roll (.par
    (.send ⟨⟨0, 3⟩, ⟨0, 2⟩, 1, .wire 0⟩)
    (.send ⟨⟨0, 4⟩, ⟨0, 5⟩, 2, .wire 0⟩)))
    (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 3, .endpoint ⟨0, 3⟩⟩))

theorem shape_checker_keeps_a_disjoint_branch_after_resource_return :
    shapePermits (shapeStates disjointReturnedBranch
      ((shapePlans disjointReturnedBranch).map ShapePlan.of) [0, 1, 2, 3, 4, 5, 6, 7])
      [] 2 false = true := by decide

-- Equal whole-body visit counts are insufficient even for this two-leaf par.
-- The control uses valid existing unfoldings, rather than changed Rust results.
theorem a_shared_whole_body_unfolding_misses_the_disjoint_revisit :
    let one := SourceUnfolding.oneCopy disjointReturnedBranch
    let two := shapeTwoCopyUnfolding disjointReturnedBranch
    let plans := (checkedPlans disjointReturnedBranch _ one ++
      checkedPlans disjointReturnedBranch _ two).filter shapeOriginalActorsMatch
    shapePermits (shapeStates disjointReturnedBranch (plans.map ShapePlan.of)
      [0, 1, 2, 3, 4, 5, 6, 7]) [] 2 false = false := by decide

private def nestedRollCuts : Source := .roll (.seq
  (.roll (.send ⟨⟨0, 0⟩, ⟨0, 0⟩, 0, .wire 0⟩))
  (.send ⟨⟨0, 0⟩, ⟨0, 0⟩, 1, .wire 0⟩))

private def resumedOutgoingTail : Source := .seq
  (.roll (.seq
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .wire 0⟩)
    (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 1, .wire 0⟩)))
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 2, .wire 0⟩)

theorem a_resumed_sender_finishes_its_current_body_before_the_continuation :
    shapePermits (shapeStates resumedOutgoingTail
      ((shapePlans resumedOutgoingTail).map ShapePlan.of) [0, 1, 2, 3, 0]) [] 2 false = false := by decide

theorem a_completed_resumed_body_releases_the_continuation :
    shapePermits (shapeStates resumedOutgoingTail
      ((shapePlans resumedOutgoingTail).map ShapePlan.of) [0, 1, 2, 3, 0, 1, 2, 3]) [] 2 false = true := by decide

#print axioms a_resumed_sender_finishes_its_current_body_before_the_continuation
#print axioms a_completed_resumed_body_releases_the_continuation

private def parallelRetainedRolls : Source := .seq
  (.par
    (.seq (.roll (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 0, .wire 0⟩))
      (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .wire 0⟩))
    (.seq (.roll (.send ⟨⟨0, 1⟩, ⟨0, 2⟩, 2, .wire 0⟩))
      (.send ⟨⟨0, 2⟩, ⟨0, 1⟩, 3, .wire 0⟩)))
  (.send ⟨⟨0, 1⟩, ⟨0, 1⟩, 4, .wire 0⟩)

theorem a_left_roll_can_resume_after_the_common_parallel_continuation :
    shapePermits (shapeStates parallelRetainedRolls
      ((shapePlans parallelRetainedRolls).map ShapePlan.of)
      [0, 1, 0, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8]) [] 0 false = true := by decide

theorem a_right_roll_can_resume_after_the_common_parallel_continuation :
    shapePermits (shapeStates parallelRetainedRolls
      ((shapePlans parallelRetainedRolls).map ShapePlan.of)
      [0, 1, 0, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8]) [] 2 false = true := by decide

#print axioms a_left_roll_can_resume_after_the_common_parallel_continuation
#print axioms a_right_roll_can_resume_after_the_common_parallel_continuation

private def returnedRollEndpoint : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .endpoint ⟨0, 3⟩⟩)
  (.seq
    (.seq (.roll (.send ⟨⟨0, 3⟩, ⟨0, 2⟩, 1, .wire 0⟩))
      (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 2, .endpoint ⟨0, 3⟩⟩))
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 3, .endpoint ⟨0, 3⟩⟩))

private def ownerPreservingContinuation : Source := .seq
  (.roll (.seq
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint ⟨0, 3⟩⟩)
    (.seq (.send ⟨⟨0, 3⟩, ⟨0, 1⟩, 4, .wire 0⟩)
      (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 1, .endpoint ⟨0, 3⟩⟩))))
  (.route (some 17)
    (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 2, .wire 0⟩)
    (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 3, .endpoint ⟨0, 3⟩⟩))

private def incomingLoanContinuation : Source := .seq
  (.roll (.seq
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint ⟨0, 3⟩⟩)
    (.seq (.send ⟨⟨0, 3⟩, ⟨0, 1⟩, 4, .wire 0⟩)
      (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 1, .endpoint ⟨0, 3⟩⟩))))
  (.route (some 0)
    (.send ⟨⟨0, 2⟩, ⟨0, 0⟩, 2, .wire 0⟩)
    (.seq (.send ⟨⟨0, 2⟩, ⟨0, 0⟩, 5, .wire 0⟩)
      (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 3, .endpoint ⟨0, 3⟩⟩)))

theorem a_new_holder_cannot_inherit_another_actors_rolled_operation :
    shapePermits (shapeStates returnedRollEndpoint
      ((shapePlans returnedRollEndpoint).map ShapePlan.of)
      [0, 1, 2, 3, 2, 3, 4, 5]) [] 1 false = false := by decide

theorem returning_the_endpoint_to_the_roll_actor_allows_reentry :
    shapePermits (shapeStates returnedRollEndpoint
      ((shapePlans returnedRollEndpoint).map ShapePlan.of)
      [0, 1, 2, 3, 2, 3, 4, 5, 6, 7]) [] 1 false = true := by decide

#print axioms a_new_holder_cannot_inherit_another_actors_rolled_operation
#print axioms returning_the_endpoint_to_the_roll_actor_allows_reentry

theorem an_inner_roll_can_exit_after_its_first_visit :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0]) [] 1 false = true := by decide

theorem nested_roll_visits_can_have_different_lengths :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 0, 2, 0]) [] 1 false = true := by decide

theorem the_next_call_can_start_a_third_outer_visit :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 0, 2, 0, 0, 2]) [] 0 false = true := by decide

theorem a_completed_tail_cannot_repeat_without_its_predecessor :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 2]) [] 1 false = false := by decide

theorem a_completed_continuation_does_not_close_its_inner_roll :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 0, 2]) [] 0 false = true := by decide

#print axioms an_inner_roll_can_exit_after_its_first_visit
#print axioms nested_roll_visits_can_have_different_lengths
#print axioms the_next_call_can_start_a_third_outer_visit
#print axioms a_completed_tail_cannot_repeat_without_its_predecessor
#print axioms a_completed_continuation_does_not_close_its_inner_roll

private def guardedInnerRoll : Source := .seq
  (.roll (.route none
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .wire 0⟩)
    (.seq
      (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .wire 0⟩)
      (.roll (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 2, .wire 0⟩)))))
  (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 3, .wire 0⟩)

private def returnedConditionalHolder : Source := .seq
  (.route none
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .wire 0⟩)
    (.seq
      (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .endpoint ⟨0, 3⟩⟩)
      (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 2, .endpoint ⟨0, 2⟩⟩)))
  (.seq (.roll (.send ⟨⟨0, 3⟩, ⟨0, 0⟩, 3, .wire 0⟩))
    (.send ⟨⟨0, 2⟩, ⟨0, 0⟩, 4, .endpoint ⟨0, 3⟩⟩))

private def movedUnrelatedResource : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .endpoint ⟨0, 3⟩⟩)
  (.seq (.roll (.send ⟨⟨0, 3⟩, ⟨0, 0⟩, 1, .wire 0⟩))
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 2, .endpoint ⟨0, 2⟩⟩))

def shapeWordPrefixes (word : List Nat) : List (List Nat) :=
  (List.range (word.length + 1)).map (word.take ·)

theorem shapeWordPrefixes_contains_empty (word : List Nat) :
    [] ∈ shapeWordPrefixes word := by
  exact List.mem_map.mpr ⟨0, by simp, by simp⟩

theorem shapeWordPrefixes_contains_the_complete_word (word : List Nat) :
    word ∈ shapeWordPrefixes word := by
  exact List.mem_map.mpr ⟨word.length, List.mem_range.mpr (Nat.lt_succ_self _), by simp⟩

theorem shapeWordPrefixes_has_only_bounded_cuts (word cutWord : List Nat)
    (present : cutWord ∈ shapeWordPrefixes word) :
    ∃ cut ≤ word.length, cutWord = word.take cut := by
  obtain ⟨cut, member, same⟩ := List.mem_map.mp present
  exact ⟨cut, Nat.le_of_lt_succ (List.mem_range.mp member), same.symm⟩

#print axioms shapeWordPrefixes_contains_empty
#print axioms shapeWordPrefixes_contains_the_complete_word
#print axioms shapeWordPrefixes_has_only_bounded_cuts

-- Exhaust every order of the first four public operations for finite sources.
-- The complete-trace prefixes above also reach deeper positions in each tree.
def shapeShortPrefixes (source : Source) (plans : List (ShapePlan source))
    (choices : List (List Nat)) : List (List Nat × List (List Nat)) :=
  if shapeRepetitions source != 0 then [] else Id.run do
    let mut level := [([], [])]
    let mut found := []
    for _ in List.range 4 do
      let mut next := []
      for (word, inputs) in level do
        let some states := shapeReplayStates source plans word inputs | continue
        for (event, _) in source.messageEntries 0 do
          for receive in [false, true] do
            for choice in choices do
              if shapePermits states choice event receive then
                next := (word ++ [2 * event + if receive then 1 else 0], inputs ++ [choice]) :: next
      level := next.eraseDups
      found := found ++ level
    return found

-- Reject omitted/duplicate probes independently of the observed outcome.
def shapeProbeKeys (source : Source)
    (prefixes : List (List Nat × List (List Nat))) (choices : List (List Nat)) :
    List (List Nat × List Nat × List (List Nat) × Nat × Bool) :=
  choices.flatMap fun choice =>
    prefixes.flatMap fun (word, inputs) =>
      (source.messageEntries 0).flatMap fun (event, _) =>
        [(choice, word, inputs, event, false), (choice, word, inputs, event, true)]

theorem probe_identity_preserves_the_recorded_input_history
    (choice word : List Nat) (a b : List (List Nat)) (event : Nat) (receive : Bool) :
    (choice, word, a, event, receive) = (choice, word, b, event, receive) ↔ a = b := by
  simp

#print axioms probe_identity_preserves_the_recorded_input_history

-- Completed scripts name exact source occurrences. Derive their persistent
-- resolver input independently from each enclosing source scope.
def shapeWordInputs (source : Source) (word : List Nat) : List (List Nat) :=
  (word.foldl (fun (state : List Nat × List (List Nat)) command =>
    let inputs := (shapeResolvers source 0).foldl (fun inputs (scope, id) =>
      if scope.start ≤ command / 2 && command / 2 < scope.stop then
        let without := inputs.filter (· != id)
        if scope.split ≤ command / 2 then (id :: without).mergeSort (· ≤ ·) else without
      else inputs) state.1
    (inputs, state.2 ++ [inputs])) ([], [])).2

private def sameContractSequence : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 7, .wire 0⟩)
  (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 7, .wire 0⟩)

private def resolvedAliasSequence : Source := .route (some 0)
  (.seq (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .wire 0⟩)
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .wire 0⟩))
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .wire 0⟩)

theorem a_resolved_alias_prefix_keeps_the_input_used_at_admission :
    let plans := (shapePlans resolvedAliasSequence).map ShapePlan.of
    (shapeReplayStates resolvedAliasSequence plans [2] [[0]]).isSome = true ∧
      (shapeReplayStates resolvedAliasSequence plans [2] [[]]).isSome = false := by decide

#print axioms a_resolved_alias_prefix_keeps_the_input_used_at_admission

private def sharedResolvedEntry : Source := .route (some 0)
  (.seq (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 7, .wire 0⟩)
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 8, .wire 0⟩))
  (.seq (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 7, .wire 0⟩)
    (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 9, .wire 0⟩))

theorem a_shared_entry_keeps_the_recorded_left_arm :
    let plans := (shapePlans sharedResolvedEntry).map ShapePlan.of
    let states := (shapeReplayStates sharedResolvedEntry plans [0] [[]]).getD []
    (shapeReplayStates sharedResolvedEntry plans [0] [[]]).isSome = true ∧
      shapePermits states [0] 1 false = true ∧
      shapePermits states [] 3 false = false := by decide

theorem a_shared_entry_keeps_the_recorded_right_arm :
    let plans := (shapePlans sharedResolvedEntry).map ShapePlan.of
    let states := (shapeReplayStates sharedResolvedEntry plans [0] [[0]]).getD []
    (shapeReplayStates sharedResolvedEntry plans [0] [[0]]).isSome = true ∧
      shapePermits states [] 3 false = true ∧
      shapePermits states [0] 1 false = false := by decide

theorem replay_does_not_choose_a_future_resolver_before_entry :
    let source := Source.seq (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 20, .wire 0⟩) sharedResolvedEntry
    let plans := (shapePlans source).map ShapePlan.of
    let states := (shapeReplayStates source plans [0, 1, 2] [[0], [0], []]).getD []
    (shapeReplayStates source plans [0, 1, 2] [[0], [0], []]).isSome = true ∧
      shapePermits states [0] 2 false = true ∧
      shapePermits states [] 4 false = false := by decide

theorem repeated_entries_can_use_different_recorded_inputs_in_each_visit :
    let source := Source.roll sharedResolvedEntry
    let plans := (shapePlans source).map ShapePlan.of
    let word := [0, 1, 2, 3, 0]
    let inputs := [[], [], [], [], [0]]
    let states := (shapeReplayStates source plans word inputs).getD []
    (shapeReplayStates source plans word inputs).isSome = true ∧
      shapePermits states [] 3 false = true ∧
      shapePermits states [0] 1 false = false := by decide

#print axioms a_shared_entry_keeps_the_recorded_left_arm
#print axioms a_shared_entry_keeps_the_recorded_right_arm
#print axioms replay_does_not_choose_a_future_resolver_before_entry
#print axioms repeated_entries_can_use_different_recorded_inputs_in_each_visit

theorem an_alias_for_a_later_send_can_issue_the_first_source_operation :
    shapePermits (shapeStates sameContractSequence
      ((shapePlans sameContractSequence).map ShapePlan.of) []) [] 1 false = true := by decide

theorem repeated_aliases_consume_both_occurrences_and_cannot_replay_them :
    let states := shapeStates sameContractSequence
      ((shapePlans sameContractSequence).map ShapePlan.of) [2, 2]
    !states.isEmpty ∧ shapePermits states [] 0 false = false := by decide

#print axioms an_alias_for_a_later_send_can_issue_the_first_source_operation
#print axioms repeated_aliases_consume_both_occurrences_and_cannot_replay_them

private def independentReceiveAlias : Source := .par
  (.route none
    (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 0, .wire 0⟩)
    (.send ⟨⟨0, 0⟩, ⟨0, 2⟩, 1, .wire 0⟩))
  (.send ⟨⟨0, 1⟩, ⟨0, 2⟩, 0, .wire 0⟩)

theorem an_unread_selected_arm_does_not_block_an_independent_receive_alias :
    let plans := (shapePlans independentReceiveAlias).map ShapePlan.of
    shapePermits (shapeStates independentReceiveAlias plans [2, 4]) [] 0 true = true ∧
      shapePermits (shapeStates independentReceiveAlias plans [2, 4, 1]) [] 1 true = true ∧
      shapePermits (shapeStates independentReceiveAlias plans [2, 4, 1]) [] 0 true = false := by
  decide

#print axioms an_unread_selected_arm_does_not_block_an_independent_receive_alias

def shapeKeyLess (a b : List Nat × List Nat × List (List Nat) × Nat × Bool) : Bool :=
  decide (a.1 < b.1) || (a.1 == b.1 && (decide (a.2.1 < b.2.1) ||
    (a.2.1 == b.2.1 && (decide (a.2.2.1 < b.2.2.1) ||
      (a.2.2.1 == b.2.2.1 && (a.2.2.2.1 < b.2.2.2.1 ||
        (a.2.2.2.1 == b.2.2.2.1 && !a.2.2.2.2 && b.2.2.2.2)))))))

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
      if attempt.prefix == word && attempts.head?.all (·.prefixChoices == attempt.prefixChoices) then (word, attempt :: attempts) :: rest
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

-- Validate the immutable script against the source. Its rank identifies its
-- choices, visit boundaries and parallel orders. Its word supplies the exact
-- public cuts to probe, including every selected parallel schedule.
def shapeScriptKey (source : Source) (script : Lean.Json) (start origin next : Nat) :
    Except String (List String × Nat × Nat × List Nat) := do
  let .arr fields := script | throw "script node is not an array"
  unless !fields.isEmpty do throw "empty script node"
  let kind : Nat ← Lean.fromJson? fields[0]!
  match source with
  | .send message =>
    unless kind == 0 && fields.size == 3 do throw "invalid message script"
    let label : Nat ← Lean.fromJson? fields[1]!
    let occurrence : Nat ← Lean.fromJson? fields[2]!
    unless label == origin && occurrence == next do throw "wrong script message or occurrence"
    pure ([], 0, next + 1,
      if message.sender == message.receiver then [2 * origin] else [2 * origin, 2 * origin + 1])
  | .seq left right | .par left right =>
    let parallel := match source with | .par _ _ => true | _ => false
    unless kind == (if parallel then 2 else 1) && fields.size == (if parallel then 4 else 3) do
      throw "invalid sequential or parallel script"
    let reverse : Bool ← if parallel then Lean.fromJson? fields[3]! else pure false
    let (first, firstRank, next, firstWord) ← shapeScriptKey left fields[1]! start origin next
    let (second, secondRank, next, secondWord) ← shapeScriptKey right fields[2]!
      (start + (shapeTwoCopies left).eventCount) (origin + left.eventCount) next
    let rank := firstRank * shapeScriptCount right + secondRank
    pure (first ++ second, if parallel then 2 * rank + (if reverse then 1 else 0) else rank, next,
      if reverse then secondWord ++ firstWord else firstWord ++ secondWord)
  | .route _ left right =>
    unless kind == 3 && fields.size == 3 do throw "invalid choice script"
    let chooseRight : Bool ← Lean.fromJson? fields[2]!
    let split := start + (shapeTwoCopies left).eventCount
    let scope : Scope := ⟨start, split, split + (shapeTwoCopies right).eventCount⟩
    let (choices, rank, next, word) ← if chooseRight then
      shapeScriptKey right fields[1]! split (origin + left.eventCount) next
    else shapeScriptKey left fields[1]! start origin next
    pure (shapeChoiceKey scope chooseRight :: choices,
      if chooseRight then shapeScriptCount left + rank else rank, next, word)
  | .roll body =>
    unless kind == 4 && fields.size == 3 do throw "invalid repeated script"
    let (first, firstRank, next, firstWord) ← shapeScriptKey body fields[1]! start origin next
    let (second, secondRank, next, secondWord) ← shapeScriptKey body fields[2]!
      (start + (shapeTwoCopies body).eventCount) origin next
    pure (first ++ second, firstRank * shapeScriptCount body + secondRank, next,
      firstWord ++ secondWord)

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
    (outputPath : String) : IO (Nat × Nat × List (List Nat)) := do
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
  let mut prefixes : Std.HashMap (List Nat) Unit := {}
  for index in [:count] do
    let script : Lean.Json ← shapeReadLine scripts
    let (choices, rank, messages, word) ← match shapeScriptKey source script 0 0 0 with
      | .ok value => pure value
      | .error error => throw (IO.userError s!"shape {name} script {index}: {error}")
    unless (parallelCoverage || rank == index) &&
        previousRank.all (fun previous => previous < rank) do
      throw (IO.userError s!"shape {name}: missing or duplicate completed script")
    previousRank := some rank
    for cutWord in shapeWordPrefixes word do
      prefixes := prefixes.insert cutWord ()
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
  pure (2 * count, calls, prefixes.toList.map Prod.fst)

def main (args : List String) : IO UInt32 := do
  -- Evaluate the whole bounded plan set natively. The general anchor theorem
  -- is kernel checked; materializing all these plans in a `by decide` proof
  -- duplicates the exhaustive search in the proof term.
  let guardedPlans := (shapePlans guardedInnerRoll).map ShapePlan.of
  for (word, expected) in [
      ([0, 1, 2, 3, 4, 5, 6, 7], true),
      ([0, 1, 2, 3, 4, 5, 6], true),
      ([2, 3, 4, 5, 0, 1, 6, 7], false),
      ([0, 1, 0, 1, 6, 7], false)] do
    unless shapePermits (shapeStates guardedInnerRoll guardedPlans word) [] 2 false == expected do
      throw (IO.userError s!"guarded inner-roll regression: word={word} expected={expected}")
  let holderPlans := (shapePlans returnedConditionalHolder).map ShapePlan.of
  for (word, expected) in [
      ([2, 3, 4, 5, 6, 7], true),
      ([2, 3, 4, 5, 6, 7, 8, 9], false),
      ([0, 1, 6, 7, 8, 9], true)] do
    unless shapePermits (shapeStates returnedConditionalHolder holderPlans word) [] 3 false == expected do
      throw (IO.userError s!"retained-owner regression: word={word} expected={expected}")
  unless shapePermits (shapeStates movedUnrelatedResource
      ((shapePlans movedUnrelatedResource).map ShapePlan.of) [0, 1, 2, 3, 4, 5]) [] 1 false do
    throw (IO.userError "an unrelated resource transfer blocked a retained roll")
  let selectedPlans := (shapePlans ownerPreservingContinuation).map ShapePlan.of
  for (word, expected) in [
      ([0, 1, 2, 3, 4, 5, 6, 7], true),
      ([0, 1, 2, 3, 4, 5, 8, 9], false)] do
    unless shapePermits (shapeStates ownerPreservingContinuation selectedPlans word) [] 0 false == expected do
      throw (IO.userError s!"selected-expansion regression: word={word} expected={expected}")
  -- Check this exact finite witness as well as the whole search. Subtree
  -- insertion can find another witness for the same word; it must not hide a
  -- check that incorrectly rejects an otherwise valid selected expansion.
  let selectedAfter := (checkedPlans ownerPreservingContinuation _
    (.resumeAfterContinuation .hole
      (.seq (.rollOnce (.oneCopy _)) (.oneCopy _)) (.oneCopy _))).map ShapePlan.of
  unless shapePermits (shapeStates ownerPreservingContinuation selectedAfter
      [0, 1, 2, 3, 4, 5, 6, 7]) [] 0 false do
    throw (IO.userError "selected-expansion regression: the exact after-continuation witness was rejected")
  let incomingPlans := (shapePlans incomingLoanContinuation).map ShapePlan.of
  for (word, choice, event, expected) in [
      ([0, 1, 2, 3, 4, 5, 0, 1], [], 3, true),
      ([0, 1, 2, 3, 4, 5, 0, 1], [0], 3, false),
      ([0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 8, 9], [0], 0, true),
      ([0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 8, 9, 10, 11], [0], 0, false)] do
    unless shapePermits (shapeStates incomingLoanContinuation incomingPlans word)
        choice event false == expected do
      throw (IO.userError s!"continuation-cut regression: word={word} choice={choice} event={event} expected={expected}")
  if args == ["--check-guarded-reentry"] then
    IO.println "Guarded reentry regressions: all fifteen source cuts passed"
    return 0
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
  let mut parkedChecked := 0
  let mut parkedRuntimeOnly := 0
  let mut parkedModelOnly := 0
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
    -- Check complete execution records before the larger admission inventory.
    -- A malformed execution is rejected without replaying every probe first.
    let (runs, calls, plannedPrefixes) ← shapeCheckCompleted source input.name input.parallelCoverage outputPath
    completedRuns := completedRuns + runs
    completedCalls := completedCalls + calls
    let plans := (shapePlans source).map ShapePlan.of
    unless !plans.isEmpty do throw (IO.userError s!"shape {input.name}: no accepted source plan")
    let choices := shapeChoicePatterns source
    let prefixes := ((plannedPrefixes.map fun word => (word, shapeWordInputs source word)) ++
      shapeShortPrefixes source plans choices).eraseDups
    unless prefixes.all (fun (word, inputs) => (shapeReplayStates source plans word inputs).isSome) do
      throw (IO.userError s!"shape {input.name}: source prefix has no accepted occurrence execution")
    let output : ShapeOutput ← shapeRead (System.FilePath.mk outputPath / s!"{input.name}.json")
    unless output.attempts.all (fun a => a.prefix.length == a.prefixChoices.length &&
        a.prefixChoices.all (fun choice => choice == choice.eraseDups.mergeSort (· ≤ ·) &&
          choice.all (· < input.resolvers))) do
      throw (IO.userError s!"shape {input.name}: invalid replayed resolver inputs")
    let actualKeys := output.attempts.toList.map fun a => (a.choice, a.prefix, a.prefixChoices, a.event, a.receive)
    let actualKeys := actualKeys.mergeSort shapeKeyLess
    let expectedKeys := (shapeProbeKeys source prefixes choices).mergeSort shapeKeyLess
    unless input.name == output.name && actualKeys == expectedKeys do
      let different := (actualKeys.zip expectedKeys).find? fun (actual, expected) => actual != expected
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory; actual={actualKeys.length} expected={expectedKeys.length} first_difference={repr different}")
    for (word, attempts) in shapeAttemptGroups output.attempts.toList do
      let some first := attempts.head?
        | throw (IO.userError "empty prefix group")
      unless attempts.all (fun attempt => attempt.prefixChoices == first.prefixChoices) &&
          first.prefixChoices.all (fun choice =>
            choice == choice.eraseDups.mergeSort (· ≤ ·) &&
              choice.all (· < input.resolvers)) do
        throw (IO.userError s!"shape {input.name}: invalid replayed resolver inputs")
      let some states := shapeReplayStates source plans word first.prefixChoices
        | throw (IO.userError s!"shape {input.name}: replayed inputs have no coherent source execution")
      -- Mutation control: preserve the exact original probe inventory, then
      -- compare with a source that omits only its outer sequential dependency.
      let permits : ShapeAttempt → Bool := if weakenRoot then
        let altered := match source with | .seq a b => .par a b | _ => source
        let alternate := (shapeReplayStates altered ((shapePlans altered).map ShapePlan.of)
          word first.prefixChoices).getD []
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
        match attempt.parked with
        | [] => pure ()
        | [(previewLabel, accepted)] =>
            unless previewLabel < 256 do
              throw (IO.userError s!"shape {input.name}: invalid parked preview label")
            -- A preview adds no source step. Matching local recv consumes the
            -- same self-role operation as send; other local recvs still need
            -- their own materialized preview. Wire operations keep their usual
            -- direction and the independently replayed source prefix.
            let expected := match shapeMessage? source attempt.event with
              | some message =>
                  if attempt.receive && message.sender == message.receiver then
                    message.label == previewLabel &&
                      shapePermits states attempt.choice attempt.event false
                  else permitted
              | none => false
            parkedChecked := parkedChecked + 1
            if expected != accepted then
              if parkedRuntimeOnly + parkedModelOnly < 10 then
                IO.println s!"shape {input.name} parked={previewLabel} choice={attempt.choice} prefix={attempt.prefix} event={attempt.event} receive={attempt.receive}: model={expected} runtime={accepted}"
              if accepted then parkedRuntimeOnly := parkedRuntimeOnly + 1
              else parkedModelOnly := parkedModelOnly + 1
              if accepted then runtimeOnly := runtimeOnly + 1 else modelOnly := modelOnly + 1
        | _ => throw (IO.userError s!"shape {input.name}: duplicate parked preview observation")
    unless parkedRuntimeOnly == 0 && parkedModelOnly == 0 do
      IO.println s!"Parked previews/source mismatch: runtime-only={parkedRuntimeOnly} model-only={parkedModelOnly}"
    unless runtimeOnly == 0 && modelOnly == 0 do
      throw (IO.userError s!"generated Endpoint/source conformance failed: runtime-only={runtimeOnly} model-only={modelOnly}")
  IO.println s!"Completed shape scripts/source: shapes={inputs.size} runs={completedRuns} calls={completedCalls} all accepted"
  IO.println s!"Generated shapes/source: shapes={inputs.size} attempts={checked} initial={checked-continuations} continuation={continuations} runtime-only={runtimeOnly} model-only={modelOnly}"
  IO.println s!"Successful parked previews/source: attempts={parkedChecked} runtime-only={parkedRuntimeOnly} model-only={parkedModelOnly}"
  pure 0
