import Hibana.AsyncCancellation
import Hibana.DistributedSemantics

namespace Hibana

/-- Runtime fault sources before they are collapsed into the five persistent
session causes encoded by the Rust association table. -/
inductive RuntimeFault where
  | transportOffline
  | transportDeadline
  | transportCapacity
  | transportFailed
  | decodeTruncated
  | decodeMalformed
  | encodeFailed
  | labelMismatch
  | schemaMismatch
  | resolverRejected
  | endpointDropped
  | phaseInvariant
  deriving Repr, DecidableEq

def RuntimeFault.sessionCause : RuntimeFault -> SessionFault
  | .transportOffline
  | .transportDeadline
  | .transportCapacity
  | .transportFailed => .transportClosed
  | .decodeTruncated
  | .decodeMalformed => .decodeFailed
  | .encodeFailed
  | .labelMismatch
  | .schemaMismatch
  | .resolverRejected => .protocolViolation
  | .endpointDropped => .endpointDropped
  | .phaseInvariant => .progressInvariantViolated

theorem runtime_fault_classification_is_total (fault : RuntimeFault) :
    fault.sessionCause = .transportClosed \/
    fault.sessionCause = .decodeFailed \/
    fault.sessionCause = .protocolViolation \/
    fault.sessionCause = .endpointDropped \/
    fault.sessionCause = .progressInvariantViolated := by
  cases fault <;> simp [RuntimeFault.sessionCause]

/-- One authority map for all resident session generations sharing a slab or
carrier. `active` is the finite authority-domain enumeration and may include a
retired generation until its owner removes that entry. -/
structure SessionPool where
  active : List Nat
  session : Nat -> Option GlobalConfig

def SessionPool.WellFormed (pool : SessionPool) : Prop :=
  pool.active.Nodup /\
  (∀ sessionId, sessionId ∈ pool.active ->
    ∃ config, pool.session sessionId = some config /\ config.session = sessionId) /\
  (∀ sessionId config, pool.session sessionId = some config ->
    sessionId ∈ pool.active /\ config.session = sessionId)

def SessionPool.withSession
    (pool : SessionPool)
    (sessionId : Nat)
    (config : GlobalConfig) : SessionPool := {
  pool with
  session := fun candidate =>
    if candidate = sessionId then some config else pool.session candidate
}

def SessionPool.protocolStep?
    (pool : SessionPool)
    (sessionId : Nat)
    (operation : GlobalOperation) : Option SessionPool :=
  match pool.session sessionId with
  | none => none
  | some config =>
      match config.step? operation with
      | none => none
      | some next =>
          if next.session = sessionId then
            some (pool.withSession sessionId next)
          else
            none

def SessionPool.faultStep?
    (pool : SessionPool)
    (sessionId reporter : Nat)
    (fault : RuntimeFault) : Option SessionPool :=
  match pool.session sessionId with
  | none => none
  | some config =>
      if config.session = sessionId then
        some (pool.withSession sessionId
          (config.beginCancellation reporter fault.sessionCause))
      else
        none

def SessionPool.observeCancellationStep?
    (pool : SessionPool)
    (sessionId role : Nat) : Option SessionPool :=
  match pool.session sessionId with
  | none => none
  | some config =>
      match config.observeCancellation? role with
      | none => none
      | some next =>
          if next.session = sessionId then
            some (pool.withSession sessionId next)
          else
            none

inductive SessionPoolOperation where
  | protocol (sessionId : Nat) (operation : GlobalOperation)
  | fault (sessionId reporter : Nat) (fault : RuntimeFault)
  | observeCancellation (sessionId role : Nat)
  deriving Repr, DecidableEq

def SessionPool.step? (pool : SessionPool) : SessionPoolOperation -> Option SessionPool
  | .protocol sessionId operation => pool.protocolStep? sessionId operation
  | .fault sessionId reporter fault => pool.faultStep? sessionId reporter fault
  | .observeCancellation sessionId role =>
      pool.observeCancellationStep? sessionId role

theorem with_session_preserves_other_session
    (pool : SessionPool)
    {target other : Nat}
    (different : other ≠ target)
    (replacement : GlobalConfig) :
    (pool.withSession target replacement).session other = pool.session other := by
  simp [SessionPool.withSession, different]

theorem with_session_preserves_well_formed
    {pool : SessionPool}
    {target : Nat}
    {replacement : GlobalConfig}
    (wellFormed : pool.WellFormed)
    (active : target ∈ pool.active)
    (identity : replacement.session = target) :
    (pool.withSession target replacement).WellFormed := by
  refine ⟨wellFormed.1, ?_, ?_⟩
  · intro sessionId member
    by_cases same : sessionId = target
    · subst sessionId
      exact ⟨replacement, by simp [SessionPool.withSession], identity⟩
    · obtain ⟨config, present, configIdentity⟩ := wellFormed.2.1 sessionId member
      exact ⟨config, by simpa [SessionPool.withSession, same] using present,
        configIdentity⟩
  · intro sessionId config present
    by_cases same : sessionId = target
    · subst sessionId
      have configExact : replacement = config := by
        exact Option.some.inj (by
          simpa [SessionPool.withSession] using present)
      subst config
      exact ⟨active, identity⟩
    · have original : pool.session sessionId = some config := by
        simpa [SessionPool.withSession, same] using present
      exact wellFormed.2.2 sessionId config original

