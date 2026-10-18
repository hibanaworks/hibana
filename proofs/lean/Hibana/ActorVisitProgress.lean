import Hibana.ActorSourceProgress
import Hibana.ActorAdmission
import Hibana.SourceTransferAvailability

namespace Hibana
open EndpointDelegation

/-- The selected source path supplies the finite domain, including both
halves of a wire message and only one local operation for a self message.
This enumeration is proof data, not an execution order or runtime ledger. -/
def sourceVisitActions (bindings : List (Nat × Binding)) : List ActionId :=
  bindings.flatMap fun (event, _) =>
    [Side.send, .receive, .local].filterMap fun side =>
      let id : ActionId := ⟨event, side⟩
      if (action? bindings id).isSome then some id else none

def sourceVisitPoints (bindings : List (Nat × Binding)) : List SourcePoint :=
  (sourceVisitActions bindings).flatMap fun id =>
    match id.side with
    | .send => [.accepted id.event, .committed id]
    | .receive | .local => [.committed id]

private theorem a_decoded_action_has_its_binding
    {bindings : List (Nat × Binding)} {id : ActionId} {action : Action}
    (decoded : action? bindings id = some action) :
    ∃ entry ∈ bindings, entry.1 = id.event := by
  unfold action? at decoded
  cases found : bindings.find? (fun row => row.1 == id.event) with
  | none => simp [found] at decoded
  | some entry =>
      exact ⟨entry, List.mem_of_find?_eq_some found, beq_iff_eq.mp
        (List.find?_some (p := fun row : Nat × Binding => row.1 == id.event) found)⟩

theorem canonical_visit_actions_are_exactly_decodable
    {bindings : List (Nat × Binding)} {id : ActionId} :
    id ∈ sourceVisitActions bindings ↔ (action? bindings id).isSome = true := by
  constructor
  · intro member
    obtain ⟨entry, _, atEntry⟩ := List.mem_flatMap.mp member
    obtain ⟨side, _, emitted⟩ := List.mem_filterMap.mp atEntry
    dsimp only at emitted
    split at emitted
    next valid =>
      have same := Option.some.inj emitted
      simpa only [same] using valid
    next invalid => contradiction
  · intro valid
    obtain ⟨action, decoded⟩ := Option.isSome_iff_exists.mp valid
    obtain ⟨entry, member, eventAt⟩ := a_decoded_action_has_its_binding decoded
    apply List.mem_flatMap.mpr
    refine ⟨entry, member, List.mem_filterMap.mpr ⟨id.side, ?_, ?_⟩⟩
    · cases id.side <;> simp
    · have same : ({ event := entry.1, side := id.side } : ActionId) = id := by
        cases id
        simp_all
      simp only [same, valid, ite_true]

theorem canonical_visit_contains_exactly_its_committed_actions
    {bindings : List (Nat × Binding)} {id : ActionId} :
    .committed id ∈ sourceVisitPoints bindings ↔ id ∈ sourceVisitActions bindings := by
  constructor
  · intro member
    obtain ⟨actual, present, atActual⟩ := List.mem_flatMap.mp member
    have same : id = actual := by
      cases sideAt : actual.side <;> simpa [sideAt] using atActual
    exact same.symm ▸ present
  · intro member
    apply List.mem_flatMap.mpr
    refine ⟨id, member, ?_⟩
    cases id.side <;> simp

theorem canonical_visit_contains_exactly_its_wire_acceptances
    {bindings : List (Nat × Binding)} {event : Nat} :
    .accepted event ∈ sourceVisitPoints bindings ↔
      (⟨event, .send⟩ : ActionId) ∈ sourceVisitActions bindings := by
  constructor
  · intro member
    obtain ⟨actual, present, atActual⟩ := List.mem_flatMap.mp member
    cases sideAt : actual.side with
    | send =>
        have same : actual.event = event := (by simpa [sideAt] using atActual : event = actual.event).symm
        have identity : actual = ⟨event, .send⟩ := by cases actual; simp_all
        exact identity ▸ present
    | receive | «local» => simp [sideAt] at atActual
  · intro member
    exact List.mem_flatMap.mpr ⟨⟨event, .send⟩, member, by simp⟩

