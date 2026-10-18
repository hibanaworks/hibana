import Hibana.SourceReceiptDependency
import Hibana.OwnedCommit

namespace Hibana
open EndpointDelegation

private theorem normalized_binding_is_an_immutable_source_message
    {source : Source} {normalized : Normalization} {path : NormalizedPath}
    {event : Nat} {binding : Binding}
    (accepted : normalize source = some normalized) (selected : path ∈ normalized.paths)
    (member : (event, binding) ∈ path.bindings) :
    (event, binding.message) ∈ source.messageEntries 0 := by
  obtain ⟨issuance, run, elaborated, _, mapped⟩ :=
    accepted_normalized_path_has_a_solved_source_derivation accepted selected
  obtain ⟨⟨sourceEvent, symbolic⟩, present, lowered⟩ :=
    successful_option_mapM_has_input mapped (event, binding) member
  obtain ⟨value, valueAt, written⟩ := Option.bind_eq_some_iff.mp lowered
  have same := Option.some.inj written
  cases same
  have messageAt := (instantiated_binding_retains_message_and_actor_references valueAt).1
  apply (elaboration_retains_exact_source_message_occurrences elaborated).subset
  exact List.mem_map.mpr ⟨(event, symbolic), present,
    by simp only [messageAt]⟩

private theorem ordered_message_lookup
    {α : Type} {entries : List (Nat × α)} {event : Nat} {value : α}
    (ordered : entries.Pairwise (fun left right => left.1 < right.1))
    (member : (event, value) ∈ entries) :
    entries.find? (fun entry => entry.1 == event) = some (event, value) := by
  induction entries with
  | nil => simp at member
  | cons head tail ih =>
      have parts := List.pairwise_cons.mp ordered
      rcases List.mem_cons.mp member with same | member
      · subst head
        have identity : (event == event) = true := decide_eq_true rfl
        simp only [List.find?_cons, identity]
      · have less := parts.1 _ member
        have different : head.1 ≠ event := by omega
        simpa [different] using ih parts.2 member

/-- Route selection and visit changes cannot change the message at a fixed
source occurrence. Physical actor bindings may differ; this does not equate
those actors or identify different visits as one value. -/
theorem normalized_paths_agree_on_a_shared_source_message
    {source : Source} {normalized : Normalization} {firstPath secondPath : NormalizedPath}
    {event : Nat} {first second : Binding}
    (accepted : normalize source = some normalized)
    (firstSelected : firstPath ∈ normalized.paths) (secondSelected : secondPath ∈ normalized.paths)
    (firstAt : (event, first) ∈ firstPath.bindings)
    (secondAt : (event, second) ∈ secondPath.bindings) : first.message = second.message :=
  congrArg Prod.snd (Option.some.inj
    ((ordered_message_lookup (source_occurrence_enumeration_increases source 0)
      (normalized_binding_is_an_immutable_source_message accepted firstSelected firstAt)).symm.trans
    (ordered_message_lookup (source_occurrence_enumeration_increases source 0)
      (normalized_binding_is_an_immutable_source_message accepted secondSelected secondAt))))

/-- One copied commit and its wire observation. These records are proof inputs,
never an additional runtime cursor or authority table. Ordinary commits ignore
the wire; owned commits must validate it, without a failed-owned fallback. -/
structure ActorCommitRecord where
  step : ActorCommitObservation
  wire : OwnedWireObservation

def ActorCommitRecord.check
    (record : ActorCommitRecord) (image : RustDescriptorImage) (choreo : Choreo)
    (session : Nat) : Bool :=
  match image.decodeActorOrder? choreo with
  | none => false
  | some data =>
      match data.normalized.paths[record.step.pathIndex]?, data.rows[record.step.event]? with
      | some path, some row =>
          match path.bindings.find? (fun entry => entry.1 == row.actionId.event) with
          | none => false
          | some (_, binding) =>
              match binding.message.payload with
              | .wire _ => record.step.check image choreo
              | .endpoint _ => record.step.ownedCheck image choreo record.wire session
      | _, _ => false

