import Hibana.DistributedRollRefinement

namespace Hibana

/-- Proof-only per-occurrence projection of the normative mixed history in
`ElasticAdmissionHistory`. `sent` and `received` are reconstructed from the
descriptor-admitted subsequence and affine delivery cursor; neither counter is
a second authority, a wire field, or a resident runtime field. -/
structure ElasticIterationQueue where
  sent : Nat
  received : Nat
  pending : List Nat
  deriving Repr, DecidableEq

def ElasticIterationQueue.initial : ElasticIterationQueue := {
  sent := 0
  received := 0
  pending := []
}

/-- Pending generations are exactly the unconsumed interval. This rules out
holes, duplicates, reordering, and replay with one compact invariant. -/
def ElasticIterationQueue.WellFormed
    (queue : ElasticIterationQueue) : Prop :=
  queue.received <= queue.sent /\
    queue.pending =
      List.range' queue.received (queue.sent - queue.received)

def ElasticIterationQueue.send
    (queue : ElasticIterationQueue) : ElasticIterationQueue := {
  queue with
  sent := queue.sent + 1
  pending := queue.pending ++ [queue.sent]
}

def ElasticIterationQueue.receive?
    (queue : ElasticIterationQueue) :
    Option (Nat × ElasticIterationQueue) :=
  match queue.pending with
  | [] => none
  | generation :: rest =>
      if generation = queue.received then
        some (generation, {
          queue with
          received := queue.received + 1
          pending := rest
        })
      else
        none