private theorem a_decodable_receive_has_a_decodable_send
    {bindings : List (Nat × Binding)} {event : Nat}
    (valid : (action? bindings ⟨event, .receive⟩).isSome = true) :
    (action? bindings ⟨event, .send⟩).isSome = true := by
  unfold action? at valid ⊢
  cases found : bindings.find? (fun row => row.1 == event) with
  | none => simp [found] at valid
  | some entry =>
      simp only [found, Option.bind_eq_bind, Option.bind_some] at valid ⊢
      split at valid
      next localAt => simp at valid
      next wireAt => simp [wireAt]

/-- Domain closure is derived from the source bindings. A caller cannot omit
an inconvenient predecessor to manufacture a ready point. Parallel siblings
remain unrelated; acceptance is distinct from sender publication. -/
theorem canonical_source_visit_is_dependency_closed
    {source : Source} {bindings : List (Nat × Binding)} {before after : SourcePoint}
    (member : after ∈ sourceVisitPoints bindings)
    (edge : SourcePointEdge source bindings before after) :
    before ∈ sourceVisitPoints bindings := by
  cases edge with
  | publish event =>
      exact canonical_visit_contains_exactly_its_wire_acceptances.mpr
        (canonical_visit_contains_exactly_its_committed_actions.mp member)
  | deliver event =>
      apply canonical_visit_contains_exactly_its_wire_acceptances.mpr
      apply canonical_visit_actions_are_exactly_decodable.mpr
      exact a_decodable_receive_has_a_decodable_send
        (canonical_visit_actions_are_exactly_decodable.mp
          (canonical_visit_contains_exactly_its_committed_actions.mp member))
  | @ordered prior next ordered =>
      apply canonical_visit_contains_exactly_its_committed_actions.mpr
      apply canonical_visit_actions_are_exactly_decodable.mpr
      unfold precedes at ordered
      cases priorAt : action? bindings prior with
      | none => simp [priorAt] at ordered
      | some action => rfl

/-- Reuse structural progress on the actual finite source domain. No new
rank argument, numerical scheduler, or assumed closed subdomain is needed. -/
theorem unfinished_canonical_source_visit_has_ready_point
    {source : Source} {bindings : List (Nat × Binding)} {observed : SourcePoint → Bool}
    (unfinished : ∃ point ∈ sourceVisitPoints bindings, observed point = false) :
    ∃ point ∈ sourceVisitPoints bindings, observed point = false ∧
      ∀ before, SourcePointEdge source bindings before point → observed before = true :=
  unfinished_source_visit_has_ready_point
    (fun _ _ member edge _ => canonical_source_visit_is_dependency_closed member edge) unfinished

/-- Copied descriptor and cursor-admission observations for one actor. The
family below is checked against source actions, not just its own row count. -/
structure ActorVisitRecord where
  image : RustDescriptorImage
  admission : ActorAdmissionObservation

def ActorVisitRecord.check (record : ActorVisitRecord) (choreo : Choreo)
    (source : Source) (path : NormalizedPath) : Bool :=
  record.admission.check record.image choreo &&
    match record.image.decodeActorOrder? choreo with
    | none => false
    | some data => decide (data.source = source) &&
        decide (data.normalized.paths[record.admission.pathIndex]? = some path)

def actorVisitRows (family : List ActorVisitRecord) (choreo : Choreo)
    (path : NormalizedPath) : List (ActorVisitRecord × DecodedActorRow) :=
  family.flatMap fun record =>
    match record.image.decodeActorOrder? choreo with
    | none => []
    | some data => (data.activeRows path).map fun row => (record, row)

def actorVisitRow? (family : List ActorVisitRecord) (choreo : Choreo)
    (path : NormalizedPath) (id : ActionId) : Option (ActorVisitRecord × DecodedActorRow) :=
  resolveUnique? ((actorVisitRows family choreo path).filter fun pair =>
    decide (pair.2.actionId = id))

def actorVisitObserved (family : List ActorVisitRecord) (choreo : Choreo)
    (path : NormalizedPath) (accepted : List Nat) : SourcePoint → Bool
  | .accepted event => accepted.contains event
  | .committed id => ((actorVisitRow? family choreo path id).map fun (record, row) =>
      record.admission.snapshot.doneState.done row.eventId).getD false

def sourceVisitPathCheck (choreo : Choreo) (source : Source) (path : NormalizedPath) : Bool :=
  decide (choreo.ownershipSource? = some source) &&
    match normalize source with
    | none => false
    | some normalized => normalized.paths.any fun candidate => decide (candidate = path)

