import Hibana.MediatedCarrier

namespace Hibana

/-- Deployment-only carrier assumptions. The index is erased before any Pico
runtime or wire artifact is built. Each successor adds one obligation and
therefore exposes exactly which global claims a deployment may use. -/
inductive CarrierProfile where
  | mediated
  | authentic
  | ordered
  | closing
  | fair
  deriving Repr, DecidableEq

def CarrierProfile.rank : CarrierProfile -> Nat
  | .mediated => 0
  | .authentic => 1
  | .ordered => 2
  | .closing => 3
  | .fair => 4

def CarrierProfile.atMost
    (lower upper : CarrierProfile) : Prop :=
  lower.rank <= upper.rank

/-- Exact initial peer binding and receive observation. These are separated
from ordering so a deployment never obtains FIFO or no-replay merely from an
authenticated carrier identity. -/
structure CarrierAuthenticity
    (carrier : CarrierSemantics)
    (abstraction : carrier.State -> TransportBoundaryState) : Prop where
  initialExact : forall channel,
    abstraction (carrier.initial channel) = .initial channel
  pollExact : forall state,
    (carrier.pollReceive state).map abstraction =
      (abstraction state).pollReceive

/-- Exact accepted-send and affine receipt-resolution order. -/
structure CarrierOrdering
    (carrier : CarrierSemantics)
    (abstraction : carrier.State -> TransportBoundaryState) : Prop where
  sendExact : forall state frameLabel,
    (carrier.send state frameLabel).map abstraction =
      (abstraction state).send frameLabel
  resolveExact : forall state resolution,
    (carrier.resolve state resolution).map abstraction =
      (abstraction state).resolve resolution

/-- Exact graceful-close and abort observation. Liveness remains a separate
premise because no finite endpoint runtime can manufacture delivery fairness. -/
structure CarrierClosing
    (carrier : CarrierSemantics)
    (abstraction : carrier.State -> TransportBoundaryState) : Prop where
  closeExact : forall state,
    abstraction (carrier.closePeer state) = (abstraction state).closePeer
  abortExact : forall state,
    abstraction (carrier.abortPeer state) = (abstraction state).abortPeer

/-- Assumptions required by one profile. `Fairness` is supplied by the
deployment's execution model, not represented by a runtime flag. -/
def CarrierProfile.Holds
    (profile : CarrierProfile)
    (carrier : CarrierSemantics)
    (abstraction : carrier.State -> TransportBoundaryState)
    (Fairness : Prop) : Prop :=
  match profile with
  | .mediated => MediatedCarrierContract carrier
  | .authentic =>
      MediatedCarrierContract carrier /\
        CarrierAuthenticity carrier abstraction
  | .ordered =>
      MediatedCarrierContract carrier /\
        CarrierAuthenticity carrier abstraction /\
        CarrierOrdering carrier abstraction
  | .closing =>
      MediatedCarrierContract carrier /\
        CarrierAuthenticity carrier abstraction /\
        CarrierOrdering carrier abstraction /\
        CarrierClosing carrier abstraction
  | .fair =>
      MediatedCarrierContract carrier /\
        CarrierAuthenticity carrier abstraction /\
        CarrierOrdering carrier abstraction /\
        CarrierClosing carrier abstraction /\
        Fairness

theorem affine_carrier_refinement_supports_closing_profile
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    (refinement : AffineCarrierRefinement carrier abstraction) :
    CarrierProfile.closing.Holds carrier abstraction True := by
  exact And.intro
    (affine_carrier_refinement_establishes_mediated_contract refinement)
    (And.intro
      { initialExact := refinement.initialExact
        pollExact := refinement.pollExact }
      (And.intro
        { sendExact := refinement.sendExact
          resolveExact := refinement.resolveExact }
        { closeExact := refinement.closeExact
          abortExact := refinement.abortExact }))

theorem closing_profile_establishes_affine_carrier_refinement
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {Fairness : Prop}
    (supported : CarrierProfile.closing.Holds carrier abstraction Fairness) :
    AffineCarrierRefinement carrier abstraction := by
  exact {
    initialExact := supported.2.1.initialExact
    sendExact := supported.2.2.1.sendExact
    closeExact := supported.2.2.2.closeExact
    abortExact := supported.2.2.2.abortExact
    pollExact := supported.2.1.pollExact
    resolveExact := supported.2.2.1.resolveExact
  }

theorem carrier_profile_supports_every_weaker_profile
    {lower upper : CarrierProfile}
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {Fairness : Prop}
    (ordered : lower.atMost upper)
    (supported : upper.Holds carrier abstraction Fairness) :
    lower.Holds carrier abstraction Fairness := by
  cases lower <;> cases upper <;>
    simp_all [CarrierProfile.atMost, CarrierProfile.rank, CarrierProfile.Holds]

theorem fair_profile_exposes_fairness_premise
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {Fairness : Prop}
    (supported : CarrierProfile.fair.Holds carrier abstraction Fairness) :
    Fairness :=
  supported.2.2.2.2

