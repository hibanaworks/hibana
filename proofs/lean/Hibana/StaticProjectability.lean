import Hibana.GlobalProgress
import Hibana.DescriptorImage
import Hibana.TransportContract

namespace Hibana

/-- The endpoint selector observed by the message-erased runtime. Outbound
operations are selected by owner role, label, and canonical schema. An inbound
`globalId` is an abstract name for the canonical transport evidence checked
below, not a wire field. -/
inductive StaticEndpointSelector where
  | outbound (role label schema : Nat)
  | inbound (globalId : Nat)
  deriving Repr, DecidableEq, BEq

def StaticEndpointSelector.isInbound : StaticEndpointSelector -> Bool
  | .outbound _ _ _ => false
  | .inbound _ => true

structure StaticOccurrence where
  globalId : Nat
  sender : Nat
  receiver : Nat
  label : Nat
  schema : Nat
  deriving Repr, DecidableEq

/-- Observable inbound identity in Hibana's existing eight-byte header. The
session is fixed by the surrounding protocol artifact. -/
structure StaticInboundEvidence where
  source : Nat
  target : Nat
  lane : Nat
  frameLabel : Nat
  deriving Repr, DecidableEq

private def canonicalInboundEvidenceFrom
    (choreo : Choreo) :
    Nat → List DecodedProgramAtom → List StaticInboundEvidence
  | _, [] => []
  | index, atom :: rest =>
      {
        source := atom.sender
        target := atom.receiver
        lane := atom.lane
        frameLabel := choreo.canonicalFrameLabel index
      } :: canonicalInboundEvidenceFrom choreo (index + 1) rest

private theorem canonical_inbound_evidence_from_getElem?
    (choreo : Choreo) (remaining : List DecodedProgramAtom) (base offset : Nat) :
    (canonicalInboundEvidenceFrom choreo base remaining)[offset]? =
      remaining[offset]?.map fun atom => {
        source := atom.sender
        target := atom.receiver
        lane := atom.lane
        frameLabel := choreo.canonicalFrameLabel (base + offset)
      } := by
  induction remaining generalizing base offset with
  | nil => simp [canonicalInboundEvidenceFrom]
  | cons head tail tailIH =>
      cases offset with
      | zero => simp [canonicalInboundEvidenceFrom]
      | succ offset =>
          simpa [canonicalInboundEvidenceFrom, Nat.add_assoc, Nat.add_comm,
            Nat.add_left_comm] using
            tailIH (base + 1) offset

def Choreo.canonicalInboundEvidence (choreo : Choreo) : List StaticInboundEvidence :=
  canonicalInboundEvidenceFrom choreo 0 choreo.canonicalProgramAtoms

private theorem canonical_inbound_evidence_getElem?
    (choreo : Choreo) (index : Nat) :
    choreo.canonicalInboundEvidence[index]? =
      choreo.canonicalProgramAtoms[index]?.map fun atom => {
        source := atom.sender
        target := atom.receiver
        lane := atom.lane
        frameLabel := choreo.canonicalFrameLabel index
      } := by
  simpa [Choreo.canonicalInboundEvidence] using
    canonical_inbound_evidence_from_getElem?
      choreo choreo.canonicalProgramAtoms 0 index

/-- Two route-membership lists are mutually exclusive when they select
different arms of the same route conflict. Nested routes need only one shared
opposite-arm witness to make the two occurrences non-coexecutable. -/
def ConflictListsMutuallyExclusive
    (left right : List ConflictArm) : Prop :=
  ∃ leftArm ∈ left, ∃ rightArm ∈ right,
    leftArm.conflict = rightArm.conflict ∧ leftArm.arm ≠ rightArm.arm

instance (left right : List ConflictArm) :
    Decidable (ConflictListsMutuallyExclusive left right) := by
  unfold ConflictListsMutuallyExclusive
  infer_instance

def ParallelListsIndependent
    (left right : List ParallelArm) : Prop :=
  ∃ leftArm ∈ left, ∃ rightArm ∈ right,
    leftArm.parallel = rightArm.parallel ∧ leftArm.arm ≠ rightArm.arm

instance (left right : List ParallelArm) :
    Decidable (ParallelListsIndependent left right) := by
  unfold ParallelListsIndependent
  infer_instance

structure StaticInboundColor where
  evidence : StaticInboundEvidence
  conflicts : List ConflictArm
  parallelArms : List ParallelArm
  rolls : List Nat
  deriving Repr, DecidableEq

private def enumerateStaticInboundColors :
    List StaticInboundEvidence -> List (List ConflictArm) ->
      List (List ParallelArm) -> List (List Nat) -> List StaticInboundColor
  | evidence :: evidenceRows, conflicts :: conflictRows,
      parallelArms :: parallelRows, rolls :: rollRows =>
      { evidence, conflicts, parallelArms, rolls } ::
        enumerateStaticInboundColors evidenceRows conflictRows parallelRows rollRows
  | _, _, _, _ => []

def Choreo.staticInboundColors (choreo : Choreo) : List StaticInboundColor :=
  enumerateStaticInboundColors choreo.canonicalInboundEvidence
    choreo.globalEventConflicts choreo.globalEventParallelArms
    choreo.globalEventRollMemberships

def RollPathsCompete (left right : StaticInboundColor) : Prop :=
  left.conflicts ≠ right.conflicts /\
    ∃ roll ∈ left.rolls, roll ∈ right.rolls

instance (left right : StaticInboundColor) :
    Decidable (RollPathsCompete left right) := by
  unfold RollPathsCompete
  infer_instance

def InboundColorsSeparate (left right : StaticInboundColor) : Prop :=
  ConflictListsMutuallyExclusive left.conflicts right.conflicts \/
      ParallelListsIndependent left.parallelArms right.parallelArms \/
      RollPathsCompete left right ->
    left.evidence ≠ right.evidence

instance (left right : StaticInboundColor) :
    Decidable (InboundColorsSeparate left right) := by
  unfold InboundColorsSeparate
  infer_instance