/-- Fail closed on omitted or duplicated source actions, invalid records, or
a completed wire operation with no accepted-send observation. Accepted sends
are an append-only qualification observation for this visit, never authority
consulted by a participant or the runtime. -/
def actorVisitCutCheck (family : List ActorVisitRecord) (choreo : Choreo)
    (source : Source) (path : NormalizedPath) (accepted : List Nat) : Bool :=
  ((sourceVisitPathCheck choreo source path &&
    family.all (fun record => record.check choreo source path)) &&
    (accepted.all (fun event => (action? path.bindings ⟨event, .send⟩).isSome) &&
      decide accepted.Nodup)) &&
    (sourceVisitActions path.bindings).all fun id =>
      match actorVisitRow? family choreo path id with
      | none => false
      | some (record, row) =>
          if id.side = .local then true
          else !record.admission.snapshot.doneState.done row.eventId || accepted.contains id.event

/-- Even an empty family cannot replace a nonempty source protocol with a
fabricated empty binding domain. The path is checked against the one global
source before per-actor coverage is examined. -/
theorem checked_actor_visit_has_its_canonical_source_path
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat}
    (checked : actorVisitCutCheck family choreo source path accepted = true) :
    choreo.ownershipSource? = some source ∧
      ∃ normalized, normalize source = some normalized ∧ path ∈ normalized.paths := by
  have sourceChecked := (Bool.and_eq_true_iff.mp
    (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).1).1
  have fields := Bool.and_eq_true_iff.mp sourceChecked
  refine ⟨of_decide_eq_true fields.1, ?_⟩
  cases decoded : normalize source with
  | none => simp [decoded] at fields
  | some normalized =>
      have contains : normalized.paths.any (fun candidate => decide (candidate = path)) = true := by
        simpa only [decoded] using fields.2
      obtain ⟨candidate, member, same⟩ := List.any_eq_true.mp contains
      exact ⟨normalized, rfl, of_decide_eq_true same ▸ member⟩

private theorem a_checked_family_record_is_checked
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat} {record : ActorVisitRecord}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (inFamily : record ∈ family) : record.check choreo source path = true :=
  List.all_eq_true.mp (Bool.and_eq_true_iff.mp
    (Bool.and_eq_true_iff.mp (Bool.and_eq_true_iff.mp checked).1).1).2 record inFamily

private theorem a_checked_record_has_its_source_and_path
    {record : ActorVisitRecord} {choreo : Choreo} {source : Source} {path : NormalizedPath}
    (checked : record.check choreo source path = true) :
    record.admission.check record.image choreo = true ∧
      ∃ data, record.image.decodeActorOrder? choreo = some data ∧ data.source = source ∧
        data.normalized.paths[record.admission.pathIndex]? = some path := by
  have fields := Bool.and_eq_true_iff.mp checked
  refine ⟨fields.1, ?_⟩
  cases decoded : record.image.decodeActorOrder? choreo with
  | none => simp [decoded] at fields
  | some data =>
      have exactAt : data.source = source ∧
          data.normalized.paths[record.admission.pathIndex]? = some path := by
        simpa only [decoded, Bool.and_eq_true, decide_eq_true_eq] using fields.2
      exact ⟨data, rfl, exactAt⟩

private theorem a_visit_row_lookup_has_its_exact_origin
    {family : List ActorVisitRecord} {choreo : Choreo} {path : NormalizedPath}
    {id : ActionId} {record : ActorVisitRecord} {row : DecodedActorRow}
    (found : actorVisitRow? family choreo path id = some (record, row)) :
    record ∈ family ∧ row.actionId = id ∧
      ∃ data, record.image.decodeActorOrder? choreo = some data ∧ row ∈ data.activeRows path := by
  have singleton := resolveUnique?_eq_some_iff.mp found
  have member : (record, row) ∈ (actorVisitRows family choreo path).filter
      (fun pair => decide (pair.2.actionId = id)) := by
    rw [singleton]
    exact List.mem_singleton_self _
  have fields := List.mem_filter.mp member
  obtain ⟨actual, inFamily, atActual⟩ := List.mem_flatMap.mp fields.1
  cases decoded : actual.image.decodeActorOrder? choreo with
  | none => simp [decoded] at atActual
  | some data =>
      have exactAt : row ∈ data.activeRows path ∧ actual = record := by
        simpa [decoded] using atActual
      obtain ⟨rowMember, rfl⟩ := exactAt
      exact ⟨inFamily, of_decide_eq_true fields.2, data, decoded, rowMember⟩