theorem ordered_profile_requeue_restores_abstract_state
    {carrier : CarrierSemantics}
    {abstraction : carrier.State -> TransportBoundaryState}
    {Fairness : Prop}
    (supported : CarrierProfile.ordered.Holds carrier abstraction Fairness)
    {current offered restored : carrier.State}
    {frame : TransportFrame}
    (polled : carrier.pollReceive current = .delivered frame offered)
    (requeued : carrier.resolve offered .requeue = some restored) :
    abstraction restored = abstraction current := by
  have pollExact := supported.2.1.pollExact current
  rw [polled] at pollExact
  have abstractPoll :
      (abstraction current).pollReceive =
        .delivered frame (abstraction offered) := by
    simpa [CarrierPoll.map] using pollExact.symm
  have resolveExact := supported.2.2.resolveExact offered .requeue
  rw [requeued] at resolveExact
  have abstractRequeue :
      (abstraction offered).resolve .requeue =
        some (abstraction restored) := by
    simpa using resolveExact.symm
  exact polled_receipt_requeue_restores_exact_state
    abstractPoll abstractRequeue

theorem dropping_carrier_supports_mediated_profile :
    CarrierProfile.mediated.Holds droppingCarrierSemantics id True :=
  dropping_carrier_satisfies_mediated_contract

/-- Proof-only carrier that erases the requested channel before initialization.
It separates exclusive receipt mediation from exact peer/channel binding. -/
def channelErasingCarrierSemantics : CarrierSemantics where
  State := Unit
  initial := fun _ => ()
  send := fun state _ => some state
  closePeer := id
  abortPeer := id
  pollReceive := fun _ => .pending
  resolve := fun _ _ => none

private theorem channel_erasing_carrier_is_mediated :
    MediatedCarrierContract channelErasingCarrierSemantics := by
  refine {
    pollOwnsExclusiveReceipt := ?_
    requeueReoffersExactObservation := ?_
    resolutionIsAffine := ?_
    abortInvalidatesReceipt := ?_
  }
  · intro current offered frame polled
    simp [channelErasingCarrierSemantics] at polled
  · intro current offered restored frame polled requeued
    simp [channelErasingCarrierSemantics] at polled
  · intro current next resolution resolved
    simp [channelErasingCarrierSemantics] at resolved
  · intro current offered frame polled resolution
    simp [channelErasingCarrierSemantics] at polled

private theorem channel_erasing_carrier_has_no_authentic_abstraction :
    Not (Exists fun abstraction =>
      CarrierAuthenticity channelErasingCarrierSemantics abstraction) := by
  rintro ⟨abstraction, authentic⟩
  let first : TransportChannel := {
    session := 0
    generation := 0
    lane := 0
    sender := 0
    receiver := 0
  }
  let second : TransportChannel := {
    session := 0
    generation := 0
    lane := 0
    sender := 0
    receiver := 1
  }
  have firstExact := authentic.initialExact first
  have secondExact := authentic.initialExact second
  have impossible := firstExact.symm.trans secondExact
  simp [first, second,
    TransportBoundaryState.initial, TransportState.initial] at impossible

/-- Proof-only carrier with exact peer binding, FIFO, and receipt resolution,
but without an observable graceful close. -/
def closeIgnoringCarrierSemantics : CarrierSemantics where
  State := TransportBoundaryState
  initial := TransportBoundaryState.initial
  send := TransportBoundaryState.send
  closePeer := id
  abortPeer := TransportBoundaryState.abortPeer
  pollReceive := TransportBoundaryState.pollReceive
  resolve := TransportBoundaryState.resolve

private theorem close_ignoring_carrier_is_mediated :
    MediatedCarrierContract closeIgnoringCarrierSemantics := by
  refine {
    pollOwnsExclusiveReceipt := ?_
    requeueReoffersExactObservation := ?_
    resolutionIsAffine := ?_
    abortInvalidatesReceipt := ?_
  }
  · intro current offered frame polled
    change current.pollReceive = .delivered frame offered at polled
    change offered.pollReceive = .unavailable
    obtain ⟨before, after, _, _, offeredExact⟩ :=
      boundary_poll_issues_exact_receipt polled
    subst offered
    rfl
  · intro current offered restored frame polled requeued
    change current.pollReceive = .delivered frame offered at polled
    change offered.resolve .requeue = some restored at requeued
    change ∃ reoffered, restored.pollReceive = .delivered frame reoffered
    exact ⟨offered, requeued_receipt_reoffers_same_frame polled requeued⟩
  · intro current next resolution resolved
    change current.resolve resolution = some next at resolved
    change ∀ later, next.resolve later = none
    exact resolved_receipt_cannot_resolve_again resolved
  · intro current offered frame polled resolution
    change current.pollReceive = .delivered frame offered at polled
    change (offered.abortPeer).resolve resolution = none
    obtain ⟨before, after, _, _, offeredExact⟩ :=
      boundary_poll_issues_exact_receipt polled
    subst offered
    rfl

