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

/-- A `u16` slot identifier names 65,536 slots: the count is one greater than
the largest representable index. -/
def endpointLeaseSlotDomain : Nat := 2 ^ 16

abbrev endpointLeaseSlotCountRepresentable (slotCount : Nat) : Prop :=
  slotCount <= endpointLeaseSlotDomain

def LeaseAllocatorState.WellFormed (state : LeaseAllocatorState) : Prop :=
  endpointLeaseSlotCountRepresentable state.tableSlots

def endpointLeaseSlotCountCheck (slotCount : Nat) : Bool :=
  decide (endpointLeaseSlotCountRepresentable slotCount)

theorem endpoint_lease_slot_count_check_is_exact (slotCount : Nat) :
    endpointLeaseSlotCountCheck slotCount = true <->
      endpointLeaseSlotCountRepresentable slotCount := by
  simp [endpointLeaseSlotCountCheck]

theorem endpoint_lease_slot_domain_is_exact : endpointLeaseSlotDomain = 65536 := by
  decide

theorem endpoint_lease_full_slot_count_is_representable :
    endpointLeaseSlotCountRepresentable endpointLeaseSlotDomain := by
  exact Nat.le_refl _

theorem endpoint_lease_slot_count_above_domain_is_rejected :
    Not (endpointLeaseSlotCountRepresentable (endpointLeaseSlotDomain + 1)) := by
  simp [endpointLeaseSlotCountRepresentable]

theorem endpoint_lease_nonempty_count_has_representable_last_index
    {slotCount : Nat}
    (nonempty : 0 < slotCount)
    (representable : endpointLeaseSlotCountRepresentable slotCount) :
    slotCount - 1 < endpointLeaseSlotDomain := by
  unfold endpointLeaseSlotCountRepresentable at representable
  omega

/-- Allocation planning is read-only. Only a complete plan may enter commit. -/
def prepareLeaseAllocation
    (state : LeaseAllocatorState)
    (requiredTableSlots payloadBytes : Nat)
    (plannedImageFrontier payloadOffset : Option Nat) :
    Except LeaseAllocationFailure LeaseAllocationPlan :=
  match nextLeaseGeneration? state.generation with
  | none => .error .generationExhausted
  | some generation =>
      if endpointLeaseSlotCountRepresentable requiredTableSlots then
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
      else .error .tableUnavailable

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

theorem poisoned_generation_aborts_lease_publication
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    (cause : SessionFault) :
    (runLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset
      (SessionGenerationState.publicationPermitted (.poisoned cause))).1 = state /\
    ∀ plan,
      (runLeaseAllocation state requiredTableSlots payloadBytes
        plannedImageFrontier payloadOffset
        (SessionGenerationState.publicationPermitted (.poisoned cause))).2 ≠ .ok plan := by
  simp only [SessionGenerationState.publicationPermitted]
  unfold runLeaseAllocation
  split <;> simp_all

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
      by_cases capacity : endpointLeaseSlotCountRepresentable requiredTableSlots
      · cases plannedImageFrontier with
        | none => simp [advanced, capacity] at prepared
        | some imageFrontier =>
            cases payloadOffset with
            | none => simp [advanced, capacity] at prepared
            | some offset =>
                simp [advanced, capacity] at prepared
                subst plan
                exact next_lease_generation_strictly_increases advanced
      · simp [advanced, capacity] at prepared

theorem prepared_lease_required_slot_count_is_representable
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    {plan : LeaseAllocationPlan}
    (prepared : prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset = .ok plan) :
    endpointLeaseSlotCountRepresentable requiredTableSlots := by
  unfold prepareLeaseAllocation at prepared
  cases advanced : nextLeaseGeneration? state.generation with
  | none => simp [advanced] at prepared
  | some generation =>
      by_cases capacity : endpointLeaseSlotCountRepresentable requiredTableSlots
      · exact capacity
      · simp [advanced, capacity] at prepared

