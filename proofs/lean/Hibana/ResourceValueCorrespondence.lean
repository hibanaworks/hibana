import Hibana.ResourceHeaderTransfer
import Hibana.ActorAdmission

namespace Hibana
open EndpointTransfer

/-- A raw issued header can belong to an application value, a sealed private
payload, or a transport-accepted sender witness. The last is zero authority:
the receiver may already have used and returned the sole operation right.
This proof relation adds no runtime table or progress state. -/
def ResourceHeadersHaveCustody (issued : View → Prop) (state : Observation View Frame) : Prop :=
  ∀ value, issued value →
    value ∈ state.usable ∨ value ∈ state.preparing ∨ value ∈ state.acceptedFutures

/-- Header effects at the actual move, acceptance, local publication and receipt
boundaries. Transport acceptance need not retire the sender header. A later
local publication can remove it through an unchanged-value step. Receipt may
interleave before publication. A disappearing future cannot leave its header
as a phantom witness. These observations must cover the complete execution. -/
def ResourceHeaderStepEvidence
    (action : Action) (before after : Observation View Frame) (first last : View → Prop) : Prop :=
  match action with
  | .seal | .accept | .fenceFrame | .unchanged =>
      ∀ value, last value → first value
  | .retireAcceptedFuture =>
      ∀ value, last value → first value ∧
        (value ∈ before.acceptedFutures → value ∈ after.acceptedFutures)
  | .abandonPreparation =>
      ∀ value, last value → first value ∧ value ∉ before.preparing
  | .dropValue => ∀ value, last value → first value ∧ value ∉ before.usable
  | .receive => ∀ value, last value → first value ∨ value ∈ after.usable.take 1

/-- Complete transfer boundaries annotated with their actual raw-header effects.
Authentic observations and affine carrier correspondence are implementation
contracts, not consequences of merely constructing a trace. -/
inductive ResourceHeaderExecution :
    (View → Prop) → Observation View Frame → (View → Prop) → Observation View Frame → Prop where
  | refl (issued state) : ResourceHeaderExecution issued state issued state
  | tail : ResourceHeaderExecution firstIssued first middleIssued middle →
      (step : Step action middle last) → ResourceHeaderStepEvidence action middle last middleIssued lastIssued →
      ResourceHeaderExecution firstIssued first lastIssued last

/-- Header coverage follows actual Rust/future custody, including accepted
witnesses. No header or operation-phase observation is treated as proof of an
application value. In particular this preservation rule does not require
sender publication to occur before receipt. -/
theorem observed_header_step_preserves_value_custody
    {action : Action} {before after : Observation View Frame} {first last : View → Prop}
    (step : Step action before after) (headers : ResourceHeaderStepEvidence action before after first last)
    (coverage : ResourceHeadersHaveCustody first before) :
    ResourceHeadersHaveCustody last after := by
  cases step with
  | «seal» state earlier later moved =>
      intro value issued
      have member := coverage value (headers value issued)
      simp only [List.mem_append, List.mem_cons] at member ⊢
      rcases member with (member | same | member) | member | member
      · exact .inl (.inl member)
      · exact .inr (.inl (.inl same))
      · exact .inl (.inr member)
      · exact .inr (.inl (.inr member))
      · exact .inr (.inr member)
  | accept state earlier later moved frame =>
      intro value issued
      have member := coverage value (headers value issued)
      simp only [List.mem_append, List.mem_cons] at member ⊢
      rcases member with member | (member | same | member) | member
      · exact .inl member
      · exact .inr (.inl (.inl member))
      · exact .inr (.inr (.inl same))
      · exact .inr (.inl (.inr member))
      · exact .inr (.inr (.inr member))
  | receive state rest frame fresh =>
      intro value issued
      rcases headers value issued with old | freshAt
      · rcases coverage value old with member | member | member
        · exact .inl (List.mem_cons.mpr (.inr member))
        · exact .inr (.inl member)
        · exact .inr (.inr member)
      · have same : value = fresh := by
          simpa only [List.take_succ_cons, List.take_zero, List.mem_singleton] using freshAt
        exact .inl (List.mem_cons.mpr (.inl same))
  | retireAcceptedFuture =>
      intro value issued
      obtain ⟨old, retained⟩ := headers value issued
      rcases coverage value old with member | member | member
      · exact .inl member
      · exact .inr (.inl member)
      · exact .inr (.inr (retained member))
  | abandonPreparation =>
      intro value issued
      obtain ⟨old, absent⟩ := headers value issued
      rcases coverage value old with member | member | member
      · exact .inl member
      · exact False.elim (absent member)
      · exact .inr (.inr member)
  | dropValue =>
      intro value issued
      obtain ⟨old, absent⟩ := headers value issued
      rcases coverage value old with member | member | member
      · exact False.elim (absent member)
      · exact .inr (.inl member)
      · exact .inr (.inr member)
  | fenceFrame => exact fun value issued => coverage value (headers value issued)
  | unchanged => exact fun value issued => coverage value (headers value issued)

