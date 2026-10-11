import Hibana.DistributedSemantics
import Hibana.RollFreshness

namespace Hibana

private theorem nat_range_nodup : ∀ count : Nat, (List.range count).Nodup
  | 0 => .nil
  | count + 1 => by
      rw [List.range_succ, List.nodup_append]
      refine ⟨nat_range_nodup count, by simp, ?_⟩
      intro left leftMember right rightMember
      simp only [List.mem_singleton] at rightMember
      subst right
      exact Nat.ne_of_lt (List.mem_range.mp leftMember)

private theorem filter_not_equal_length_of_mem_nodup
    {role : Nat} {pending : List Nat}
    (nodup : pending.Nodup)
    (member : role ∈ pending) :
    (pending.filter fun candidate => candidate != role).length + 1 = pending.length := by
  induction pending with
  | nil => simp at member
  | cons head tail induction =>
      have nodupParts := List.nodup_cons.mp nodup
      by_cases same : head = role
      · subst head
        have filteredTail :
            (tail.filter fun candidate => candidate != role) = tail := by
          apply List.filter_eq_self.mpr
          intro candidate candidateMember
          have different : candidate ≠ role := by
            intro candidateEq
            subst candidate
            exact nodupParts.1 candidateMember
          simp [different]
        simp [filteredTail]
      · have tailMember : role ∈ tail := by
          have cases : role = head ∨ role ∈ tail := by simpa using member
          rcases cases with roleEq | inTail
          · exact False.elim (same roleEq.symm)
          · exact inTail
        have tailExact := induction nodupParts.2 tailMember
        simp [same, tailExact]

/-- Proof-only state between one global roll linearization and the independent
materialization of that reset by every endpoint. It adds no resident field and
does not require a wire epoch. -/
structure DistributedRollMaterialization where
  before : DistributedConfig
  after : DistributedConfig
  rollId : Nat
  transports : List TransportState
  pending : List Nat
  physicalRoleState : Nat -> CommitState

def DistributedRollMaterialization.abstract
    (state : DistributedRollMaterialization) : GlobalConfig :=
  state.after.abstract

/-- The carrier is drained at the linearization point. Pending roles still
hold their pre-roll cursor; every removed role holds the exact post-roll
cursor. -/
structure DistributedRollMaterialization.Valid
    (state : DistributedRollMaterialization) : Prop where
  linearized : state.before.rollStep? state.rollId = some state.after
  transportDrained : TransportSnapshotDrained state.transports
  pendingNodup : state.pending.Nodup
  pendingBound : ∀ role, role ∈ state.pending -> role < state.after.roleCount
  physicalPending : ∀ role, role ∈ state.pending ->
    state.physicalRoleState role = state.before.roleState role
  physicalDone : ∀ role, role < state.after.roleCount -> role ∉ state.pending ->
    state.physicalRoleState role = state.after.roleState role

def DistributedRollMaterialization.initial
    (before after : DistributedConfig)
    (rollId : Nat)
    (transports : List TransportState) : DistributedRollMaterialization := {
  before
  after
  rollId
  transports
  pending := List.range after.roleCount
  physicalRoleState := before.roleState
}

theorem distributed_roll_materialization_initial_valid
    {before after : DistributedConfig}
    {rollId : Nat}
    {transports : List TransportState}
    (linearized : before.rollStep? rollId = some after)
    (drained : TransportSnapshotDrained transports) :
    (DistributedRollMaterialization.initial before after rollId transports).Valid := by
  refine {
    linearized
    transportDrained := drained
    pendingNodup := nat_range_nodup after.roleCount
    pendingBound := ?_
    physicalPending := ?_
    physicalDone := ?_
  }
  · intro role member
    exact List.mem_range.mp member
  · intro role _
    rfl
  · intro role bound absent
    exact False.elim (absent (List.mem_range.mpr bound))

def DistributedRollMaterialization.materializeRole
    (state : DistributedRollMaterialization)
    (role : Nat) : DistributedRollMaterialization := {
  state with
  pending := state.pending.filter fun candidate => candidate != role
  physicalRoleState := fun candidate =>
    if candidate = role then state.after.roleState candidate
    else state.physicalRoleState candidate
}

def DistributedRollMaterialization.materializeRole?
    (state : DistributedRollMaterialization)
    (role : Nat) : Option DistributedRollMaterialization :=
  if role ∈ state.pending then some (state.materializeRole role) else none

theorem distributed_roll_materialization_preserves_validity
    {state : DistributedRollMaterialization}
    (valid : state.Valid)
    (role : Nat) :
    (state.materializeRole role).Valid := by
  refine {
    linearized := valid.linearized
    transportDrained := valid.transportDrained
    pendingNodup := List.filter_sublist.nodup valid.pendingNodup
    pendingBound := ?_
    physicalPending := ?_
    physicalDone := ?_
  }
  · intro candidate member
    exact valid.pendingBound candidate (List.mem_filter.mp member).1
  · intro candidate member
    have filtered := List.mem_filter.mp member
    have prior : candidate ∈ state.pending := filtered.1
    have different : candidate ≠ role := by simpa using filtered.2
    simp [DistributedRollMaterialization.materializeRole, different,
      valid.physicalPending candidate prior]
  · intro candidate bound absent
    by_cases same : candidate = role
    · subst candidate
      simp [DistributedRollMaterialization.materializeRole]
    · have priorAbsent : candidate ∉ state.pending := by
        intro prior
        apply absent
        apply List.mem_filter.mpr
        exact ⟨prior, by simpa using same⟩
      simp [DistributedRollMaterialization.materializeRole, same,
        valid.physicalDone candidate bound priorAbsent]