theorem elastic_roll_paths_have_distinct_inbound_evidence
    {left right : StaticInboundColor}
    (separate : InboundColorsSeparate left right)
    (compete : RollPathsCompete left right) :
    left.evidence ≠ right.evidence :=
  separate (Or.inr (Or.inr compete))

/-- Frame colors distinguish route alternatives, parallel frontiers, and exact
route paths sharing an elastic roll. Locally ordered occurrences on one route
path may reuse the same eight-byte header evidence. -/
def Choreo.InboundOccurrenceColoring (choreo : Choreo) : Prop :=
  let colors := choreo.staticInboundColors
  colors.length = choreo.globalEvents.length /\
    colors.all (fun color => color.evidence.frameLabel < 256) = true /\
    colors.Pairwise InboundColorsSeparate

instance (choreo : Choreo) : Decidable choreo.InboundOccurrenceColoring := by
  unfold Choreo.InboundOccurrenceColoring
  infer_instance

def Choreo.checkInboundOccurrenceColoring (choreo : Choreo) : Bool :=
  decide choreo.InboundOccurrenceColoring

theorem inbound_occurrence_coloring_checker_sound
    {choreo : Choreo}
    (accepted : choreo.checkInboundOccurrenceColoring = true) :
    choreo.InboundOccurrenceColoring :=
  of_decide_eq_true accepted

structure StaticGlobalOccurrence where
  globalId : Nat
  event : GlobalEvent
  conflicts : List ConflictArm
  parallelArms : List ParallelArm
  deriving Repr, DecidableEq

private def enumerateStaticGlobalOccurrences :
    Nat → List GlobalEvent → List (List ConflictArm) →
      List (List ParallelArm) → List StaticGlobalOccurrence
  | globalId, event :: events, conflicts :: conflictRows,
      parallelArms :: parallelRows =>
      { globalId, event, conflicts, parallelArms } ::
        enumerateStaticGlobalOccurrences (globalId + 1) events conflictRows parallelRows
  | _, _, _, _ => []

def Choreo.staticGlobalOccurrences (choreo : Choreo) : List StaticGlobalOccurrence :=
  enumerateStaticGlobalOccurrences 0 choreo.globalEvents
    choreo.globalEventConflicts choreo.globalEventParallelArms

def occurrenceOnEndpointRoutePath
    (earlier later candidate : StaticGlobalOccurrence) : Bool :=
  candidate.conflicts.all fun membership =>
    earlier.conflicts.contains membership || later.conflicts.contains membership

def occurrenceLocallyOrdered
    (earlier later : StaticGlobalOccurrence) : Bool :=
  earlier.globalId < later.globalId &&
    !decide (ConflictListsMutuallyExclusive earlier.conflicts later.conflicts) &&
    !decide (ParallelListsIndependent earlier.parallelArms later.parallelArms)

abbrev CausalWitnesses := Nat → Option StaticGlobalOccurrence

def addCausalWitness
    (witnesses : CausalWitnesses) (role : Nat)
    (witness : StaticGlobalOccurrence) : CausalWitnesses :=
  fun candidate =>
    if candidate = role then
      match witnesses candidate with
      | none => some witness
      | present => present
    else
      witnesses candidate

theorem add_causal_witness_first_write_wins
    {witnesses : CausalWitnesses} {role : Nat}
    {witness : StaticGlobalOccurrence} :
    addCausalWitness witnesses role witness role =
      match witnesses role with
      | none => some witness
      | present => present := by
  simp [addCausalWitness]

theorem add_causal_witness_preserves_other_role
    {witnesses : CausalWitnesses} {role query : Nat}
    {witness : StaticGlobalOccurrence}
    (different : query ≠ role) :
    addCausalWitness witnesses role witness query = witnesses query := by
  simp [addCausalWitness, different]

def propagateCausalWitness
    (earlier later : StaticGlobalOccurrence)
    (roleCount : Nat)
    (witnesses : CausalWitnesses)
    (candidate : StaticGlobalOccurrence) : CausalWitnesses :=
  if candidate.event.sender < roleCount &&
      candidate.event.receiver < roleCount &&
      occurrenceOnEndpointRoutePath earlier later candidate then
    match witnesses candidate.event.sender with
    | none => witnesses
    | some witness =>
        if occurrenceLocallyOrdered witness candidate then
          addCausalWitness witnesses candidate.event.receiver candidate
        else
          witnesses
  else
    witnesses

theorem propagate_causal_witness_without_route_conflicts_is_endpoint_independent
    {earlier left right candidate : StaticGlobalOccurrence}
    {roleCount : Nat} {witnesses : CausalWitnesses}
    (noConflicts : candidate.conflicts = []) :
    propagateCausalWitness earlier left roleCount witnesses candidate =
      propagateCausalWitness earlier right roleCount witnesses candidate := by
  simp [propagateCausalWitness, occurrenceOnEndpointRoutePath, noConflicts]

theorem causal_witness_fold_reuses_prefix_exactly
    {earlier later : StaticGlobalOccurrence} {roleCount : Nat}
    (witnesses : CausalWitnesses)
    (before after : List StaticGlobalOccurrence) :
    (before ++ after).foldl
        (propagateCausalWitness earlier later roleCount) witnesses =
      after.foldl (propagateCausalWitness earlier later roleCount)
        (before.foldl
          (propagateCausalWitness earlier later roleCount) witnesses) := by
  simp

/-- Executable causal closure from the earlier receive to the later send. It
uses only local projected order and send-to-receive edges, excludes direct
ordering across parallel arms, and ignores route-local traffic unless one
endpoint fixes that route arm. -/
def receivePrecedesLaterSend
    (occurrences : List StaticGlobalOccurrence)
    (roleCount : Nat)
    (earlier later : StaticGlobalOccurrence) : Bool :=
  if earlier.event.receiver < roleCount && later.event.sender < roleCount then
    let initial : CausalWitnesses := fun role =>
      if role = earlier.event.receiver then some earlier else none
    let between := occurrences.filter fun candidate =>
      earlier.globalId < candidate.globalId && candidate.globalId < later.globalId
    let witnesses := between.foldl
      (propagateCausalWitness earlier later roleCount) initial
    match witnesses later.event.sender with
    | none => false
    | some witness => occurrenceLocallyOrdered witness later
  else
    false

