import Hibana.DistributedCallExecution
import Hibana.OwnedFutureInventory

namespace Hibana.OwnedCallHistory
open EndpointDelegation EndpointTransfer

/-- Actor-local value identity. Actor identities must be bound to their actual
process/runtime observations; process-local pointer addresses are not globally
unique actor identities. No part of this tag is added to the wire. -/
abbrev Tag := Nat × Nat × Nat × Nat
abbrev Request := OwnedFutureInventory.Request Tag (Nat × Nat)

/-- Bind the actual header retired by this observation to the tracked
resource and moved Rust value. A fault from another view or another actor
cannot stand in for retiring this value. -/
def retirementMatches (resource : EndpointKey) (value : Tag)
    (sample : ViewRetirementObservation) (image : RustDescriptorImage) (choreo : Choreo) : Bool :=
  resource.component == 0 && sample.view == resource.role && sample.check image choreo &&
    ((observedActorView? sample.before sample.view).map fun current =>
      (sample.before.actor, current.header)) == some value

/-- The actual FIFO receipt and acceptance identity, not the repeated envelope
bytes. These are erased observations supplied by the affine carrier. -/
structure Receipt where
  call : TransportChannel × Nat
  frame : TransportFrame
  deriving DecidableEq

structure Cut where
  requests : List Request
  custody : Observation Tag Receipt

def initial (value : Tag) : Cut := ⟨[], EndpointTransfer.initial value⟩

def tracks (resource : EndpointKey) (choreo : Choreo) (entry : SourceAcceptance) : Bool :=
  match entry.sample.ownedFacts? choreo with
  | some facts => decide (facts.resource = resource)
  | none => false

/-- Constructor identity is matched at actual preparation/acceptance, not at
construction. Independent route or roll work may precede the first poll. -/
def Request.matches (request : Request) (sample : SourceSendAdmission)
    (facts : OwnedSourceFacts) : Prop :=
  observedResourceTag? sample.base facts.binding.message.sender.role = some request.carrier ∧
    observedResourceTag? sample.base facts.resource.role = some request.payload ∧
    request.label = facts.binding.message.label ∧
    request.borrow.1 ≤ facts.effIndex ∧ facts.effIndex < request.borrow.2

/-- One merged observation word. Source publication, receiver publication and
future destruction are distinct boundaries; receipt need not wait for sender
publication or for the old sender's witness to be destroyed. -/
inductive OwnedCallEvent where
  | construct (request : Request)
  | poll (request : Request)
  | retire (request : Request)
  | abandon (request : Request) (sample : ViewRetirementObservation)
  | dropValue (value : Tag) (sample : ViewRetirementObservation)
  | fence (receipt : Receipt)
  | source (boundary : DistributedCallBoundary)
  | accept (entry : SourceAcceptance) (request : Request) (frame : TransportFrame)
  | receive (key : TransportChannel × Nat) (actor : Nat) (record : ActorCommitRecord)
      (frame : TransportFrame) (fresh : Tag)

def OwnedCallEvent.retirement? : OwnedCallEvent → Option (Tag × ViewRetirementObservation)
  | .abandon request sample => some (request.payload, sample)
  | .dropValue value sample => some (value, sample)
  | _ => none

def OwnedCallEvent.source? : OwnedCallEvent → Option DistributedCallBoundary
  | .source boundary => some boundary
  | .accept entry _ _ => some (.accept entry)
  | .receive key actor record _ _ => some (.receive key actor record)
  | .abandon _ sample | .dropValue _ sample => some (.retireView sample)
  | _ => none

def sourceWord (events : List OwnedCallEvent) : List DistributedCallBoundary :=
  events.filterMap OwnedCallEvent.source?

def acceptedEntries (events : List OwnedCallEvent) : List SourceAcceptance :=
  (sourceWord events).filterMap DistributedCallBoundary.acceptance?

