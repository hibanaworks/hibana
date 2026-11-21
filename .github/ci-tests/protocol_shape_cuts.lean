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

private def fixtureCandidateReady (image : RustDescriptorImage) (view : Nat)
    (frames : List (List Nat)) (receive : Bool) (candidate : Nat) : Except String Bool := do
  let some action := image.eventAction? candidate | throw "source candidate has no operation"
  let some actualView := image.eventView? candidate | throw "source candidate has no view"
  let some lane := image.eventLane? candidate | throw "source candidate has no lane"
  let some color := image.eventFrameLabel? candidate | throw "source candidate has no wire label"
  unless actualView == view do throw "source candidate has a foreign view"
  match action with
  | .send .. =>
      if receive then throw "source send candidate has the wrong direction"
      pure true
  | .local .. => pure true
  | .recv peer .. =>
      unless receive do throw "source receive candidate has the wrong direction"
      pure (fixtureHead frames view lane == some [peer, view, lane, color])

private def collectReadyRows (check : Nat → Except String Bool)
    (ready : List Nat) : List Nat → Except String (List Nat)
  | [] => .ok ready
  | row :: tail => match check row with
    | .error reason => .error reason
    | .ok matched => collectReadyRows check (if matched then row :: ready else ready) tail

/-- Source-authorized sends and local actions are ready. A receive also needs
its exact FIFO head. The result retains every matching source row. -/
def fixtureReadyRows (image : RustDescriptorImage) (view : Nat) (frames : List (List Nat))
    (receive : Bool) : List Nat → Except String (List Nat) :=
  collectReadyRows (fixtureCandidateReady image view frames receive) []

private theorem recorded_rows_exact (check : Nat → Except String Bool) (rows acc out : List Nat)
    (result : collectReadyRows check acc rows = .ok out) (r : Nat) :
    r ∈ out ↔ r ∈ acc ∨ (r ∈ rows ∧ check r = .ok true) := by
  induction rows generalizing acc with
  | nil =>
    have same : acc = out := by simpa [collectReadyRows] using result
    simp [same]
  | cons a as ih =>
    cases h : check a with
    | error e => simp [collectReadyRows, h] at result
    | ok matched =>
      cases matched <;> simp [collectReadyRows, h] at result
      · rw [ih _ result]
        by_cases eq : r = a <;> simp_all
      · rw [ih _ result]
        by_cases eq : r = a <;> simp_all

theorem checked_ready_rows_match_the_fifo_predicate
    {image : RustDescriptorImage} {view : Nat} {frames : List (List Nat)}
    {receive : Bool} {rows out : List Nat} {row : Nat}
    (recorded : fixtureReadyRows image view frames receive rows = .ok out) :
    row ∈ out ↔ row ∈ rows ∧ fixtureCandidateReady image view frames receive row = .ok true := by
  simpa using recorded_rows_exact _ rows [] out recorded row

#print axioms checked_ready_rows_match_the_fifo_predicate

/-- For any candidate population, a source-authorized receive with its exact
FIFO head can never be classified as unavailable by the executable cut oracle.
Malformed evidence remains an error, including malformed rows after a hit. -/
theorem a_matching_fifo_receive_cannot_be_classified_as_unready
    {image : RustDescriptorImage} {frames : List (List Nat)} {rows : List Nat}
    {row view lane peer label schema wire : Nat} (candidate : row ∈ rows)
    (action : image.eventAction? row = some (.recv peer label schema))
    (role : image.eventView? row = some view) (port : image.eventLane? row = some lane)
    (color : image.eventFrameLabel? row = some wire)
    (arrived : fixtureHead frames view lane = some [peer, view, lane, wire]) :
    fixtureReadyRows image view frames true rows ≠ .ok [] := by
  intro empty
  have matched : fixtureCandidateReady image view frames true row = .ok true := by
    simp [fixtureCandidateReady, action, role, port, color, arrived,
      Bind.bind, Except.bind, Pure.pure, Except.pure]
  exact List.not_mem_nil ((checked_ready_rows_match_the_fifo_predicate empty).2 ⟨candidate, matched⟩)

