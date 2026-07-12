import Hibana.StaticProjectability

namespace Hibana

/-- A concrete receive-to-send causal chain accepted by the static checker.
Each relay is a descriptor occurrence whose sender is the role reached by the
previous receive. -/
inductive CausalHandoffPath
    (occurrences : List StaticGlobalOccurrence)
    (earlier later : StaticGlobalOccurrence)
    (roleCount : Nat) : StaticGlobalOccurrence -> Prop where
  | origin : CausalHandoffPath occurrences earlier later roleCount earlier
  | handoff {source target : StaticGlobalOccurrence}
      (chain : CausalHandoffPath occurrences earlier later roleCount source)
      (member : target ∈ occurrences)
      (senderBound : target.event.sender < roleCount)
      (receiverBound : target.event.receiver < roleCount)
      (sameRole : source.event.receiver = target.event.sender)
      (locallyOrdered : occurrenceLocallyOrdered source target = true) :
      CausalHandoffPath occurrences earlier later roleCount target

def CausalWitnessesSound
    (occurrences : List StaticGlobalOccurrence)
    (earlier later : StaticGlobalOccurrence)
    (roleCount : Nat)
    (witnesses : CausalWitnesses) : Prop :=
  ∀ role witness, witnesses role = some witness ->
    witness.event.receiver = role ∧
      CausalHandoffPath occurrences earlier later roleCount witness

theorem initial_causal_witnesses_sound
    {occurrences : List StaticGlobalOccurrence}
    {earlier later : StaticGlobalOccurrence}
    {roleCount : Nat} :
    CausalWitnessesSound occurrences earlier later roleCount
      (fun role => if role = earlier.event.receiver then some earlier else none) := by
  intro role witness found
  by_cases same : role = earlier.event.receiver
  · simp [same] at found
    subst witness
    exact ⟨same.symm, .origin⟩
  · simp [same] at found

theorem add_causal_witness_preserves_soundness
    {occurrences : List StaticGlobalOccurrence}
    {earlier later candidate : StaticGlobalOccurrence}
    {roleCount role : Nat}
    {witnesses : CausalWitnesses}
    (sound : CausalWitnessesSound occurrences earlier later roleCount witnesses)
    (candidateReceiver : candidate.event.receiver = role)
    (candidatePath : CausalHandoffPath occurrences earlier later roleCount candidate) :
    CausalWitnessesSound occurrences earlier later roleCount
      (addCausalWitness witnesses role candidate) := by
  intro query witness found
  by_cases same : query = role
  · subst query
    cases prior : witnesses role with
    | none =>
        simp [addCausalWitness, prior] at found
        subst witness
        exact ⟨candidateReceiver, candidatePath⟩
    | some existing =>
        simp [addCausalWitness, prior] at found
        subst witness
        exact sound role existing prior
  · have base : witnesses query = some witness := by
      simpa [addCausalWitness, same] using found
    exact sound query witness base

theorem propagate_causal_witness_preserves_soundness
    {occurrences : List StaticGlobalOccurrence}
    {earlier later candidate : StaticGlobalOccurrence}
    {roleCount : Nat}
    {witnesses : CausalWitnesses}
    (candidateMember : candidate ∈ occurrences)
    (sound : CausalWitnessesSound occurrences earlier later roleCount witnesses) :
    CausalWitnessesSound occurrences earlier later roleCount
      (propagateCausalWitness earlier later roleCount witnesses candidate) := by
  unfold propagateCausalWitness
  split
  next accepted =>
    simp only [Bool.and_eq_true] at accepted
    rcases accepted with ⟨⟨senderBound, receiverBound⟩, onRoutePath⟩
    cases found : witnesses candidate.event.sender with
    | none => exact sound
    | some prior =>
        simp only
        by_cases locallyOrdered : occurrenceLocallyOrdered prior candidate = true
        · rw [if_pos locallyOrdered]
          have priorSound := sound candidate.event.sender prior found
          exact add_causal_witness_preserves_soundness sound rfl <|
            .handoff priorSound.2 candidateMember
              (of_decide_eq_true senderBound) (of_decide_eq_true receiverBound)
              priorSound.1 locallyOrdered
        · rw [if_neg locallyOrdered]
          exact sound
  next _ => exact sound

theorem fold_causal_witnesses_sound
    {occurrences candidates : List StaticGlobalOccurrence}
    {earlier later : StaticGlobalOccurrence}
    {roleCount : Nat}
    {witnesses : CausalWitnesses}
    (candidatesSubset : ∀ candidate ∈ candidates, candidate ∈ occurrences)
    (sound : CausalWitnessesSound occurrences earlier later roleCount witnesses) :
    CausalWitnessesSound occurrences earlier later roleCount
      (candidates.foldl
        (propagateCausalWitness earlier later roleCount) witnesses) := by
  induction candidates generalizing witnesses with
  | nil => exact sound
  | cons candidate rest induction =>
      simp only [List.foldl_cons]
      apply induction
      · intro member memberInRest
        exact candidatesSubset member (List.mem_cons_of_mem candidate memberInRest)
      · exact propagate_causal_witness_preserves_soundness
          (candidatesSubset candidate (by simp)) sound