def ReceiveLanePairCausallySafe
    (occurrences : List StaticGlobalOccurrence)
    (roleCount : Nat)
    (left right : StaticGlobalOccurrence) : Prop :=
  left.event.sender = left.event.receiver ∨
    right.event.sender = right.event.receiver ∨
    left.event.receiver ≠ right.event.receiver ∨
    left.event.lane ≠ right.event.lane ∨
    left.event.sender = right.event.sender ∨
    ConflictListsMutuallyExclusive left.conflicts right.conflicts ∨
    receivePrecedesLaterSend occurrences roleCount left right = true

instance (occurrences : List StaticGlobalOccurrence) (roleCount : Nat)
    (left right : StaticGlobalOccurrence) :
    Decidable (ReceiveLanePairCausallySafe occurrences roleCount left right) := by
  unfold ReceiveLanePairCausallySafe
  infer_instance

theorem receive_lane_sender_change_requires_exclusion_or_causal_handoff
    {occurrences : List StaticGlobalOccurrence} {roleCount : Nat}
    {left right : StaticGlobalOccurrence}
    (safe : ReceiveLanePairCausallySafe occurrences roleCount left right)
    (leftNonlocal : left.event.sender ≠ left.event.receiver)
    (rightNonlocal : right.event.sender ≠ right.event.receiver)
    (sameReceiver : left.event.receiver = right.event.receiver)
    (sameLane : left.event.lane = right.event.lane)
    (differentSender : left.event.sender ≠ right.event.sender) :
    ConflictListsMutuallyExclusive left.conflicts right.conflicts \/
      receivePrecedesLaterSend occurrences roleCount left right = true := by
  rcases safe with localLeft | localRight | receiverMismatch |
      laneMismatch | sameSender | exclusive | causal
  · exact False.elim (leftNonlocal localLeft)
  · exact False.elim (rightNonlocal localRight)
  · exact False.elim (receiverMismatch sameReceiver)
  · exact False.elim (laneMismatch sameLane)
  · exact False.elim (differentSender sameSender)
  · exact Or.inl exclusive
  · exact Or.inr causal

/-- Production has one receive FIFO per `(role,lane)` and intentionally does
not buffer an early frame from a future sender. A sender change is safe only
after a descriptor-derived causal handoff proves the earlier receive happened,
or when the two occurrences are in mutually exclusive route arms. -/
def Choreo.ReceiveLaneCausalSafety
    (choreo : Choreo) (roleCount : Nat) : Prop :=
  let occurrences := choreo.staticGlobalOccurrences
  occurrences.length = choreo.globalEvents.length /\
    occurrences.Pairwise (ReceiveLanePairCausallySafe occurrences roleCount)

instance (choreo : Choreo) (roleCount : Nat) :
    Decidable (choreo.ReceiveLaneCausalSafety roleCount) := by
  unfold Choreo.ReceiveLaneCausalSafety
  infer_instance

def Choreo.checkReceiveLaneCausality
    (choreo : Choreo) (roleCount : Nat) : Bool :=
  decide (choreo.ReceiveLaneCausalSafety roleCount)

theorem receive_lane_causality_checker_sound
    {choreo : Choreo} {roleCount : Nat}
    (accepted : choreo.checkReceiveLaneCausality roleCount = true) :
    choreo.ReceiveLaneCausalSafety roleCount :=
  of_decide_eq_true accepted

inductive StaticRollIteration where
  | current
  | next
  deriving Repr, DecidableEq

def StaticRollIteration.ordinal : StaticRollIteration -> Nat
  | .current => 0
  | .next => 1

private def ConflictArm.inRollIteration
    (iteration : StaticRollIteration) (membership : ConflictArm) : ConflictArm := {
  conflict := membership.conflict * 2 + iteration.ordinal
  arm := membership.arm
}

private def ParallelArm.inRollIteration
    (iteration : StaticRollIteration) (membership : ParallelArm) : ParallelArm := {
  parallel := membership.parallel * 2 + iteration.ordinal
  arm := membership.arm
}

/-- A static occurrence renamed into one copy of a one-step roll unfolding.
The event contract is unchanged; event, route, and parallel identities are
fresh between iterations. -/
def StaticGlobalOccurrence.inRollIteration
    (eventCount : Nat) (iteration : StaticRollIteration)
    (occurrence : StaticGlobalOccurrence) : StaticGlobalOccurrence := {
  globalId := eventCount * iteration.ordinal + occurrence.globalId
  event := occurrence.event
  conflicts := occurrence.conflicts.map (ConflictArm.inRollIteration iteration)
  parallelArms := occurrence.parallelArms.map
    (ParallelArm.inRollIteration iteration)
}

/-- One explicit unfolding is sufficient to expose every receive-lane sender
change from the tail of a roll body to the next iteration's head. -/
def Choreo.rollUnfoldedOccurrences
    (body : Choreo) : List StaticGlobalOccurrence :=
  let occurrences := body.staticGlobalOccurrences
  let eventCount := body.globalEvents.length
  occurrences.map (StaticGlobalOccurrence.inRollIteration eventCount .current) ++
    occurrences.map (StaticGlobalOccurrence.inRollIteration eventCount .next)

def Choreo.RollBodyReceiveLaneCausalSafety
    (body : Choreo) (roleCount : Nat) : Prop :=
  let occurrences := body.staticGlobalOccurrences
  occurrences.length = body.globalEvents.length /\
    body.rollUnfoldedOccurrences.Pairwise
      (ReceiveLanePairCausallySafe body.rollUnfoldedOccurrences roleCount)

instance (body : Choreo) (roleCount : Nat) :
    Decidable (body.RollBodyReceiveLaneCausalSafety roleCount) := by
  unfold Choreo.RollBodyReceiveLaneCausalSafety
  infer_instance

def Choreo.rollBodies : Choreo -> List Choreo
  | .send _ _ _ _ => []
  | .seq left right | .par left right | .route _ left right =>
      left.rollBodies ++ right.rollBodies
  | .roll body => body :: body.rollBodies