private theorem close_ignoring_carrier_is_authentic :
    CarrierAuthenticity closeIgnoringCarrierSemantics id := by
  refine {
    initialExact := ?_
    pollExact := ?_
  }
  · intro channel
    rfl
  · intro state
    cases polled : state.pollReceive <;>
      simp [closeIgnoringCarrierSemantics, CarrierPoll.map, polled]

private theorem close_ignoring_carrier_is_ordered :
    CarrierOrdering closeIgnoringCarrierSemantics id := by
  refine {
    sendExact := ?_
    resolveExact := ?_
  }
  · intro state frameLabel
    cases state <;>
      simp [closeIgnoringCarrierSemantics, TransportBoundaryState.send]
  · intro state resolution
    cases state <;> cases resolution <;>
      simp [closeIgnoringCarrierSemantics, TransportBoundaryState.resolve]

private theorem close_ignoring_carrier_is_not_closing :
    Not (CarrierClosing closeIgnoringCarrierSemantics id) := by
  intro closing
  let channel : TransportChannel := {
    session := 0
    generation := 0
    lane := 0
    sender := 0
    receiver := 1
  }
  have impossible := closing.closeExact
    (TransportBoundaryState.initial channel)
  simp [closeIgnoringCarrierSemantics, channel,
    TransportBoundaryState.initial, TransportBoundaryState.closePeer,
    TransportState.initial, TransportState.closePeer] at impossible

private theorem dropping_carrier_is_authentic :
    CarrierAuthenticity droppingCarrierSemantics id := by
  refine {
    initialExact := ?_
    pollExact := ?_
  }
  · intro channel
    rfl
  · intro state
    cases polled : state.pollReceive <;>
      simp [droppingCarrierSemantics, CarrierPoll.map, polled]

private theorem dropping_carrier_is_not_ordered :
    Not (CarrierOrdering droppingCarrierSemantics id) := by
  intro ordered
  let channel : TransportChannel := {
    session := 0
    generation := 0
    lane := 0
    sender := 0
    receiver := 1
  }
  have impossible := ordered.sendExact
    (TransportBoundaryState.initial channel) (0 : Fin 256)
  simp [droppingCarrierSemantics, channel, TransportBoundaryState.initial,
    TransportBoundaryState.send, TransportState.initial, TransportState.send]
    at impossible

/-- A strict profile step has one carrier/deployment witness that satisfies the
lower profile while failing the immediately stronger profile. -/
def CarrierProfile.StrictStep
    (lower upper : CarrierProfile) : Prop :=
  Exists fun carrier : CarrierSemantics =>
    Exists fun abstraction : carrier.State -> TransportBoundaryState =>
      Exists fun Fairness : Prop =>
        lower.Holds carrier abstraction Fairness /\
          Not (upper.Holds carrier abstraction Fairness)

/-- Every adjacent carrier profile adds a semantically independent premise.
The witnesses exist only in Lean and cannot enlarge the runtime or wire state. -/
theorem carrier_profile_hierarchy_is_strict :
    CarrierProfile.StrictStep .mediated .authentic /\
    CarrierProfile.StrictStep .authentic .ordered /\
    CarrierProfile.StrictStep .ordered .closing /\
    CarrierProfile.StrictStep .closing .fair := by
  constructor
  · let channel : TransportChannel := {
      session := 0
      generation := 0
      lane := 0
      sender := 0
      receiver := 0
    }
    let abstraction : Unit -> TransportBoundaryState := fun _ =>
      TransportBoundaryState.initial channel
    exact ⟨channelErasingCarrierSemantics, abstraction, True,
      channel_erasing_carrier_is_mediated, by
        intro supported
        exact channel_erasing_carrier_has_no_authentic_abstraction
          ⟨abstraction, supported.2⟩⟩
  · constructor
    · exact ⟨droppingCarrierSemantics, id, True,
        ⟨dropping_carrier_satisfies_mediated_contract,
          dropping_carrier_is_authentic⟩, by
            intro supported
            exact dropping_carrier_is_not_ordered supported.2.2⟩
    · constructor
      · exact ⟨closeIgnoringCarrierSemantics, id, True,
          ⟨close_ignoring_carrier_is_mediated,
            close_ignoring_carrier_is_authentic,
            close_ignoring_carrier_is_ordered⟩, by
              intro supported
              exact close_ignoring_carrier_is_not_closing supported.2.2.2⟩
      · exact ⟨referenceCarrierSemantics, id, False,
          affine_carrier_refinement_supports_closing_profile
            reference_carrier_refines_affine_boundary, by
              simp [CarrierProfile.Holds]⟩

/-- The hierarchy is semantically strict: exclusive receive mediation alone
does not imply the closing profile's ordered, no-replay refinement. -/
theorem mediated_profile_does_not_imply_closing_profile :
    Not (Exists fun abstraction =>
      CarrierProfile.closing.Holds droppingCarrierSemantics abstraction True) := by
  rintro ⟨abstraction, supported⟩
  exact dropping_carrier_has_no_affine_refinement
    ⟨abstraction,
      closing_profile_establishes_affine_carrier_refinement supported⟩

end Hibana