theorem a_checked_actor_visit_covers_each_source_action
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat} {id : ActionId}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (member : id ∈ sourceVisitActions path.bindings) :
    ∃ record data row, record ∈ family ∧
      actorVisitRow? family choreo path id = some (record, row) ∧ row.actionId = id ∧
      record.admission.check record.image choreo = true ∧
      record.image.decodeActorOrder? choreo = some data ∧ data.source = source ∧
      data.normalized.paths[record.admission.pathIndex]? = some path ∧
      row ∈ data.activeRows path := by
  have fields := Bool.and_eq_true_iff.mp checked
  have covered := List.all_eq_true.mp fields.2 id member
  cases found : actorVisitRow? family choreo path id with
  | none => simp [found] at covered
  | some pair =>
      rcases pair with ⟨record, row⟩
      obtain ⟨inFamily, identity, data, decoded, rowMember⟩ :=
        a_visit_row_lookup_has_its_exact_origin found
      obtain ⟨valid, actual, actualAt, sourceAt, pathAt⟩ :=
        a_checked_record_has_its_source_and_path
          (a_checked_family_record_is_checked checked inFamily)
      have same := Option.some.inj (actualAt.symm.trans decoded)
      subst actual
      exact ⟨record, data, row, inFamily, rfl, identity, valid, decoded, sourceAt, pathAt, rowMember⟩

private theorem a_covered_member_is_its_unique_lookup
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat}
    {record : ActorVisitRecord} {data : DecodedActorOrder} {row : DecodedActorRow}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (inFamily : record ∈ family) (decoded : record.image.decodeActorOrder? choreo = some data)
    (rowMember : row ∈ data.activeRows path) :
    actorVisitRow? family choreo path row.actionId = some (record, row) := by
  have recordChecked := a_checked_family_record_is_checked checked inFamily
  obtain ⟨valid, actual, actualAt, _, pathAt⟩ := a_checked_record_has_its_source_and_path recordChecked
  have same := Option.some.inj (actualAt.symm.trans decoded)
  subst actual
  have orderChecked : record.image.actorOrderCheck choreo = true :=
    (Bool.and_eq_true_iff.mp ((Bool.and_eq_true_iff.mp valid).1)).1
  have rowsChecked := (accepted_actor_order_binds_source_readiness orderChecked).1
  have idMember : row.actionId ∈ sourceVisitActions path.bindings := by
    apply canonical_visit_actions_are_exactly_decodable.mpr
    rw [active_decoded_row_has_its_exact_source_action rowsChecked decoded
      (List.mem_of_getElem? pathAt) rowMember]
    rfl
  obtain ⟨other, _, otherRow, _, found, _, _, _, _, _, _⟩ :=
    a_checked_actor_visit_covers_each_source_action checked idMember
  have singleton := resolveUnique?_eq_some_iff.mp found
  have member : (record, row) ∈ (actorVisitRows family choreo path).filter
      (fun pair => decide (pair.2.actionId = row.actionId)) := by
    apply List.mem_filter.mpr
    refine ⟨List.mem_flatMap.mpr ⟨record, inFamily, ?_⟩, by simp⟩
    simp only [decoded]
    exact List.mem_map.mpr ⟨row, rowMember, rfl⟩
  rw [singleton, List.mem_singleton] at member
  simpa only [member] using found

/-- A complete checked family reads the same committed bit from a source
action and its actual actor-local row. Missing/duplicate records cannot be
replaced by false bits to manufacture a candidate. -/
theorem checked_actor_visit_committed_observation_is_its_actual_row
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat}
    {record : ActorVisitRecord} {data : DecodedActorOrder} {row : DecodedActorRow}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (inFamily : record ∈ family) (decoded : record.image.decodeActorOrder? choreo = some data)
    (rowMember : row ∈ data.activeRows path) :
    actorVisitObserved family choreo path accepted (.committed row.actionId) =
      record.admission.snapshot.doneState.done row.eventId := by
  simp only [actorVisitObserved, a_covered_member_is_its_unique_lookup checked inFamily decoded rowMember,
    Option.map_some, Option.getD_some]