#print axioms a_matching_fifo_receive_cannot_be_classified_as_unready

private theorem mapped_input_occurs {f : α → Option β} {inputs : List α} {outputs : List β}
    {x : α} {y : β} (mapped : inputs.mapM f = some outputs) (member : x ∈ inputs)
    (atInput : f x = some y) : y ∈ outputs := by
  induction inputs generalizing outputs with
  | nil => simp at member
  | cons first rest ih =>
    cases firstAt : f first with
    | none => simp [firstAt] at mapped
    | some value =>
      cases restAt : rest.mapM f with
      | none => simp [firstAt, restAt] at mapped
      | some tail =>
        have eq : value :: tail = outputs := by simpa [firstAt, restAt] using mapped
        subst outputs
        rcases List.mem_cons.mp member with same | later
        · subst first
          rw [atInput] at firstAt
          simp [Option.some.inj firstAt]
        · exact List.mem_cons_of_mem _ (ih restAt later)

/-- The maximum used to allocate a view's slots includes every projected row
of that view, regardless of where it occurs in the descriptor. -/
theorem projected_row_lane_lt_operation_slot_count
    {image : RustDescriptorImage} {row view lane : Nat} (present : row < image.eventCount)
    (onView : image.eventView? row = some view) (laneAt : image.eventLane? row = some lane) :
    lane < image.operationSlotCount view := by
  unfold RustDescriptorImage.operationSlotCount
  apply scan (target := row)
  · intro count index
    by_cases same : image.eventView? index = some view
    · cases read : image.eventLane? index with
      | none => simp [same]
      | some found => simpa [same, read] using Nat.le_max_left count (found + 1)
    · simp [same]
  · intro count
    simp only [onView, laneAt]
    exact Nat.lt_of_lt_of_le (Nat.lt_succ_self lane) (Nat.le_max_right count (lane + 1))
  · exact Or.inl (List.mem_range.mpr present)
where
  scan {update : Nat → Nat → Nat} {target lane : Nat}
      (grows : ∀ count index, count ≤ update count index)
      (found : ∀ count, lane < update count target)
      {rows : List Nat} {count : Nat} (known : target ∈ rows ∨ lane < count) :
      lane < rows.foldl update count := by
    induction rows generalizing count with
    | nil => simpa using known
    | cons head tail ih =>
      apply ih
      rcases known with member | counted
      · rcases List.mem_cons.mp member with same | later
        · subst head
          exact Or.inr (found count)
        · exact Or.inl later
      · exact Or.inr (Nat.lt_of_lt_of_le counted (grows count head))

/-- Every descriptor row of the target view whose original source position
is inside the borrow contributes its lane to the checked operation handle. -/
theorem a_checked_borrow_owns_each_in_range_row_lane
    {cut : PublicOperationCut} {image : RustDescriptorImage} {row origin lane : Nat}
    (checked : cut.matches image = true) (present : row < image.eventCount)
    (onView : image.eventView? row = some cut.target.view)
    (originAt : image.eventEffIndex? row = some origin) (laneAt : image.eventLane? row = some lane)
    (inside : cut.target.start ≤ origin ∧ origin < cut.target.stop) :
    lane ∈ cut.target.ownedLanes := by
  rw [a_checked_operation_cut_uses_the_descriptor_borrow_mask checked]
  apply List.mem_filter.mpr
  refine ⟨List.mem_range.mpr (projected_row_lane_lt_operation_slot_count present onView laneAt),
    ?_⟩
  rw [Bool.or_eq_true]
  right
  exact List.any_eq_true.mpr ⟨row, List.mem_range.mpr present, by simp [onView, laneAt, originAt, inside]⟩

