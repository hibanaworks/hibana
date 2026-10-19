import Hibana.ResourceValueCorrespondence
import Hibana.PublicOperationKernel

namespace Hibana
open EndpointTransfer

/-- One fixed actor/run/role's existing nonce and issued header. This is a
proof observation, not a runtime record. Other roles may advance the actor's
counter without changing this header. Reattachment to a new run is outside
this history, just as it is outside one live session execution. -/
structure ViewNonceCut where
  counter : Nat
  issued : Option Nat

def resourceNonceCut (snapshot : ActorProgressObservation) (role : Nat) : ViewNonceCut := {
  counter := snapshot.counter
  issued := (observedResourceTag? snapshot role).map (·.2.2.2)
}

theorem a_valid_issued_resource_nonce_is_bounded
    {snapshot : ActorProgressObservation} {role : Nat}
    {tag : Nat × Nat × Nat × Nat}
    (valid : snapshot.valid = true)
    (found : observedResourceTag? snapshot role = some tag) : tag.2.2.2 ≤ snapshot.counter := by
  unfold observedResourceTag? at found
  cases headerAt : observedResourceHeader? snapshot role with
  | none => simp [headerAt] at found
  | some header =>
      simp only [headerAt, Option.bind_eq_bind, Option.bind_some] at found
      split at found
      next issued =>
        have same := Option.some.inj found
        have odd : header.2.2 % 2 = 1 := beq_iff_eq.mp (Bool.and_eq_true_iff.mp issued).2
        have present : header ∈ snapshot.viewHeaders := by
          have singleton := resolveUnique?_eq_some_iff.mp headerAt
          have member : header ∈ snapshot.viewHeaders.filter (fun h => h.1 == role) := by
            rw [singleton]
            exact List.mem_singleton_self _
          exact (List.mem_filter.mp member).1
        obtain ⟨view, member, headerSame⟩ := List.mem_map.mp present
        have parts := valid
        simp only [ActorProgressObservation.valid, Bool.and_eq_true, and_assoc] at parts
        have checked := List.all_eq_true.mp parts.2.2.2.2.2.2.2 view member
        simp only [Bool.and_eq_true, and_assoc, beq_iff_eq, decide_eq_true_eq] at checked
        have bounded := checked.2.2.1
        have counterOdd : snapshot.counter % 2 = 1 := beq_iff_eq.mp parts.2.2.1
        have nonceSame : view.nonce = header.2.2 := congrArg (·.2.2) headerSame
        have tagSame : tag.2.2.2 = header.2.2 := congrArg (·.2.2.2) same.symm
        omega
      next impossible => contradiction

inductive ViewNonceStep : ViewNonceCut → ViewNonceCut → Prop where
  | unchanged (cut) : ViewNonceStep cut cut
  | otherIssuance {counter next header}
      (fresh : nextViewGeneration? counter = some next) :
      ViewNonceStep ⟨counter, header⟩ ⟨next, header⟩
  | issuance {counter next}
      (fresh : nextViewGeneration? counter = some next) :
      ViewNonceStep ⟨counter, none⟩ ⟨next, some next⟩
  | retirement (counter header) :
      ViewNonceStep ⟨counter, header⟩ ⟨counter, none⟩

inductive ViewNonceHistory : ViewNonceCut → ViewNonceCut → Prop where
  | refl (cut) : ViewNonceHistory cut cut
  | tail : ViewNonceHistory first middle → ViewNonceStep middle last →
      ViewNonceHistory first last

/-- A completed send's old tag cannot become live again, even if the resource
returns many times before its completed future is destroyed. This includes
issuances for other views, and does not order independent source actions. -/
theorem retired_view_nonce_stays_absent_in_its_run
    {old : Nat} {before after : ViewNonceCut}
    (history : ViewNonceHistory before after)
    (bounded : old ≤ before.counter) (absent : before.issued ≠ some old) :
    old ≤ after.counter ∧ after.issued ≠ some old := by
  induction history with
  | refl => exact ⟨bounded, absent⟩
  | tail history step ih =>
      cases step with
      | unchanged => exact ih
      | otherIssuance fresh =>
          exact ⟨Nat.le_trans ih.1 (Nat.le_of_lt
            (fresh_view_generation_strictly_increases fresh)), ih.2⟩
      | issuance fresh =>
          have greater := fresh_view_generation_strictly_increases fresh
          exact ⟨Nat.le_trans ih.1 (Nat.le_of_lt greater),
            fun same => Nat.ne_of_lt (Nat.lt_of_le_of_lt ih.1 greater)
              (Option.some.inj same).symm⟩
      | retirement => exact ⟨ih.1, by simp⟩