private theorem a_header_history_preserves_its_initial_custody
    {first last : View → Prop} {before after : Observation View Frame}
    (history : ResourceHeaderExecution first before last after)
    (coverage : ResourceHeadersHaveCustody first before) (bound : before.authorityCount ≤ 1) :
    ResourceHeadersHaveCustody last after ∧ after.authorityCount ≤ 1 := by
  induction history with
  | refl => exact ⟨coverage, bound⟩
  | tail history step headers ih =>
      exact ⟨observed_header_step_preserves_value_custody step headers ih.1,
        Nat.le_trans (transfer_step_cannot_duplicate_authority step) ih.2⟩

private theorem a_preparing_member_is_the_exact_value
    {state : Observation View Frame} {value : View}
    (bound : state.authorityCount ≤ 1) (member : value ∈ state.preparing) :
    state.usable = [] ∧ state.preparing = [value] ∧ state.inFlight = [] := by
  have positive : 0 < state.preparing.length := List.length_pos_iff.mpr (by
    intro empty
    simp only [empty, List.mem_nil_iff] at member)
  have lengths : state.usable.length = 0 ∧ state.preparing.length = 1 ∧ state.inFlight.length = 0 := by
    simp only [Observation.authorityCount] at bound
    exact ⟨by omega, by omega, by omega⟩
  obtain ⟨only, singleton⟩ := List.length_eq_one_iff.mp lengths.2.1
  have same : value = only := by simpa only [singleton, List.mem_singleton] using member
  exact ⟨List.length_eq_zero_iff.mp lengths.1, by simpa only [same] using singleton,
    List.length_eq_zero_iff.mp lengths.2.2⟩

/-- Derive actual custody from initial issuance and the complete observed
history. An accepted witness is explicitly separate from usable or preparing
authority. The receiver is not required to wait for sender publication. -/
theorem an_issued_header_in_a_checked_trace_has_its_actual_custody
    {first last : View → Prop} {initialValue value : View} {state : Observation View Frame}
    (history : ResourceHeaderExecution first (initial initialValue) last state)
    (initialHeaders : ∀ value, first value ↔ value = initialValue)
    (issued : last value) :
    TransferValueLocated value state ∨ value ∈ state.acceptedFutures := by
  have initialCoverage : ResourceHeadersHaveCustody first (initial initialValue : Observation View Frame) := by
    intro value issuedAt
    exact .inl (List.mem_singleton.mpr ((initialHeaders value).mp issuedAt))
  have finalCoverage := a_header_history_preserves_its_initial_custody history initialCoverage
    (by simp only [initial, Observation.authorityCount, List.length_singleton, List.length_nil]; omega)
  rcases finalCoverage.1 value issued with usable | preparing | accepted
  · exact .inl (.inl (one_usable_authority_is_the_exact_value finalCoverage.2 usable))
  · exact .inl (.inr (a_preparing_member_is_the_exact_value finalCoverage.2 preparing))
  · exact .inr accepted

