import Hibana.SessionComposition

namespace Hibana

/-- A pool invariant combines unique session authority with the global protocol
invariant of every live or retiring session generation. -/
def SessionPool.Invariant (pool : SessionPool) : Prop :=
  pool.WellFormed /\
    ∀ sessionId config,
      pool.session sessionId = some config -> GlobalInvariant config

def SessionPool.withInitial
    (pool : SessionPool)
    (sessionId roleCount : Nat)
    (choreo : Choreo) : SessionPool := {
  active := sessionId :: pool.active
  session := fun candidate =>
    if candidate = sessionId then
      some (GlobalConfig.initial sessionId roleCount choreo)
    else
      pool.session candidate
}

/-- Attach is fail-closed even for a malformed authority map: both the finite
domain and the backing lookup must report the identity as absent. -/
def SessionPool.attachInitial?
    (pool : SessionPool)
    (sessionId roleCount : Nat)
    (choreo : Choreo) : Option SessionPool :=
  if sessionId ∈ pool.active then
    none
  else
    match pool.session sessionId with
    | some _ => none
    | none => some (pool.withInitial sessionId roleCount choreo)

theorem attach_initial_exact
    {pool next : SessionPool}
    {sessionId roleCount : Nat}
    {choreo : Choreo}
    (attached : pool.attachInitial? sessionId roleCount choreo = some next) :
    sessionId ∉ pool.active /\
      pool.session sessionId = none /\
      next = pool.withInitial sessionId roleCount choreo := by
  unfold SessionPool.attachInitial? at attached
  by_cases active : sessionId ∈ pool.active
  · simp [active] at attached
  · rw [if_neg active] at attached
    cases present : pool.session sessionId with
    | none =>
        exact ⟨active, rfl,
          (Option.some.inj (by simpa [present] using attached)).symm⟩
    | some config => simp [present] at attached

theorem with_initial_preserves_other_session
    (pool : SessionPool)
    {sessionId other roleCount : Nat}
    {choreo : Choreo}
    (different : other ≠ sessionId) :
    (pool.withInitial sessionId roleCount choreo).session other =
      pool.session other := by
  simp [SessionPool.withInitial, different]

theorem with_initial_preserves_well_formed
    {pool : SessionPool}
    {sessionId roleCount : Nat}
    {choreo : Choreo}
    (wellFormed : pool.WellFormed)
    (fresh : sessionId ∉ pool.active) :
    (pool.withInitial sessionId roleCount choreo).WellFormed := by
  refine ⟨List.nodup_cons.mpr ⟨fresh, wellFormed.1⟩, ?_, ?_⟩
  · intro candidate member
    by_cases same : candidate = sessionId
    · subst candidate
      exact ⟨GlobalConfig.initial sessionId roleCount choreo,
        by simp [SessionPool.withInitial], rfl⟩
    · have originalMember : candidate ∈ pool.active := by
        change candidate ∈ sessionId :: pool.active at member
        simpa [same] using member
      obtain ⟨config, present, identity⟩ :=
        wellFormed.2.1 candidate originalMember
      exact ⟨config, by simpa [SessionPool.withInitial, same] using present,
        identity⟩
  · intro candidate config present
    by_cases same : candidate = sessionId
    · subst candidate
      have configExact : GlobalConfig.initial sessionId roleCount choreo = config :=
        Option.some.inj (by
          simpa [SessionPool.withInitial] using present)
      subst config
      exact ⟨List.mem_cons_self, rfl⟩
    · have original : pool.session candidate = some config := by
        simpa [SessionPool.withInitial, same] using present
      obtain ⟨active, identity⟩ := wellFormed.2.2 candidate config original
      exact ⟨List.mem_cons.mpr (Or.inr active), identity⟩

theorem attach_initial_preserves_invariant
    {pool next : SessionPool}
    {sessionId roleCount : Nat}
    {choreo : Choreo}
    (invariant : pool.Invariant)
    (roleCountPositive : 0 < roleCount)
    (programWellFormed : GlobalProgramWellFormed roleCount choreo)
    (attached : pool.attachInitial? sessionId roleCount choreo = some next) :
    next.Invariant := by
  obtain ⟨fresh, _, nextExact⟩ := attach_initial_exact attached
  subst next
  refine ⟨with_initial_preserves_well_formed invariant.1 fresh, ?_⟩
  intro candidate config present
  by_cases same : candidate = sessionId
  · subst candidate
    have configExact : GlobalConfig.initial sessionId roleCount choreo = config :=
      Option.some.inj (by
        simpa [SessionPool.withInitial] using present)
    subst config
    exact initial_global_invariant sessionId roleCount choreo
      roleCountPositive programWellFormed
  · have original : pool.session candidate = some config := by
      simpa [SessionPool.withInitial, same] using present
    exact invariant.2 candidate config original

def SessionPool.withoutSession
    (pool : SessionPool) (sessionId : Nat) : SessionPool := {
  active := pool.active.filter fun candidate => candidate != sessionId
  session := fun candidate =>
    if candidate = sessionId then none else pool.session candidate
}