/-- The originating public send call, viewed at a later execution cut. An
accepted sender witness either still blocks use of its payload, has returned
after publication, or belongs to a faulted run. For an asynchronous send,
blocking includes the existing send-payload phase between polls, not only the
short synchronous mutation lease. Success
contains the complete subsequent nonce history; a newer returned Endpoint
is not the old witness. The production call boundary and complete history
must be supplied by implementation correspondence. -/
inductive AcceptedSendCallEvidence (old : Nat) (current : ViewNonceCut)
    (callBlocked faulted : Prop) : Prop where
  | inside (held : callBlocked)
  | published {counter : Nat} (bounded : old ≤ counter)
      (suffix : ViewNonceHistory ⟨counter, none⟩ current)
  | failed (fault : faulted)

/-- Supply the successful originating call from the same checked owned source
commit that retires its exact sender tag. The suffix starts at that retirement,
not at physical acceptance, so receiver-first schedules remain admissible. -/
theorem checked_owned_publication_supplies_its_accepted_call_evidence
    {sample : ActorCommitObservation}
    {image : RustDescriptorImage} {choreo : Choreo} {wire : OwnedWireObservation}
    {session : Nat} {facts : OwnedCommitFacts} {tag : Nat × Nat × Nat × Nat}
    {current : ViewNonceCut} {callBlocked faulted : Prop}
    (checked : sample.ownedCheck image choreo wire session = true)
    (decoded : sample.ownedFacts? image choreo = some facts) (sending : facts.side = .send)
    (valid : sample.before.valid = true)
    (tagAt : observedResourceTag? sample.before facts.resource.role = some tag)
    (suffix : ViewNonceHistory (resourceNonceCut sample.after facts.resource.role) current) :
    AcceptedSendCallEvidence tag.2.2.2 current callBlocked faulted := by
  obtain ⟨view, viewAt, beforeTag, retired⟩ :=
    checked_owned_send_retires_its_exact_observed_tag checked decoded sending
  have delta := (checked_owned_commit_has_its_exact_resource_header_change checked decoded).2.2
  have counterSame : sample.after.counter = sample.before.counter := by
    simp only [observedViewCommitMatches, sending, viewAt, Bool.and_eq_true,
      and_assoc, beq_iff_eq, decide_eq_true_eq] at delta
    exact delta.2.2.2.2.1
  have bounded := a_valid_issued_resource_nonce_is_bounded valid tagAt
  have empty : resourceNonceCut sample.after facts.resource.role =
      ⟨sample.after.counter, none⟩ := by
    simp only [resourceNonceCut, retired, Option.map_none]
  rw [empty] at suffix
  exact .published (counterSame ▸ bounded) suffix

/-- Exclude stale accepted witnesses at a live public admission, rather than
assuming their absence at every internal transport cut. No receiver-to-sender
acknowledgment or runtime availability predicate is introduced. -/
theorem a_live_public_cut_cannot_issue_an_accepted_sender_tag
    {old : Nat} {current : ViewNonceCut} {callBlocked faulted : Prop}
    (origin : AcceptedSendCallEvidence old current callBlocked faulted)
    (available : ¬ callBlocked) (live : ¬ faulted) : current.issued ≠ some old := by
  cases origin with
  | inside held => exact False.elim (available held)
  | published bounded suffix =>
      exact (retired_view_nonce_stays_absent_in_its_run suffix bounded (by simp)).2
  | failed fault => exact False.elim (live fault)

/-- A physical lease alone cannot supply the originating-call evidence at a
lease-free pending-send cut whose old header is still issued. This is a
counterexample to that implementation correspondence, not to the conditional
public-cut theorem. -/
theorem a_lease_only_origin_does_not_cover_a_pending_issued_header (old : Nat) :
    ¬ AcceptedSendCallEvidence old ⟨old, some old⟩ False False := by
  intro origin
  exact a_live_public_cut_cannot_issue_an_accepted_sender_tag origin
    (fun held => held) (fun fault => fault) rfl

/-- The existing payload phase supplies exclusion after physical acceptance
while the send awaits I/O. It is not a second owner field or a new runtime
permission; the caller supplies this observation for the same resident view. -/
theorem a_pending_payload_supplies_its_blocked_call_evidence
    {old : Nat} {current : ViewNonceCut} {leaseHeld faulted : Prop}
    {phase : PublicOperationPhase} (pending : phase = .sendPayload) :
    AcceptedSendCallEvidence old current (leaseHeld ∨ phase = .sendPayload) faulted :=
  .inside (Or.inr pending)

/-- Actual fresh public entry excludes the payload phase. This derives the
unborrowed condition from the existing checked public-operation transition. -/
theorem an_admitted_fresh_public_operation_has_no_payload_borrow
    {phase : PublicOperationPhase} {edge : PublicOperationEdge}
    (fresh : edge = .beginSend ∨ edge = .beginRecv ∨ edge = .beginOffer)
    (entered : (transitionPublicOperation phase edge).lease = .held) :
    phase ≠ .sendPayload := by
  intro pending
  subst phase
  rcases fresh with rfl | rfl | rfl <;> cases entered