theorem checked_record_is_a_checked_commit
    {record : ActorCommitRecord} {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    (checked : record.check image choreo session = true) :
    record.step.check image choreo = true := by
  unfold ActorCommitRecord.check at checked
  split at checked
  · contradiction
  · split at checked
    · split at checked
      · contradiction
      · split at checked
        · exact checked
        · exact (Bool.and_eq_true_iff.mp checked).1
    · contradiction

/-- A checked source commit only completes its own event. It cannot clear an
already completed event; visit resets are separate from this boundary. -/
theorem a_checked_source_commit_preserves_completed_events
    {record : ActorCommitRecord} {image : RustDescriptorImage} {choreo : Choreo}
    {session event : Nat}
    (checked : record.check image choreo session = true)
    (completed : record.step.before.doneState.done event = true) :
    record.step.after.doneState.done event = true := by
  have ordered := accepted_actor_commit_matches_source_progress
    (checked_record_is_a_checked_commit checked)
  obtain ⟨_, data, path, row, binding, _, _, _, _, _, _, _, _, afterDone, _, unchanged, _⟩ := ordered
  by_cases same : event = record.step.event
  · simpa only [same] using afterDone
  · exact (unchanged event same).trans completed

/-- Owned records never pass using only an ordinary completion check. The
payload kind comes from the decoded source binding, before wire validation. -/
theorem checked_owned_record_has_a_checked_wire
    {record : ActorCommitRecord} {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {entry : Nat × Binding} {key : EndpointKey}
    (checked : record.check image choreo session = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : data.normalized.paths[record.step.pathIndex]? = some path)
    (located : data.rows[record.step.event]? = some row)
    (bindingAt : path.bindings.find? (fun entry => entry.1 == row.actionId.event) = some entry)
    (payload : entry.2.message.payload = .endpoint key) :
    record.step.ownedCheck image choreo record.wire session = true := by
  simpa only [ActorCommitRecord.check, decoded, selected, located, bindingAt, payload] using checked

/-- Gaps may change temporary operation leases or clear a completed visit.
They cannot invent completion. This checks existing observations only. -/
def actorCommitGapCheck (before after : ActorProgressObservation) : Bool :=
  before.actor == after.actor && before.run == after.run &&
    (List.range after.progress.length).all fun event =>
      !after.doneState.done event || before.doneState.done event

private theorem checked_gap_cannot_create_completion
    {before after : ActorProgressObservation}
    (checked : actorCommitGapCheck before after = true) {event : Nat}
    (completed : after.doneState.done event = true) : before.doneState.done event = true := by
  have rows := (Bool.and_eq_true_iff.mp checked).2
  by_cases inside : event < after.progress.length
  · have one := List.all_eq_true.mp rows event (List.mem_range.mpr inside)
    simpa only [completed, Bool.not_true, Bool.false_or] using one
  · have missing : after.progress[event]? = none := List.getElem?_eq_none (by omega)
    change after.progress[event]?.getD false = true at completed
    simp only [missing, Option.getD_none] at completed
    contradiction

/-- A finite observation history, retaining its exact prefix and suffix. A gap
can clear bits, so this definition does not equate static event IDs with visits.
The actor/run agreement is checked on every gap and every actual commit.
Recording every commit remains an observation-completeness contract: this
checker cannot detect an omitted reset/commit pair with identical endpoints. -/
def actorCommitHistoryCheck
    (image : RustDescriptorImage) (choreo : Choreo) (session : Nat)
    (first final : ActorProgressObservation) (records : List ActorCommitRecord) : Bool :=
  match records with
  | [] => actorCommitGapCheck first final
  | record :: rest =>
      actorCommitGapCheck first record.step.before && record.check image choreo session &&
        actorCommitHistoryCheck image choreo session record.step.after final rest

/-- Every newly completed event has an actual checked commit in the supplied
history. Neither a reset nor a lease-only gap can manufacture that witness.
This proves occurrence of a commit, not continued possession of its value. -/
theorem newly_completed_event_has_a_checked_commit
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {first final : ActorProgressObservation} {records : List ActorCommitRecord}
    (history : actorCommitHistoryCheck image choreo session first final records = true)
    (unfinished : first.doneState.done event = false)
    (completed : final.doneState.done event = true) :
    ∃ earlier record later, records = earlier ++ record :: later ∧
      record.step.event = event ∧ record.check image choreo session = true ∧
      record.step.SourceOrdered image choreo := by
  induction records generalizing first with
  | nil =>
      have done := checked_gap_cannot_create_completion history completed
      rw [unfinished] at done
      contradiction
  | cons record rest ih =>
      have pieces := Bool.and_eq_true_iff.mp history
      have head := Bool.and_eq_true_iff.mp pieces.1
      have ordered := accepted_actor_commit_matches_source_progress
        (checked_record_is_a_checked_commit head.2)
      by_cases same : record.step.event = event
      · exact ⟨[], record, rest, rfl, same, head.2, ordered⟩
      · obtain ⟨_, data, path, row, binding, _, _, _, _, _, _, _, _, _, _, unchanged, _⟩ := ordered
        have beforeFalse : record.step.before.doneState.done event = false := by
          cases doneAt : record.step.before.doneState.done event with
          | false => rfl
          | true =>
              have done := checked_gap_cannot_create_completion head.1 doneAt
              rw [unfinished] at done
              contradiction
        have afterFalse : record.step.after.doneState.done event = false :=
          (unchanged event (Ne.symm same)).trans beforeFalse
        obtain ⟨earlier, found, later, written, eventAt, checked, sourceAt⟩ :=
          ih pieces.2 afterFalse
        exact ⟨record :: earlier, found, later, by simp only [written, List.cons_append],
          eventAt, checked, sourceAt⟩

/-- Source-derived predecessors marked complete in a qualified history are
backed by its checked commits, without a premise asserting issued headers or
live Endpoint values. Prefix/suffix witnesses preserve the actual interleaving. -/
theorem ready_source_predecessor_has_a_checked_commit
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {first final : ActorProgressObservation} {records : List ActorCommitRecord} {event : Nat}
    (history : actorCommitHistoryCheck image choreo session first final records = true)
    (unfinished : first.doneState.done event = false)
    (predecessor : event ∈ data.sourcePredecessors path row.eventId)
    (ready : sourceRowReady data path final row = true) :
    ∃ earlier record later, records = earlier ++ record :: later ∧
      record.step.event = event ∧ record.check image choreo session = true ∧
      record.step.SourceOrdered image choreo :=
  newly_completed_event_has_a_checked_commit history unfinished
    (List.all_eq_true.mp (Bool.and_eq_true_iff.mp ready).2 event predecessor)

private theorem decoded_order_retains_rows
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    (decoded : image.decodeActorOrder? choreo = some data) :
    image.decodeActorRows? = some data.rows := by
  cases recoveryAt : image.ownedRecoveryIndexCheck with
  | false => simp [RustDescriptorImage.decodeActorOrder?, recoveryAt] at decoded
  | true =>
      cases entryAt : image.ownedEntryResourcesCheck with
      | false => simp [RustDescriptorImage.decodeActorOrder?, recoveryAt, entryAt] at decoded
      | true =>
          simp only [RustDescriptorImage.decodeActorOrder?, recoveryAt, entryAt,
            ite_true, Option.pure_def, Option.bind_eq_bind, Option.bind_some,
            Option.bind_eq_some_iff, Option.some.injEq] at decoded
          rcases decoded with ⟨source, _, normalized, _, columns, _, markers, _,
            rows, rowsAt, lanes, _, eventDependencies, _, dependencies, _, rfl⟩
          exact rowsAt

private theorem event_has_a_last_record
    {records : List ActorCommitRecord} {event : Nat}
    (present : ∃ record ∈ records, record.step.event = event) :
    ∃ earlier record later, records = earlier ++ record :: later ∧
      record.step.event = event ∧ ∀ next ∈ later, next.step.event ≠ event := by
  induction records with
  | nil => simp at present
  | cons head tail ih =>
      cases contains : tail.any (fun record => record.step.event == event) with
      | true =>
          obtain ⟨record, member, guard⟩ := List.any_eq_true.mp contains
          obtain ⟨earlier, found, later, written, same, last⟩ :=
            ih ⟨record, member, beq_iff_eq.mp guard⟩
          exact ⟨head :: earlier, found, later, by simp only [written, List.cons_append], same, last⟩
      | false =>
          have last : ∀ next ∈ tail, next.step.event ≠ event := by
            intro next member same
            have guard : (next.step.event == event) = true := decide_eq_true same
            have impossible : tail.any (fun record => record.step.event == event) = true :=
              List.any_eq_true.mpr ⟨next, member, guard⟩
            rw [contains] at impossible
            contradiction
          obtain ⟨record, member, same⟩ := present
          rcases List.mem_cons.mp member with equal | member
          · subst record
            exact ⟨[], head, tail, rfl, same, last⟩
          · exact False.elim (last record member same)

private theorem checked_history_has_its_checked_suffix
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {first final : ActorProgressObservation} {record : ActorCommitRecord}
    {earlier later : List ActorCommitRecord}
    (history : actorCommitHistoryCheck image choreo session first final
      (earlier ++ record :: later) = true) :
    record.check image choreo session = true ∧
      actorCommitHistoryCheck image choreo session record.step.after final later = true := by
  induction earlier generalizing first with
  | nil =>
      have parts := Bool.and_eq_true_iff.mp history
      exact ⟨(Bool.and_eq_true_iff.mp parts.1).2, parts.2⟩
  | cons head tail ih =>
      exact ih (Bool.and_eq_true_iff.mp history).2

/-- After the last recorded commit of an event, a final true bit requires
true completion at every snapshot boundary in the checked suffix. Under the
complete-commit observation contract, a reset cannot silently resurrect an
earlier visit's completion. This does not establish observation completeness
or assert that its Endpoint value survived subsequent transfers. -/
theorem checked_unrepeated_suffix_retains_completion
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {first final : ActorProgressObservation} {records : List ActorCommitRecord}
    (history : actorCommitHistoryCheck image choreo session first final records = true)
    (unrepeated : ∀ record ∈ records, record.step.event ≠ event)
    (completed : final.doneState.done event = true) :
    first.doneState.done event = true ∧
      ∀ record ∈ records, record.step.before.doneState.done event = true ∧
        record.step.after.doneState.done event = true := by
  induction records generalizing first with
  | nil => exact ⟨checked_gap_cannot_create_completion history completed, by simp⟩
  | cons record rest ih =>
      have parts := Bool.and_eq_true_iff.mp history
      have head := Bool.and_eq_true_iff.mp parts.1
      have tail := ih parts.2 (fun next member => unrepeated next (List.mem_cons_of_mem _ member))
      have different := unrepeated record List.mem_cons_self
      have ordered := accepted_actor_commit_matches_source_progress
        (checked_record_is_a_checked_commit head.2)
      obtain ⟨_, data, path, row, binding, _, _, _, _, _, _, _, _, _, _, unchanged, _⟩ := ordered
      have beforeDone : record.step.before.doneState.done event = true :=
        (unchanged event (Ne.symm different)).symm.trans tail.1
      refine ⟨checked_gap_cannot_create_completion head.1 beforeDone, ?_⟩
      intro next member
      rcases List.mem_cons.mp member with same | member
      · subst next
        exact ⟨beforeDone, tail.1⟩
      · exact tail.2 next member

/-- Extract the most recent actual commit, with a checked suffix in which
its completion remains true. This distinguishes a currently recorded receipt
from the mere existence of a receipt in an earlier rolled visit. -/
theorem newly_completed_event_has_its_current_checked_commit
    {image : RustDescriptorImage} {choreo : Choreo} {session event : Nat}
    {first final : ActorProgressObservation} {records : List ActorCommitRecord}
    (history : actorCommitHistoryCheck image choreo session first final records = true)
    (unfinished : first.doneState.done event = false)
    (completed : final.doneState.done event = true) :
    ∃ earlier record later, records = earlier ++ record :: later ∧
      record.step.event = event ∧ record.check image choreo session = true ∧
      actorCommitHistoryCheck image choreo session record.step.after final later = true ∧
      (∀ next ∈ later, next.step.event ≠ event) ∧
      record.step.after.doneState.done event = true ∧
      ∀ next ∈ later, next.step.before.doneState.done event = true ∧
        next.step.after.doneState.done event = true := by
  obtain ⟨earlier, record, later, written, same, _, _⟩ :=
    newly_completed_event_has_a_checked_commit history unfinished completed
  have present : ∃ record ∈ records, record.step.event = event :=
    ⟨record, by rw [written]; simp, same⟩
  obtain ⟨earlier, record, later, written, same, last⟩ := event_has_a_last_record present
  have suffix := checked_history_has_its_checked_suffix (by simpa only [written] using history)
  have retained := checked_unrepeated_suffix_retains_completion suffix.2 last completed
  exact ⟨earlier, record, later, written, same, suffix.1, suffix.2, last, retained⟩

/-- A source receipt that is complete in a qualified history has its actual
wire-checked owned commit. The recorded selected path may differ from the
current path: source identity, not equality of route choices, binds its payload.
No issued-header or live-value premise substitutes for the receipt. -/
theorem completed_owned_source_row_has_a_wire_checked_commit
    {image : RustDescriptorImage} {choreo : Choreo} {session : Nat}
    {data : DecodedActorOrder} {path : NormalizedPath} {row : DecodedActorRow}
    {binding : Binding} {key : EndpointKey}
    {first final : ActorProgressObservation} {records : List ActorCommitRecord}
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : path ∈ data.normalized.paths) (member : row ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (payload : binding.message.payload = .endpoint key)
    (history : actorCommitHistoryCheck image choreo session first final records = true)
    (unfinished : first.doneState.done row.eventId = false)
    (completed : final.doneState.done row.eventId = true) :
    ∃ earlier record later, records = earlier ++ record :: later ∧
      record.step.event = row.eventId ∧
      record.step.ownedCheck image choreo record.wire session = true ∧
      record.step.SourceOrdered image choreo ∧
      actorCommitHistoryCheck image choreo session record.step.after final later = true ∧
      (∀ next ∈ later, next.step.event ≠ row.eventId) ∧
      record.step.after.doneState.done row.eventId = true ∧
      ∀ next ∈ later, next.step.before.doneState.done row.eventId = true ∧
        next.step.after.doneState.done row.eventId = true := by
  obtain ⟨earlier, record, later, written, eventAt, checked, suffix, last, retained⟩ :=
    newly_completed_event_has_its_current_checked_commit history unfinished completed
  have ordered := accepted_actor_commit_matches_source_progress
    (checked_record_is_a_checked_commit checked)
  obtain ⟨_, actualData, actualPath, actualRow, entry, dataAt, pathAt, rowAt,
    _, _, _, bindingAt, _⟩ := ordered
  have dataSame := Option.some.inj (dataAt.symm.trans decoded)
  subst actualData
  have ownIndex := decoded_actor_row_is_at_its_own_index (decoded_order_retains_rows decoded)
    (List.mem_filter.mp member).1
  have rowSame := Option.some.inj (rowAt.symm.trans (by simpa only [eventAt] using ownIndex))
  subst actualRow
  have entryGuard : (entry.1 == row.actionId.event) = true :=
    List.find?_some (p := fun (pair : Nat × Binding) => pair.1 == row.actionId.event)
      (a := entry) bindingAt
  have entryEvent : entry.1 = row.actionId.event := beq_iff_eq.mp entryGuard
  have keyed : (row.actionId.event, entry.2) ∈ actualPath.bindings := by
    have pair : entry = (row.actionId.event, entry.2) := Prod.ext entryEvent rfl
    rw [← pair]
    exact List.mem_of_find?_eq_some bindingAt
  have messageSame := normalized_paths_agree_on_a_shared_source_message
    (decoded_actor_order_retains_its_normalized_source decoded).2 selected
    (List.mem_of_getElem? pathAt) (List.mem_of_find?_eq_some located) keyed
  refine ⟨earlier, record, later, written, eventAt, ?_,
    accepted_actor_commit_matches_source_progress (checked_record_is_a_checked_commit checked),
    suffix, last, retained⟩
  exact checked_owned_record_has_a_checked_wire checked decoded pathAt rowAt bindingAt
    (by simpa only [← messageSame] using payload)

end Hibana
