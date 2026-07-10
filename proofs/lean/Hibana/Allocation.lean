import Hibana.Generation

namespace Hibana

inductive LeaseAllocationFailure where
  | generationExhausted
  | tableUnavailable
  | payloadUnavailable
  | initializationRejected
  deriving DecidableEq

structure LeaseAllocatorState where
  generation : Nat
  tableSlots : Nat
  imageFrontier : Nat
  deriving DecidableEq

structure LeaseAllocationPlan where
  generation : Nat
  tableSlots : Nat
  imageFrontier : Nat
  payloadOffset : Nat
  payloadBytes : Nat
  deriving DecidableEq

/-- Allocation planning is read-only. Only a complete plan may enter commit. -/
def prepareLeaseAllocation
    (state : LeaseAllocatorState)
    (requiredTableSlots payloadBytes : Nat)
    (plannedImageFrontier payloadOffset : Option Nat) :
    Except LeaseAllocationFailure LeaseAllocationPlan :=
  match nextLeaseGeneration? state.generation with
  | none => .error .generationExhausted
  | some generation =>
      match plannedImageFrontier with
      | none => .error .tableUnavailable
      | some imageFrontier =>
          match payloadOffset with
          | none => .error .payloadUnavailable
          | some offset => .ok {
              generation
              tableSlots := max state.tableSlots requiredTableSlots
              imageFrontier
              payloadOffset := offset
              payloadBytes
            }

def commitLeaseAllocation
    (_state : LeaseAllocatorState) (plan : LeaseAllocationPlan) : LeaseAllocatorState := {
  generation := plan.generation
  tableSlots := plan.tableSlots
  imageFrontier := plan.imageFrontier
}

def runLeaseAllocation
    (state : LeaseAllocatorState)
    (requiredTableSlots payloadBytes : Nat)
    (plannedImageFrontier payloadOffset : Option Nat)
    (publishReady : Bool) :
    LeaseAllocatorState × Except LeaseAllocationFailure LeaseAllocationPlan :=
  match prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset with
  | .error failure => (state, .error failure)
  | .ok plan =>
      if publishReady then (commitLeaseAllocation state plan, .ok plan)
      else (state, .error .initializationRejected)

theorem failed_lease_allocation_preserves_state
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    {publishReady : Bool}
    {failure : LeaseAllocationFailure}
    (failed :
      (runLeaseAllocation state requiredTableSlots payloadBytes
        plannedImageFrontier payloadOffset publishReady).2 = .error failure) :
    (runLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset publishReady).1 = state := by
  unfold runLeaseAllocation at failed ⊢
  generalize prepared : prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset = result at failed ⊢
  cases result <;> cases publishReady <;> simp_all

theorem successful_lease_allocation_commits_exact_plan
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    {publishReady : Bool}
    {plan : LeaseAllocationPlan}
    (accepted :
      (runLeaseAllocation state requiredTableSlots payloadBytes
        plannedImageFrontier payloadOffset publishReady).2 = .ok plan) :
    (runLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset publishReady).1 = commitLeaseAllocation state plan := by
  unfold runLeaseAllocation at accepted ⊢
  generalize prepared : prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset = result at accepted ⊢
  cases result <;> cases publishReady <;> simp_all

theorem prepared_lease_generation_strictly_increases
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    {plan : LeaseAllocationPlan}
    (prepared : prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset = .ok plan) :
    state.generation < plan.generation := by
  unfold prepareLeaseAllocation at prepared
  cases advanced : nextLeaseGeneration? state.generation with
  | none => simp [advanced] at prepared
  | some generation =>
      cases plannedImageFrontier with
      | none => simp [advanced] at prepared
      | some imageFrontier =>
          cases payloadOffset with
          | none => simp [advanced] at prepared
          | some offset =>
              simp [advanced] at prepared
              subst plan
              exact next_lease_generation_strictly_increases advanced

theorem prepared_lease_capacity_never_shrinks
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    {plan : LeaseAllocationPlan}
    (prepared : prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset = .ok plan) :
    state.tableSlots <= plan.tableSlots := by
  unfold prepareLeaseAllocation at prepared
  cases advanced : nextLeaseGeneration? state.generation with
  | none => simp [advanced] at prepared
  | some generation =>
      cases plannedImageFrontier with
      | none => simp [advanced] at prepared
      | some imageFrontier =>
          cases payloadOffset with
          | none => simp [advanced] at prepared
          | some offset =>
              simp [advanced] at prepared
              subst plan
              exact Nat.le_max_left _ _

structure LeaseSlotSnapshot where
  generation : Nat
  session : Nat
  role : Nat
  offset : Nat
  bytes : Nat
  state : Nat
  deriving DecidableEq

structure LeaseAllocatorSnapshot where
  generation : Nat
  tableBytes : Nat
  assocBytes : Nat
  routeBytes : Nat
  resolverBytes : Nat
  imageFrontier : Nat
  workspaceBytes : Nat
  endpointFloor : Nat
  slots : List LeaseSlotSnapshot
  deriving DecidableEq

structure LeaseAllocationFailureCertificate where
  before : LeaseAllocatorSnapshot
  after : LeaseAllocatorSnapshot

def LeaseAllocationFailureCertificate.PreservesState
    (certificate : LeaseAllocationFailureCertificate) : Prop :=
  certificate.after = certificate.before

def LeaseAllocationFailureCertificate.check
    (certificate : LeaseAllocationFailureCertificate) : Bool :=
  decide (certificate.after = certificate.before)

theorem lease_allocation_failure_certificate_sound
    {certificate : LeaseAllocationFailureCertificate}
    (accepted : certificate.check = true) : certificate.PreservesState := by
  simpa [LeaseAllocationFailureCertificate.check,
    LeaseAllocationFailureCertificate.PreservesState] using accepted

end Hibana