theorem distributed_roll_materialization_is_abstract_stutter
    (state : DistributedRollMaterialization)
    (role : Nat) :
    (state.materializeRole role).abstract = state.abstract := rfl

theorem distributed_roll_materialization_is_affine
    (state : DistributedRollMaterialization)
    (role : Nat) :
    role ∉ (state.materializeRole role).pending := by
  change role ∉ state.pending.filter fun candidate => candidate != role
  intro member
  have different := (List.mem_filter.mp member).2
  simp at different

theorem distributed_roll_materialization_accepts_exact_pending_role
    {state next : DistributedRollMaterialization}
    {role : Nat}
    (accepted : state.materializeRole? role = some next) :
    role ∈ state.pending /\ next = state.materializeRole role := by
  unfold DistributedRollMaterialization.materializeRole? at accepted
  by_cases pending : role ∈ state.pending
  · have exact : some (state.materializeRole role) = some next := by
      simpa [pending] using accepted
    exact ⟨pending, Option.some.inj exact |>.symm⟩
  · simp [pending] at accepted

theorem distributed_roll_materialization_duplicate_rejected
    {state next : DistributedRollMaterialization}
    {role : Nat}
    (accepted : state.materializeRole? role = some next) :
    next.materializeRole? role = none := by
  obtain ⟨_, rfl⟩ :=
    distributed_roll_materialization_accepts_exact_pending_role accepted
  simp [DistributedRollMaterialization.materializeRole?,
    distributed_roll_materialization_is_affine state role]

theorem distributed_roll_materialization_measure_decreases
    {state : DistributedRollMaterialization}
    (valid : state.Valid)
    {role : Nat}
    (pending : role ∈ state.pending) :
    (state.materializeRole role).pending.length + 1 = state.pending.length := by
  exact filter_not_equal_length_of_mem_nodup
    valid.pendingNodup pending

theorem distributed_roll_materialization_progress
    {state : DistributedRollMaterialization}
    (valid : state.Valid)
    (remaining : state.pending ≠ []) :
    ∃ role next,
      role ∈ state.pending /\
      state.materializeRole? role = some next /\
      next.Valid /\
      next.pending.length < state.pending.length := by
  obtain ⟨role, rest, pendingEq⟩ := List.exists_cons_of_ne_nil remaining
  have member : role ∈ state.pending := by simp [pendingEq]
  refine ⟨role, state.materializeRole role, member, ?_,
    distributed_roll_materialization_preserves_validity valid role, ?_⟩
  · simp [DistributedRollMaterialization.materializeRole?, member]
  have decrease := distributed_roll_materialization_measure_decreases valid member
  omega

theorem distributed_roll_materialization_complete_is_exact
    {state : DistributedRollMaterialization}
    (valid : state.Valid)
    (complete : state.pending = [])
    {role : Nat}
    (bound : role < state.after.roleCount) :
    state.physicalRoleState role = state.after.roleState role := by
  apply valid.physicalDone role bound
  simp [complete]

theorem distributed_roll_materialization_distinct_roles_commute
    (state : DistributedRollMaterialization)
    {left right : Nat}
    (different : left ≠ right) :
    (state.materializeRole left).materializeRole right =
      (state.materializeRole right).materializeRole left := by
  cases state
  simp only [DistributedRollMaterialization.materializeRole]
  congr 1
  · rw [List.filter_filter, List.filter_filter]
    congr 1
    funext candidate
    simp [Bool.and_comm]
  · funext candidate
    by_cases candidateLeft : candidate = left
    · subst candidate
      simp [different]
    · by_cases candidateRight : candidate = right
      · subst candidate
        simp [candidateLeft]
      · simp [candidateLeft, candidateRight]

/-- A successful global roll plus a drained carrier snapshot always admits the
finite, order-independent endpoint materialization protocol above. -/
theorem distributed_roll_linearization_has_materialization_refinement
    {before after : DistributedConfig}
    {rollId : Nat}
    {transports : List TransportState}
    (linearized : before.rollStep? rollId = some after)
    (drained : TransportSnapshotDrained transports) :
    ∃ state : DistributedRollMaterialization,
      state.Valid /\
      state.abstract = after.abstract /\
      state.pending.length = after.roleCount := by
  let state := DistributedRollMaterialization.initial before after rollId transports
  exact ⟨state,
    distributed_roll_materialization_initial_valid linearized drained,
    rfl,
    by simp [state, DistributedRollMaterialization.initial]⟩

private def pipelinedSingleSendRoll : Choreo :=
  .roll (.send 0 1 94 0)

private def globalAtomicRollBlocksPipelinedReentry : Bool :=
  match (GlobalConfig.initial 7 2 pipelinedSingleSendRoll).sendStep? 0 with
  | none => false
  | some queued => (queued.rollStep? 0).isNone

/-- The current atomic global model cannot simulate the production cursor's
legal FIFO output pipelining: after the first send is queued, global roll waits
for the remote receive before it can expose the next iteration. This theorem is
a checked limitation, not a runtime guarantee. -/
theorem global_atomic_roll_cannot_model_pipelined_single_send_reentry :
    globalAtomicRollBlocksPipelinedReentry = true := by
  decide

end Hibana