private theorem a_retained_witness_was_present_or_just_accepted
    {action : Action} {before after : Observation View Frame} {value : View}
    (step : Step action before after) (member : value ∈ after.acceptedFutures) :
    value ∈ before.acceptedFutures ∨
      (action = .accept ∧ after.acceptedFutures.head? = some value) := by
  cases step with
  | accept state left right moved frame =>
      rcases List.mem_cons.mp member with same | old
      · exact .inr ⟨rfl, by simp only [List.head?_cons, same]⟩
      · exact .inl old
  | retireAcceptedFuture state left right witness =>
      exact .inl (by
        simp only [List.mem_append, List.mem_cons] at member ⊢
        rcases member with left | right
        · exact .inl left
        · exact .inr (.inr right))
  | _ => exact .inl member

/-- Every retained zero-authority witness has an actual acceptance origin in
the complete transfer history. A custody snapshot cannot invent an origin,
and destroying a witness cannot create a new one. -/
theorem retained_sender_witness_has_its_acceptance_origin
    {initialValue value : View} {state : Observation View Frame}
    (history : Execution (initial initialValue) state)
    (member : value ∈ state.acceptedFutures) :
    ∃ before after, Execution (initial initialValue) before ∧
      Step .accept before after ∧ after.acceptedFutures.head? = some value ∧
      Execution after state := by
  induction history with
  | refl => simp only [initial, List.not_mem_nil] at member
  | tail history step ih =>
      rcases a_retained_witness_was_present_or_just_accepted step member with old | fresh
      · obtain ⟨before, after, earlier, accepted, tag, suffix⟩ := ih old
        exact ⟨before, after, earlier, accepted, tag, .tail suffix step⟩
      · obtain ⟨rfl, tag⟩ := fresh
        exact ⟨_, _, history, step, tag, .refl _⟩

/-- Connect live public admission to actual value/future custody. The call
correspondence is required only for acceptance origins found in this same
complete history; it is not an assumed source-ready-to-value oracle. Raw
headers inside a suspended send callback retain the weaker custody result.
This theorem does not yet establish route/roll admission or global progress. -/
theorem a_live_public_header_has_actual_authority_custody
    {first last : (Nat × Nat × Nat × Nat) → Prop}
    {initialValue value : Nat × Nat × Nat × Nat}
    {state : Observation (Nat × Nat × Nat × Nat) Frame}
    {current : ViewNonceCut} {callBlocked faulted : Prop}
    (headers : ResourceHeaderExecution first (initial initialValue) last state)
    (transfers : Execution (initial initialValue) state)
    (initialHeaders : ∀ tag, first tag ↔ tag = initialValue)
    (issued : last value) (exactTag : current.issued = some value.2.2.2)
    (available : ¬ callBlocked) (live : ¬ faulted)
    (calls : ∀ before after, Execution (initial initialValue) before →
      Step .accept before after → after.acceptedFutures.head? = some value →
      Execution after state →
      AcceptedSendCallEvidence value.2.2.2 current callBlocked faulted) :
    TransferValueLocated value state := by
  apply an_issued_header_without_an_accepted_witness_has_its_actual_value
    headers initialHeaders issued
  intro accepted
  obtain ⟨before, after, earlier, step, tag, suffix⟩ :=
    retained_sender_witness_has_its_acceptance_origin transfers accepted
  exact a_live_public_cut_cannot_issue_an_accepted_sender_tag
    (calls before after earlier step tag suffix) available live exactTag

/-- Apply custody to a real fresh public entry, retaining asynchronous payload
exclusion between polls. No source-ready-to-value availability is assumed. -/
theorem an_admitted_live_public_header_has_asynchronous_authority_custody
    {first last : (Nat × Nat × Nat × Nat) → Prop}
    {initialValue value : Nat × Nat × Nat × Nat}
    {state : Observation (Nat × Nat × Nat × Nat) Frame}
    {current : ViewNonceCut} {leaseHeld faulted : Prop}
    {phase : PublicOperationPhase} {edge : PublicOperationEdge}
    (headers : ResourceHeaderExecution first (initial initialValue) last state)
    (transfers : Execution (initial initialValue) state)
    (initialHeaders : ∀ tag, first tag ↔ tag = initialValue)
    (issued : last value) (exactTag : current.issued = some value.2.2.2)
    (fresh : edge = .beginSend ∨ edge = .beginRecv ∨ edge = .beginOffer)
    (entered : (transitionPublicOperation phase edge).lease = .held)
    (available : ¬ leaseHeld) (live : ¬ faulted)
    (calls : ∀ before after, Execution (initial initialValue) before →
      Step .accept before after → after.acceptedFutures.head? = some value →
      Execution after state →
      AcceptedSendCallEvidence value.2.2.2 current
        (leaseHeld ∨ phase = .sendPayload) faulted) :
    TransferValueLocated value state := by
  apply a_live_public_header_has_actual_authority_custody headers transfers
    initialHeaders issued exactTag ?_ live calls
  intro blocked
  cases blocked with
  | inl held => exact available held
  | inr pending =>
      exact an_admitted_fresh_public_operation_has_no_payload_borrow fresh entered pending

end Hibana