theorem receive_precedes_later_send_has_causal_path
    {occurrences : List StaticGlobalOccurrence}
    {roleCount : Nat}
    {earlier later : StaticGlobalOccurrence}
    (accepted : receivePrecedesLaterSend occurrences roleCount earlier later = true)
    (laterMember : later ∈ occurrences)
    (laterReceiverBound : later.event.receiver < roleCount) :
    CausalHandoffPath occurrences earlier later roleCount later := by
  unfold receivePrecedesLaterSend at accepted
  split at accepted
  next endpointsBound =>
    simp only [Bool.and_eq_true] at endpointsBound
    let initial : CausalWitnesses := fun role =>
      if role = earlier.event.receiver then some earlier else none
    let between := occurrences.filter fun candidate =>
      earlier.globalId < candidate.globalId && candidate.globalId < later.globalId
    let witnesses := between.foldl
      (propagateCausalWitness earlier later roleCount) initial
    have initialSound : CausalWitnessesSound occurrences earlier later roleCount initial :=
      initial_causal_witnesses_sound
    have betweenSubset : ∀ candidate ∈ between, candidate ∈ occurrences := by
      intro candidate member
      exact (List.mem_filter.mp member).1
    have witnessesSound :
        CausalWitnessesSound occurrences earlier later roleCount witnesses :=
      fold_causal_witnesses_sound betweenSubset initialSound
    change (match witnesses later.event.sender with
      | none => false
      | some witness => occurrenceLocallyOrdered witness later) = true at accepted
    cases found : witnesses later.event.sender with
    | none => simp [found] at accepted
    | some witness =>
        have ordered : occurrenceLocallyOrdered witness later = true := by
          simpa [found] using accepted
        have witnessSound := witnessesSound later.event.sender witness found
        apply CausalHandoffPath.handoff witnessSound.2 laterMember
          (of_decide_eq_true endpointsBound.2)
          laterReceiverBound witnessSound.1
        exact ordered
  next endpointsOutOfRange => simp at accepted

/-- A schedule interpretation for the checker: each send precedes its receive,
and locally ordered receive-to-send handoffs preserve endpoint program order. -/
structure CausalSchedule (occurrences : List StaticGlobalOccurrence) where
  sendTime : Nat -> Nat
  receiveTime : Nat -> Nat
  deliveryOrder : ∀ occurrence ∈ occurrences,
    sendTime occurrence.globalId < receiveTime occurrence.globalId
  localOrder : ∀ source target,
    source.event.receiver = target.event.sender ->
    occurrenceLocallyOrdered source target = true ->
    receiveTime source.globalId < sendTime target.globalId

theorem causal_handoff_path_orders_receive_before_send
    {occurrences : List StaticGlobalOccurrence}
    {earlier later : StaticGlobalOccurrence}
    {roleCount : Nat}
    (schedule : CausalSchedule occurrences)
    (path : CausalHandoffPath occurrences earlier later roleCount later)
    (different : earlier ≠ later) :
    schedule.receiveTime earlier.globalId < schedule.sendTime later.globalId := by
  have receiveProgress : ∀ occurrence,
      CausalHandoffPath occurrences earlier later roleCount occurrence ->
      occurrence = earlier ∨
        schedule.receiveTime earlier.globalId < schedule.receiveTime occurrence.globalId := by
    intro occurrence occurrencePath
    induction occurrencePath with
    | origin => exact Or.inl rfl
    | handoff chain member _ _ sameRole locallyOrdered induction =>
        right
        have localStep := schedule.localOrder _ _ sameRole locallyOrdered
        have delivered := schedule.deliveryOrder _ member
        rcases induction with rfl | prior
        · exact Nat.lt_trans localStep delivered
        · exact Nat.lt_trans (Nat.lt_trans prior localStep) delivered
  cases path with
  | origin => exact False.elim (different rfl)
  | handoff chain _ _ _ sameRole locallyOrdered =>
      have localStep := schedule.localOrder _ _ sameRole locallyOrdered
      rcases receiveProgress _ chain with rfl | prior
      · exact localStep
      · exact Nat.lt_trans prior localStep

/-- Hibana's iteration-erasure criterion: a reused receive lane is ordered by
one authenticated FIFO channel when the sender is stable, otherwise by a real
receive-to-send causal handoff before the next iteration can publish. -/
theorem roll_reentry_has_fifo_or_causal_order
    {body : Choreo} {roleCount : Nat}
    {left right : StaticGlobalOccurrence}
    (safe : body.RollBodyReceiveLaneCausalSafety roleCount)
    (leftMember : left ∈ body.staticGlobalOccurrences)
    (rightMember : right ∈ body.staticGlobalOccurrences)
    (leftNonlocal : left.event.sender ≠ left.event.receiver)
    (rightNonlocal : right.event.sender ≠ right.event.receiver)
    (sameReceiver : left.event.receiver = right.event.receiver)
    (sameLane : left.event.lane = right.event.lane)
    (rightReceiverBound : right.event.receiver < roleCount) :
    left.event.sender = right.event.sender ∨
      CausalHandoffPath body.rollUnfoldedOccurrences
        (left.inRollIteration body.globalEvents.length .current)
        (right.inRollIteration body.globalEvents.length .next)
        roleCount
        (right.inRollIteration body.globalEvents.length .next) := by
  by_cases sameSender : left.event.sender = right.event.sender
  · exact Or.inl sameSender
  · right
    apply receive_precedes_later_send_has_causal_path
    · exact roll_reentry_sender_change_requires_causal_handoff safe leftMember rightMember
        leftNonlocal rightNonlocal sameReceiver sameLane sameSender
    · exact List.mem_append_right _ (List.mem_map.mpr ⟨right, rightMember, rfl⟩)
    · exact rightReceiverBound

end Hibana