private theorem range_extend
    {received sent : Nat}
    (bound : received <= sent) :
    List.range' received ((sent + 1) - received) =
      List.range' received (sent - received) ++ [sent] := by
  have lengthEq : (sent + 1) - received = (sent - received) + 1 := by
    omega
  rw [lengthEq, <- List.range'_append]
  have startEq : received + (sent - received) = sent := by
    omega
  simp [startEq]

private theorem range_values_at_least_start
    (start count : Nat)
    {value : Nat}
    (member : value ∈ List.range' start count) :
    start <= value := by
  induction count generalizing start with
  | zero => simp at member
  | succ count induction =>
      rw [List.range'_succ] at member
      rcases List.mem_cons.mp member with rfl | inTail
      · omega
      · have lower := induction (start := start + 1) inTail
        omega

theorem elastic_iteration_queue_initial_well_formed :
    ElasticIterationQueue.initial.WellFormed := by
  simp [ElasticIterationQueue.initial, ElasticIterationQueue.WellFormed]

theorem elastic_iteration_queue_send_preserves_well_formed
    {queue : ElasticIterationQueue}
    (wellFormed : queue.WellFormed) :
    queue.send.WellFormed := by
  constructor
  · change queue.received <= queue.sent + 1
    have bound := wellFormed.1
    omega
  · simp only [ElasticIterationQueue.send]
    rw [wellFormed.2]
    exact (range_extend wellFormed.1).symm

theorem elastic_iteration_queue_send_appends_exact_generation
    (queue : ElasticIterationQueue) :
    queue.send.sent = queue.sent + 1 /\
      queue.send.received = queue.received /\
      queue.send.pending = queue.pending ++ [queue.sent] := by
  exact ⟨rfl, rfl, rfl⟩

theorem elastic_iteration_queue_receive_acceptance_is_exact
    {queue next : ElasticIterationQueue}
    {generation : Nat}
    (accepted : queue.receive? = some (generation, next)) :
    generation = queue.received /\
      queue.pending = queue.received :: next.pending /\
      next.sent = queue.sent /\
      next.received = queue.received + 1 := by
  unfold ElasticIterationQueue.receive? at accepted
  cases pendingEq : queue.pending with
  | nil => simp [pendingEq] at accepted
  | cons head rest =>
      by_cases exactHead : head = queue.received
      · subst head
        have pairEq :
            (queue.received, {
              queue with
              received := queue.received + 1
              pending := rest
            }) = (generation, next) := by
          exact Option.some.inj (by simpa [pendingEq] using accepted)
        cases pairEq
        exact ⟨rfl, rfl, rfl, rfl⟩
      · simp [pendingEq, exactHead] at accepted

theorem elastic_iteration_queue_receive_preserves_well_formed
    {queue next : ElasticIterationQueue}
    {generation : Nat}
    (wellFormed : queue.WellFormed)
    (accepted : queue.receive? = some (generation, next)) :
    next.WellFormed := by
  obtain ⟨_, pendingShape, sentEq, receivedEq⟩ :=
    elastic_iteration_queue_receive_acceptance_is_exact accepted
  have nonemptyInterval : queue.sent - queue.received ≠ 0 := by
    intro emptyInterval
    have empty : queue.pending = [] := by
      rw [wellFormed.2, emptyInterval]
      rfl
    rw [empty] at pendingShape
    contradiction
  have strict : queue.received < queue.sent := by
    omega
  constructor
  · rw [sentEq, receivedEq]
    omega
  · have lengthEq :
        queue.sent - queue.received =
          (queue.sent - (queue.received + 1)) + 1 := by
      omega
    have exactPending := wellFormed.2
    rw [lengthEq, List.range'_succ] at exactPending
    rw [sentEq, receivedEq]
    have tails :
        next.pending =
          List.range' (queue.received + 1)
            (queue.sent - (queue.received + 1)) := by
      rw [pendingShape] at exactPending
      exact List.cons.inj exactPending |>.2
    exact tails

theorem elastic_iteration_queue_receive_advances_affinely
    {queue next : ElasticIterationQueue}
    {generation : Nat}
    (accepted : queue.receive? = some (generation, next)) :
    queue.received < next.received := by
  have exact := elastic_iteration_queue_receive_acceptance_is_exact accepted
  rw [exact.2.2.2]
  omega

theorem elastic_iteration_queue_received_generation_cannot_replay
    {queue next : ElasticIterationQueue}
    {generation : Nat}
    (wellFormed : queue.WellFormed)
    (accepted : queue.receive? = some (generation, next)) :
    generation ∉ next.pending := by
  have exact := elastic_iteration_queue_receive_acceptance_is_exact accepted
  have nextWellFormed :=
    elastic_iteration_queue_receive_preserves_well_formed wellFormed accepted
  rw [exact.1]
  intro member
  rw [nextWellFormed.2, exact.2.2.1, exact.2.2.2] at member
  have bounds := range_values_at_least_start
    (queue.received + 1) _ member
  omega

/-- Two sends may be accepted before any receive. The resulting queue carries
both erased roll generations in FIFO order, closing the concrete mismatch
exhibited by `global_atomic_roll_cannot_model_pipelined_single_send_reentry`. -/
theorem elastic_iteration_queue_models_pipelined_roll_reentry :
    let queued := ElasticIterationQueue.initial.send.send
    queued.WellFormed /\ queued.pending = [0, 1] /\ queued.received = 0 := by
  unfold ElasticIterationQueue.WellFormed
  decide

/-- Complete ghost identity for an occurrence inside a rolled body. -/
structure ElasticOccurrenceKey where
  globalId : Nat
  iteration : Nat
  deriving Repr, DecidableEq

def ElasticIterationQueue.nextOccurrence
    (queue : ElasticIterationQueue)
    (globalId : Nat) : ElasticOccurrenceKey := {
  globalId
  iteration := queue.sent
}

theorem elastic_send_occurrence_has_exact_iteration
    (queue : ElasticIterationQueue)
    (globalId : Nat) :
    queue.nextOccurrence globalId = {
      globalId
      iteration := queue.sent
    } /\ queue.send.pending = queue.pending ++ [queue.sent] := by
  exact ⟨rfl, rfl⟩

/-- Route choices are publication-scoped proof facts. Production still carries
the choice through exact in-band branch evidence; the ordinal is reconstructed
from the conflict's affine publication history. -/
structure ElasticConflictKey where
  conflict : Nat
  iteration : Nat
  deriving Repr, DecidableEq

end Hibana
