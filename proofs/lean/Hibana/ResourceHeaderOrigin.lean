import Hibana.PublicAdmissionCustody

namespace Hibana
open EndpointTransfer

/-- Introduction of a particular header at an actual receive boundary.
Static source position is not the identity of a repeated receipt. -/
def receivedHeaderAt (value : View) (action : Action) (after : Observation View Frame) : Prop :=
  action = .receive ∧ value ∈ after.usable.take 1

/-- The existing transfer/header steps, restricted to a suffix that does not
introduce this header again. The relation is erased proof evidence and does
not add a runtime epoch or a second ownership authority. -/
inductive HeaderSuffixWithoutReceipt (value : View) :
    (View → Prop) → Observation View Frame → (View → Prop) → Observation View Frame → Prop where
  | refl {issued state} : HeaderSuffixWithoutReceipt value issued state issued state
  | tail : HeaderSuffixWithoutReceipt value firstIssued first middleIssued middle →
      (step : Step action middle last) →
      ResourceHeaderStepEvidence action middle last middleIssued lastIssued →
      ¬receivedHeaderAt value action last →
      HeaderSuffixWithoutReceipt value firstIssued first lastIssued last

private theorem an_unintroduced_header_was_already_issued
    {value : View} {action : Action} {before after : Observation View Frame}
    {first last : View → Prop}
    (evidence : ResourceHeaderStepEvidence action before after first last)
    (issued : last value) (notReceived : ¬receivedHeaderAt value action after) : first value := by
  cases action with
  | «seal» | accept | fenceFrame | unchanged => exact evidence value issued
  | retireAcceptedFuture | abandonPreparation | dropValue => exact (evidence value issued).1
  | receive =>
      rcases evidence value issued with old | fresh
      · exact old
      · exact False.elim (notReceived ⟨rfl, fresh⟩)

/-- Recover the most recent introduction of the current issued header from
the complete transfer/header history. An earlier source body may legally run
after a later source receipt, so no ordering of static event IDs is assumed.
The result names the actual receive and a suffix without another receipt of
that same header, rather than taking a last lexical source occurrence. -/
theorem an_issued_header_has_initial_or_latest_receive_origin
    {View Frame : Type} [DecidableEq View]
    {first last : View → Prop} {initialValue value : View} {state : Observation View Frame}
    (history : ResourceHeaderExecution first (initial initialValue) last state)
    (initialHeaders : ∀ tag, first tag ↔ tag = initialValue)
    (issued : last value) :
    (value = initialValue ∧ HeaderSuffixWithoutReceipt value
      first (initial initialValue) last state) ∨
    ∃ beforeIssued before afterIssued after,
      ResourceHeaderExecution first (initial initialValue) beforeIssued before ∧
      Step .receive before after ∧
      ResourceHeaderStepEvidence .receive before after beforeIssued afterIssued ∧
      value ∈ after.usable.take 1 ∧
      HeaderSuffixWithoutReceipt value afterIssued after last state := by
  letI : DecidableEq Action := fun left right => by
    cases left <;> cases right <;> first
    | exact .isTrue rfl
    | exact .isFalse (fun impossible => by cases impossible)
  induction history with
  | refl => exact .inl ⟨(initialHeaders value).mp issued, .refl⟩
  | @tail middleIssued middle action next nextIssued prior step evidence ih =>
      haveI : Decidable (receivedHeaderAt value action next) :=
        inferInstanceAs (Decidable (action = .receive ∧ value ∈ next.usable.take 1))
      by_cases received : receivedHeaderAt value action next
      · obtain ⟨rfl, member⟩ := received
        exact .inr ⟨middleIssued, middle, nextIssued, next, prior, step, evidence, member, .refl⟩
      · have old := an_unintroduced_header_was_already_issued evidence issued received
        rcases ih old with ⟨initialAt, suffix⟩ |
          ⟨beforeIssued, before, afterIssued, after, prefixAt, receipt, headers, member, suffix⟩
        · exact .inl ⟨initialAt, .tail suffix step evidence received⟩
        · exact .inr ⟨beforeIssued, before, afterIssued, after,
            prefixAt, receipt, headers, member, .tail suffix step evidence received⟩

/-- At a live public cut, the current fresh header has both actual authority
custody and a receive origin in the same complete boundary history. Sender
acceptance witnesses remain distinct and receiver-first schedules are allowed.
The source/call and same-binding/run correspondence stay explicit contracts. -/
theorem a_fresh_live_public_header_has_actual_custody_and_receive_origin
    {Frame : Type} {first last : (Nat × Nat × Nat × Nat) → Prop}
    {initialValue value : Nat × Nat × Nat × Nat}
    {state : Observation (Nat × Nat × Nat × Nat) Frame}
    {current : ViewNonceCut} {callBlocked faulted : Prop}
    (headers : ResourceHeaderExecution first (initial initialValue) last state)
    (transfers : Execution (initial initialValue) state)
    (initialHeaders : ∀ tag, first tag ↔ tag = initialValue)
    (issued : last value) (exactTag : current.issued = some value.2.2.2)
    (fresh : value ≠ initialValue) (available : ¬callBlocked) (live : ¬faulted)
    (calls : ∀ before after, Execution (initial initialValue) before →
      Step .accept before after → after.acceptedFutures.head? = some value →
      Execution after state →
      AcceptedSendCallEvidence value.2.2.2 current callBlocked faulted) :
    TransferValueLocated value state ∧
      ∃ beforeIssued before afterIssued after,
        ResourceHeaderExecution first (initial initialValue) beforeIssued before ∧
        Step .receive before after ∧
        ResourceHeaderStepEvidence .receive before after beforeIssued afterIssued ∧
        value ∈ after.usable.take 1 ∧
        HeaderSuffixWithoutReceipt value afterIssued after last state := by
  refine ⟨a_live_public_header_has_actual_authority_custody headers transfers initialHeaders
    issued exactTag available live calls, ?_⟩
  rcases an_issued_header_has_initial_or_latest_receive_origin headers initialHeaders issued with
    ⟨same, _⟩ | received
  · exact False.elim (fresh same)
  · exact received

end Hibana