/-- Every non-local row of the requested direction, view, label and schema
inside the borrowed fragment occurs in the executable contract lookup.
The call can name a different occurrence, including one at another peer. -/
theorem typed_wire_lookup_includes_each_borrowed_alias
    {cut : PublicOperationCut} {image : RustDescriptorImage} {origin row event lane peer : Nat}
    {atoms : List DecodedProgramAtom} {requested : DecodedProgramAtom} {query} (receive : Bool)
    (prepared : cut.contractQueries? image = some query)
    (decoded : image.decodeAtoms? = some atoms)
    (selected : atoms.find? (fun atom => atom.effIndex == origin) = some requested)
    (inImage : row < image.eventCount)
    (view : image.eventView? row = some cut.target.view)
    (source : image.eventEffIndex? row = some event)
    (port : image.eventLane? row = some lane)
    (action : image.eventAction? row = some (if receive then .recv peer requested.label requested.schema
                                               else .send peer requested.label requested.schema))
    (range : cut.target.start ≤ event ∧ event < cut.target.stop)
    (checked : cut.matches image = true) :
    ∃ candidates, query origin receive = some candidates ∧ row ∈ candidates := by
  have borrowed := a_checked_borrow_owns_each_in_range_row_lane checked inImage view source port range
  unfold PublicOperationCut.contractQueries? at prepared
  obtain ⟨actualAtoms, actualAt, prepared⟩ := Option.bind_eq_some_iff.mp prepared
  rw [decoded] at actualAt
  cases actualAt
  obtain ⟨rows, mapped, built⟩ := Option.bind_eq_some_iff.mp prepared
  cases Option.some.inj built
  simp only [selected, Bind.bind, Option.bind, Pure.pure]
  refine ⟨_, rfl, List.mem_filterMap.mpr ?_⟩
  let wireAction : LocalAction := if receive then .recv peer requested.label requested.schema
                                 else .send peer requested.label requested.schema
  refine ⟨(row, cut.target.view, event, lane, wireAction),
    mapped_input_occurs mapped (List.mem_range.mpr inImage) ?_, ?_⟩
  · simp [view, source, port, action, wireAction]
  · cases receive <;> simp [wireAction, range, borrowed, LocalAction.label, LocalAction.schema]

#print axioms typed_wire_lookup_includes_each_borrowed_alias

/-- The completion-bit prefilter of the candidate iterator may discard a row
only in an image without reentry. A source-ready row in a checked image is
unfinished now or has an explicit sealed roll. This holds for arbitrary cuts,
including fresh visits whose previous occurrence already completed; it uses
neither a recorded Rust admission nor the outcome of a public call. -/
theorem source_ready_survives_completed_row_filter
    {image : RustDescriptorImage} {choreo : Choreo} {retained : List Nat}
    {data : DecodedActorOrder} {path : NormalizedPath} {before : ActorProgressObservation}
    {row : DecodedActorRow} {previous next : Nat}
    (sealedRolls : image.actorRollCheck choreo = true)
    (ready : (sourceCurrentCandidateReady image choreo retained data path before row ||
      sourceFreshCandidateReady image choreo retained before previous next data path row) = true) :
    before.doneState.done row.eventId = false ∨ 0 < image.rollScopeCount := by
  cases count : image.rollScopeCount with
  | succ => exact Or.inr (by omega)
  | zero =>
    obtain ⟨_, _, rows, sourceRows, decoded⟩ := accepted_actor_rolls_bind_source_ranges sealedRolls
    have empty : image.decodeRollRows? = some [] := by
      simp [RustDescriptorImage.decodeRollRows?, count]
    rw [empty] at decoded
    cases decoded
    have current : sourceCurrentCandidateReady image choreo retained data path before row = true := by
      simpa [sourceFreshCandidateReady, sourceRows] using ready
    have candidate := (Bool.and_eq_true_iff.mp current).2
    exact Or.inl (by simpa using (Bool.and_eq_true_iff.mp candidate).1)

#print axioms source_ready_survives_completed_row_filter

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