theorem prepared_lease_capacity_is_representable
    {state : LeaseAllocatorState}
    {requiredTableSlots payloadBytes : Nat}
    {plannedImageFrontier payloadOffset : Option Nat}
    {plan : LeaseAllocationPlan}
    (stateValid : state.WellFormed)
    (prepared : prepareLeaseAllocation state requiredTableSlots payloadBytes
      plannedImageFrontier payloadOffset = .ok plan) :
    endpointLeaseSlotCountRepresentable plan.tableSlots := by
  unfold prepareLeaseAllocation at prepared
  cases advanced : nextLeaseGeneration? state.generation with
  | none => simp [advanced] at prepared
  | some generation =>
      by_cases capacity : endpointLeaseSlotCountRepresentable requiredTableSlots
      · cases plannedImageFrontier with
        | none => simp [advanced, capacity] at prepared
        | some imageFrontier =>
            cases payloadOffset with
            | none => simp [advanced, capacity] at prepared
            | some offset =>
                simp [advanced, capacity] at prepared
                subst plan
                exact (Nat.max_le).2 ⟨stateValid, capacity⟩
      · simp [advanced, capacity] at prepared

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
      by_cases capacity : endpointLeaseSlotCountRepresentable requiredTableSlots
      · cases plannedImageFrontier with
        | none => simp [advanced, capacity] at prepared
        | some imageFrontier =>
            cases payloadOffset with
            | none => simp [advanced, capacity] at prepared
            | some offset =>
                simp [advanced, capacity] at prepared
                subst plan
                exact Nat.le_max_left _ _
      · simp [advanced, capacity] at prepared

structure LeaseSlotSnapshot where
  generation : Nat
  session : Nat
  role : Nat
  offset : Nat
  bytes : Nat
  state : Nat
  deriving DecidableEq

structure LeaseAssociationWitness where
  session : Nat
  lane : Nat
  attached : Bool
  deriving DecidableEq

structure LeaseAllocatorSnapshot where
  generation : Nat
  tableBytes : Nat
  assocBytes : Nat
  resolverBytes : Nat
  imageFrontier : Nat
  workspaceBytes : Nat
  endpointFloor : Nat
  activeLaneAttachments : Nat
  associationWitnesses : List LeaseAssociationWitness
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

structure LeaseAllocationAbortCertificate where
  before : LeaseAllocatorSnapshot
  after : LeaseAllocatorSnapshot

def LeaseAllocationAbortCertificate.PreservesAuthorityAndCapacity
    (certificate : LeaseAllocationAbortCertificate) : Prop :=
  certificate.after.generation = certificate.before.generation /\
  certificate.after.tableBytes = certificate.before.tableBytes /\
  certificate.after.assocBytes = certificate.before.assocBytes /\
  certificate.after.resolverBytes = certificate.before.resolverBytes /\
  certificate.after.workspaceBytes = certificate.before.workspaceBytes /\
  certificate.after.endpointFloor = certificate.before.endpointFloor /\
  certificate.after.activeLaneAttachments = certificate.before.activeLaneAttachments /\
  certificate.after.associationWitnesses = certificate.before.associationWitnesses /\
  certificate.after.slots = certificate.before.slots /\
  certificate.after.imageFrontier <= certificate.before.imageFrontier

def LeaseAllocationAbortCertificate.check
    (certificate : LeaseAllocationAbortCertificate) : Bool :=
  certificate.after.generation == certificate.before.generation &&
    (certificate.after.tableBytes == certificate.before.tableBytes &&
      (certificate.after.assocBytes == certificate.before.assocBytes &&
        (certificate.after.resolverBytes == certificate.before.resolverBytes &&
          (certificate.after.workspaceBytes == certificate.before.workspaceBytes &&
            (certificate.after.endpointFloor == certificate.before.endpointFloor &&
              (certificate.after.activeLaneAttachments ==
                  certificate.before.activeLaneAttachments &&
                (certificate.after.associationWitnesses ==
                    certificate.before.associationWitnesses &&
                  (certificate.after.slots == certificate.before.slots &&
                    certificate.after.imageFrontier <=
                      certificate.before.imageFrontier))))))))

theorem lease_allocation_abort_certificate_sound
    {certificate : LeaseAllocationAbortCertificate}
    (accepted : certificate.check = true) :
    certificate.PreservesAuthorityAndCapacity := by
  simpa [LeaseAllocationAbortCertificate.check,
    LeaseAllocationAbortCertificate.PreservesAuthorityAndCapacity] using accepted

end Hibana