theorem a_source_predecessor_has_its_exact_actor_row
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {path : NormalizedPath} {row : DecodedActorRow} {event : Nat}
    (decoded : image.decodeActorOrder? choreo = some data)
    (rowMember : row ∈ data.activeRows path)
    (member : event ∈ data.sourcePredecessors path row.eventId) :
    ∃ prior ∈ data.activeRows path, prior.eventId = event ∧
      precedes data.source path.bindings prior.actionId row.actionId = true := by
  obtain ⟨prior, priorMember, emitted⟩ := List.mem_filterMap.mp member
  split at emitted
  next hasAfter =>
    obtain ⟨candidate, candidateMember, relates⟩ := List.any_eq_true.mp hasAfter
    have fields : candidate.eventId = row.eventId ∧
        precedes data.source path.bindings prior.actionId candidate.actionId = true := by
      simpa only [Bool.and_eq_true, beq_iff_eq] using relates
    have rowsAt := (decoded_actor_order_fields decoded).2.2.2.2.1
    have candidateAt := decoded_actor_row_is_at_its_own_index rowsAt
      (List.mem_filter.mp candidateMember).1
    have rowAt := decoded_actor_row_is_at_its_own_index rowsAt
      (List.mem_filter.mp rowMember).1
    rw [fields.1] at candidateAt
    have same := Option.some.inj (candidateAt.symm.trans rowAt)
    subst candidate
    exact ⟨prior, priorMember, Option.some.inj emitted, fields.2⟩
  next impossible => contradiction

/-- A ready entry in the global source graph enables its actual actor row.
Readiness is obtained through source dependency edges and unique descriptor
coverage, rather than supplied as an oracle about the runtime. -/
theorem ready_source_entry_has_checked_actor_admission
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat}
    {record : ActorVisitRecord} {data : DecodedActorOrder} {row : DecodedActorRow}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (inFamily : record ∈ family) (decoded : record.image.decodeActorOrder? choreo = some data)
    (rowMember : row ∈ data.activeRows path)
    (pending : actorVisitObserved family choreo path accepted (.committed row.actionId) = false)
    (ready : ∀ before, SourcePointEdge source path.bindings before row.actionId.entryPoint →
      actorVisitObserved family choreo path accepted before = true) :
    sourceRowReady data path record.admission.snapshot row = true ∧
      record.admission.admitted[row.eventId]? = some true ∧
      sourceRowViewsIssued path record.admission.snapshot row = true := by
  have recordChecked := a_checked_family_record_is_checked checked inFamily
  obtain ⟨valid, actual, actualAt, sourceAt, pathAt⟩ := a_checked_record_has_its_source_and_path recordChecked
  have same := Option.some.inj (actualAt.symm.trans decoded)
  subst actual
  have unfinished : record.admission.snapshot.doneState.done row.eventId = false := by
    rw [checked_actor_visit_committed_observation_is_its_actual_row checked inFamily decoded rowMember] at pending
    exact pending
  have sourceReady : sourceRowReady data path record.admission.snapshot row = true := by
    apply Bool.and_eq_true_iff.mpr
    refine ⟨by simp only [unfinished, Bool.not_false], List.all_eq_true.mpr ?_⟩
    intro event predecessor
    obtain ⟨prior, priorMember, eventAt, ordered⟩ :=
      a_source_predecessor_has_its_exact_actor_row decoded rowMember predecessor
    have edge : SourcePointEdge source path.bindings (.committed prior.actionId)
        row.actionId.entryPoint := .ordered (by simpa only [sourceAt] using ordered)
    have done := ready _ edge
    rw [checked_actor_visit_committed_observation_is_its_actual_row checked inFamily decoded priorMember] at done
    simpa only [eventAt] using done
  exact ⟨sourceReady,
    source_ready_row_is_admitted_by_a_checked_cursor_observation valid decoded pathAt rowMember sourceReady⟩