/-- Every source boundary is retained. An ordinary boundary cannot hide a
transfer of the tracked binding. In particular a receive must identify its
actual accepted call, including ordinary frames on the same logical FIFO. -/
def Untracked (resource : EndpointKey) (choreo : Choreo)
    (events : List OwnedCallEvent) : DistributedCallBoundary → Prop
  | .accept entry => tracks resource choreo entry = false
  | .receive key _ _ => ∃ entry ∈ acceptedEntries events,
      entry.callKey = key ∧ tracks resource choreo entry = false
  | .publish _ _ _ | .localCommit _ _ | .quiet _ => True
  | .retireView sample => sample.view ≠ resource.role

/-- One boundary of the same chronological run. This relation adds no admission
guard to Rust. Authentic complete observations and their source qualification
are required to apply it to an implementation. It does not assert that a live
request is reachable or pollable by application code. -/
inductive Boundary (images : Nat → RustDescriptorImage) (resource : EndpointKey) (choreo : Choreo) :
    List OwnedCallEvent → OwnedCallEvent → Cut → Cut → Prop where
  | construct (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (left right : List Tag) (requests : List Request) (request : Request) :
      Boundary images resource choreo events (.construct request)
        ⟨requests, { state with usable := left ++ request.payload :: right }⟩
        ⟨request :: requests,
          { state with usable := left ++ right, preparing := request.payload :: state.preparing }⟩
  | poll (events : List OwnedCallEvent) (cut : Cut) (request : Request)
      (present : request ∈ cut.requests) :
      Boundary images resource choreo events (.poll request) cut cut
  | retire (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (left right : List Tag) (earlier later : List Request) (request : Request) :
      Boundary images resource choreo events (.retire request)
        ⟨earlier ++ request :: later, { state with acceptedFutures := left ++ request.payload :: right }⟩
        ⟨earlier ++ later, { state with acceptedFutures := left ++ right }⟩
  | abandon (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (left right : List Tag) (earlier later : List Request) (request : Request)
      (sample : ViewRetirementObservation)
      (checked : retirementMatches resource request.payload sample (images sample.before.actor) choreo = true) :
      Boundary images resource choreo events (.abandon request sample)
        ⟨earlier ++ request :: later, { state with preparing := left ++ request.payload :: right }⟩
        ⟨earlier ++ later, { state with preparing := left ++ right }⟩
  | dropValue (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (left right : List Tag) (requests : List Request) (value : Tag)
      (sample : ViewRetirementObservation)
      (checked : retirementMatches resource value sample (images sample.before.actor) choreo = true) :
      Boundary images resource choreo events (.dropValue value sample)
        ⟨requests, { state with usable := left ++ value :: right }⟩
        ⟨requests, { state with usable := left ++ right }⟩
  | fence (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (left right : List Receipt) (requests : List Request) (receipt : Receipt) :
      Boundary images resource choreo events (.fence receipt)
        ⟨requests, { state with inFlight := left ++ receipt :: right }⟩
        ⟨requests, { state with inFlight := left ++ right }⟩
  | source (events : List OwnedCallEvent) (cut : Cut) (boundary : DistributedCallBoundary)
      (untracked : Untracked resource choreo events boundary) :
      Boundary images resource choreo events (.source boundary) cut cut
  | accept (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (left right : List Tag) (requests : List Request)
      (entry : SourceAcceptance) (request : Request) (frame : TransportFrame)
      (facts : OwnedSourceFacts)
      (factsAt : entry.sample.ownedFacts? choreo = some facts)
      (resourceAt : facts.resource = resource) (sending : facts.side = .send)
      (present : request ∈ requests) (matched : request.matches entry.sample facts)
      (wire : entry.sample.wire.identifies frame)
      (channel : frame.channel = entry.channel)
      (position : frame.sequence = (sourceChannelEntries (acceptedEntries events) entry.channel).length) :
      Boundary images resource choreo events (.accept entry request frame)
        ⟨requests, { state with preparing := left ++ request.payload :: right }⟩
        ⟨requests, { state with
          preparing := left ++ right
          acceptedFutures := request.payload :: state.acceptedFutures
          inFlight := state.inFlight ++ [⟨entry.callKey, frame⟩] }⟩
  | receive (events : List OwnedCallEvent) (state : Observation Tag Receipt)
      (rest : List Receipt) (requests : List Request)
      (key : TransportChannel × Nat) (actor : Nat) (record : ActorCommitRecord)
      (frame : TransportFrame) (facts : OwnedCommitFacts)
      (factsAt : record.step.ownedFacts? (images actor) choreo = some facts)
      (resourceAt : facts.resource = resource) (receiving : facts.side = .receive)
      (actorAt : record.step.after.actor = actor)
      (wire : record.wire.identifies frame) :
      Boundary images resource choreo events
        (.receive key actor record frame (actor, facts.afterView.header))
        ⟨requests, { state with inFlight := ⟨key, frame⟩ :: rest }⟩
        ⟨requests, { state with inFlight := rest, usable := (actor, facts.afterView.header) :: state.usable }⟩

inductive History (images : Nat → RustDescriptorImage) (resource : EndpointKey) (choreo : Choreo) (value : Tag) :
    List OwnedCallEvent → Cut → Prop where
  | nil : History images resource choreo value [] (initial value)
  | tail : History images resource choreo value events before →
      Boundary images resource choreo events event before after →
      History images resource choreo value (events ++ [event]) after

end Hibana.OwnedCallHistory

namespace Hibana
open OwnedCallHistory EndpointTransfer

theorem a_matched_retirement_uses_its_actual_value
    (checked : retirementMatches resource value sample image choreo = true) :
    sample.check image choreo = true ∧
      ∃ current, observedActorView? sample.before sample.view = some current ∧
        value = (sample.before.actor, current.header) := by
  have fields := Bool.and_eq_true_iff.mp checked
  have validated := (Bool.and_eq_true_iff.mp fields.1).2
  refine ⟨validated, ?_⟩
  cases currentAt : observedActorView? sample.before sample.view with
  | none =>
      have impossible := beq_iff_eq.mp fields.2
      simp only [currentAt, Option.map_none, reduceCtorEq] at impossible
  | some current =>
      have same : some (sample.before.actor, current.header) = some value :=
        beq_iff_eq.mp (by simpa only [currentAt, Option.map_some] using fields.2)
      exact ⟨current, rfl, (Option.some.inj same).symm⟩

/-- Retirement is retained as a checked physical source boundary in the same
word. Its header and fault cannot come from an independently chosen trace. -/
theorem a_joint_value_retirement_keeps_its_checked_source_boundary
    (boundary : OwnedCallHistory.Boundary images resource choreo events event before after)
    (recorded : event.retirement? = some (value, sample)) :
    retirementMatches resource value sample (images sample.before.actor) choreo = true ∧
      event.source? = some (.retireView sample) := by
  cases boundary <;> simp only [OwnedCallEvent.retirement?, reduceCtorEq] at recorded
  case abandon state left right earlier later request observed checked =>
    cases Option.some.inj recorded
    exact ⟨checked, rfl⟩
  case dropValue state left right requests value observed checked =>
    cases Option.some.inj recorded
    exact ⟨checked, rfl⟩

/-- This classifies the real recorded retirement. Completed idle views may
retire without stopping independent work; a faulted retirement cannot authorize
publication. It is not a claim of distributed closure or future callability. -/
theorem a_joint_value_retirement_is_complete_or_faulted
    (boundary : OwnedCallHistory.Boundary images resource choreo events event before after)
    (recorded : event.retirement? = some (value, sample)) :
    (sample.after.fault = none ∧
      sourceViewComplete? sample.before (images sample.before.actor) choreo sample.view = some true ∧
      ∃ current, observedActorView? sample.before sample.view = some current ∧ current.operation = .idle) ∨
    ∃ cause, sample.after.fault = some cause ∧
      (observedGeneration sample.after.fault).publicationPermitted = false := by
  have checked := (a_matched_retirement_uses_its_actual_value
    (a_joint_value_retirement_keeps_its_checked_source_boundary boundary recorded).1).1
  cases faultAt : sample.after.fault with
  | none => exact .inl ⟨rfl, live_retirement_requires_source_completion checked faultAt⟩
  | some cause =>
      exact .inr ⟨cause, rfl, rfl⟩

/-- Erase the source annotations without changing any future or receipt
boundary. This is a projection of one history, not an independently supplied
custody trace that could disagree with its communication log. -/
theorem a_joint_call_boundary_has_its_actual_future_boundary
    (boundary : OwnedCallHistory.Boundary images resource choreo events event before after) :
    ∃ action, OwnedFutureInventory.Boundary action
      before.requests before.custody after.requests after.custody := by
  cases boundary with
  | construct state left right requests request => exact ⟨_, .construct state left right requests request⟩
  | poll cut request present => exact ⟨_, .poll request present _⟩
  | retire state left right earlier later request => exact ⟨_, .retire state left right earlier later request⟩
  | abandon state left right earlier later request sample checked => exact ⟨_, .abandon state left right earlier later request⟩
  | dropValue state left right requests value sample checked => exact ⟨_, .dropValue (.dropValue state left right value)⟩
  | fence state left right requests receipt => exact ⟨_, .fenceFrame (.fenceFrame state left right receipt)⟩
  | source cut boundary untracked => exact ⟨_, .localBoundary _ _⟩
  | accept state left right requests entry request frame facts factsAt resourceAt sending present matched wire channel position =>
      exact ⟨_, .accept (.accept state left right request.payload ⟨entry.callKey, frame⟩)⟩
  | receive state rest requests key actor record frame facts factsAt resourceAt receiving actorAt wire =>
      exact ⟨_, .receive (.receive state rest ⟨key, frame⟩ (actor, facts.afterView.header))⟩

theorem a_joint_call_history_has_its_same_future_history
    (history : OwnedCallHistory.History images resource choreo value events cut) :
    OwnedFutureInventory.History [] (EndpointTransfer.initial value)
      cut.requests cut.custody := by
  induction history with
  | nil => exact .refl _ _
  | tail prior boundary ih =>
      obtain ⟨_, actual⟩ := a_joint_call_boundary_has_its_actual_future_boundary boundary
      exact .tail ih actual

theorem a_joint_call_history_preserves_affine_custody
    (history : OwnedCallHistory.History images resource choreo value events cut) :
    cut.custody.authorityCount ≤ 1 ∧
      OwnedFutureInventory.Covers cut.requests cut.custody := by
  have actual := a_joint_call_history_has_its_same_future_history history
  exact ⟨OwnedFutureInventory.actual_future_history_preserves_unique_authority actual,
    OwnedFutureInventory.a_complete_future_history_covers_every_private_payload actual⟩

private theorem a_boundary_keeps_each_live_constructor
    (boundary : OwnedCallHistory.Boundary images resource choreo events event before after)
    (prior : ∀ request ∈ before.requests, OwnedCallEvent.construct request ∈ events) :
    ∀ request ∈ after.requests, OwnedCallEvent.construct request ∈ events ++ [event] := by
  intro request member
  cases boundary with
  | construct state left right requests created =>
      rcases List.mem_cons.mp member with same | old
      · subst request; exact List.mem_append_right _ (List.mem_cons_self)
      · exact List.mem_append_left _ (prior _ old)
  | retire state left right earlier later removed
  | abandon state left right earlier later removed sample checked =>
      have old : request ∈ earlier ++ removed :: later := by
        rcases List.mem_append.mp member with left | right
        · exact List.mem_append_left _ left
        · exact List.mem_append_right _ (List.mem_cons_of_mem _ right)
      exact List.mem_append_left _ (prior _ old)
  | poll | dropValue | fence | source | accept | receive =>
      exact List.mem_append_left _ (prior _ member)

/-- A retained request must have an actual constructor earlier in this very
word. An issued header, an Idle slot or source readiness cannot create it. -/
theorem a_joint_call_request_has_its_actual_constructor
    (history : OwnedCallHistory.History images resource choreo value events cut) :
    ∀ request ∈ cut.requests, OwnedCallEvent.construct request ∈ events := by
  induction history with
  | nil => simp [OwnedCallHistory.initial]
  | tail prior boundary ih => exact a_boundary_keeps_each_live_constructor boundary ih

/-- Authority in a private future has its real retained request and the
constructor in this chronological run. This is not a pollability theorem. -/
theorem a_joint_private_payload_has_its_same_history_constructor
    (history : OwnedCallHistory.History images resource choreo value events cut)
    (privateAt : payload ∈ cut.custody.preparing) :
    ∃ request ∈ cut.requests, request.payload = payload ∧ OwnedCallEvent.construct request ∈ events := by
  obtain ⟨request, member, actual⟩ := OwnedFutureInventory.a_private_payload_has_its_actual_request
    (a_joint_call_history_has_its_same_future_history history) privateAt
  exact ⟨request, member, actual, a_joint_call_request_has_its_actual_constructor history request member⟩

end Hibana

namespace Hibana
open OwnedCallHistory EndpointTransfer

/-- All acceptances, including ordinary frames, are reconstructed from the
chronological source word. No caller-selected subtrace supplies their indices. -/
theorem a_distributed_call_execution_has_its_exact_acceptance_word
    (execution : DistributedCallExecution images choreo session first last word entries published pending) :
    word.filterMap DistributedCallBoundary.acceptance? = entries := by
  induction execution with
  | refl => rfl
  | quiet history checked _headers ih
  | publish history actorAt paired committed ih
  | localCommit history actorRole localAt committed ih
  | receive history actorRole received entryAt identity paired committed ih =>
      simpa [List.filterMap_append, DistributedCallBoundary.acceptance?] using ih
  | accept history imageAt checked decoded pathAt rowAt prepared slotFree freshWitness sourceAt accepted identity ih =>
      simp [List.filterMap_append, DistributedCallBoundary.acceptance?, ih]

private def AcceptedFrom (resource : EndpointDelegation.EndpointKey) (choreo : Choreo)
    (receipt : Receipt) (events : List OwnedCallEvent) : Prop :=
  ∃ earlier later entry request facts,
    events = earlier ++ OwnedCallEvent.accept entry request receipt.frame :: later ∧
    entry.callKey = receipt.call ∧ OwnedCallEvent.construct request ∈ earlier ∧
    entry.sample.ownedFacts? choreo = some facts ∧ facts.resource = resource ∧
    facts.side = .send ∧ request.matches entry.sample facts ∧
    entry.sample.wire.identifies receipt.frame ∧ receipt.frame.channel = entry.channel ∧
    receipt.frame.sequence = (sourceChannelEntries (acceptedEntries earlier) entry.channel).length

private theorem an_accepted_origin_survives_a_later_event
    (origin : AcceptedFrom resource choreo receipt events) (event : OwnedCallEvent) :
    AcceptedFrom resource choreo receipt (events ++ [event]) := by
  obtain ⟨earlier, later, entry, request, facts, wordAt, rest⟩ := origin
  exact ⟨earlier, later ++ [event], entry, request, facts,
    by simp only [wordAt, List.append_assoc, List.cons_append], rest⟩

private theorem a_joint_boundary_keeps_its_actual_frame_origins
    (history : OwnedCallHistory.History images resource choreo value events before)
    (boundary : OwnedCallHistory.Boundary images resource choreo events event before after)
    (origins : ∀ receipt ∈ before.custody.inFlight, AcceptedFrom resource choreo receipt events) :
    ∀ receipt ∈ after.custody.inFlight, AcceptedFrom resource choreo receipt (events ++ [event]) := by
  intro receipt member
  cases boundary with
  | accept state left right requests entry request frame facts factsAt resourceAt sending present matched wire channel position =>
      rcases List.mem_append.mp member with old | added
      · exact an_accepted_origin_survives_a_later_event (origins receipt old) _
      · have same := List.mem_singleton.mp added
        subst receipt
        exact ⟨events, [], entry, request, facts, by simp, rfl,
          a_joint_call_request_has_its_actual_constructor history request present,
          factsAt, resourceAt, sending, matched, wire, channel, position⟩
  | receive state rest requests key actor record frame facts factsAt resourceAt receiving actorAt wire =>
      exact an_accepted_origin_survives_a_later_event
        (origins receipt (List.mem_cons_of_mem _ member)) _
  | fence state left right requests removed =>
      have old : receipt ∈ left ++ removed :: right := by
        rcases List.mem_append.mp member with earlier | later
        · exact List.mem_append_left _ earlier
        · exact List.mem_append_right _ (List.mem_cons_of_mem _ later)
      exact an_accepted_origin_survives_a_later_event (origins receipt old) _
  | construct | poll | retire | abandon | dropValue | source =>
      exact an_accepted_origin_survives_a_later_event (origins receipt member) _

private theorem a_joint_history_has_all_its_actual_frame_origins
    (history : OwnedCallHistory.History images resource choreo value events cut) :
    ∀ receipt ∈ cut.custody.inFlight, AcceptedFrom resource choreo receipt events := by
  induction history with
  | nil => simp [OwnedCallHistory.initial, EndpointTransfer.initial]
  | tail prior boundary ih => exact a_joint_boundary_keeps_its_actual_frame_origins prior boundary ih

/-- Couple source qualification to actual value/future/receipt custody on ONE
word. A queued transfer has its real earlier constructor, matched source
preparation and complete-FIFO position. A source-only or future-only trace
cannot establish this result. Ordinary frames contribute to that position.

This finite-prefix result assumes authentic complete Rust/carrier observations.
It does not prove current callability, eventual polling, or fault/closure
composition, and does not require publication before receiver progress. -/
theorem a_joint_queued_transfer_has_its_checked_source_and_actual_constructor
    (history : OwnedCallHistory.History images resource choreo value events cut)
    (source : DistributedCallExecution images choreo session first last
      (sourceWord events) entries published pending)
    (queued : receipt ∈ cut.custody.inFlight) :
    ∃ earlier later entry request facts,
      events = earlier ++ OwnedCallEvent.accept entry request receipt.frame :: later ∧
      entry ∈ entries ∧ entry.callKey = receipt.call ∧
      OwnedCallEvent.construct request ∈ earlier ∧
      entry.sample.image = images entry.sample.image.role ∧
      entry.sample.check choreo session = true ∧
      entry.sample.ownedFacts? choreo = some facts ∧ facts.resource = resource ∧
      facts.side = .send ∧ request.matches entry.sample facts ∧
      entry.sample.wire.identifies receipt.frame ∧ receipt.frame.channel = entry.channel ∧
      receipt.frame.sequence = (sourceChannelEntries (acceptedEntries earlier) entry.channel).length := by
  obtain ⟨earlier, later, entry, request, facts, wordAt, keyAt, constructed,
    factsAt, resourceAt, sending, matched, wire, channel, position⟩ :=
      a_joint_history_has_all_its_actual_frame_origins history receipt queued
  have acceptedAt : entry ∈ acceptedEntries events := by
    simp only [acceptedEntries, sourceWord, wordAt, List.filterMap_append,
      List.filterMap_cons, OwnedCallEvent.source?]
    exact List.mem_append_right _ (List.mem_cons_self)
  have exactWord := a_distributed_call_execution_has_its_exact_acceptance_word source
  have actual : entry ∈ entries := by
    change entry ∈ (sourceWord events).filterMap DistributedCallBoundary.acceptance? at acceptedAt
    rw [exactWord] at acceptedAt
    exact acceptedAt
  have checked := a_distributed_call_execution_retains_each_accepted_source source entry actual
  exact ⟨earlier, later, entry, request, facts, wordAt, actual, keyAt, constructed,
    checked.1, checked.2.1, factsAt, resourceAt, sending, matched, wire, channel, position⟩

end Hibana