def Choreo.RollReceiveLaneCausalSafety
    (choreo : Choreo) (roleCount : Nat) : Prop :=
  ∀ body, body ∈ choreo.rollBodies ->
    body.RollBodyReceiveLaneCausalSafety roleCount

instance (choreo : Choreo) (roleCount : Nat) :
    Decidable (choreo.RollReceiveLaneCausalSafety roleCount) := by
  unfold Choreo.RollReceiveLaneCausalSafety
  exact List.decidableBAll
    (fun body => body.RollBodyReceiveLaneCausalSafety roleCount)
    choreo.rollBodies

def Choreo.checkRollReceiveLaneCausality
    (choreo : Choreo) (roleCount : Nat) : Bool :=
  decide (choreo.RollReceiveLaneCausalSafety roleCount)

theorem roll_receive_lane_causality_checker_sound
    {choreo : Choreo} {roleCount : Nat}
    (accepted : choreo.checkRollReceiveLaneCausality roleCount = true) :
    choreo.RollReceiveLaneCausalSafety roleCount :=
  of_decide_eq_true accepted

private theorem roll_iteration_conflicts_are_distinct
    (left right : StaticGlobalOccurrence) :
    ¬ ConflictListsMutuallyExclusive
        (left.inRollIteration 0 .current).conflicts
        (right.inRollIteration 0 .next).conflicts := by
  intro exclusive
  rcases exclusive with
    ⟨leftArm, leftMember, rightArm, rightMember, sameConflict, _⟩
  rcases List.mem_map.mp leftMember with ⟨leftOriginal, _, leftEq⟩
  rcases List.mem_map.mp rightMember with ⟨rightOriginal, _, rightEq⟩
  subst leftArm
  subst rightArm
  simp only [ConflictArm.inRollIteration, StaticRollIteration.ordinal] at sameConflict
  omega

theorem roll_body_occurrences_cross_iteration_safe
    {body : Choreo} {roleCount : Nat}
    {left right : StaticGlobalOccurrence}
    (safe : body.RollBodyReceiveLaneCausalSafety roleCount)
    (leftMember : left ∈ body.staticGlobalOccurrences)
    (rightMember : right ∈ body.staticGlobalOccurrences) :
    ReceiveLanePairCausallySafe body.rollUnfoldedOccurrences roleCount
      (left.inRollIteration body.globalEvents.length .current)
      (right.inRollIteration body.globalEvents.length .next) := by
  apply safe.2.rel_of_mem_append
  · exact List.mem_map.mpr ⟨left, leftMember, rfl⟩
  · exact List.mem_map.mpr ⟨right, rightMember, rfl⟩

/-- Across two roll iterations route exclusions are fresh, so a physical-lane
sender change can be accepted only by a real causal handoff. -/
theorem roll_reentry_sender_change_requires_causal_handoff
    {body : Choreo} {roleCount : Nat}
    {left right : StaticGlobalOccurrence}
    (safe : body.RollBodyReceiveLaneCausalSafety roleCount)
    (leftMember : left ∈ body.staticGlobalOccurrences)
    (rightMember : right ∈ body.staticGlobalOccurrences)
    (leftNonlocal : left.event.sender ≠ left.event.receiver)
    (rightNonlocal : right.event.sender ≠ right.event.receiver)
    (sameReceiver : left.event.receiver = right.event.receiver)
    (sameLane : left.event.lane = right.event.lane)
    (differentSender : left.event.sender ≠ right.event.sender) :
    receivePrecedesLaterSend body.rollUnfoldedOccurrences roleCount
      (left.inRollIteration body.globalEvents.length .current)
      (right.inRollIteration body.globalEvents.length .next) = true := by
  have pairSafe := roll_body_occurrences_cross_iteration_safe safe
    leftMember rightMember
  have consequence := receive_lane_sender_change_requires_exclusion_or_causal_handoff
    pairSafe leftNonlocal rightNonlocal sameReceiver sameLane differentSender
  rcases consequence with excluded | causal
  · exact False.elim (roll_iteration_conflicts_are_distinct left right excluded)
  · exact causal

/-- The exact fields that production can observe in Hibana's fixed core
header. Session is checked separately because it identifies the surrounding
protocol artifact; connection sequence remains carrier-private. -/
def TransportFrame.inboundEvidence (frame : TransportFrame) : StaticInboundEvidence := {
  source := frame.channel.sender
  target := frame.channel.receiver
  lane := frame.channel.lane
  frameLabel := frame.frameLabel.val
}

/-- Admission is the descriptor-checking boundary from a raw carrier frame to
one semantic global occurrence. Logical label, schema, epoch, and global ID are
not fields supplied by the carrier. -/
structure TransportAdmission
    (choreo : Choreo) (frame : TransportFrame)
    (globalId : Nat) (event : GlobalEvent) : Prop where
  evidenceAt : choreo.canonicalInboundEvidence[globalId]? = some frame.inboundEvidence
  eventAt : choreo.globalEvents[globalId]? = some event

private def transportAdmissionCandidates
    (choreo : Choreo) (frame : TransportFrame) :
    List Nat -> List (Nat × GlobalEvent)
  | [] => []
  | globalId :: rest =>
      let candidates := transportAdmissionCandidates choreo frame rest
      match choreo.canonicalInboundEvidence[globalId]?, choreo.globalEvents[globalId]? with
      | some expected, some event =>
          if expected = frame.inboundEvidence then (globalId, event) :: candidates
          else candidates
      | _, _ => candidates