private theorem an_unaccepted_checked_send_is_uncommitted
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat} {event : Nat}
    {record : ActorVisitRecord} {row : DecodedActorRow}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (member : (⟨event, .send⟩ : ActionId) ∈ sourceVisitActions path.bindings)
    (found : actorVisitRow? family choreo path ⟨event, .send⟩ = some (record, row))
    (notAccepted : accepted.contains event = false) :
    record.admission.snapshot.doneState.done row.eventId = false := by
  have guarded := List.all_eq_true.mp (Bool.and_eq_true_iff.mp checked).2 _ member
  simpa only [found, reduceCtorEq, ↓reduceIte, notAccepted, Bool.or_false,
    Bool.not_eq_true'] using guarded

/-- General selected-visit progress for a complete family of actual cursor
observations. An unfinished source has an admitted descriptor operation, or
an already accepted send whose local publication is unfinished. The latter
is the existing send call, never permission to send the payload again.
This leaves transport delivery/fault and fresh-visit refinement explicit. -/
theorem checked_unfinished_actor_visit_has_admission_or_publication
    {family : List ActorVisitRecord} {choreo : Choreo} {source : Source}
    {path : NormalizedPath} {accepted : List Nat}
    (checked : actorVisitCutCheck family choreo source path accepted = true)
    (unfinished : ∃ point ∈ sourceVisitPoints path.bindings,
      actorVisitObserved family choreo path accepted point = false) :
    ∃ point record data row,
      point ∈ sourceVisitPoints path.bindings ∧
      actorVisitObserved family choreo path accepted point = false ∧ record ∈ family ∧
      record.image.decodeActorOrder? choreo = some data ∧ row ∈ data.activeRows path ∧
      ((point = .committed row.actionId ∧ row.actionId.side = .send ∧
          actorVisitObserved family choreo path accepted (.accepted row.actionId.event) = true ∧
          record.admission.snapshot.doneState.done row.eventId = false) ∨
        (point = row.actionId.entryPoint ∧
          sourceRowReady data path record.admission.snapshot row = true ∧
          record.admission.admitted[row.eventId]? = some true ∧
          sourceRowViewsIssued path record.admission.snapshot row = true)) := by
  obtain ⟨point, member, pending, ready⟩ :=
    unfinished_canonical_source_visit_has_ready_point (source := source) unfinished
  cases point with
  | accepted event =>
      have idMember := canonical_visit_contains_exactly_its_wire_acceptances.mp member
      obtain ⟨record, data, row, inFamily, found, identity, _, decoded, _, _, rowMember⟩ :=
        a_checked_actor_visit_covers_each_source_action checked idMember
      have entryAt : row.actionId.entryPoint = .accepted event := by rw [identity]; rfl
      have unfinished := an_unaccepted_checked_send_is_uncommitted checked idMember found pending
      have committed : actorVisitObserved family choreo path accepted (.committed row.actionId) = false := by
        rw [checked_actor_visit_committed_observation_is_its_actual_row checked inFamily decoded rowMember]
        exact unfinished
      have admitted := ready_source_entry_has_checked_actor_admission checked inFamily decoded rowMember committed
        (fun before edge => ready before (by simpa only [entryAt] using edge))
      exact ⟨_, record, data, row, member, pending, inFamily, decoded, rowMember, .inr ⟨entryAt.symm, admitted⟩⟩
  | committed id =>
      have idMember := canonical_visit_contains_exactly_its_committed_actions.mp member
      obtain ⟨record, data, row, inFamily, found, identity, _, decoded, _, _, rowMember⟩ :=
        a_checked_actor_visit_covers_each_source_action checked idMember
      have committed : actorVisitObserved family choreo path accepted (.committed row.actionId) = false := by
        simpa only [identity] using pending
      have unfinished : record.admission.snapshot.doneState.done row.eventId = false := by
        simpa only [checked_actor_visit_committed_observation_is_its_actual_row checked inFamily decoded rowMember] using committed
      refine ⟨_, record, data, row, member, pending, inFamily, decoded, rowMember, ?_⟩
      cases id with
      | mk event side =>
          cases side with
          | send =>
              refine .inl ⟨by rw [identity], by rw [identity], ?_, unfinished⟩
              simpa only [identity] using ready (.accepted event) (.publish event)
          | receive | «local» =>
              have entryAt : row.actionId.entryPoint = .committed row.actionId := by rw [identity]; rfl
              exact .inr ⟨by simpa only [identity] using entryAt.symm,
                ready_source_entry_has_checked_actor_admission checked inFamily decoded rowMember committed
                  (fun before edge => ready before (by simpa only [entryAt, identity] using edge))⟩

end Hibana