/-- At a history cut where this tag has no accepted witness, its raw issued
header identifies actual authority. Witness absence must come from the actual
execution; it is not a new runtime admission predicate or a progress oracle. -/
theorem an_issued_header_without_an_accepted_witness_has_its_actual_value
    {first last : View → Prop} {initialValue value : View} {state : Observation View Frame}
    (history : ResourceHeaderExecution first (initial initialValue) last state)
    (initialHeaders : ∀ value, first value ↔ value = initialValue)
    (issued : last value) (notAccepted : value ∉ state.acceptedFutures) :
    TransferValueLocated value state :=
  (an_issued_header_in_a_checked_trace_has_its_actual_custody history initialHeaders issued).resolve_right notAccepted

private theorem an_observed_tag_names_its_actor
    {snapshot : ActorProgressObservation} {role : Nat} {value : Nat × Nat × Nat × Nat}
    (found : observedResourceTag? snapshot role = some value) : value.1 = snapshot.actor := by
  unfold observedResourceTag? at found
  cases headerAt : observedResourceHeader? snapshot role with
  | none => simp [headerAt] at found
  | some header =>
      simp only [headerAt, Option.bind_eq_bind, Option.bind_some] at found
      split at found
      · exact (congrArg Prod.fst (Option.some.inj found)).symm
      · contradiction

private theorem an_observed_removal_has_its_exact_header_effect
    {before after : ActorProgressObservation} {role : Nat} {moved : Nat × Nat × Nat × Nat}
    {other : (Nat × Nat × Nat × Nat) → Prop}
    (beforeTag : observedResourceTag? before role = some moved)
    (afterTag : observedResourceTag? after role = none)
    (separate : ∀ value, other value → value.1 ≠ before.actor) :
    ∀ value, (observedResourceTag? after role = some value ∨ other value) ↔
      (observedResourceTag? before role = some value ∨ other value) ∧ value ≠ moved := by
  have actorAt := an_observed_tag_names_its_actor beforeTag
  intro value
  simp only [beforeTag, afterTag, Option.some.injEq, reduceCtorEq, false_or]
  constructor
  · intro present
    exact ⟨.inr present, fun same => separate value present (by simpa only [same] using actorAt)⟩
  · intro fields
    rcases fields.1 with same | present
    · exact False.elim (fields.2 same.symm)
    · exact present

/-- The checked send commit is local publication, after transport acceptance.
Its retired header is an unchanged-value boundary, not an atomic acceptance
step. The receiver may have progressed before this boundary is reached. -/
theorem a_checked_owned_send_supplies_its_local_publication_header_evidence
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts} {Frame : Type}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) (sending : facts.side = .send)
    (other : (Nat × Nat × Nat × Nat) → Prop) :
    ∀ state : Observation (Nat × Nat × Nat × Nat) Frame,
      ResourceHeaderStepEvidence .unchanged state state
        (fun value => observedResourceTag? sample.before facts.resource.role = some value ∨ other value)
        (fun value => observedResourceTag? sample.after facts.resource.role = some value ∨ other value) := by
  obtain ⟨_, _, _, afterTag⟩ := checked_owned_send_retires_its_exact_observed_tag checked decoded sending
  intro state value retained
  rcases retained with impossible | contextual
  · rw [afterTag] at impossible
    contradiction
  · exact .inr contextual