/-- Resource authority may disappear only after the global session has reached
its retired state. Cancelling or merely absent participants are not enough. -/
def SessionPool.retire?
    (pool : SessionPool) (sessionId : Nat) : Option SessionPool :=
  match pool.session sessionId with
  | none => none
  | some config =>
      match config.status with
      | .retired _ => some (pool.withoutSession sessionId)
      | .live | .cancelling _ _ => none

theorem retire_session_exact
    {pool next : SessionPool} {sessionId : Nat}
    (retired : pool.retire? sessionId = some next) :
    ∃ config cause,
      pool.session sessionId = some config /\
      config.status = .retired cause /\
      next = pool.withoutSession sessionId := by
  unfold SessionPool.retire? at retired
  cases present : pool.session sessionId with
  | none => simp [present] at retired
  | some config =>
      cases status : config.status with
      | live => simp [present, status] at retired
      | cancelling cause awaiting => simp [present, status] at retired
      | retired cause =>
          exact ⟨config, cause, rfl, status,
            (Option.some.inj (by simpa [present, status] using retired)).symm⟩

theorem without_session_clears_identity
    (pool : SessionPool) (sessionId : Nat) :
    (pool.withoutSession sessionId).session sessionId = none /\
      sessionId ∉ (pool.withoutSession sessionId).active := by
  simp [SessionPool.withoutSession]

theorem without_session_preserves_other_session
    (pool : SessionPool) {sessionId other : Nat}
    (different : other ≠ sessionId) :
    (pool.withoutSession sessionId).session other = pool.session other := by
  simp [SessionPool.withoutSession, different]

theorem without_session_preserves_well_formed
    {pool : SessionPool} {sessionId : Nat}
    (wellFormed : pool.WellFormed) :
    (pool.withoutSession sessionId).WellFormed := by
  refine ⟨wellFormed.1.filter _, ?_, ?_⟩
  · intro candidate member
    have filtered : candidate ∈ pool.active /\ candidate ≠ sessionId := by
      simpa [SessionPool.withoutSession, and_comm] using member
    obtain ⟨config, present, identity⟩ :=
      wellFormed.2.1 candidate filtered.1
    exact ⟨config,
      by simpa [SessionPool.withoutSession, filtered.2] using present,
      identity⟩
  · intro candidate config present
    have different : candidate ≠ sessionId := by
      intro same
      subst candidate
      simp [SessionPool.withoutSession] at present
    have original : pool.session candidate = some config := by
      simpa [SessionPool.withoutSession, different] using present
    obtain ⟨active, identity⟩ := wellFormed.2.2 candidate config original
    exact ⟨by simpa [SessionPool.withoutSession, different] using active,
      identity⟩

theorem retire_session_preserves_invariant
    {pool next : SessionPool} {sessionId : Nat}
    (invariant : pool.Invariant)
    (retired : pool.retire? sessionId = some next) :
    next.Invariant := by
  obtain ⟨_, _, _, _, nextExact⟩ := retire_session_exact retired
  subst next
  refine ⟨without_session_preserves_well_formed invariant.1, ?_⟩
  intro candidate config present
  have different : candidate ≠ sessionId := by
    intro same
    subst candidate
    simp [SessionPool.withoutSession] at present
  have original : pool.session candidate = some config := by
    simpa [SessionPool.withoutSession, different] using present
  exact invariant.2 candidate config original

theorem retired_session_allows_fresh_attach
    {pool retiredPool : SessionPool}
    {sessionId roleCount : Nat}
    {choreo : Choreo}
    (retired : pool.retire? sessionId = some retiredPool) :
    retiredPool.attachInitial? sessionId roleCount choreo =
      some (retiredPool.withInitial sessionId roleCount choreo) := by
  obtain ⟨_, _, _, _, retiredExact⟩ := retire_session_exact retired
  subst retiredPool
  obtain ⟨missing, inactive⟩ := without_session_clears_identity pool sessionId
  simp [SessionPool.attachInitial?, missing, inactive]

theorem with_session_updates_commute
    (pool : SessionPool)
    {left right : Nat}
    (different : left ≠ right)
    (leftConfig rightConfig : GlobalConfig) :
    (pool.withSession left leftConfig).withSession right rightConfig =
      (pool.withSession right rightConfig).withSession left leftConfig := by
  cases pool with
  | mk active session =>
      simp only [SessionPool.withSession]
      rw [SessionPool.mk.injEq]
      refine ⟨rfl, funext ?_⟩
      intro candidate
      by_cases isLeft : candidate = left
      · subst candidate
        simp [different]
      · by_cases isRight : candidate = right
        · subst candidate
          simp [isLeft]
        · simp [isLeft, isRight]

def SessionPoolOperation.targetSession : SessionPoolOperation -> Nat
  | .protocol sessionId _ => sessionId
  | .fault sessionId _ _ => sessionId
  | .observeCancellation sessionId _ => sessionId