theorem protocol_step_preserves_other_session
    {pool next : SessionPool}
    {target other : Nat}
    {operation : GlobalOperation}
    (different : other ≠ target)
    (stepped : pool.protocolStep? target operation = some next) :
    next.session other = pool.session other := by
  unfold SessionPool.protocolStep? at stepped
  cases sessionCase : pool.session target with
  | none => simp [sessionCase] at stepped
  | some config =>
      cases operationCase : config.step? operation with
      | none => simp [sessionCase, operationCase] at stepped
      | some replacement =>
          by_cases identity : replacement.session = target
          · have nextEq : pool.withSession target replacement = next :=
              Option.some.inj (by
                simpa [sessionCase, operationCase, identity] using stepped)
            subst next
            exact with_session_preserves_other_session pool different replacement
          · simp [sessionCase, operationCase, identity] at stepped

theorem fault_step_preserves_other_session
    {pool next : SessionPool}
    {target reporter other : Nat}
    {fault : RuntimeFault}
    (different : other ≠ target)
    (stepped : pool.faultStep? target reporter fault = some next) :
    next.session other = pool.session other := by
  unfold SessionPool.faultStep? at stepped
  cases sessionCase : pool.session target with
  | none => simp [sessionCase] at stepped
  | some config =>
      by_cases identity : config.session = target
      · have nextEq :
            pool.withSession target
              (config.beginCancellation reporter fault.sessionCause) = next :=
          Option.some.inj (by simpa [sessionCase, identity] using stepped)
        subst next
        exact with_session_preserves_other_session pool different _
      · simp [sessionCase, identity] at stepped

theorem cancellation_observation_preserves_other_session
    {pool next : SessionPool}
    {target role other : Nat}
    (different : other ≠ target)
    (stepped : pool.observeCancellationStep? target role = some next) :
    next.session other = pool.session other := by
  unfold SessionPool.observeCancellationStep? at stepped
  cases sessionCase : pool.session target with
  | none => simp [sessionCase] at stepped
  | some config =>
      cases observationCase : config.observeCancellation? role with
      | none => simp [sessionCase, observationCase] at stepped
      | some replacement =>
          by_cases identity : replacement.session = target
          · have nextEq : pool.withSession target replacement = next :=
              Option.some.inj (by
                simpa [sessionCase, observationCase, identity] using stepped)
            subst next
            exact with_session_preserves_other_session pool different replacement
          · simp [sessionCase, observationCase, identity] at stepped

theorem protocol_step_preserves_well_formed
    {pool next : SessionPool}
    {sessionId : Nat}
    {operation : GlobalOperation}
    (wellFormed : pool.WellFormed)
    (stepped : pool.protocolStep? sessionId operation = some next) :
    next.WellFormed := by
  unfold SessionPool.protocolStep? at stepped
  cases sessionCase : pool.session sessionId with
  | none => simp [sessionCase] at stepped
  | some config =>
      have authority := wellFormed.2.2 sessionId config sessionCase
      cases operationCase : config.step? operation with
      | none => simp [sessionCase, operationCase] at stepped
      | some replacement =>
          have protocolIdentity := global_step_preserves_protocol_identity operationCase
          have replacementIdentity : replacement.session = sessionId := by
            have sessionPreserved :=
              congrArg GlobalProtocolIdentity.session protocolIdentity
            exact sessionPreserved.trans authority.2
          have nextEq : pool.withSession sessionId replacement = next :=
            Option.some.inj (by
              simpa [sessionCase, operationCase, replacementIdentity] using stepped)
          subst next
          exact with_session_preserves_well_formed wellFormed authority.1
            replacementIdentity

theorem fault_step_preserves_well_formed
    {pool next : SessionPool}
    {sessionId reporter : Nat}
    {fault : RuntimeFault}
    (wellFormed : pool.WellFormed)
    (stepped : pool.faultStep? sessionId reporter fault = some next) :
    next.WellFormed := by
  unfold SessionPool.faultStep? at stepped
  cases sessionCase : pool.session sessionId with
  | none => simp [sessionCase] at stepped
  | some config =>
      have authority := wellFormed.2.2 sessionId config sessionCase
      have identity : config.session = sessionId := authority.2
      have nextEq :
          pool.withSession sessionId
            (config.beginCancellation reporter fault.sessionCause) = next :=
        Option.some.inj (by simpa [sessionCase, identity] using stepped)
      subst next
      apply with_session_preserves_well_formed wellFormed authority.1
      cases statusCase : config.status with
      | live =>
          cases pendingCase : removeAwaitingRole
              (List.range config.roleCount) reporter <;>
            simp [GlobalConfig.beginCancellation, statusCase, pendingCase, identity]
      | cancelling cause awaiting =>
          simpa [GlobalConfig.beginCancellation, statusCase] using identity
      | retired cause =>
          simpa [GlobalConfig.beginCancellation, statusCase] using identity