/-- A source-checked receipt publishes only the fresh tag of its actual view.
The affine carrier contract still relates this step to exactly one accepted
frame. It imposes no sender-publication dependency. -/
theorem a_checked_owned_receive_supplies_its_transfer_header_evidence
    {sample : ActorCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {wire : OwnedWireObservation} {session : Nat} {facts : OwnedCommitFacts} {Frame : Type}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) (receiving : facts.side = .receive)
    (other : (Nat × Nat × Nat × Nat) → Prop) :
    ∀ (state : Observation (Nat × Nat × Nat × Nat) Frame) (rest : List Frame) (frame : Frame),
      let fresh := (sample.after.actor, facts.afterView.header)
      let before := { state with inFlight := frame :: rest }
      let after := { state with inFlight := rest, usable := fresh :: state.usable }
      Step .receive before after ∧
        ResourceHeaderStepEvidence .receive before after
          (fun value => observedResourceTag? sample.before facts.resource.role = some value ∨ other value)
          (fun value => observedResourceTag? sample.after facts.resource.role = some value ∨ other value) := by
  obtain ⟨beforeTag, afterTag⟩ := checked_owned_receive_publishes_its_exact_observed_tag checked decoded receiving
  intro state rest frame
  refine ⟨.receive state rest frame (sample.after.actor, facts.afterView.header), ?_⟩
  intro value retained
  rcases retained with freshAt | contextual
  · right
    have same : value = (sample.after.actor, facts.afterView.header) :=
      (Option.some.inj (afterTag.symm.trans freshAt)).symm
    simpa only [List.take_succ_cons, List.take_zero, List.mem_singleton] using same
  · exact .inl (.inr contextual)

theorem a_checked_retirement_supplies_its_transfer_header_evidence
    {sample : ViewRetirementObservation} {image : RustDescriptorImage} {choreo : Choreo} {Frame : Type}
    {other : (Nat × Nat × Nat × Nat) → Prop}
    (checked : sample.check image choreo = true)
    (separate : ∀ value, other value → value.1 ≠ sample.before.actor) :
    ∃ view, observedActorView? sample.before sample.view = some view ∧
      ∀ state : Observation (Nat × Nat × Nat × Nat) Frame,
        let moved := (sample.before.actor, view.header)
        ResourceHeaderStepEvidence .dropValue
          { state with usable := [moved] } { state with usable := [] }
          (fun value => observedResourceTag? sample.before sample.view = some value ∨ other value)
          (fun value => observedResourceTag? sample.after sample.view = some value ∨ other value) ∧
        ResourceHeaderStepEvidence .abandonPreparation
          { state with preparing := [moved] } { state with preparing := [] }
          (fun value => observedResourceTag? sample.before sample.view = some value ∨ other value)
          (fun value => observedResourceTag? sample.after sample.view = some value ∨ other value) := by
  obtain ⟨view, viewAt, beforeTag, afterTag, _⟩ := checked_retirement_invalidates_its_exact_observed_tag checked
  refine ⟨view, viewAt, ?_⟩
  intro state
  constructor <;> intro value retained <;>
    simpa only [List.mem_singleton] using
      (an_observed_removal_has_its_exact_header_effect beforeTag afterTag separate value).mp retained

