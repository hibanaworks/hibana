-- Appended to endpoint_conformance.lean. This uses the established occurrence
-- semantics, not the Rust generator's first-event calculation.
-- Retain one elaboration job at a time under the descendant memory limit.
set_option Elab.async false

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

-- One immutable query matrix per source; every history retains every answer.
-- JSONL stores shared prefix/input data once and permits bounded-memory reads.
structure ShapeOutputHeader where
  name : Nat
  choices : List (List Nat)
  events : List Nat
  deriving Lean.FromJson

structure ShapeOutputGroup where
  «prefix» : List Nat
  prefixChoices : List (List Nat)
  answers : Array (Bool × List (Nat × Bool))
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
def shapeResolverUnchangedFrom (originals : List (Scope × Nat))
    (occurrences : List (Nat × Message)) (scope : Scope) (id : Nat)
    (before after : List SourcePoint) : Bool :=
  shapeResolverEnteredFrom originals occurrences scope id before ||
    !shapeResolverEnteredFrom originals occurrences scope id after

def shapeEntryChoicesFrom (originals resolvers : List (Scope × Nat))
    (occurrences : List (Nat × Message)) (selections : List Selection)
    (before after : List SourcePoint) (choice : List Nat) : Bool :=
  resolvers.all fun (scope, id) =>
    shapeResolverUnchangedFrom originals occurrences scope id before after ||
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
  simp only [shapeResolverUnchangedFrom] at scopeChecked
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

-- Retain subtree sizes in the order predicate's closure. Dependency pairs
-- must not recount the whole expanded source whenever they descend a node.
def shapeSourceOrder : Source → Nat × (Nat → Nat → Bool)
  | .send _ => (1, fun _ _ => false)
  | .seq left right =>
      let (split, beforeLeft) := shapeSourceOrder left
      let (size, beforeRight) := shapeSourceOrder right
      (split + size, fun before after =>
        if before < split && after < split then beforeLeft before after
        else if before ≥ split && after ≥ split then beforeRight (before - split) (after - split)
        else before < split && after ≥ split)
  | .par left right | .route _ left right =>
      let (split, beforeLeft) := shapeSourceOrder left
      let (size, beforeRight) := shapeSourceOrder right
      (split + size, fun before after =>
        if before < split && after < split then beforeLeft before after
        else if before ≥ split && after ≥ split then beforeRight (before - split) (after - split)
        else false)
  | .roll body => shapeSourceOrder body

theorem shapeSourceOrder_matches_source (source : Source) :
    (shapeSourceOrder source).1 = source.eventCount ∧
      (shapeSourceOrder source).2 = source.before := by
  induction source with
  | send => exact ⟨rfl, rfl⟩
  | seq left right leftIH rightIH | par left right leftIH rightIH
  | route resolver left right leftIH rightIH =>
      rcases leftIH with ⟨leftSize, leftOrder⟩
      rcases rightIH with ⟨rightSize, rightOrder⟩
      constructor
      · simp only [shapeSourceOrder, Source.eventCount, leftSize, rightSize]
      · funext before after
        simp only [shapeSourceOrder, Source.before, leftSize, rightSize, leftOrder, rightOrder]
  | roll body ih => exact ih

def shapePrecedes (before : Nat → Nat → Bool) (bindings : List (Nat × Binding))
    (first second : ActionId) : Bool :=
  match action? bindings first, action? bindings second with
  | some left, some right =>
      left.actor == right.actor &&
        if first.event == second.event then first.side == .send && second.side == .receive
        else before first.event second.event
  | _, _ => false

def shapePointDepends (before : Nat → Nat → Bool) (bindings : List (Nat × Binding)) :
    SourcePoint → SourcePoint → Bool
  | .accepted event, .committed target =>
      event == target.event && decide (target.side = .send ∨ target.side = .receive)
  | .committed first, .accepted event => shapePrecedes before bindings first ⟨event, .send⟩
  | .committed first, .committed target =>
      decide (target.side ≠ .send) && shapePrecedes before bindings first target
  | .accepted _, .accepted _ => false

theorem shapePointDepends_matches_source (source : Source) (bindings : List (Nat × Binding)) :
    shapePointDepends (shapeSourceOrder source).2 bindings = sourcePointDepends source bindings := by
  rw [(shapeSourceOrder_matches_source source).2]
  rfl

#print axioms shapeSourceOrder_matches_source
#print axioms shapePointDepends_matches_source

def shapePointActor (bindings : List (Nat × Binding)) : SourcePoint → Option Nat
  | .accepted event => (action? bindings ⟨event, .send⟩).map (·.actor)
  | .committed action => (action? bindings action).map (·.actor)

def shapeResolvedPrecedes (before : Nat → Nat → Bool)
    (first second : ActionId × Thunk (Option Nat)) : Bool :=
  match first.2.get, second.2.get with
  | some left, some right => left == right &&
      if first.1.event == second.1.event then first.1.side == .send && second.1.side == .receive
      else before first.1.event second.1.event
  | _, _ => false

theorem shapeResolvedPrecedes_matches_raw (before : Nat → Nat → Bool)
    (bindings : List (Nat × Binding)) (first second : ActionId) :
    shapeResolvedPrecedes before
      (first, Thunk.mk fun _ => (action? bindings first).map (·.actor))
      (second, Thunk.mk fun _ => (action? bindings second).map (·.actor)) =
        shapePrecedes before bindings first second := by
  cases left : action? bindings first <;> cases right : action? bindings second <;>
    simp [shapeResolvedPrecedes, shapePrecedes, Thunk.get, left, right]

def shapeResolvedPointDepends (before : Nat → Nat → Bool) :
    (SourcePoint × Thunk (Option Nat)) → (SourcePoint × Thunk (Option Nat)) → Bool
  | (.accepted event, _), (.committed target, _) =>
      event == target.event && decide (target.side = .send ∨ target.side = .receive)
  | (.committed first, actor), (.accepted event, target) =>
      shapeResolvedPrecedes before (first, actor) (⟨event, .send⟩, target)
  | (.committed first, actor), (.committed target, receiver) =>
      decide (target.side ≠ .send) && shapeResolvedPrecedes before (first, actor) (target, receiver)
  | (.accepted _, _), (.accepted _, _) => false

theorem shapeResolvedPointDepends_matches_raw (before : Nat → Nat → Bool)
    (bindings : List (Nat × Binding)) (first second : SourcePoint) :
    shapeResolvedPointDepends before
      (first, Thunk.mk fun _ => shapePointActor bindings first)
      (second, Thunk.mk fun _ => shapePointActor bindings second) =
        shapePointDepends before bindings first second := by
  cases first <;> cases second <;>
    simp [shapeResolvedPointDepends, shapePointDepends, shapePointActor,
      shapeResolvedPrecedes_matches_raw]

#print axioms shapeResolvedPrecedes_matches_raw
#print axioms shapeResolvedPointDepends_matches_raw

theorem shapePointRank_injective : Function.Injective SourcePoint.rank := by
  intro first second same
  cases first with
  | accepted a =>
      cases second with
      | accepted b =>
          simp only [SourcePoint.rank] at same
          have equal : a = b := by omega
          subst b
          rfl
      | committed action =>
          obtain ⟨b, side⟩ := action
          cases side <;> simp only [SourcePoint.rank] at same <;> omega
  | committed action =>
      obtain ⟨a, side⟩ := action
      cases second with
      | accepted b => cases side <;> simp only [SourcePoint.rank] at same <;> omega
      | committed action =>
          obtain ⟨b, other⟩ := action
          cases side <;> cases other <;> simp only [SourcePoint.rank] at same <;> try omega
          all_goals
            have equal : a = b := by omega
            subst b
            rfl

def shapeObservationBits (points : List SourcePoint) : Nat :=
  points.foldl (fun bits point => bits ||| (1 <<< point.rank)) 0

theorem shapeObservationFold_matches_list (points : List SourcePoint) (initial : Nat)
    (point : SourcePoint) :
    (points.foldl (fun bits point => bits ||| (1 <<< point.rank)) initial).testBit point.rank =
      (initial.testBit point.rank || points.contains point) := by
  induction points generalizing initial with
  | nil => simp
  | cons head rest ih =>
      rw [List.foldl_cons, ih]
      simp only [Nat.testBit_or, Nat.shiftLeft_eq,
        Nat.one_mul, Nat.testBit_two_pow, List.contains_cons, Bool.or_assoc]
      have ranks : (head.rank = point.rank) ↔ head = point :=
        ⟨fun same => shapePointRank_injective same, congrArg SourcePoint.rank⟩
      by_cases same : head = point
      · subst head
        simp
      · have notEqual : (point == head) = false := beq_eq_false_iff_ne.mpr (Ne.symm same)
        simp [ranks, same, notEqual]

theorem shapeObservationBits_matches_list (points : List SourcePoint) (point : SourcePoint) :
    (shapeObservationBits points).testBit point.rank = points.contains point := by
  simpa [shapeObservationBits] using shapeObservationFold_matches_list points 0 point


-- A row remembers only the prerequisite prefix actually inspected. Its tail
-- is forced after those prerequisites have been observed, then shared by all
-- later questions about this same source witness. No execution state is stored.
inductive ShapeRequirements where
  | done
  | next (point : SourcePoint) (rest : Thunk ShapeRequirements)

def ShapeRequirements.all (observed : SourcePoint → Bool) (required : ShapeRequirements) : Bool :=
  match required with
  | .done => true
  -- The projection exposes structural descent to the kernel. Lean's standard
  -- Thunk.fn_eq_fnImpl compiler theorem rewrites it to the memoized get.
  | .next point rest => if observed point then (rest.fn ()).all observed else false
termination_by structural required

def shapeRequirements (before : Nat → Nat → Bool)
    (point : SourcePoint × Thunk (Option Nat)) :
    List (SourcePoint × Thunk (Option Nat)) → ShapeRequirements
  | [] => .done
  | prior :: rest =>
      if shapeResolvedPointDepends before prior point then
        .next prior.1 (Thunk.mk fun _ => shapeRequirements before point rest)
      else shapeRequirements before point rest

theorem shapeRequirements_all_matches_filter
    (before : Nat → Nat → Bool) (bindings : List (Nat × Binding)) (point : SourcePoint)
    (domain : List SourcePoint) (observed : SourcePoint → Bool) :
    (shapeRequirements before (point, Thunk.mk fun _ => shapePointActor bindings point)
      (domain.map fun prior => (prior, Thunk.mk fun _ => shapePointActor bindings prior))).all observed =
      (domain.filter fun prior => shapePointDepends before bindings prior point).all observed := by
  induction domain with
  | nil => rfl
  | cons prior rest ih =>
      cases edge : shapePointDepends before bindings prior point <;>
        cases known : observed prior <;>
          simp [shapeRequirements, shapeResolvedPointDepends_matches_raw, edge,
            ShapeRequirements.all, known, ih]

def shapeDependencyRows (plan : SourceOccurrenceCut source) :
    List (SourcePoint × Thunk ShapeRequirements) :=
  let before := (shapeSourceOrder plan.expanded).2
  let domain := (sourceVisitPoints plan.path.bindings).map fun point =>
    (point, Thunk.mk fun _ => shapePointActor plan.path.bindings point)
  domain.map fun point =>
    (point.1, Thunk.mk fun _ => shapeRequirements before point domain)