theorem cancellation_observation_preserves_pool_well_formed
    {pool next : SessionPool}
    {sessionId role : Nat}
    (wellFormed : pool.WellFormed)
    (stepped : pool.observeCancellationStep? sessionId role = some next) :
    next.WellFormed := by
  unfold SessionPool.observeCancellationStep? at stepped
  cases sessionCase : pool.session sessionId with
  | none => simp [sessionCase] at stepped
  | some config =>
      have authority := wellFormed.2.2 sessionId config sessionCase
      cases observationCase : config.observeCancellation? role with
      | none => simp [sessionCase, observationCase] at stepped
      | some replacement =>
          have replacementIdentity : replacement.session = sessionId :=
            (cancellation_observation_preserves_protocol observationCase).1.trans
              authority.2
          have nextEq : pool.withSession sessionId replacement = next :=
            Option.some.inj (by
              simpa [sessionCase, observationCase, replacementIdentity] using stepped)
          subst next
          exact with_session_preserves_well_formed wellFormed authority.1
            replacementIdentity

theorem session_pool_step_preserves_other_session
    {pool next : SessionPool}
    {operation : SessionPoolOperation}
    (stepped : pool.step? operation = some next) :
    ∀ other,
      (match operation with
        | .protocol target _ => other ≠ target
        | .fault target _ _ => other ≠ target
        | .observeCancellation target _ => other ≠ target) ->
      next.session other = pool.session other := by
  cases operation with
  | protocol target protocolOperation =>
      exact fun other different =>
        protocol_step_preserves_other_session different stepped
  | fault target reporter fault =>
      exact fun other different =>
        fault_step_preserves_other_session different stepped
  | observeCancellation target role =>
      exact fun other different =>
        cancellation_observation_preserves_other_session different stepped

theorem session_pool_step_preserves_well_formed
    {pool next : SessionPool}
    {operation : SessionPoolOperation}
    (wellFormed : pool.WellFormed)
    (stepped : pool.step? operation = some next) :
    next.WellFormed := by
  cases operation with
  | protocol sessionId protocolOperation =>
      exact protocol_step_preserves_well_formed wellFormed stepped
  | fault sessionId reporter fault =>
      exact fault_step_preserves_well_formed wellFormed stepped
  | observeCancellation sessionId role =>
      exact cancellation_observation_preserves_pool_well_formed wellFormed stepped

/-- Exact preservation of another session entails non-interference for every
queue, cursor, route/resolver selection, and participant resource owner field. -/
theorem session_pool_step_preserves_other_session_state
    {pool next : SessionPool}
    {operation : SessionPoolOperation}
    {other : Nat}
    {before : GlobalConfig}
    (present : pool.session other = some before)
    (different :
      match operation with
      | .protocol target _ => other ≠ target
      | .fault target _ _ => other ≠ target
      | .observeCancellation target _ => other ≠ target)
    (stepped : pool.step? operation = some next) :
    next.session other = some before := by
  rw [session_pool_step_preserves_other_session stepped other different]
  exact present

theorem fault_step_preserves_first_fault
    {pool next : SessionPool}
    {sessionId reporter : Nat}
    {fault : RuntimeFault}
    {config replacement : GlobalConfig}
    {existing : SessionFault}
    {awaiting : List Nat}
    (present : pool.session sessionId = some config)
    (started : config.status = .cancelling existing awaiting \/
      config.status = .retired existing)
    (stepped : pool.faultStep? sessionId reporter fault = some next)
    (replacementAt : next.session sessionId = some replacement) :
    replacement.status = config.status := by
  unfold SessionPool.faultStep? at stepped
  rw [present] at stepped
  by_cases identity : config.session = sessionId
  · have nextEq :
        pool.withSession sessionId
          (config.beginCancellation reporter fault.sessionCause) = next :=
      Option.some.inj (by simpa [identity] using stepped)
    subst next
    have exactReplacement :
        config.beginCancellation reporter fault.sessionCause = replacement := by
      have someExact :
          some (config.beginCancellation reporter fault.sessionCause) =
            some replacement := by
        simpa [SessionPool.withSession] using replacementAt
      exact Option.some.inj someExact
    subst replacement
    exact begin_cancellation_preserves_first_fault started
  · simp [identity] at stepped

end Hibana