/-- Reset clears source progress but cannot mint a header or recover a moved
value. The next owned operation still supplies its checked local publication
or receive evidence at the same prepared base. -/
theorem prepared_reentry_supplies_unchanged_value_header_evidence
    {sample : ActorReentryCommitObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {roll : DecodedRollRow} {step : ActorCommitObservation} {Frame : Type}
    (prepared : sample.prepared? image choreo = some (roll, step))
    (state : Observation (Nat × Nat × Nat × Nat) Frame) (role : Nat)
    (other : (Nat × Nat × Nat × Nat) → Prop) :
    ResourceHeaderStepEvidence .unchanged state state
      (fun value => observedResourceTag? sample.before role = some value ∨ other value)
      (fun value => observedResourceTag? step.before role = some value ∨ other value) := by
  intro value retained
  simpa only [prepared_reentry_preserves_all_observed_resource_tags prepared] using retained

/-- Source readiness plus complete header/custody history identifies either
actual carrier authority or the old accepted-future witness. It does not infer
an application value from a header or add an ordering before receipt. -/
theorem a_ready_admitted_row_has_its_actual_carrier_custody
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {data : DecodedActorOrder} {path : EndpointDelegation.NormalizedPath} {row : DecodedActorRow}
    {Frame : Type} {first : (Nat × Nat × Nat × Nat) → Prop}
    {initialValue : Nat × Nat × Nat × Nat} {state : Observation (Nat × Nat × Nat × Nat) Frame}
    (checked : sample.check image choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (ready : sourceRowReady data path sample.snapshot row = true)
    (other : (Nat × Nat × Nat × Nat) → Prop)
    (history : ResourceHeaderExecution first (initial initialValue)
      (fun value => observedResourceTag? sample.snapshot row.view = some value ∨ other value) state)
    (initialHeaders : ∀ value, first value ↔ value = initialValue) :
    ∃ view, observedActorView? sample.snapshot row.view = some view ∧
      observedResourceTag? sample.snapshot row.view = some (sample.snapshot.actor, view.header) ∧
      (TransferValueLocated (sample.snapshot.actor, view.header) state ∨
        (sample.snapshot.actor, view.header) ∈ state.acceptedFutures) := by
  obtain ⟨actualData, actualPath, dataAt, pathAt, rows⟩ := accepted_actor_admission_has_exact_source_readiness checked
  have dataSame := Option.some.inj (dataAt.symm.trans decoded)
  subst actualData
  have pathSame := Option.some.inj (pathAt.symm.trans selected)
  subst actualPath
  have issued := (Bool.and_eq_true_iff.mp ((rows row member).2.2 ready)).1
  obtain ⟨view, found, _, _, tagAt⟩ := an_issued_resource_header_has_its_exact_observed_tag issued
  exact ⟨view, found, tagAt,
    an_issued_header_in_a_checked_trace_has_its_actual_custody history initialHeaders (.inl tagAt)⟩

theorem a_ready_owned_admitted_row_has_its_actual_payload_custody
    {sample : ActorAdmissionObservation} {image : RustDescriptorImage} {choreo : Choreo}
    {data : DecodedActorOrder} {path : EndpointDelegation.NormalizedPath} {row : DecodedActorRow}
    {binding : EndpointDelegation.Binding} {key : EndpointDelegation.EndpointKey}
    {Frame : Type} {first : (Nat × Nat × Nat × Nat) → Prop}
    {initialValue : Nat × Nat × Nat × Nat} {state : Observation (Nat × Nat × Nat × Nat) Frame}
    (checked : sample.check image choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data)
    (selected : data.normalized.paths[sample.pathIndex]? = some path)
    (member : row ∈ data.activeRows path)
    (located : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (payload : binding.message.payload = .endpoint key) (sending : row.actionId.side = .send)
    (ready : sourceRowReady data path sample.snapshot row = true)
    (other : (Nat × Nat × Nat × Nat) → Prop)
    (history : ResourceHeaderExecution first (initial initialValue)
      (fun value => observedResourceTag? sample.snapshot key.role = some value ∨ other value) state)
    (initialHeaders : ∀ value, first value ↔ value = initialValue) :
    ∃ view, observedActorView? sample.snapshot key.role = some view ∧
      observedResourceTag? sample.snapshot key.role = some (sample.snapshot.actor, view.header) ∧
      (TransferValueLocated (sample.snapshot.actor, view.header) state ∨
        (sample.snapshot.actor, view.header) ∈ state.acceptedFutures) := by
  obtain ⟨actualData, actualPath, dataAt, pathAt, rows⟩ := accepted_actor_admission_has_exact_source_readiness checked
  have dataSame := Option.some.inj (dataAt.symm.trans decoded)
  subst actualData
  have pathSame := Option.some.inj (pathAt.symm.trans selected)
  subst actualPath
  have issued := (Bool.and_eq_true_iff.mp ((rows row member).2.2 ready)).2
  have payloadIssued : (key.component == 0 && sample.snapshot.viewIssued key.role) = true := by
    simpa only [located, payload, sending] using issued
  obtain ⟨view, found, _, _, tagAt⟩ := an_issued_resource_header_has_its_exact_observed_tag
    (Bool.and_eq_true_iff.mp payloadIssued).2
  exact ⟨view, found, tagAt,
    an_issued_header_in_a_checked_trace_has_its_actual_custody history initialHeaders (.inl tagAt)⟩

/-- The native preemption schedule is admitted: the receiver can use, return,
and retire its future before the sender publishes. The raw sender header is
still live, but there is only one authority: the returned in-flight frame. -/
theorem a_receiver_can_return_before_sender_header_publication
    (sender receiver : View) (outgoing returned : Frame) :
    ∃ state : Observation View Frame,
      ResourceHeaderExecution (fun value => value = sender) (initial sender)
        (fun value => value = sender) state ∧
      state.usable = [] ∧ state.preparing = [] ∧ state.inFlight = [returned] ∧
      state.acceptedFutures = [sender] ∧ state.authorityCount = 1 := by
  let sealed : Observation View Frame := { initial sender with usable := [], preparing := [sender] }
  let sent : Observation View Frame := { sealed with preparing := [], acceptedFutures := [sender], inFlight := [outgoing] }
  let received : Observation View Frame := { sent with usable := [receiver], inFlight := [] }
  let returning : Observation View Frame := { received with usable := [], preparing := [receiver] }
  let accepted : Observation View Frame := { returning with preparing := [], acceptedFutures := [receiver, sender], inFlight := [returned] }
  let retired : Observation View Frame := { accepted with acceptedFutures := [sender] }
  have sealedAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender) sealed :=
    .tail (.refl _ _) (.seal (initial sender) [] [] sender) (fun _ present => present)
  have sentAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender) sent :=
    .tail sealedAt (.accept sealed [] [] sender outgoing) (fun _ present => present)
  have receivedAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender ∨ value = receiver) received := by
    apply ResourceHeaderExecution.tail sentAt (.receive sent [] outgoing receiver)
    intro value present
    exact present.elim Or.inl (fun same => .inr (List.mem_singleton.mpr same))
  have returningAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender ∨ value = receiver) returning :=
    .tail receivedAt (.seal received [] [] receiver) (fun _ present => present)
  have acceptedAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender ∨ value = receiver) accepted :=
    .tail returningAt (.accept returning [] [] receiver returned) (fun _ present => present)
  have publishedAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender) accepted :=
    .tail acceptedAt (.unchanged accepted) (fun _ present => .inl present)
  have retiredAt : ResourceHeaderExecution (fun value => value = sender) (initial sender)
      (fun value => value = sender) retired := by
    apply ResourceHeaderExecution.tail publishedAt (.retireAcceptedFuture accepted [] [sender] receiver)
    intro value present
    exact ⟨present, fun _ => List.mem_singleton.mpr present⟩
  exact ⟨retired, retiredAt, rfl, rfl, rfl, rfl, rfl⟩

/-- A counterexample to identifying every raw live header with usable or
preparing authority. Here its actual custody is only the suspended sender's
accepted witness, while the returned frame carries the sole authority. -/
theorem an_unpublished_sender_header_need_not_name_local_authority
    (sender receiver : View) (outgoing returned : Frame) :
    ∃ state : Observation View Frame,
      ResourceHeaderExecution (fun value => value = sender) (initial sender)
        (fun value => value = sender) state ∧
      TransferFrameLocated returned state ∧ sender ∈ state.acceptedFutures ∧
      ¬ TransferValueLocated sender state := by
  obtain ⟨state, history, usable, preparing, flight, accepted, _⟩ :=
    a_receiver_can_return_before_sender_header_publication sender receiver outgoing returned
  refine ⟨state, history, ⟨usable, preparing, flight⟩, ?_, ?_⟩
  · rw [accepted]
    exact List.mem_singleton_self _
  · intro located
    rcases located with fields | fields <;>
      have impossible := flight.symm.trans fields.2.2 <;> cases impossible

end Hibana