def shapeDependenciesReady (rows : List (SourcePoint × Thunk ShapeRequirements))
    (observed : SourcePoint → Bool) (point : SourcePoint) : Bool :=
  match rows.lookup point with
  | none => false
  | some required => if observed point then false else required.get.all observed

theorem shapeDependenciesReady_matches_source (plan : SourceOccurrenceCut source)
    (points : List SourcePoint) (point : SourcePoint) :
    shapeDependenciesReady (shapeDependencyRows plan) points.contains point =
      (occurrenceCut plan points).ready point := by
  by_cases present : point ∈ sourceVisitPoints plan.path.bindings <;>
    simp [shapeDependenciesReady, shapeDependencyRows, shapeLookupMappedRows, present,
      Thunk.get, shapeRequirements_all_matches_filter, shapePointDepends_matches_source,
      List.map_map, Function.comp_def, SourceOccurrenceCut.ready, occurrenceCut]

def shapeResumptionRows (plan : SourceOccurrenceCut source) :
    List (SourcePoint × Thunk (List ResumptionQuery)) :=
  let points := sourceVisitPoints plan.path.bindings
  let inputs := (points, plan.unfolding.resumptions 0)
  points.map fun point => (point, Thunk.mk fun _ => plan.prepareResumptionFrom inputs point)

theorem shapeResumptionRows_matches_preparation (plan : SourceOccurrenceCut source) :
    shapeResumptionRows plan = (sourceVisitPoints plan.path.bindings).map
      (fun point => (point, Thunk.mk fun _ => plan.prepareResumption point)) := rfl

def shapeResumptionsReady (rows : List (SourcePoint × Thunk (List ResumptionQuery)))
    (observed : SourcePoint → Bool) (point : SourcePoint) : Bool :=
  match rows.lookup point with
  | none => false
  | some checks => checks.get.all (fun query => query.check observed)

def shapeOriginalActorsMatch (plan : SourceOccurrenceCut source) : Bool :=
  let occurrences := (plan.unfolding.occurrences 0).toArray
  plan.path.bindings.all fun (event, binding) =>
    match occurrences[event]? with
    | none => false
    | some (origin, message) => decide (binding.message = message) &&
        plan.original.paths.any fun path =>
          path.bindings.any fun original => decide (original = (origin, binding))

def shapeMessage? (source : Source) (event : Nat) : Option Message :=
  ((source.messageEntries 0).find? (fun entry => entry.1 == event)).map Prod.snd

structure ShapePlan (source : Source) where
  plan : SourceOccurrenceCut source
  dependencies : List (SourcePoint × Thunk ShapeRequirements)
  dependencies_eq : dependencies = shapeDependencyRows plan
  resumptions : List (SourcePoint × Thunk (List ResumptionQuery))
  resumptions_eq : resumptions = shapeResumptionRows plan
  message : Nat → Option Message
  message_eq : ∀ event, message event = shapeMessage? source event
  entryChoices : List SourcePoint → List SourcePoint → List Nat → Bool
  entryChoices_eq : ∀ before after choice,
    entryChoices before after choice = shapeEntryChoices plan before after choice

def ShapePlan.of (plan : SourceOccurrenceCut source) : ShapePlan source :=
  let originals := shapeResolvers source 0
  let resolvers := shapeResolvers plan.expanded 0
  let occurrences := plan.unfolding.occurrences 0
  let messages := source.messageEntries 0
  ⟨plan, shapeDependencyRows plan, rfl,
    shapeResumptionRows plan, rfl,
    (fun event => (messages.find? (fun entry => entry.1 == event)).map Prod.snd), fun _ => rfl,
    shapeEntryChoicesFrom originals resolvers occurrences plan.path.selections,
    fun _ _ _ => rfl⟩

theorem ShapePlan.entryChoices_matches_source (prepared : ShapePlan source)
    (before after : List SourcePoint) (choice : List Nat) :
    prepared.entryChoices before after choice = shapeEntryChoices prepared.plan before after choice :=
  prepared.entryChoices_eq before after choice

theorem ShapePlan.message_matches_source (prepared : ShapePlan source) (event : Nat) :
    prepared.message event = shapeMessage? source event := prepared.message_eq event

#print axioms ShapePlan.message_matches_source

def ShapePlan.ready (prepared : ShapePlan source) (observed : SourcePoint → Bool)
    (point : SourcePoint) : Bool :=
  shapeDependenciesReady prepared.dependencies observed point &&
    shapeResumptionsReady prepared.resumptions observed point

theorem ShapePlan.ready_matches_source (prepared : ShapePlan source)
    (points : List SourcePoint) (point : SourcePoint) :
    prepared.ready points.contains point = ((occurrenceCut prepared.plan points).ready point &&
      (occurrenceCut prepared.plan points).resumptionReady point) := by
  unfold ShapePlan.ready
  rw [prepared.dependencies_eq, prepared.resumptions_eq, shapeDependenciesReady_matches_source]
  by_cases present : point ∈ sourceVisitPoints prepared.plan.path.bindings
  · have observations : points.contains = (fun observed => decide (observed ∈ points)) := by
      funext observed
      exact List.contains_eq_mem observed points
    simp [shapeResumptionsReady, shapeResumptionRows_matches_preparation, shapeLookupMappedRows, present,
      prepared_resumption_matches_source, occurrenceCut, observations, Thunk.get]
  · simp [present, SourceOccurrenceCut.ready, occurrenceCut]

theorem shapeObservationBits_append (points : List SourcePoint) (point : SourcePoint) :
    shapeObservationBits (points ++ [point]) =
      shapeObservationBits points ||| (1 <<< point.rank) := by
  simp [shapeObservationBits, List.foldl_append]

theorem ShapePlan.observed_ready_matches_list (prepared : ShapePlan source)
    (points : List SourcePoint) (point : SourcePoint) :
    prepared.ready (fun prior => (shapeObservationBits points).testBit prior.rank) point =
      prepared.ready points.contains point := by
  have same : (fun prior => (shapeObservationBits points).testBit prior.rank) = points.contains := by
    funext prior
    exact shapeObservationBits_matches_list points prior
  rw [same]

#print axioms shapeLookupMappedRows
#print axioms shapeRequirements_all_matches_filter
#print axioms shapeDependenciesReady_matches_source
#print axioms shapeResumptionRows_matches_preparation
#print axioms ShapePlan.ready_matches_source
#print axioms shapePointRank_injective
#print axioms shapeObservationBits_matches_list
#print axioms shapeObservationBits_append
#print axioms ShapePlan.observed_ready_matches_list
#print axioms ShapePlan.entryChoices_matches_source

theorem shapeOriginalActorsMatch_checks_each_occurrence (plan : SourceOccurrenceCut source)
    (checked : shapeOriginalActorsMatch plan = true)
    (present : (event, binding) ∈ plan.path.bindings) :
    ∃ origin message, (plan.unfolding.occurrences 0)[event]? = some (origin, message) ∧
      binding.message = message ∧
      ∃ path ∈ plan.original.paths, (origin, binding) ∈ path.bindings := by
  have checkedAt := List.all_eq_true.mp checked (event, binding) present
  simp only [List.getElem?_toArray] at checkedAt
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

def shapeBindingStep (prepared : ShapePlan source) (points : List SourcePoint)
    (observed : Nat) (entry : Nat × Binding) (receive : Bool) : Option (List SourcePoint) := do
  let (event, binding) := entry
  let isLocal := binding.message.sender == binding.message.receiver
  if isLocal && receive then none else do
    let point := if isLocal then .committed ⟨event, .local⟩
      else if receive then .committed ⟨event, .receive⟩ else .accepted event
    if !prepared.ready (fun prior => observed.testBit prior.rank) point then none else do
      let points := points ++ [point]
      if isLocal || receive then some points else do
        let publication := SourcePoint.committed ⟨event, .send⟩
        let after := observed ||| (1 <<< point.rank)
        if prepared.ready (fun prior => after.testBit prior.rank) publication then
          some (points ++ [publication]) else none

def shapeAdvance (prepared : ShapePlan source) (points : List SourcePoint)
    (request : Nat) (receive : Bool) : List (List SourcePoint) :=
  match prepared.message request with
  | none => []
  | some expected =>
      let observed := shapeObservationBits points
      prepared.plan.path.bindings.filterMap fun entry =>
        if shapeMatchesRequest expected entry.2.message receive then
          shapeBindingStep prepared points observed entry receive else none

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

/-- Evaluate the same finite nondeterministic replay per witness. This avoids
materializing every candidate's intermediate state when an existential query
already has a witness. It changes evaluation order, not the source rule. -/
def shapeExistsReplay {command state : Type}
    (step : state → command → List state) (accept : state → Bool) :
    List command → state → Bool
  | [], before => accept before
  | item :: rest, before => (step before item).any (shapeExistsReplay step accept rest)

theorem shapeExistsReplay_matches_batched_replay {command state : Type}
    (step : state → command → List state) (accept : state → Bool)
    (word : List command) (states : List state) :
    (word.foldl (fun before item => before.flatMap (fun state => step state item)) states).any accept =
      states.any (shapeExistsReplay step accept word) := by
  induction word generalizing states with
  | nil => rfl
  | cons item rest ih =>
    simp only [List.foldl_cons, ih, List.any_flatMap, shapeExistsReplay]

/-- Prepare one candidate, answer all query groups, then release its tables.
The accumulator contains query results, not model states. -/
def shapeQueryAnswers {candidate query prepared answer : Type}
    (prepare : candidate → prepared) (ask : prepared → query → answer)
    (initial : query → answer) (merge : answer → answer → answer)
    (candidates : List candidate) (queries : List query) : List answer :=
  candidates.foldl (fun answers candidate =>
    let prepared := prepare candidate
    answers.zipWith (fun previous query => merge previous (ask prepared query)) queries)
    (queries.map initial)

theorem shapeQueryAnswers_accumulate {candidate query prepared answer : Type}
    (prepare : candidate → prepared) (ask : prepared → query → answer)
    (merge : answer → answer → answer)
    (candidates : List candidate) (queries : List query) (previous : query → answer) :
    candidates.foldl (fun answers candidate =>
      answers.zipWith (fun prior query => merge prior (ask (prepare candidate) query)) queries)
        (queries.map previous) =
      queries.map (fun query => candidates.foldl
        (fun prior candidate => merge prior (ask (prepare candidate) query)) (previous query)) := by
  induction candidates generalizing previous with
  | nil => rfl
  | cons candidate rest ih =>
    simp only [List.foldl_cons]
    have combined : (queries.map previous).zipWith
        (fun prior query => merge prior (ask (prepare candidate) query)) queries =
        queries.map (fun query => merge (previous query) (ask (prepare candidate) query)) := by
      clear ih
      induction queries with
      | nil => rfl
      | cons query queries ih => simp only [List.map_cons, List.zipWith_cons_cons, ih]
    rw [combined, ih]