private theorem transport_admission_candidate_sound
    {choreo : Choreo} {frame : TransportFrame}
    {ids : List Nat} {globalId : Nat} {event : GlobalEvent}
    (member : (globalId, event) ∈ transportAdmissionCandidates choreo frame ids) :
    globalId ∈ ids /\
      choreo.canonicalInboundEvidence[globalId]? = some frame.inboundEvidence /\
      choreo.globalEvents[globalId]? = some event := by
  induction ids with
  | nil => simp [transportAdmissionCandidates] at member
  | cons head tail tailIH =>
      cases evidenceCase : choreo.canonicalInboundEvidence[head]? with
      | none =>
          simp [transportAdmissionCandidates, evidenceCase] at member
          obtain ⟨idMember, evidenceAt, eventAt⟩ := tailIH member
          exact ⟨by simp [idMember], evidenceAt, eventAt⟩
      | some expected =>
          cases eventCase : choreo.globalEvents[head]? with
          | none =>
              simp [transportAdmissionCandidates, evidenceCase, eventCase] at member
              obtain ⟨idMember, evidenceAt, eventAt⟩ := tailIH member
              exact ⟨by simp [idMember], evidenceAt, eventAt⟩
          | some candidate =>
              by_cases matching : expected = frame.inboundEvidence
              · simp [transportAdmissionCandidates, evidenceCase, eventCase, matching] at member
                rcases member with headMatch | tailMember
                · obtain ⟨rfl, rfl⟩ := headMatch
                  exact ⟨by simp, matching ▸ evidenceCase, eventCase⟩
                · obtain ⟨idMember, evidenceAt, eventAt⟩ := tailIH tailMember
                  exact ⟨by simp [idMember], evidenceAt, eventAt⟩
              · simp [transportAdmissionCandidates, evidenceCase, eventCase, matching] at member
                obtain ⟨idMember, evidenceAt, eventAt⟩ := tailIH member
                exact ⟨by simp [idMember], evidenceAt, eventAt⟩

def Choreo.transportAdmissionFrom?
    (choreo : Choreo) (ids : List Nat) (frame : TransportFrame) :
    Option (Nat × GlobalEvent) :=
  match transportAdmissionCandidates choreo frame ids with
  | [candidate] => some candidate
  | _ => none

theorem transport_admission_is_unique
    {choreo : Choreo} {ids : List Nat} {frame : TransportFrame}
    {leftId rightId : Nat} {leftEvent rightEvent : GlobalEvent}
    (left : choreo.transportAdmissionFrom? ids frame = some (leftId, leftEvent))
    (right : choreo.transportAdmissionFrom? ids frame = some (rightId, rightEvent)) :
    leftId = rightId /\ leftEvent = rightEvent := by
  have same : (leftId, leftEvent) = (rightId, rightEvent) :=
    Option.some.inj (left.symm.trans right)
  exact ⟨congrArg Prod.fst same, congrArg Prod.snd same⟩

def Choreo.transportAdmission?
    (choreo : Choreo) (frame : TransportFrame) : Option (Nat × GlobalEvent) :=
  choreo.transportAdmissionFrom? (List.range choreo.globalEvents.length) frame

/-- Runtime admission depends on the fields observed in the fixed core header,
not on carrier-private generation, sequence, or arrival history. -/
def TransportFrame.SameObservation
    (left right : TransportFrame) : Prop :=
  left.channel.session = right.channel.session /\
    left.inboundEvidence = right.inboundEvidence

private theorem transport_admission_candidates_observation_congr
    {choreo : Choreo} {left right : TransportFrame}
    (sameEvidence : left.inboundEvidence = right.inboundEvidence) :
    ∀ ids,
      transportAdmissionCandidates choreo left ids =
        transportAdmissionCandidates choreo right ids := by
  intro ids
  induction ids with
  | nil => rfl
  | cons globalId rest restIH =>
      simp only [transportAdmissionCandidates]
      rw [sameEvidence, restIH]

theorem transport_admission_from_depends_only_on_observation
    {choreo : Choreo} {ids : List Nat} {left right : TransportFrame}
    (sameEvidence : left.inboundEvidence = right.inboundEvidence) :
    choreo.transportAdmissionFrom? ids left =
      choreo.transportAdmissionFrom? ids right := by
  unfold Choreo.transportAdmissionFrom?
  rw [transport_admission_candidates_observation_congr sameEvidence]

theorem transport_admission_depends_only_on_observation
    {choreo : Choreo} {left right : TransportFrame}
    (sameEvidence : left.inboundEvidence = right.inboundEvidence) :
    choreo.transportAdmission? left = choreo.transportAdmission? right := by
  unfold Choreo.transportAdmission? Choreo.transportAdmissionFrom?
  rw [transport_admission_candidates_observation_congr sameEvidence]

private theorem transport_admission_from_checker_sound
    {choreo : Choreo} {ids : List Nat} {frame : TransportFrame}
    {globalId : Nat} {event : GlobalEvent}
    (accepted : choreo.transportAdmissionFrom? ids frame = some (globalId, event)) :
    TransportAdmission choreo frame globalId event /\ globalId ∈ ids := by
  unfold Choreo.transportAdmissionFrom? at accepted
  cases candidatesCase : transportAdmissionCandidates choreo frame ids with
  | nil => simp [candidatesCase] at accepted
  | cons candidate rest =>
      cases rest with
      | nil =>
          have candidateEq : candidate = (globalId, event) := by
            simpa [candidatesCase] using accepted
          subst candidate
          have sound := transport_admission_candidate_sound
            (choreo := choreo) (frame := frame) (ids := ids)
            (globalId := globalId) (event := event) (by simp [candidatesCase])
          exact ⟨⟨sound.2.1, sound.2.2⟩, sound.1⟩
      | cons second remaining => simp [candidatesCase] at accepted

theorem transport_admission_checker_sound
    {choreo : Choreo} {frame : TransportFrame}
    {globalId : Nat} {event : GlobalEvent}
    (accepted : choreo.transportAdmission? frame = some (globalId, event)) :
    TransportAdmission choreo frame globalId event := by
  exact (transport_admission_from_checker_sound accepted).1

def GlobalConfig.inboundCandidateIds (config : GlobalConfig) : List Nat :=
  (List.range config.choreo.globalEvents.length).filter fun globalId =>
    match config.status, config.event? globalId, config.phase globalId with
    | .live, some event, .queued epoch =>
        epoch == config.epoch && event.sender != event.receiver &&
          (config.commitLocal? event.receiver globalId
            (.recv event.sender event.label event.schema)).isSome
    | _, _, _ => false