/-- A successful pool operation depends only on its target session. Replaying
it in any pool with the same target binding produces the same replacement and
preserves every other session in that pool. -/
theorem session_pool_step_is_target_local
    {pool next : SessionPool}
    {operation : SessionPoolOperation}
    (stepped : pool.step? operation = some next) :
    ∃ replacement,
      next = pool.withSession operation.targetSession replacement /\
      ∀ context : SessionPool,
        context.session operation.targetSession =
          pool.session operation.targetSession ->
        context.step? operation =
          some (context.withSession operation.targetSession replacement) := by
  cases operation with
  | protocol sessionId protocolOperation =>
      change pool.protocolStep? sessionId protocolOperation = some next at stepped
      unfold SessionPool.protocolStep? at stepped
      cases currentAt : pool.session sessionId with
      | none => simp [currentAt] at stepped
      | some current =>
          cases replacementAt : current.step? protocolOperation with
          | none => simp [currentAt, replacementAt] at stepped
          | some replacement =>
              by_cases identity : replacement.session = sessionId
              · have nextExact : next = pool.withSession sessionId replacement :=
                  (Option.some.inj (by
                    simpa [currentAt, replacementAt, identity] using stepped)).symm
                refine ⟨replacement, nextExact, ?_⟩
                intro context sameBinding
                have contextAt : context.session sessionId = some current :=
                  sameBinding.trans currentAt
                simp [SessionPool.step?, SessionPool.protocolStep?,
                  SessionPoolOperation.targetSession, contextAt, replacementAt,
                  identity]
              · simp [currentAt, replacementAt, identity] at stepped
  | fault sessionId reporter fault =>
      change pool.faultStep? sessionId reporter fault = some next at stepped
      unfold SessionPool.faultStep? at stepped
      cases currentAt : pool.session sessionId with
      | none => simp [currentAt] at stepped
      | some current =>
          by_cases identity : current.session = sessionId
          · let replacement :=
              current.beginCancellation reporter fault.sessionCause
            have nextExact : next = pool.withSession sessionId replacement :=
              (Option.some.inj (by
                simpa [currentAt, identity, replacement] using stepped)).symm
            refine ⟨replacement, nextExact, ?_⟩
            intro context sameBinding
            have contextAt : context.session sessionId = some current :=
              sameBinding.trans currentAt
            simp [SessionPool.step?, SessionPool.faultStep?,
              SessionPoolOperation.targetSession, contextAt, identity, replacement]
          · simp [currentAt, identity] at stepped
  | observeCancellation sessionId role =>
      change pool.observeCancellationStep? sessionId role = some next at stepped
      unfold SessionPool.observeCancellationStep? at stepped
      cases currentAt : pool.session sessionId with
      | none => simp [currentAt] at stepped
      | some current =>
          cases replacementAt : current.observeCancellation? role with
          | none => simp [currentAt, replacementAt] at stepped
          | some replacement =>
              by_cases identity : replacement.session = sessionId
              · have nextExact : next = pool.withSession sessionId replacement :=
                  (Option.some.inj (by
                    simpa [currentAt, replacementAt, identity] using stepped)).symm
                refine ⟨replacement, nextExact, ?_⟩
                intro context sameBinding
                have contextAt : context.session sessionId = some current :=
                  sameBinding.trans currentAt
                simp [SessionPool.step?, SessionPool.observeCancellationStep?,
                  SessionPoolOperation.targetSession, contextAt, replacementAt,
                  identity]
              · simp [currentAt, replacementAt, identity] at stepped

/-- Every operation on distinct session instances commutes, including protocol
progress, first fault, and cancellation observation. Dynamic streams and RPC
rounds are runtime `SessionId` values, not new Rust continuation types. -/
theorem distinct_session_steps_commute
    {pool leftNext rightNext : SessionPool}
    {leftOperation rightOperation : SessionPoolOperation}
    (different : leftOperation.targetSession ≠ rightOperation.targetSession)
    (leftStepped : pool.step? leftOperation = some leftNext)
    (rightStepped : pool.step? rightOperation = some rightNext) :
    ∃ both,
      leftNext.step? rightOperation = some both /\
      rightNext.step? leftOperation = some both := by
  obtain ⟨leftReplacement, leftExact, leftLocal⟩ :=
    session_pool_step_is_target_local leftStepped
  obtain ⟨rightReplacement, rightExact, rightLocal⟩ :=
    session_pool_step_is_target_local rightStepped
  subst leftNext
  subst rightNext
  let both :=
    (pool.withSession leftOperation.targetSession leftReplacement).withSession
      rightOperation.targetSession rightReplacement
  refine ⟨both, ?_, ?_⟩
  · have sameBinding := with_session_preserves_other_session pool
      (Ne.symm different) leftReplacement
    simpa [both] using rightLocal
      (pool.withSession leftOperation.targetSession leftReplacement) sameBinding
  · have sameBinding := with_session_preserves_other_session pool
      different rightReplacement
    have replayed := leftLocal
      (pool.withSession rightOperation.targetSession rightReplacement) sameBinding
    have commute := with_session_updates_commute pool different
      leftReplacement rightReplacement
    simpa [both, commute] using replayed

end Hibana