theorem shapeQueryAnswers_matches_independent_folds
    {candidate query prepared answer : Type}
    (prepare : candidate → prepared) (ask : prepared → query → answer)
    (initial : query → answer) (merge : answer → answer → answer)
    (candidates : List candidate) (queries : List query) :
    shapeQueryAnswers prepare ask initial merge candidates queries =
      queries.map (fun query => candidates.foldl
        (fun prior candidate => merge prior (ask (prepare candidate) query)) (initial query)) := by
  exact shapeQueryAnswers_accumulate prepare ask merge candidates queries initial

#print axioms shapeQueryAnswers_accumulate
#print axioms shapeQueryAnswers_matches_independent_folds

theorem shapePermits_after_states (source : Source) (plans : List (ShapePlan source))
    (word choice : List Nat) (label : Nat) (receive : Bool) :
    shapePermits (shapeStates source plans word) choice label receive =
      plans.any (fun plan =>
        shapeExistsReplay
          (fun (plan, points) command =>
            (shapeAdvance plan points (command / 2) (command % 2 == 1)).map (plan, ·))
          (fun (plan, points) =>
            (shapeAdvance plan points label receive).any (fun after =>
              plan.entryChoices points after choice)) word (plan, [])) := by
  unfold shapePermits shapeStates
  rw [shapeExistsReplay_matches_batched_replay]
  simp only [List.any_map, Function.comp_def]

#print axioms shapeExistsReplay_matches_batched_replay
#print axioms shapePermits_after_states

/-- One checked source witness suffices for an existential query over the
whole candidate set. The witness must actually be a member of that set. -/
theorem shapePermits_after_states_of_witness (source : Source)
    (plans : List (ShapePlan source)) (word choice : List Nat) (label : Nat) (receive : Bool)
    (plan : ShapePlan source) (present : plan ∈ plans)
    (enabled : shapePermits (shapeStates source [plan] word) choice label receive = true) :
    shapePermits (shapeStates source plans word) choice label receive = true := by
  rw [shapePermits_after_states] at enabled ⊢
  simp only [List.any_cons, List.any_nil, Bool.or_false] at enabled
  exact List.any_eq_true.mpr ⟨plan, present, enabled⟩

#print axioms shapePermits_after_states_of_witness

def shapeInputStep (states : List (ShapePlan source × List SourcePoint))
    (command : Nat) (choice : List Nat) : List (ShapePlan source × List SourcePoint) :=
  states.flatMap fun (plan, points) =>
    ((shapeAdvance plan points (command / 2) (command % 2 == 1)).filter fun after =>
      plan.entryChoices points after choice).map (plan, ·)

-- Input-opaque calls retain no arbitrary callback bits in their history.
-- Only the logical sender at a resolved source scope's entry controls it.
-- The model additionally validates that
-- erasure enters no resolver in any possible occurrence execution.
def shapeRecordsResolverInput (source : Source) (command : Nat) : Bool :=
  match shapeMessage? source (command / 2) with
  | none => true
  | some requested =>
      let view := if command % 2 == 1 then requested.receiver else requested.sender
      (shapeResolvers source 0).any fun (scope, _) =>
        match shapeMessage? source scope.start with
        | some first => first.sender == view
        | none => true

def shapeNoFreshResolver (plan : ShapePlan source)
    (before after : List SourcePoint) : Bool :=
  (shapeResolvers plan.plan.expanded 0).all fun (scope, id) =>
    shapeResolverUnchangedFrom (shapeResolvers source 0) (plan.plan.unfolding.occurrences 0)
      scope id before after

theorem shapeEntryChoices_without_fresh_resolver (plan : ShapePlan source)
    (before after : List SourcePoint) (choice : List Nat)
    (safe : shapeNoFreshResolver plan before after = true) :
    plan.entryChoices before after choice = true := by
  rw [plan.entryChoices_eq]
  unfold shapeEntryChoices shapeEntryChoicesFrom
  apply List.all_eq_true.mpr
  intro entry present
  obtain ⟨scope, id⟩ := entry
  have allowed :=  List.all_eq_true.mp safe (scope, id) present
  have combined := congrArg (fun b => b || plan.plan.path.selections.all fun selection =>
      selection.scope != scope || selection.arm ==
        if choice.contains id then .right else .left) allowed
  simpa only [Bool.true_or] using combined

def shapeInputIrrelevant (states : List (ShapePlan source × List SourcePoint))
    (command : Nat) : Bool :=
  states.all fun (plan, before) =>
    (shapeAdvance plan before (command / 2) (command % 2 == 1)).all fun after =>
      shapeNoFreshResolver plan before after

theorem shapeInputStep_ignores_unread_input
    (states : List (ShapePlan source × List SourcePoint)) (command : Nat) (choice : List Nat)
    (safe : shapeInputIrrelevant states command = true) :
    shapeInputStep states command choice = shapeInputStep states command [] := by
  induction states with
  | nil => rfl
  | cons state states ih =>
      obtain ⟨plan, before⟩ := state
      have safeRow := (Bool.and_eq_true_iff.mp safe).1
      have safeTail := (Bool.and_eq_true_iff.mp safe).2
      have unfiltered (input : List Nat) :
          (shapeAdvance plan before (command / 2) (command % 2 == 1)).filter
            (fun after => plan.entryChoices before after input) =
            shapeAdvance plan before (command / 2) (command % 2 == 1) := by
        apply List.filter_eq_self.mpr
        intro after present
        exact shapeEntryChoices_without_fresh_resolver plan before after input
          (List.all_eq_true.mp safeRow after present)
      simp only [shapeInputStep, List.flatMap_cons, unfiltered]
      exact congrArg (List.append ((shapeAdvance plan before (command / 2)
        (command % 2 == 1)).map (plan, ·))) (ih safeTail)