def GlobalConfig.admitTransportFrame?
    (config : GlobalConfig) (frame : TransportFrame) : Option (Nat × GlobalEvent) :=
  if frame.channel.session = config.session then
    config.choreo.transportAdmissionFrom? config.inboundCandidateIds frame
  else
    none

theorem global_transport_admission_is_unique
    {config : GlobalConfig} {frame : TransportFrame}
    {leftId rightId : Nat} {leftEvent rightEvent : GlobalEvent}
    (left : config.admitTransportFrame? frame = some (leftId, leftEvent))
    (right : config.admitTransportFrame? frame = some (rightId, rightEvent)) :
    leftId = rightId /\ leftEvent = rightEvent := by
  have same : (leftId, leftEvent) = (rightId, rightEvent) :=
    Option.some.inj (left.symm.trans right)
  exact ⟨congrArg Prod.fst same, congrArg Prod.snd same⟩

theorem observed_transport_admission_ignores_carrier_history
    {config : GlobalConfig} {left right : TransportFrame}
    (same : left.SameObservation right) :
    config.admitTransportFrame? left = config.admitTransportFrame? right := by
  unfold GlobalConfig.admitTransportFrame?
  rw [same.1]
  by_cases sameSession : right.channel.session = config.session
  · simp only [sameSession, if_true]
    exact transport_admission_from_depends_only_on_observation same.2
  · simp [sameSession]

theorem global_transport_admission_checker_sound
    {config : GlobalConfig} {frame : TransportFrame}
    {globalId : Nat} {event : GlobalEvent}
    (accepted : config.admitTransportFrame? frame = some (globalId, event)) :
    frame.channel.session = config.session /\
      TransportAdmission config.choreo frame globalId event /\
      globalId ∈ config.inboundCandidateIds := by
  unfold GlobalConfig.admitTransportFrame? at accepted
  by_cases sameSession : frame.channel.session = config.session
  · have candidateAccepted :
        config.choreo.transportAdmissionFrom? config.inboundCandidateIds frame =
          some (globalId, event) := by simpa [sameSession] using accepted
    exact ⟨sameSession, transport_admission_from_checker_sound candidateAccepted⟩
  · simp [sameSession] at accepted

private def transportAdmissionRegressionChoreo : Choreo :=
  .send 0 1 9 4

private def transportAdmissionRegressionFrame
    (session lane : Nat) : TransportFrame := {
  channel := {
    session
    generation := 3
    lane
    sender := 0
    receiver := 1
  }
  sequence := 0
  frameLabel := ⟨0, by omega⟩
}

private def transportAdmissionRegressionConfig : GlobalConfig :=
  (GlobalConfig.initial 7 2 transportAdmissionRegressionChoreo).withPhase 0 (.queued 0)

example :
    (transportAdmissionRegressionConfig.admitTransportFrame?
        (transportAdmissionRegressionFrame 7 0)).isSome = true := by
  native_decide

example :
    (transportAdmissionRegressionConfig.admitTransportFrame?
        (transportAdmissionRegressionFrame 7 1)).isSome = false := by
  native_decide

example :
    (transportAdmissionRegressionConfig.admitTransportFrame?
        (transportAdmissionRegressionFrame 8 0)).isSome = false := by
  native_decide

theorem transport_admission_binds_exact_descriptor_occurrence
    {choreo : Choreo} {frame : TransportFrame}
    {globalId : Nat} {event : GlobalEvent}
    (admission : TransportAdmission choreo frame globalId event) :
    ∃ atom,
      choreo.canonicalProgramAtoms[globalId]? = some atom /\
      atom.globalEvent = event /\
      frame.channel.sender = event.sender /\
      frame.channel.receiver = event.receiver /\
      frame.channel.lane = event.lane /\
      frame.frameLabel.val = choreo.canonicalFrameLabel globalId := by
  cases atomCase : choreo.canonicalProgramAtoms[globalId]? with
  | none =>
      have evidenceAt := admission.evidenceAt
      rw [canonical_inbound_evidence_getElem?, atomCase] at evidenceAt
      simp at evidenceAt
  | some atom =>
      have evidenceEq :
          ({
            source := atom.sender
            target := atom.receiver
            lane := atom.lane
            frameLabel := choreo.canonicalFrameLabel globalId
          } : StaticInboundEvidence) = frame.inboundEvidence := by
        have evidenceAt := admission.evidenceAt
        rw [canonical_inbound_evidence_getElem?, atomCase] at evidenceAt
        simpa using evidenceAt
      have mappedEventAt :
          (choreo.canonicalProgramAtoms.map DecodedProgramAtom.globalEvent)[
              globalId]? = some event := by
        rw [canonical_program_atoms_global_events]
        exact admission.eventAt
      have eventEq : atom.globalEvent = event := by
        simpa [atomCase] using mappedEventAt
      have sourceEq := congrArg (fun evidence : StaticInboundEvidence => evidence.source) evidenceEq
      have targetEq := congrArg (fun evidence : StaticInboundEvidence => evidence.target) evidenceEq
      have laneEq := congrArg (fun evidence : StaticInboundEvidence => evidence.lane) evidenceEq
      have labelEq := congrArg (fun evidence : StaticInboundEvidence => evidence.frameLabel) evidenceEq
      have eventSenderEq := congrArg (fun event : GlobalEvent => event.sender) eventEq
      have eventReceiverEq := congrArg (fun event : GlobalEvent => event.receiver) eventEq
      have eventLaneEq := congrArg (fun event : GlobalEvent => event.lane) eventEq
      refine ⟨atom, rfl, eventEq, ?_, ?_, ?_, ?_⟩
      · exact (by
          simpa [TransportFrame.inboundEvidence] using sourceEq.symm.trans eventSenderEq)
      · exact (by
          simpa [TransportFrame.inboundEvidence] using targetEq.symm.trans eventReceiverEq)
      · exact (by
          simpa [TransportFrame.inboundEvidence] using laneEq.symm.trans eventLaneEq)
      · simpa [TransportFrame.inboundEvidence] using labelEq.symm

def TransportAdmission.admittedMessage
    {choreo : Choreo} {frame : TransportFrame}
    {globalId : Nat} {event : GlobalEvent}
    (_admission : TransportAdmission choreo frame globalId event)
    (config : GlobalConfig)
    (_ : config.choreo = choreo) : AdmittedMessage :=
  config.expectedMessage globalId event

theorem transport_admission_produces_exact_admitted_message
    {choreo : Choreo} {frame : TransportFrame}
    {globalId : Nat} {event : GlobalEvent}
    (admission : TransportAdmission choreo frame globalId event)
    (config : GlobalConfig)
    (sameChoreo : config.choreo = choreo)
    (sameSession : frame.channel.session = config.session) :
    let message := admission.admittedMessage config sameChoreo
    message.session = frame.channel.session /\
    message.epoch = config.epoch /\
    message.globalId = globalId /\
    message.sender = frame.channel.sender /\
    message.receiver = frame.channel.receiver /\
    message.lane = frame.channel.lane /\
    message.label = event.label /\
    message.schema = event.schema := by
  obtain ⟨_, _, _, senderEq, receiverEq, laneEq, _⟩ :=
    transport_admission_binds_exact_descriptor_occurrence admission
  simp [TransportAdmission.admittedMessage, GlobalConfig.expectedMessage,
    sameSession, senderEq, receiverEq, laneEq]

def Choreo.roleEndpointSelectorsFrom
    (role base : Nat) : Choreo -> List StaticEndpointSelector
  | .send sender receiver label schema =>
      if role = sender then
        [.outbound sender label schema]
      else if role = receiver then
        [.inbound base]
      else
        []
  | .seq left right | .par left right | .route _ left right =>
      left.roleEndpointSelectorsFrom role base ++
        right.roleEndpointSelectorsFrom role (base + left.globalEvents.length)
  | .roll body => body.roleEndpointSelectorsFrom role base

def Choreo.endpointSelectorsFrom
    (base : Nat) : Choreo -> List StaticEndpointSelector
  | .send sender _ label schema => [.outbound sender label schema, .inbound base]
  | .seq left right | .par left right | .route _ left right =>
      left.endpointSelectorsFrom base ++
        right.endpointSelectorsFrom (base + left.globalEvents.length)
  | .roll body => body.endpointSelectorsFrom base

/-- First visible global frontier. `seq` exposes only its left operand because
the normalized choreography has no empty term. Parallel and route expose both
frontiers. -/
def Choreo.firstOccurrencesFrom (base : Nat) : Choreo -> List StaticOccurrence
  | .send sender receiver label schema =>
      [{ globalId := base, sender, receiver, label, schema }]
  | .seq left _ => left.firstOccurrencesFrom base
  | .par left right | .route _ left right =>
      left.firstOccurrencesFrom base ++
        right.firstOccurrencesFrom (base + left.globalEvents.length)
  | .roll body => body.firstOccurrencesFrom base

def Choreo.firstEndpointSelectorsFrom
    (base : Nat) (choreo : Choreo) : List StaticEndpointSelector :=
  (choreo.firstOccurrencesFrom base).flatMap fun occurrence =>
    [.outbound occurrence.sender occurrence.label occurrence.schema,
      .inbound occurrence.globalId]

def staticSelectorsDisjoint
    (left right : List StaticEndpointSelector) : Bool :=
  left.all fun selector => !right.contains selector

private def observerPathsMergeableFrom :
    List StaticEndpointSelector -> List StaticEndpointSelector -> Bool
  | [], [] => true
  | [], _ :: _ | _ :: _, [] => false
  | left :: leftRest, right :: rightRest =>
      if left == right then
        left.isInbound && observerPathsMergeableFrom leftRest rightRest
      else
        left.isInbound && right.isInbound

theorem observer_absence_is_not_branch_evidence
    (selector : StaticEndpointSelector)
    (rest : List StaticEndpointSelector) :
    observerPathsMergeableFrom [] (selector :: rest) = false := rfl

theorem observer_outbound_heads_are_not_mergeable
    (leftRole leftLabel leftSchema rightRole rightLabel rightSchema : Nat)
    (leftRest rightRest : List StaticEndpointSelector) :
    observerPathsMergeableFrom
        (.outbound leftRole leftLabel leftSchema :: leftRest)
        (.outbound rightRole rightLabel rightSchema :: rightRest) = false := by
  simp [observerPathsMergeableFrom, StaticEndpointSelector.isInbound]

def Choreo.observerPathsMergeable
    (role leftBase rightBase : Nat)
    (left right : Choreo) : Bool :=
  observerPathsMergeableFrom
    (left.roleEndpointSelectorsFrom role leftBase)
    (right.roleEndpointSelectorsFrom role rightBase)

def checkRouteSiteProjectability
    (roleCount : Nat)
    (authority : RouteAuthority)
    (leftBase rightBase : Nat)
    (left right : Choreo) : Bool :=
  match Choreo.routeController? left right with
  | none => false
  | some controller =>
      let observersKnow := (List.range roleCount).all fun role =>
        role == controller ||
          left.observerPathsMergeable role leftBase rightBase right
      match authority with
      | .dynamic _ => observersKnow
      | .intrinsic =>
          staticSelectorsDisjoint
              (left.firstEndpointSelectorsFrom leftBase)
              (right.firstEndpointSelectorsFrom rightBase) &&
            observersKnow

theorem accepted_dynamic_route_has_unique_controller
    {roleCount resolver leftBase rightBase : Nat}
    {left right : Choreo}
    (accepted : checkRouteSiteProjectability roleCount (.dynamic resolver)
      leftBase rightBase left right = true) :
    ∃ controller,
      Choreo.routeController? left right = some controller := by
  unfold checkRouteSiteProjectability at accepted
  cases controller : Choreo.routeController? left right with
  | none => simp [controller] at accepted
  | some role => exact ⟨role, rfl⟩