def shapeCanonicalChoice (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (command : Nat) (choice : List Nat) :
    List Nat :=
  if choice.isEmpty then []
  else if !shapeRecordsResolverInput source command && shapeInputIrrelevant states command then []
  else choice

theorem shapeCanonicalChoice_preserves_transition (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (command : Nat) (choice : List Nat) :
    shapeInputStep states command (shapeCanonicalChoice source states command choice) =
      shapeInputStep states command choice := by
  unfold shapeCanonicalChoice
  split
  · rename_i empty
    have same : choice = [] := List.isEmpty_iff.mp empty
    subst choice
    rfl
  · split
    · rename_i erased
      exact (shapeInputStep_ignores_unread_input states command choice
        (Bool.and_eq_true_iff.mp erased).2).symm
    · rfl

def shapeCanonicalInputs (source : Source) :
    List (ShapePlan source × List SourcePoint) → List Nat → List (List Nat) → List (List Nat)
  | states, command :: commands, choice :: choices =>
      let retained := shapeCanonicalChoice source states command choice
      retained :: shapeCanonicalInputs source (shapeInputStep states command retained) commands choices
  | _, _, _ => []

theorem shapeCanonicalInputs_length (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (commands : List Nat) (inputs : List (List Nat))
    (same : commands.length = inputs.length) :
    (shapeCanonicalInputs source states commands inputs).length = inputs.length := by
  induction commands generalizing states inputs with
  | nil => cases inputs <;> simp_all [shapeCanonicalInputs]
  | cons command commands ih =>
      cases inputs with
      | nil => simp at same
      | cons choice inputs =>
          simp only [List.length_cons, Nat.add_right_cancel_iff] at same
          simp only [shapeCanonicalInputs, List.length_cons]
          exact congrArg Nat.succ (ih _ inputs same)

def shapeInputsAlreadyCanonical (source : Source) (commands : List Nat)
    (inputs : List (List Nat)) : Bool :=
  commands.length == inputs.length && (commands.zip inputs).all fun (command, choice) =>
    choice.isEmpty || shapeRecordsResolverInput source command

theorem shapeCanonicalChoice_retains_input (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (command : Nat) (choice : List Nat)
    (retained : (choice.isEmpty || shapeRecordsResolverInput source command) = true) :
    shapeCanonicalChoice source states command choice = choice := by
  cases empty : choice.isEmpty with
  | true =>
      have same : choice = [] := List.isEmpty_iff.mp empty
      subst choice
      simp [shapeCanonicalChoice]
  | false =>
      have controller : shapeRecordsResolverInput source command = true := by
        simpa only [empty, Bool.false_or] using retained
      simp [shapeCanonicalChoice, empty, controller]

theorem shapeCanonicalInputs_already_canonical (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (commands : List Nat) (inputs : List (List Nat))
    (canonical : shapeInputsAlreadyCanonical source commands inputs = true) :
    shapeCanonicalInputs source states commands inputs = inputs := by
  obtain ⟨fits, retained⟩ := Bool.and_eq_true_iff.mp canonical
  have same : commands.length = inputs.length := beq_iff_eq.mp fits
  clear canonical fits
  induction commands generalizing states inputs with
  | nil => cases inputs <;> simp_all [shapeCanonicalInputs]
  | cons command commands ih =>
      cases inputs with
      | nil => simp at same
      | cons choice inputs =>
          have parts := Bool.and_eq_true_iff.mp retained
          simp only [List.length_cons, Nat.add_right_cancel_iff] at same
          simp only [shapeCanonicalInputs,
            shapeCanonicalChoice_retains_input source states command choice parts.1]
          exact congrArg (List.cons choice) (ih _ inputs parts.2 same)

#print axioms shapeCanonicalChoice_retains_input
#print axioms shapeCanonicalInputs_already_canonical

def shapeInputTrace : List (ShapePlan source × List SourcePoint) → List Nat →
    List (List Nat) → List (List (ShapePlan source × List SourcePoint))
  | states, command :: commands, choice :: choices =>
      states :: shapeInputTrace (shapeInputStep states command choice) commands choices
  | states, _, _ => [states]

theorem shapeCanonicalInputs_preserves_every_state (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (commands : List Nat) (inputs : List (List Nat)) :
    shapeInputTrace states commands (shapeCanonicalInputs source states commands inputs) =
      shapeInputTrace states commands inputs := by
  induction commands generalizing states inputs with
  | nil => rfl
  | cons command commands ih =>
      cases inputs with
      | nil => rfl
      | cons choice inputs =>
          simp only [shapeCanonicalInputs, shapeInputTrace,
            shapeCanonicalChoice_preserves_transition]
          exact congrArg (List.cons states) (ih _ inputs)

theorem shapeCanonicalInputs_preserves_replay (source : Source)
    (states : List (ShapePlan source × List SourcePoint)) (commands : List Nat) (inputs : List (List Nat)) :
    (commands.zip (shapeCanonicalInputs source states commands inputs)).foldl
        (fun states (command, choice) => shapeInputStep states command choice) states =
      (commands.zip inputs).foldl
        (fun states (command, choice) => shapeInputStep states command choice) states := by
  induction commands generalizing states inputs with
  | nil => rfl
  | cons command commands ih =>
      cases inputs with
      | nil => rfl
      | cons choice inputs =>
          simp only [shapeCanonicalInputs, List.zip_cons_cons, List.foldl_cons,
            shapeCanonicalChoice_preserves_transition]
          exact ih _ inputs

#print axioms shapeEntryChoices_without_fresh_resolver
#print axioms shapeInputStep_ignores_unread_input
#print axioms shapeCanonicalChoice_preserves_transition
#print axioms shapeCanonicalInputs_length
#print axioms shapeCanonicalInputs_preserves_every_state
#print axioms shapeCanonicalInputs_preserves_replay

-- Resolver inputs belong to a call, not to the source alias used to name it.
-- Keep one coherent source-plan witness through all the recorded calls.
def shapeReplayStates (source : Source) (plans : List (ShapePlan source))
    (word : List Nat) (inputs : List (List Nat)) :
    Option (List (ShapePlan source × List SourcePoint)) :=
  if word.length != inputs.length then none else
    let states := (word.zip inputs).foldl (fun states (command, choice) =>
      shapeInputStep states command choice) (plans.map (·, []))
    if states.isEmpty then none else some states

-- A group names one prefix and its questions. Missing input leaves abstract
-- choices existential; observed prefixes always provide every recorded input.
-- A missing request asks whether the prefix has a coherent source execution.
abbrev ShapeRequest := List Nat × Nat × Bool
abbrev ShapeQueryGroup := List Nat × Option (List (List Nat)) × List (Option ShapeRequest)

def shapeStateQuery (states : List (ShapePlan source × List SourcePoint)) :
    Option ShapeRequest → Bool
  | none => !states.isEmpty
  | some (choice, event, receive) => shapePermits states choice event receive

abbrev ShapeEnabledCall := Message × List SourcePoint

def shapeEnabledCalls (plan : ShapePlan source) (points : List SourcePoint)
    (observed : Nat) (receive : Bool) : List ShapeEnabledCall :=
  plan.plan.path.bindings.filterMap fun entry =>
    (shapeBindingStep plan points observed entry receive).map (entry.2.message, ·)

abbrev ShapeQueryState (source : Source) :=
  (ShapePlan source × List SourcePoint) ×
    (Thunk (List ShapeEnabledCall) × Thunk (List ShapeEnabledCall))

-- Questions with different resolver inputs share one readiness calculation
-- per direction. A completed question does not force either lazy frontier.
def shapePrepareQueryStates (states : List (ShapePlan source × List SourcePoint)) :
    List (ShapeQueryState source) :=
  states.map fun (plan, points) =>
    let observed := shapeObservationBits points
    ((plan, points),
      (Thunk.mk fun _ => shapeEnabledCalls plan points observed false,
       Thunk.mk fun _ => shapeEnabledCalls plan points observed true))

def shapePreparedQuery (states : List (ShapeQueryState source)) : Option ShapeRequest → Bool
  | none => !states.isEmpty
  | some (choice, event, receive) => states.any fun ((plan, points), send, recv) =>
      match plan.message event with
      | none => false
      | some expected => (if receive then recv.get else send.get).any fun (actual, after) =>
          shapeMatchesRequest expected actual receive && plan.entryChoices points after choice

theorem shapePreparedQuery_matches_state (states : List (ShapePlan source × List SourcePoint))
    (request : Option ShapeRequest) :
    shapePreparedQuery (shapePrepareQueryStates states) request = shapeStateQuery states request := by
  cases request with
  | none => simp [shapePreparedQuery, shapePrepareQueryStates, shapeStateQuery]
  | some request =>
      obtain ⟨choice, event, receive⟩ := request
      simp only [shapePreparedQuery, shapePrepareQueryStates, List.any_map, shapeStateQuery, shapePermits]
      apply congrArg (fun predicate => states.any predicate)
      funext state
      obtain ⟨plan, points⟩ := state
      dsimp only [Function.comp_def]
      cases message : plan.message event with
      | none => simp [message, shapeAdvance]
      | some expected =>
          cases receive <;>
            simp [message, shapeAdvance, shapeEnabledCalls, Thunk.get, List.any_filterMap]
          all_goals
            apply congrArg (fun predicate => plan.plan.path.bindings.any predicate)
            funext call
            cases step : shapeBindingStep plan points (shapeObservationBits points) call _ <;>
              cases contract : shapeMatchesRequest expected call.2.message _ <;>
                simp [contract]

#print axioms shapePreparedQuery_matches_state

-- A cache entry carries the equality to a fresh replay. It cannot manufacture
-- a model state, even if a caller supplies a populated cache.
abbrev ShapeReplayMemo {command state : Type} [BEq command] [Hashable command]
    (step : state → command → state) (initial : state) :=
  Std.DHashMap (List command)
    (fun reversed => { after : state // after = reversed.reverse.foldl step initial })

def shapeCachedReplay {command state : Type} [BEq command] [Hashable command]
    [LawfulBEq command] (step : state → command → state) (initial : state) :
    (reversed : List command) → ShapeReplayMemo step initial →
      { after : state // after = reversed.reverse.foldl step initial } ×
        ShapeReplayMemo step initial
  | [], memo => (⟨initial, rfl⟩, memo)
  | item :: rest, memo =>
    match memo.get? (item :: rest) with
    | some after => (after, memo)
    | none =>
      let (before, memo) := shapeCachedReplay step initial rest memo
      let after : { after : state // after = (item :: rest).reverse.foldl step initial } :=
        ⟨step before.val item, by simp only [List.reverse_cons, List.foldl_append,
          List.foldl_cons, List.foldl_nil]; rw [before.property]⟩
      (after, memo.insert (item :: rest) after)

theorem shapeCachedReplay_matches_fresh_replay {command state : Type}
    [BEq command] [Hashable command] [LawfulBEq command]
    (step : state → command → state) (initial : state)
    (word : List command) (memo : ShapeReplayMemo step initial) :
    (shapeCachedReplay step initial word.reverse memo).1.val = word.foldl step initial := by
  simpa using (shapeCachedReplay step initial word.reverse memo).1.property

abbrev ShapeReplayInput := Nat × Option (List Nat)

def shapeReplayStep (states : List (ShapePlan source × List SourcePoint))
    (input : ShapeReplayInput) : List (ShapePlan source × List SourcePoint) :=
  match input.2 with
  | none => states.flatMap fun (plan, points) =>
      (shapeAdvance plan points (input.1 / 2) (input.1 % 2 == 1)).map (plan, ·)
  | some choice => shapeInputStep states input.1 choice

def shapeGroupWord (group : ShapeQueryGroup) : Option (List ShapeReplayInput) :=
  match group.2.1 with
  | none => some (group.1.map (·, none))
  | some inputs => if group.1.length != inputs.length then none
      else some ((group.1.zip inputs).map fun (command, choice) => (command, some choice))

def shapePreparedGroupStates (plan : ShapePlan source) (group : ShapeQueryGroup) :=
  match shapeGroupWord group with
  | none => []
  | some word => word.foldl shapeReplayStep [(plan, [])]

theorem shapePreparedGroupStates_matches_independent_replay
    (plan : ShapePlan source) (group : ShapeQueryGroup) :
    shapePreparedGroupStates plan group =
      match group.2.1 with
      | none => shapeStates source [plan] group.1
      | some inputs => (shapeReplayStates source [plan] group.1 inputs).getD [] := by
  obtain ⟨word, choices, requests⟩ := group
  cases choices with
  | none => simp [shapePreparedGroupStates, shapeGroupWord,
      List.foldl_map, shapeReplayStep, shapeStates]
  | some inputs =>
      have discardEmpty (states : List (ShapePlan source × List SourcePoint)) :
          (if states = [] then none else some states).getD [] = states := by
        cases states <;> rfl
      by_cases matched : word.length = inputs.length <;>
        simp [shapePreparedGroupStates, shapeGroupWord, shapeReplayStates,
          matched, List.foldl_map, shapeReplayStep, discardEmpty]

abbrev ShapePreparedQuery := Option (List ShapeReplayInput) × Array (Option ShapeRequest)

def shapePrepareQuery (group : ShapeQueryGroup) : ShapePreparedQuery :=
  ((shapeGroupWord group).map List.reverse, group.2.2.toArray)

def shapePreparedStates (plan : ShapePlan source) (query : ShapePreparedQuery) :=
  match query.1 with
  | none => []
  | some reversed => reversed.reverse.foldl shapeReplayStep [(plan, [])]

theorem shapePreparedStates_matches_group (plan : ShapePlan source) (group : ShapeQueryGroup) :
    shapePreparedStates plan (shapePrepareQuery group) = shapePreparedGroupStates plan group := by
  cases word : shapeGroupWord group <;>
    simp [shapePreparedStates, shapePrepareQuery, shapePreparedGroupStates, word]

theorem shapeEmptyAnswers_are_neutral (previous : List Bool)
    (requests : List (Option ShapeRequest)) (sameLength : previous.length = requests.length) :
    previous = previous.zipWith (· || ·) (requests.map fun _ => false) := by
  induction previous generalizing requests with
  | nil => simp
  | cons value previous ih =>
      cases requests with
      | nil => simp at sameLength
      | cons request requests =>
          simp only [List.map_cons, List.zipWith_cons_cons, Bool.or_false]
          exact congrArg (List.cons value) (ih requests (by simpa using sameLength))

theorem shapeTrueAnswers_are_absorbing {question : Type} (previous : List Bool)
    (requests : List question) (answer : question → Bool)
    (sameLength : previous.length = requests.length) (finished : previous.all id = true) :
    previous = previous.zipWith (· || ·) (requests.map answer) := by
  induction previous generalizing requests with
  | nil => simp
  | cons value previous ih =>
      cases requests with
      | nil => simp at sameLength
      | cons request requests =>
          cases value with
          | false => simp at finished
          | true =>
              simp only [List.map_cons, List.zipWith_cons_cons, Bool.true_or]
              exact congrArg (List.cons true)
                (ih requests (by simpa using sameLength) (by simpa using finished))

def shapeMergeAnswers (states : List (ShapePlan source × List SourcePoint))
    (requests : Array (Option ShapeRequest)) (previous : Array Bool) : Array Bool :=
  if states.isEmpty && previous.size == requests.size then previous
  else if requests == #[none] then previous.zipWith (· || ·) #[!states.isEmpty]
  else
    let prepared := shapePrepareQueryStates states
    previous.zipWith
      (fun accepted request => if accepted then true else shapePreparedQuery prepared request) requests

theorem shapeMergeAnswers_preserves_all_answers
    (states : List (ShapePlan source × List SourcePoint))
    (requests : Array (Option ShapeRequest)) (previous : Array Bool) :
    (shapeMergeAnswers states requests previous).toList =
      previous.toList.zipWith (· || ·) (requests.toList.map (shapeStateQuery states)) := by
  unfold shapeMergeAnswers
  split
  · rename_i empty
    have fields := Bool.and_eq_true_iff.mp empty
    have noStates : states = [] := List.isEmpty_iff.mp fields.1
    have sameLength : previous.toList.length = requests.toList.length := by
      simpa using (of_decide_eq_true fields.2 : previous.size = requests.size)
    subst states
    have noWitness : shapeStateQuery (source := source) [] = (fun _ => false) := by
      funext request
      cases request <;> rfl
    rw [noWitness]
    exact shapeEmptyAnswers_are_neutral previous.toList requests.toList sameLength
  · split
    · rename_i singleQuery
      have onlyPrefix : requests = #[none] := beq_iff_eq.mp singleQuery
      subst requests
      simp [Array.toList_zipWith, shapeStateQuery]
    · simp only [Array.toList_zipWith, List.zipWith_map_right]
      apply congrArg (fun combine => previous.toList.zipWith combine requests.toList)
      funext accepted request
      cases accepted <;> simp [shapePreparedQuery_matches_state]

-- Query inputs are prepared once, outside the source-candidate loop. Reuse an
-- unchanged answer row when this candidate has no execution of its prefix.
-- The accumulator keeps the group loop tail-recursive for large populations.
-- Array sizes make the row-shape check constant-time; lists would rescan the
-- complete probe inventory for every source candidate even on empty states.
def shapeAccumulateAnswers (plan : ShapePlan source) :
    List ShapePreparedQuery → List (Array Bool) →
      ShapeReplayMemo shapeReplayStep [(plan, [])] → List (Array Bool) → List (Array Bool)
  | [], _, _, accumulated => accumulated.reverse
  | _ :: _, [], _, accumulated => accumulated.reverse
  | query :: queries, previous :: rows, memo, accumulated =>
      if previous.size == query.2.size && previous.all id then
        shapeAccumulateAnswers plan queries rows memo (previous :: accumulated)
      else
      let (states, memo) := match query.1 with
        | none => ([], memo)
        | some reversed =>
            let (checked, memo) := shapeCachedReplay shapeReplayStep [(plan, [])] reversed memo
            (checked.val, memo)
      let current := shapeMergeAnswers states query.2 previous
      shapeAccumulateAnswers plan queries rows memo (current :: accumulated)

theorem shapeAccumulateAnswers_preserves_every_query (plan : ShapePlan source)
    (queries : List ShapePreparedQuery) (rows accumulated : List (Array Bool))
    (memo : ShapeReplayMemo shapeReplayStep [(plan, [])]) :
    (shapeAccumulateAnswers plan queries rows memo accumulated).map Array.toList =
      accumulated.reverse.map Array.toList ++ rows.zipWith (fun previous query =>
        previous.toList.zipWith (· || ·)
          (query.2.toList.map (shapeStateQuery (shapePreparedStates plan query)))) queries := by
  induction queries generalizing rows memo accumulated with
  | nil => simp [shapeAccumulateAnswers]
  | cons query queries ih =>
      cases rows with
      | nil => simp [shapeAccumulateAnswers]
      | cons previous rows =>
          simp only [shapeAccumulateAnswers]
          split
          · rename_i finished
            have fields := Bool.and_eq_true_iff.mp finished
            have fit : previous.toList.length = query.2.toList.length := by
              simpa using (of_decide_eq_true fields.1 : previous.size = query.2.size)
            have allTrue : previous.toList.all id = true := by simpa using fields.2
            have same := shapeTrueAnswers_are_absorbing previous.toList query.2.toList
              (shapeStateQuery (shapePreparedStates plan query)) fit allTrue
            rw [ih, List.reverse_cons]
            simp only [List.map_append, List.map_cons, List.map_nil, List.append_assoc,
              List.singleton_append, List.zipWith_cons_cons]
            rw [← same]
          · cases word : query.1 <;> dsimp only
            · rw [ih, List.reverse_cons]
              simp [shapeMergeAnswers_preserves_all_answers, shapePreparedStates, word]
            · rw [ih, List.reverse_cons]
              rw [(shapeCachedReplay shapeReplayStep [(plan, [])] _ memo).1.property]
              simp [shapeMergeAnswers_preserves_all_answers, shapePreparedStates, word]

theorem shapeMapFold {input state output : Type} (transform : state → output)
    (step : state → input → state) (next : output → input → output)
    (commutes : ∀ state item, transform (step state item) = next (transform state) item)
    (items : List input) (initial : state) :
    transform (items.foldl step initial) = items.foldl next (transform initial) := by
  induction items generalizing initial with
  | nil => rfl
  | cons item items ih => simp only [List.foldl_cons, ih, commutes]

def shapeGroupAnswers (source : Source) (plans : List (SourceOccurrenceCut source))
    (groups : List ShapeQueryGroup) : List (List Bool) :=
  let queries := groups.map shapePrepareQuery
  let total := plans.length
  ((plans.foldl (fun (state : List (Array Bool) × Nat) candidate =>
    let after := (shapeAccumulateAnswers (ShapePlan.of candidate) queries state.1 ∅ [], state.2 + 1)
    if after.2 % 4096 == 0 then
      dbgTrace s!"source oracle: {after.2}/{total} witnesses checked" (fun _ => after)
    else after)
    (groups.map (fun group => group.2.2.toArray.map fun _ => false), 0)).1).map Array.toList

theorem shapeGroupAnswers_matches_independent_queries (source : Source)
    (plans : List (SourceOccurrenceCut source)) (groups : List ShapeQueryGroup) :
    shapeGroupAnswers source plans groups = shapeQueryAnswers ShapePlan.of
    (fun plan group =>
      let states := match group.2.1 with
        | none => shapeStates source [plan] group.1
        | some inputs => (shapeReplayStates source [plan] group.1 inputs).getD []
      group.2.2.map (shapeStateQuery states))
    (fun group => group.2.2.map fun _ => false)
    (List.zipWith (· || ·)) plans groups := by
  let ask := fun (candidate : SourceOccurrenceCut source) (group : ShapeQueryGroup) =>
    group.2.2.map (shapeStateQuery (shapePreparedGroupStates (ShapePlan.of candidate) group))
  let next := fun (answers : List (List Bool)) candidate =>
    answers.zipWith (fun previous group => previous.zipWith (· || ·) (ask candidate group)) groups
  have commutes (rows : List (Array Bool)) (candidate : SourceOccurrenceCut source) :
      (shapeAccumulateAnswers (ShapePlan.of candidate) (groups.map shapePrepareQuery) rows ∅ []).map
        Array.toList = next (rows.map Array.toList) candidate := by
    simp only [shapeAccumulateAnswers_preserves_every_query, List.reverse_nil, List.map_nil,
      List.nil_append, next, ask, List.zipWith_map_left, List.zipWith_map_right,
      shapePreparedStates_matches_group]
    rfl
  unfold shapeGroupAnswers
  simp only [dbgTrace, ite_self]
  rw [shapeMapFold (fun state : List (Array Bool) × Nat => state.1.map Array.toList)
    (fun state candidate =>
      (shapeAccumulateAnswers (ShapePlan.of candidate) (groups.map shapePrepareQuery) state.1 ∅ [],
        state.2 + 1))
    next (fun state candidate => commutes state.1 candidate) plans
    (groups.map (fun group => group.2.2.toArray.map fun _ => false), 0)]
  simp only [List.map_map, Array.toList_map, Function.comp_def]
  simp only [shapeQueryAnswers, next, ask, shapePreparedGroupStates_matches_independent_replay]

theorem shapeStateQuery_as_any (states : List (ShapePlan source × List SourcePoint))
    (request : Option ShapeRequest) :
    shapeStateQuery states request =
      states.any (fun state => shapeStateQuery [state] request) := by
  cases request with
  | none => cases states <;> simp [shapeStateQuery]
  | some request =>
      obtain ⟨choice, event, receive⟩ := request
      simp [shapeStateQuery, shapePermits]

theorem shapeStateQuery_after_replay {command : Type}
    (step : (ShapePlan source × List SourcePoint) → command →
      List (ShapePlan source × List SourcePoint))
    (word : List command) (states : List (ShapePlan source × List SourcePoint))
    (request : Option ShapeRequest) :
    shapeStateQuery
      (word.foldl (fun before command => before.flatMap (fun state => step state command)) states)
      request =
      states.any (fun state => shapeStateQuery
        (word.foldl (fun before command => before.flatMap (fun state => step state command)) [state])
        request) := by
  rw [shapeStateQuery_as_any, shapeExistsReplay_matches_batched_replay]
  apply congrArg (fun accept => states.any accept)
  funext state
  rw [shapeStateQuery_as_any, shapeExistsReplay_matches_batched_replay]
  simp

theorem shapeReplayStates_query (source : Source) (plans : List (ShapePlan source))
    (word : List Nat) (inputs : List (List Nat)) (request : Option ShapeRequest) :
    shapeStateQuery ((shapeReplayStates source plans word inputs).getD []) request =
      plans.any (fun plan =>
        shapeStateQuery ((shapeReplayStates source [plan] word inputs).getD []) request) := by
  by_cases matched : word.length = inputs.length
  · have discardEmpty (states : List (ShapePlan source × List SourcePoint)) :
        (if states.isEmpty then none else some states).getD [] = states := by
      cases states <;> rfl
    simp only [shapeReplayStates, matched, bne_self_eq_false, Bool.false_eq_true,
      ↓reduceIte, discardEmpty]
    simp only [shapeInputStep]
    rw [shapeStateQuery_after_replay]
    simp only [List.any_map, Function.comp_def, List.map_cons, List.map_nil]
  · cases request <;> simp [shapeReplayStates, matched, shapeStateQuery, shapePermits]

theorem shapeStates_query (source : Source) (plans : List (ShapePlan source))
    (word : List Nat) (request : Option ShapeRequest) :
    shapeStateQuery (shapeStates source plans word) request =
      plans.any (fun plan => shapeStateQuery (shapeStates source [plan] word) request) := by
  unfold shapeStates
  rw [shapeStateQuery_after_replay]
  simp only [List.any_map, Function.comp_def, List.map_cons, List.map_nil]

theorem shapeQueryRows_fold {candidate query : Type}
    (ask : candidate → query → Bool) (candidates : List candidate) (queries : List query)
    (previous : query → Bool) :
    candidates.foldl
        (fun answers candidate =>
          answers.zipWith (· || ·) (queries.map (ask candidate))) (queries.map previous) =
      queries.map (fun query => previous query || candidates.any (fun candidate => ask candidate query)) := by
  induction candidates generalizing previous with
  | nil => simp
  | cons candidate candidates ih =>
      simp only [List.foldl_cons, List.zipWith_map, List.zipWith_self]
      rw [ih]
      simp only [List.any_cons, Bool.or_assoc]

theorem shapeGroupAnswers_match_whole_family (source : Source)
    (plans : List (SourceOccurrenceCut source)) (groups : List ShapeQueryGroup) :
    shapeGroupAnswers source plans groups =
      groups.map (fun group =>
        let states := match group.2.1 with
          | none => shapeStates source (plans.map ShapePlan.of) group.1
          | some inputs => (shapeReplayStates source (plans.map ShapePlan.of) group.1 inputs).getD []
        group.2.2.map (shapeStateQuery states)) := by
  rw [shapeGroupAnswers_matches_independent_queries, shapeQueryAnswers_matches_independent_folds]
  apply List.map_congr_left
  intro group _
  rw [shapeQueryRows_fold]
  apply List.map_congr_left
  intro request _
  simp only [Bool.false_or]
  cases groupInputs : group.2.1 with
  | none =>
      dsimp only
      rw [shapeStates_query]
      simp only [List.any_map, Function.comp_def]
  | some inputs =>
      dsimp only
      rw [shapeReplayStates_query]
      simp only [List.any_map, Function.comp_def]

theorem shapeGroupAnswers_as_candidate_queries (source : Source)
    (plans : List (SourceOccurrenceCut source)) (groups : List ShapeQueryGroup) :
    shapeGroupAnswers source plans groups = groups.map (fun group =>
      group.2.2.map fun request => plans.any fun candidate =>
        shapeStateQuery (shapePreparedGroupStates (ShapePlan.of candidate) group) request) := by
  rw [shapeGroupAnswers_matches_independent_queries, shapeQueryAnswers_matches_independent_folds]
  apply List.map_congr_left
  intro group _
  rw [shapeQueryRows_fold]
  simp only [Bool.false_or, shapePreparedGroupStates_matches_independent_replay]

theorem shapeGroupAnswers_query_batches (source : Source)
    (plans : List (SourceOccurrenceCut source)) (first second : List ShapeQueryGroup) :
    shapeGroupAnswers source plans (first ++ second) =
      shapeGroupAnswers source plans first ++ shapeGroupAnswers source plans second := by
  simp only [shapeGroupAnswers_as_candidate_queries, List.map_append]

#print axioms shapeGroupAnswers_query_batches

theorem shapeGroupAnswers_append (source : Source)
    (first second : List (SourceOccurrenceCut source)) (groups : List ShapeQueryGroup) :
    shapeGroupAnswers source (first ++ second) groups =
      (shapeGroupAnswers source first groups).zipWith (List.zipWith (· || ·))
        (shapeGroupAnswers source second groups) := by
  simp only [shapeGroupAnswers_as_candidate_queries, List.any_append,
    List.zipWith_map, List.zipWith_self]

-- Tasks divide one witness inventory, not independent verification jobs.
-- Only leaves run source queries. Continuations join results without blocking
-- worker threads or keeping a second application/protocol state machine.
def shapeParallelGroupAnswers (source : Source) :
    Nat → List (SourceOccurrenceCut source) → List ShapeQueryGroup → Task (List (List Bool))
  | 0, plans, groups => Task.spawn fun _ => shapeGroupAnswers source plans groups
  | levels + 1, plans, groups =>
      if plans.length ≤ 1 then Task.spawn fun _ => shapeGroupAnswers source plans groups
      else
        let middle := plans.length / 2
        let first := shapeParallelGroupAnswers source levels (plans.take middle) groups
        let second := shapeParallelGroupAnswers source levels (plans.drop middle) groups
        first.bind fun left => second.map fun right => left.zipWith (List.zipWith (· || ·)) right

theorem shapeParallelGroupAnswers_matches_serial (source : Source) (levels : Nat)
    (plans : List (SourceOccurrenceCut source)) (groups : List ShapeQueryGroup) :
    (shapeParallelGroupAnswers source levels plans groups).get =
      shapeGroupAnswers source plans groups := by
  induction levels generalizing plans with
  | zero => rfl
  | succ levels ih =>
      simp only [shapeParallelGroupAnswers]
      split
      · rfl
      · simp only [Task.bind, Task.map, ih]
        rw [← shapeGroupAnswers_append, List.take_append_drop]

#print axioms shapeGroupAnswers_as_candidate_queries
#print axioms shapeGroupAnswers_append
#print axioms shapeParallelGroupAnswers_matches_serial

#print axioms shapeStateQuery_as_any
#print axioms shapeCachedReplay_matches_fresh_replay
#print axioms shapePreparedGroupStates_matches_independent_replay
#print axioms shapePreparedStates_matches_group
#print axioms shapeMergeAnswers_preserves_all_answers
#print axioms shapeEmptyAnswers_are_neutral
#print axioms shapeTrueAnswers_are_absorbing
#print axioms shapeAccumulateAnswers_preserves_every_query
#print axioms shapeMapFold
#print axioms shapeGroupAnswers_matches_independent_queries
#print axioms shapeStateQuery_after_replay
#print axioms shapeReplayStates_query
#print axioms shapeStates_query
#print axioms shapeQueryRows_fold
#print axioms shapeGroupAnswers_match_whole_family


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

/-- Total visits at original source sites, not a separate limit on each copy
of a nested roll. Completed scripts visit every site twice per enclosing copy;
one further entry is available to a next-call probe. -/
def shapeVisitAllowance : Source → Nat → List Nat
  | .send _, _ => []
  | .seq left right, copies | .par left right, copies | .route _ left right, copies =>
      shapeVisitAllowance left copies ++ shapeVisitAllowance right copies
  | .roll body, copies => (2 * copies + 1) :: shapeVisitAllowance body (2 * copies)

theorem shapeVisitAllowance_has_one_count_per_original_roll (source : Source) (copies : Nat) :
    (shapeVisitAllowance source copies).length = shapeRepetitions source := by
  induction source generalizing copies <;>
    simp_all [shapeVisitAllowance, shapeRepetitions, Nat.add_comm]

#print axioms shapeVisitAllowance_has_one_count_per_original_roll

abbrev ShapeBudgetCut (source : Source) :=
  Σ expanded : Source, SourceUnfolding source expanded × Array Nat

def shapeBudgetStep (extend : (source : Source) → Array Nat → List (ShapeBudgetCut source)) :
    (source : Source) → Array Nat → List (ShapeBudgetCut source)
  | .send message, remaining => [⟨.send message, .send message, remaining⟩]
  | .seq left right, remaining =>
      let split := shapeRepetitions left
      (shapeBudgetStep extend left (remaining.extract 0 split)).flatMap fun first =>
        (shapeBudgetStep extend right (remaining.extract split remaining.size)).map fun second =>
          ⟨.seq first.1 second.1, .seq first.2.1 second.2.1, first.2.2 ++ second.2.2⟩
  | .par left right, remaining =>
      let split := shapeRepetitions left
      (shapeBudgetStep extend left (remaining.extract 0 split)).flatMap fun first =>
        (shapeBudgetStep extend right (remaining.extract split remaining.size)).map fun second =>
          ⟨.par first.1 second.1, .par first.2.1 second.2.1, first.2.2 ++ second.2.2⟩
  | .route resolver left right, remaining =>
      let split := shapeRepetitions left
      (shapeBudgetStep extend left (remaining.extract 0 split)).flatMap fun first =>
        (shapeBudgetStep extend right (remaining.extract split remaining.size)).map fun second =>
          ⟨.route resolver first.1 second.1, .route resolver first.2.1 second.2.1, first.2.2 ++ second.2.2⟩
  | .roll body, remaining =>
      let available := remaining[0]?.getD 0
      if available == 0 then [] else
      let inside := remaining.extract 1 remaining.size
      let once : List (ShapeBudgetCut (.roll body)) := (shapeBudgetStep extend body inside).map fun cut =>
        ⟨cut.1, .rollOnce cut.2.1, #[available - 1] ++ cut.2.2⟩
      let more : List (ShapeBudgetCut (.roll body)) :=
        (extend (.roll body) (#[available - 1] ++ inside)).flatMap fun first =>
          (shapeBudgetStep extend body (first.2.2.extract 1 first.2.2.size)).map fun second =>
            ⟨.seq first.1 second.1, .rollMore first.2.1 second.2.1,
              #[(first.2.2[0]?.getD 0)] ++ second.2.2⟩
      let independent : List (ShapeBudgetCut (.roll body)) := match body with
        | .par left right =>
          let split := shapeRepetitions left
          (extend (.roll left) (#[available] ++ inside.extract 0 split)).flatMap fun first =>
            (extend (.roll right) (#[available] ++ inside.extract split inside.size)).map fun second =>
              let unused := min (first.2.2[0]?.getD 0) (second.2.2[0]?.getD 0)
              ⟨.par first.1 second.1, .rollParallel first.2.1 second.2.1,
                #[unused] ++ first.2.2.extract 1 first.2.2.size ++
                  second.2.2.extract 1 second.2.2.size⟩
        | _ => []
      once ++ more ++ independent

def shapeBudgetCuts : Nat → (source : Source) → Array Nat → List (ShapeBudgetCut source)
  | 0 => shapeBudgetStep (fun _ _ => [])
  | fuel + 1 => shapeBudgetStep (shapeBudgetCuts fuel)

def shapeBoundedUnfoldings (source : Source) :
    List (Σ expanded : Source, SourceUnfolding source expanded) :=
  let allowance := (shapeVisitAllowance source 1).toArray
  -- Independent splits descend through source par nodes without spending a
  -- new visit. Include source structure in the termination fuel as well.
  (shapeBudgetCuts (allowance.toList.sum + source.eventCount + 1) source allowance).map fun cut =>
    ⟨cut.1, cut.2.1⟩

theorem a_skewed_parallel_roll_keeps_unequal_independent_visit_counts :
    let message := fun (actor : Nat) =>
      Source.send ⟨⟨0, actor⟩, ⟨0, actor⟩, actor, .wire 0⟩
    let original := Source.roll (.par (message 0)
      (.par (message 1) (.par (message 2) (message 3))))
    let expected := Source.par (.seq (.seq (message 0) (message 0)) (message 0))
      (.par (message 1) (.par (.seq (message 2) (message 2))
        (.seq (.seq (message 3) (message 3)) (message 3))))
    (shapeBoundedUnfoldings original).any (fun cut => decide (cut.1 = expected)) = true := by
  decide +kernel

#print axioms a_skewed_parallel_roll_keeps_unequal_independent_visit_counts

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
      ((shapeRollFocuses left).flatMap fun focus =>
        shapeAfterPlans focus right ++ shapeAlongsidePlans focus right) ++ inside
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
      let cuts := shapeBoundedUnfoldings source ++ shapeElasticUnfoldings source
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

private def redistributedNestedVisits : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .endpoint ⟨0, 3⟩⟩)
  (.seq (.roll (.par
    (.send ⟨⟨0, 3⟩, ⟨0, 2⟩, 1, .wire 0⟩)
    (.roll (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 2, .wire 0⟩))))
    (.send ⟨⟨0, 1⟩, ⟨0, 0⟩, 3, .endpoint ⟨0, 3⟩⟩))

-- Same global and operation word as the actual Endpoint regression.
theorem four_inner_visits_can_finish_before_the_parent_resource_return :
    shapePermits (shapeStates redistributedNestedVisits
      ((shapePlans redistributedNestedVisits).map ShapePlan.of)
      [0, 1, 2, 3, 4, 5, 4, 5, 4, 5, 4, 5]) [] 3 false = true := by
  rw [shapePermits_after_states]
  decide

#print axioms four_inner_visits_can_finish_before_the_parent_resource_return

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
      [0, 1, 0, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8]) [] 0 false = true := by
  rw [shapePermits_after_states]
  decide

theorem a_right_roll_can_resume_after_the_common_parallel_continuation :
    shapePermits (shapeStates parallelRetainedRolls
      ((shapePlans parallelRetainedRolls).map ShapePlan.of)
      [0, 1, 0, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8]) [] 2 false = true := by
  rw [shapePermits_after_states]
  decide

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
      [0, 1, 2, 3, 2, 3, 4, 5, 6, 7]) [] 1 false = true := by
  rw [shapePermits_after_states]
  decide

#print axioms a_new_holder_cannot_inherit_another_actors_rolled_operation
#print axioms returning_the_endpoint_to_the_roll_actor_allows_reentry

theorem an_inner_roll_can_exit_after_its_first_visit :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0]) [] 1 false = true := by
  rw [shapePermits_after_states]
  decide

theorem nested_roll_visits_can_have_different_lengths :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 0, 2, 0]) [] 1 false = true := by
  let tick : Source := .send ⟨⟨0, 0⟩, ⟨0, 0⟩, 0, .wire 0⟩
  let tail : Source := .send ⟨⟨0, 0⟩, ⟨0, 0⟩, 1, .wire 0⟩
  let expected : Source := .seq (.seq (.seq tick tick) tail) (.seq tick tail)
  let selected := (shapePlans nestedRollCuts).find?
    (fun plan => decide (plan.expanded = expected))
  have found : selected.isSome = true := by decide
  let witness := selected.get found
  refine shapePermits_after_states_of_witness _ _ _ _ _ _ (ShapePlan.of witness) ?_ ?_
  · exact List.mem_map.mpr
      ⟨witness, List.mem_of_find?_eq_some (Option.some_get found).symm, rfl⟩
  · decide +kernel +revert

theorem the_next_call_can_start_a_third_outer_visit :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 0, 2, 0, 0, 2]) [] 0 false = true := by
  rw [shapePermits_after_states]
  decide +kernel

theorem a_completed_tail_cannot_repeat_without_its_predecessor :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 2]) [] 1 false = false := by
  rw [shapePermits_after_states]
  decide +kernel

theorem a_completed_continuation_does_not_close_its_inner_roll :
    shapePermits (shapeStates nestedRollCuts ((shapePlans nestedRollCuts).map ShapePlan.of)
      [0, 0, 2]) [] 0 false = true := by
  rw [shapePermits_after_states]
  decide

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
                let command := 2 * event + if receive then 1 else 0
                let retained := shapeCanonicalChoice source states command choice
                next := (word ++ [command], inputs ++ [retained]) :: next
      level := next.eraseDups
      found := found ++ level
    return found

-- Every group carries this whole matrix, in source-event/direction order.
-- Group identity retains the recorded history; word-only deduplication is invalid.
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

private def opaqueControlBeforeDelegatedChoice : Source := .seq
  (.send ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .endpoint ⟨0, 3⟩⟩)
  (.route (some 0)
    (.send ⟨⟨0, 3⟩, ⟨0, 2⟩, 1, .wire 0⟩)
    (.send ⟨⟨0, 3⟩, ⟨0, 2⟩, 1, .wire 0⟩))

theorem transfer_inputs_are_opaque_but_the_delegated_controller_input_is_retained :
    let source := opaqueControlBeforeDelegatedChoice
    let initial := ((shapePlans source).map ShapePlan.of).map (·, [])
    shapeCanonicalInputs source initial [0, 1, 2] [[0], [0], [0]] = [[], [], [0]] := by decide

theorem a_controller_history_is_not_erased_when_its_choice_is_already_known :
    let source := opaqueControlBeforeDelegatedChoice
    let initial := ((shapePlans source).map ShapePlan.of).map (·, [])
    shapeCanonicalInputs source initial [0, 1, 2, 5] [[0], [0], [0], []] = [[], [], [0], []] := by decide

#print axioms transfer_inputs_are_opaque_but_the_delegated_controller_input_is_retained
#print axioms a_controller_history_is_not_erased_when_its_choice_is_already_known

private def selfResolvedRoll : Source := .roll (.route (some 0)
  (.send ⟨⟨0, 0⟩, ⟨0, 0⟩, 0, .wire 0⟩)
  (.send ⟨⟨0, 0⟩, ⟨0, 0⟩, 1, .wire 0⟩))

theorem self_controller_inputs_are_canonical_in_every_replay_state
    (states : List (ShapePlan selfResolvedRoll × List SourcePoint)) :
    shapeCanonicalInputs selfResolvedRoll states [0, 2, 0, 2] [[], [0], [], [0]] =
      [[], [0], [], [0]] := by
  apply shapeCanonicalInputs_already_canonical
  decide

#print axioms self_controller_inputs_are_canonical_in_every_replay_state

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

def shapeRead [Lean.FromJson α] (path : System.FilePath) : IO α := do
  let raw ← IO.FS.readFile path
  match Lean.Json.parse raw >>= Lean.fromJson? with
  | .ok value => pure value
  | .error error => throw (IO.userError s!"invalid shape JSON in {path}: {error}")

def shapeExpandRows (word : List Nat) (inputs : List (List Nat)) :
    List ShapeRequest → List (Bool × List (Nat × Bool)) → List ShapeAttempt
  | (choice, event, receive) :: questions, (accepted, parked) :: answers =>
      { choice, «prefix» := word, prefixChoices := inputs, event, receive, accepted, parked } ::
        shapeExpandRows word inputs questions answers
  | _, _ => []

theorem shapeExpandRows_preserves_every_answer (word : List Nat) (inputs : List (List Nat))
    (questions : List ShapeRequest) (answers : List (Bool × List (Nat × Bool)))
    (same : questions.length = answers.length) :
    ((shapeExpandRows word inputs questions answers).map fun attempt =>
      (attempt.accepted, attempt.parked)) = answers := by
  induction questions generalizing answers with
  | nil => cases answers <;> simp_all [shapeExpandRows]
  | cons question questions ih =>
      cases answers with
      | nil => simp at same
      | cons answer answers =>
          obtain ⟨choice, event, receive⟩ := question
          obtain ⟨accepted, parked⟩ := answer
          simp only [List.length_cons, Nat.add_right_cancel_iff] at same
          simp only [shapeExpandRows, List.map_cons]
          exact congrArg (List.cons (accepted, parked)) (ih answers same)

theorem shapeExpandRows_preserves_every_question (word : List Nat) (inputs : List (List Nat))
    (questions : List ShapeRequest) (answers : List (Bool × List (Nat × Bool)))
    (same : questions.length = answers.length) :
    ((shapeExpandRows word inputs questions answers).map fun attempt =>
      (attempt.choice, attempt.event, attempt.receive)) = questions := by
  induction questions generalizing answers with
  | nil => cases answers <;> simp_all [shapeExpandRows]
  | cons question questions ih =>
      cases answers with
      | nil => simp at same
      | cons answer answers =>
          obtain ⟨choice, event, receive⟩ := question
          obtain ⟨accepted, parked⟩ := answer
          simp only [List.length_cons, Nat.add_right_cancel_iff] at same
          simp only [shapeExpandRows, List.map_cons]
          exact congrArg (List.cons (choice, event, receive)) (ih answers same)

#print axioms shapeExpandRows_preserves_every_answer
#print axioms shapeExpandRows_preserves_every_question


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

def shapeCheckGuardedReentry : IO Unit := do
  -- Evaluate the whole bounded plan set natively. The general anchor theorem
  -- is kernel checked; materializing all these plans in a `by decide` proof
  -- duplicates the exhaustive search in the proof term.
  let guardedCases := [
      ([0, 1, 2, 3, 4, 5, 6, 7], true),
      ([0, 1, 2, 3, 4, 5, 6], true),
      ([2, 3, 4, 5, 0, 1, 6, 7], false),
      ([0, 1, 0, 1, 6, 7], false)]
  let guardedPlans := shapePlans guardedInnerRoll
  let guardedQueries := guardedCases.map fun (word, _) => (word, none, [some ([], 2, false)])
  let guardedAnswers := shapeGroupAnswers guardedInnerRoll guardedPlans guardedQueries
  unless (shapeParallelGroupAnswers guardedInnerRoll 2 guardedPlans guardedQueries).get ==
      guardedAnswers do
    throw (IO.userError "parallel source batches changed a guarded answer")
  unless guardedAnswers.length == guardedCases.length do throw (IO.userError "missing guarded answers")
  for ((word, expected), answers) in guardedCases.zip guardedAnswers do
    unless answers == [expected] do
      throw (IO.userError s!"guarded inner-roll regression: word={word} expected={expected}")
  let holderCases := [
      ([2, 3, 4, 5, 6, 7], true),
      ([2, 3, 4, 5, 6, 7, 8, 9], false),
      ([0, 1, 6, 7, 8, 9], true)]
  let holderAnswers := shapeGroupAnswers returnedConditionalHolder (shapePlans returnedConditionalHolder)
    (holderCases.map fun (word, _) => (word, none, [some ([], 3, false)]))
  unless holderAnswers.length == holderCases.length do throw (IO.userError "missing holder answers")
  for ((word, expected), answers) in holderCases.zip holderAnswers do
    unless answers == [expected] do
      throw (IO.userError s!"retained-owner regression: word={word} expected={expected}")
  unless shapeGroupAnswers movedUnrelatedResource (shapePlans movedUnrelatedResource)
      [([0, 1, 2, 3, 4, 5], none, [some ([], 1, false)])] == [[true]] do
    throw (IO.userError "an unrelated resource transfer blocked a retained roll")
  let selectedCases := [
      ([0, 1, 2, 3, 4, 5, 6, 7], true),
      ([0, 1, 2, 3, 4, 5, 8, 9], false)]
  let selectedAnswers := shapeGroupAnswers ownerPreservingContinuation (shapePlans ownerPreservingContinuation)
    (selectedCases.map fun (word, _) => (word, none, [some ([], 0, false)]))
  unless selectedAnswers.length == selectedCases.length do throw (IO.userError "missing selected answers")
  for ((word, expected), answers) in selectedCases.zip selectedAnswers do
    unless answers == [expected] do
      throw (IO.userError s!"selected-expansion regression: word={word} expected={expected}")
  -- This exact finite witness remains required even if another candidate
  -- could admit the same word.
  let selectedAfter := checkedPlans ownerPreservingContinuation _
    (.resumeAfterContinuation .hole
      (.seq (.rollOnce (.oneCopy _)) (.oneCopy _)) (.oneCopy _))
  unless shapeGroupAnswers ownerPreservingContinuation selectedAfter
      [([0, 1, 2, 3, 4, 5, 6, 7], none, [some ([], 0, false)])] == [[true]] do
    throw (IO.userError "selected-expansion regression: the exact after-continuation witness was rejected")
  let incomingCases := [
      ([0, 1, 2, 3, 4, 5, 0, 1], [], 3, true),
      ([0, 1, 2, 3, 4, 5, 0, 1], [0], 3, false),
      ([0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 8, 9], [0], 0, true),
      ([0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 8, 9, 10, 11], [0], 0, false)]
  let incomingAnswers := shapeGroupAnswers incomingLoanContinuation (shapePlans incomingLoanContinuation)
    (incomingCases.map fun (word, choice, event, _) => (word, none, [some (choice, event, false)]))
  unless incomingAnswers.length == incomingCases.length do throw (IO.userError "missing incoming answers")
  for ((word, choice, event, expected), answers) in incomingCases.zip incomingAnswers do
    unless answers == [expected] do
      throw (IO.userError s!"continuation-cut regression: word={word} choice={choice} event={event} expected={expected}")
  IO.println "Guarded reentry regressions: all fifteen source cuts passed"

def main (args : List String) : IO UInt32 := do
  if args == ["--check-guarded-reentry"] then
    shapeCheckGuardedReentry
    return 0
  let (inputPath, outputPath, weakenRoot, selected) ← match args with
    | [input, output] => pure (input, output, false, none)
    | [input, output, "--mutant-root-seq-as-par"] => pure (input, output, true, none)
    | ["--source", name, input, output] =>
      let some name := name.toNat? | throw (IO.userError "invalid source worker name")
      pure (input, output, false, some name)
    | ["--source", name, input, output, "--mutant-root-seq-as-par"] =>
      let some name := name.toNat? | throw (IO.userError "invalid source worker name")
      pure (input, output, true, some name)
    | _ => throw (IO.userError "usage: protocol-shapes INPUT.json OBSERVATION_DIRECTORY [--mutant-root-seq-as-par]")
  let inputs : Array ShapeInput ← shapeRead inputPath
  unless !inputs.isEmpty do throw (IO.userError "empty shape inventory")
  let expectedNames := inputs.toList.map (fun input => s!"{input.name}.jsonl")
  let actualNames := (← System.FilePath.readDir outputPath).toList.map (·.fileName)
  unless expectedNames.mergeSort (· ≤ ·) == actualNames.mergeSort (· ≤ ·) do
    throw (IO.userError "missing, duplicate or unknown shape observation")
  let expectedScripts := inputs.toList.map (fun input => s!"{input.name}.jsonl")
  for directory in ["scripts", "scope-observations"] do
    let observed := (← System.FilePath.readDir
      (System.FilePath.mk outputPath / ".." / directory)).toList.map (·.fileName)
    unless expectedScripts.mergeSort (· ≤ ·) == observed.mergeSort (· ≤ ·) do
      throw (IO.userError "missing, duplicate or unknown completed script file")
  if selected.isNone then
    -- One source owns one native worker's allocation lifetime. The parent
    -- validates the whole inventory and runs every source, sequentially,
    -- under the caller's single descendant-memory guard.
    let executable ← IO.appPath
    let mut checked := 0
    let mut continuations := 0
    let mut parkedChecked := 0
    let mut completedRuns := 0
    let mut completedCalls := 0
    let mut completedSources := 0
    for input in inputs do
      let flags := if weakenRoot then #["--mutant-root-seq-as-par"] else #[]
      let child ← IO.Process.spawn {
        cmd := executable.toString
        args := #["--source", toString input.name, inputPath, outputPath] ++ flags
        stdin := .null
        stdout := .piped
        stderr := .inherit
      }
      let output ← child.stdout.readToEnd
      let status ← child.wait
      unless status == 0 do
        IO.print output
        throw (IO.userError s!"shape {input.name}: source worker failed with exit {status}")
      let counts : Array Nat ← match
          (Lean.Json.parse output >>= Lean.fromJson? : Except String (Array Nat)) with
        | .ok counts => pure counts
        | .error error => throw (IO.userError s!"invalid source worker report: {error}")
      let #[name, attempts, continued, parked, runs, calls] := counts
        | throw (IO.userError "invalid source worker report fields")
      unless name == input.name do
        throw (IO.userError "source worker returned another source's report")
      checked := checked + attempts
      continuations := continuations + continued
      parkedChecked := parkedChecked + parked
      completedRuns := completedRuns + runs
      completedCalls := completedCalls + calls
      completedSources := completedSources + 1
    unless completedSources == inputs.size do
      throw (IO.userError "incomplete source worker inventory")
    IO.println s!"Completed shape scripts/source: shapes={completedSources} runs={completedRuns} calls={completedCalls} all accepted"
    IO.println s!"Generated shapes/source: shapes={completedSources} attempts={checked} initial={checked-continuations} continuation={continuations} runtime-only=0 model-only=0"
    IO.println s!"Successful parked previews/source: attempts={parkedChecked} runtime-only=0 model-only=0"
    return 0
  let some name := selected | throw (IO.userError "missing source worker name")
  let inputs := inputs.filter (·.name == name)
  let #[input] := inputs | throw (IO.userError "unknown or duplicate source worker name")
  let name := input.name
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
    IO.eprintln s!"shape {input.name}: checking complete executions and probe inventory"
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
    let candidates := shapePlans source
    unless !candidates.isEmpty do throw (IO.userError s!"shape {input.name}: no accepted source plan")
    let choices := shapeChoicePatterns source
    IO.eprintln s!"shape {input.name}: completed runs={runs} calls={calls} cuts={plannedPrefixes.length} source-witnesses={candidates.length}"
    let plannedInputs := plannedPrefixes.map fun word => (word, shapeWordInputs source word)
    let normalizedInputs := if plannedInputs.all (fun (word, inputs) =>
        shapeInputsAlreadyCanonical source word inputs) then plannedInputs
      else
        let prepared := candidates.map ShapePlan.of
        plannedInputs.map fun (word, inputs) =>
          (word, if shapeInputsAlreadyCanonical source word inputs then inputs
            else shapeCanonicalInputs source (prepared.map (·, [])) word inputs)
    let shortPrefixes := if shapeRepetitions source == 0 then
      shapeShortPrefixes source (candidates.map ShapePlan.of) choices else []
    let prefixes := (normalizedInputs ++ shortPrefixes).eraseDups
    let handle ← IO.FS.Handle.mk (System.FilePath.mk outputPath / s!"{input.name}.jsonl") .read
    let header : ShapeOutputHeader ← match
        (Lean.Json.parse (← handle.getLine) >>= Lean.fromJson? : Except String ShapeOutputHeader) with
      | .ok value => pure value
      | .error error => throw (IO.userError s!"invalid shape JSON: {error}")
    unless header.name == input.name && header.choices == choices &&
        header.events == (source.messageEntries 0).map Prod.fst do
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory; wrong query matrix")
    let questions := choices.flatMap fun choice => header.events.flatMap fun event =>
      [(choice, event, false), (choice, event, true)]
    unless !questions.isEmpty do throw (IO.userError "empty operation query matrix")
    let mut missing : Std.HashSet (List Nat × List (List Nat)) := {}
    for key in prefixes do missing := missing.insert key
    let mut ended := false
    while !ended do
      let mut attemptGroups : List (List Nat × List ShapeAttempt) := []
      -- This partitions work, not inputs: EOF and the exact independent
      -- prefix inventory are mandatory, including histories sharing a word.
      for _ in List.range (max 1 (65536 / questions.length)) do
        let line ← handle.getLine
        if line.isEmpty then
          ended := true
          break
        let observed : ShapeOutputGroup ← match
            (Lean.Json.parse line >>= Lean.fromJson? : Except String ShapeOutputGroup) with
          | .ok value => pure value
          | .error error => throw (IO.userError s!"invalid shape JSON: {error}")
        unless observed.prefix.length == observed.prefixChoices.length &&
            observed.prefixChoices.all (fun choice => choice == choice.eraseDups.mergeSort (· ≤ ·) &&
              choice.all (· < input.resolvers)) do
          throw (IO.userError s!"shape {input.name}: invalid replayed resolver inputs")
        let key := (observed.prefix, observed.prefixChoices)
        unless missing.contains key && observed.answers.size == questions.length do
          throw (IO.userError s!"shape {input.name}: incomplete probe inventory; unknown, duplicate or missing probe group")
        missing := missing.erase key
        let attempts := shapeExpandRows observed.prefix observed.prefixChoices questions observed.answers.toList
        attemptGroups := (observed.prefix, attempts) :: attemptGroups
      attemptGroups := attemptGroups.reverse
      let mut groups : Array ShapeQueryGroup := #[]
      for (word, attempts) in attemptGroups do
        let some first := attempts.head?
          | throw (IO.userError "empty prefix group")
        unless attempts.all (fun attempt => attempt.prefixChoices == first.prefixChoices) &&
            first.prefixChoices.all (fun choice =>
              choice == choice.eraseDups.mergeSort (· ≤ ·) &&
                choice.all (· < input.resolvers)) do
          throw (IO.userError s!"shape {input.name}: invalid replayed resolver inputs")
        let mut requests : Array (Option ShapeRequest) := #[none]
        for attempt in attempts do
          unless attempt.prefix == word do throw (IO.userError "inconsistent prefix group")
          requests := requests.push (some (attempt.choice, attempt.event, attempt.receive))
          match attempt.parked with
          | [] => pure ()
          | [(label, _)] =>
              unless label < 256 do
                throw (IO.userError s!"shape {input.name}: invalid parked preview label")
              if attempt.receive && (shapeMessage? source attempt.event).any
                  (fun message => message.sender == message.receiver) then
                requests := requests.push (some (attempt.choice, attempt.event, false))
          | _ => throw (IO.userError s!"shape {input.name}: duplicate parked preview observation")
        groups := groups.push (word, some first.prefixChoices, requests.toList)
      let answers := (shapeParallelGroupAnswers source (Nat.log2 (candidates.length / 4096))
        candidates groups.toList).get
      let observedAnswers := answers
      unless observedAnswers.length == attemptGroups.length do
        throw (IO.userError "incomplete source query answers")
      -- The defect control changes only the outer dependency. Prefix validity
      -- and parked local receives still use the original accepted source.
      let comparisonAnswers := if weakenRoot then
        let altered := match source with | .seq a b => .par a b | _ => source
        let plans := shapePlans altered
        (shapeParallelGroupAnswers altered (Nat.log2 (plans.length / 4096))
          plans groups.toList).get
      else observedAnswers
      unless comparisonAnswers.length == attemptGroups.length do
        throw (IO.userError "incomplete comparison query answers")
      for ((_, attempts), (original, comparison)) in
          attemptGroups.zip (observedAnswers.zip comparisonAnswers) do
        let (valid, original) ← match original with
          | valid :: rest => pure (valid, rest)
          | [] => throw (IO.userError "missing source-prefix answer")
        unless valid do
          throw (IO.userError s!"shape {input.name}: replayed inputs have no coherent source execution")
        let comparison ← match comparison with
          | _ :: rest => pure rest
          | [] => throw (IO.userError "missing comparison-prefix answer")
        unless original.length == comparison.length do
          throw (IO.userError "query answer lengths differ")
        let mut remaining := original.zip comparison
        for attempt in attempts do
          let (answer, rest) ← match remaining with
            | answer :: rest => pure (answer, rest)
            | [] => throw (IO.userError "missing operation query answer")
          remaining := rest
          let permitted := answer.2
          let localPreview ← if !attempt.parked.isEmpty && attempt.receive &&
              (shapeMessage? source attempt.event).any
                (fun message => message.sender == message.receiver) then
            match remaining with
            | answer :: rest =>
                remaining := rest
                pure answer.1
            | [] => throw (IO.userError "missing parked local query answer")
          else pure false
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
                        localPreview
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
        unless remaining.isEmpty do throw (IO.userError "extra operation query answers")
    unless missing.isEmpty do
      throw (IO.userError s!"shape {input.name}: incomplete probe inventory; missing prefix or input history")
    unless parkedRuntimeOnly == 0 && parkedModelOnly == 0 do
      IO.println s!"Parked previews/source mismatch: runtime-only={parkedRuntimeOnly} model-only={parkedModelOnly}"
    unless runtimeOnly == 0 && modelOnly == 0 do
      throw (IO.userError s!"generated Endpoint/source conformance failed: runtime-only={runtimeOnly} model-only={modelOnly}")
    IO.eprintln s!"shape {input.name}: complete source comparison passed"
  -- A worker returns only consumed-source metrics, after every original
  -- comparison and mismatch check above. Only the parent certifies a population.
  IO.println (Lean.toJson #[name, checked, continuations, parkedChecked,
    completedRuns, completedCalls]).compress
  pure 0