/-- Every non-controller role admitted at a dynamic route learns the selected
arm from occurrence-identified inbound evidence. No runtime-local publication
cell is part of the projectability argument. -/
theorem accepted_dynamic_route_knowledge_is_controller_or_inbound
    {roleCount resolver leftBase rightBase : Nat}
    {left right : Choreo}
    (accepted : checkRouteSiteProjectability roleCount (.dynamic resolver)
      leftBase rightBase left right = true) :
    ∃ controller,
      Choreo.routeController? left right = some controller /\
      ∀ role, role < roleCount ->
        role = controller ∨
          left.observerPathsMergeable role leftBase rightBase right = true := by
  obtain ⟨controller, controllerEq⟩ :=
    accepted_dynamic_route_has_unique_controller accepted
  unfold checkRouteSiteProjectability at accepted
  rw [controllerEq] at accepted
  refine ⟨controller, controllerEq, ?_⟩
  simpa using accepted

def Choreo.checkRouteKnowledgeFrom
    (roleCount base : Nat) : Choreo -> Bool
  | .send _ _ _ _ => true
  | .seq left right | .par left right =>
      left.checkRouteKnowledgeFrom roleCount base &&
        right.checkRouteKnowledgeFrom roleCount
          (base + left.globalEvents.length)
  | .route authority left right =>
      let rightBase := base + left.globalEvents.length
      checkRouteSiteProjectability roleCount authority
          base rightBase left right &&
        left.checkRouteKnowledgeFrom roleCount base &&
        right.checkRouteKnowledgeFrom roleCount rightBase
  | .roll body => body.checkRouteKnowledgeFrom roleCount base

/-- Parallel arms may not expose the same endpoint selector. Inbound identities
are occurrence-indexed, so this primarily excludes repeated outbound
`(role,label,schema)` selectors across concurrently active lanes. -/
def Choreo.checkParallelEndpointLinearityFrom (base : Nat) : Choreo -> Bool
  | .send _ _ _ _ => true
  | .seq left right | .route _ left right =>
      left.checkParallelEndpointLinearityFrom base &&
        right.checkParallelEndpointLinearityFrom
          (base + left.globalEvents.length)
  | .par left right =>
      let rightBase := base + left.globalEvents.length
      staticSelectorsDisjoint
          (left.endpointSelectorsFrom base)
          (right.endpointSelectorsFrom rightBase) &&
        left.checkParallelEndpointLinearityFrom base &&
        right.checkParallelEndpointLinearityFrom rightBase
  | .roll body => body.checkParallelEndpointLinearityFrom base

/-- A roll body and its following continuation must not begin with the same
endpoint selector. This is the host-side counterpart of the production
reentry-ambiguity rejection. -/
def Choreo.checkRollReentryLinearityFrom
    (base : Nat)
    (continuation : List StaticEndpointSelector) : Choreo -> Bool
  | .send _ _ _ _ => true
  | .seq left right =>
      let rightBase := base + left.globalEvents.length
      left.checkRollReentryLinearityFrom base
          (right.firstEndpointSelectorsFrom rightBase) &&
        right.checkRollReentryLinearityFrom rightBase continuation
  | .par left right | .route _ left right =>
      let rightBase := base + left.globalEvents.length
      left.checkRollReentryLinearityFrom base continuation &&
        right.checkRollReentryLinearityFrom rightBase continuation
  | .roll body =>
      staticSelectorsDisjoint
          (body.firstEndpointSelectorsFrom base) continuation &&
        body.checkRollReentryLinearityFrom base []

structure Choreo.StaticProjectable
    (roleCount : Nat) (choreo : Choreo) : Prop where
  inboundOccurrenceColoring : choreo.InboundOccurrenceColoring
  receiveLaneCausality : choreo.ReceiveLaneCausalSafety roleCount
  rollReceiveLaneCausality : choreo.RollReceiveLaneCausalSafety roleCount
  parallelEndpointLinear : choreo.checkParallelEndpointLinearityFrom 0 = true
  routeKnowledge : choreo.checkRouteKnowledgeFrom roleCount 0 = true
  rollReentryLinear : choreo.checkRollReentryLinearityFrom 0 [] = true

def checkStaticProjectability (roleCount : Nat) (choreo : Choreo) : Bool :=
    choreo.checkInboundOccurrenceColoring &&
    choreo.checkReceiveLaneCausality roleCount &&
    choreo.checkRollReceiveLaneCausality roleCount &&
    choreo.checkParallelEndpointLinearityFrom 0 &&
    choreo.checkRouteKnowledgeFrom roleCount 0 &&
    choreo.checkRollReentryLinearityFrom 0 []

theorem static_projectability_checker_sound
    {roleCount : Nat} {choreo : Choreo}
    (accepted : checkStaticProjectability roleCount choreo = true) :
    choreo.StaticProjectable roleCount := by
  simp only [checkStaticProjectability, Bool.and_eq_true] at accepted
  rcases accepted with
    ⟨⟨⟨⟨⟨inbound, receiveLane⟩, rollReceiveLane⟩, parallel⟩, route⟩, roll⟩
  exact ⟨inbound_occurrence_coloring_checker_sound inbound,
    receive_lane_causality_checker_sound receiveLane,
    roll_receive_lane_causality_checker_sound rollReceiveLane,
    parallel, route, roll⟩

/-- A resolver disambiguates one controller's operation; it is not a
cross-runtime agreement oracle for competing first senders. -/
theorem dynamic_route_competing_first_senders_are_rejected :
    checkStaticProjectability 3
      (.route (.dynamic 7) (.send 0 2 1 0) (.send 1 2 2 0)) = false := by
  decide

theorem dynamic_route_outbound_observer_without_evidence_is_rejected :
    checkStaticProjectability 3
      (.route (.dynamic 7)
        (.seq (.send 2 2 1 0) (.send 0 2 3 0))
        (.seq (.send 2 2 2 0) (.send 1 2 4 0))) = false := by
  decide

theorem dynamic_route_inbound_observers_are_projectable :
    checkStaticProjectability 3
      (.route (.dynamic 7)
        (.seq (.send 2 0 1 0) (.send 0 2 3 0))
        (.seq (.send 2 0 2 0) (.send 0 2 4 0))) = true := by
  decide

end Hibana
