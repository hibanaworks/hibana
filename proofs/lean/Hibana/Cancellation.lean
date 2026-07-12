import Hibana.GlobalFidelity

namespace Hibana

def GlobalConfig.observeCancellation?
    (config : GlobalConfig) (role : Nat) : Option GlobalConfig :=
  match config.status with
  | .live | .retired _ => none
  | .cancelling cause awaiting =>
      if role ∈ awaiting then
        match removeAwaitingRole awaiting role with
        | [] => some {
            config with
            status := .retired cause
            resources := .empty
          }
        | remaining => some {
            config with
            status := .cancelling cause remaining
            resources := .forRoles remaining
          }
      else
        none

def GlobalConfig.cancellationMeasure (config : GlobalConfig) : Nat :=
  match config.status with
  | .live => config.roleCount + 1
  | .cancelling _ awaiting => awaiting.length
  | .retired _ => 0

def GlobalConfig.resourcesRetired (config : GlobalConfig) : Prop :=
  ∃ cause, config.status = .retired cause ∧
    (∀ globalId, config.queue globalId = none) ∧
    config.resources = .empty

theorem begin_cancellation_preserves_first_fault
    {config : GlobalConfig} {reporter : Nat} {cause existing : SessionFault}
    {awaiting : List Nat}
    (started : config.status = .cancelling existing awaiting ∨
      config.status = .retired existing) :
    (config.beginCancellation reporter cause).status = config.status := by
  rcases started with cancelling | retired
  · simp [GlobalConfig.beginCancellation, cancelling]
  · simp [GlobalConfig.beginCancellation, retired]

theorem begin_cancellation_quarantines_all_messages
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (live : config.status = .live) (globalId : Nat) :
    (config.beginCancellation reporter cause).queuedMessage? globalId = none := by
  cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter with
  | nil =>
      simp [GlobalConfig.beginCancellation, GlobalConfig.queuedMessage?,
        live, pendingCase]
  | cons head tail =>
      simp [GlobalConfig.beginCancellation, GlobalConfig.queuedMessage?,
        live, pendingCase]

theorem begin_cancellation_clears_queue
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (live : config.status = .live) (globalId : Nat) :
    (config.beginCancellation reporter cause).queue globalId = none := by
  cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter with
  | nil => simp [GlobalConfig.beginCancellation, live, pendingCase]
  | cons head tail => simp [GlobalConfig.beginCancellation, live, pendingCase]

theorem begin_cancellation_disables_protocol_steps
    {config : GlobalConfig} {reporter globalId : Nat} {cause : SessionFault}
    (live : config.status = .live) :
    (config.beginCancellation reporter cause).sendStep? globalId = none ∧
    (config.beginCancellation reporter cause).recvStep? globalId = none ∧
    (config.beginCancellation reporter cause).localStep? globalId = none := by
  cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter with
  | nil =>
      simp [GlobalConfig.beginCancellation, GlobalConfig.sendStep?,
        GlobalConfig.recvStep?, GlobalConfig.localStep?, live, pendingCase]
  | cons head tail =>
      simp [GlobalConfig.beginCancellation, GlobalConfig.sendStep?,
        GlobalConfig.recvStep?, GlobalConfig.localStep?, live, pendingCase]

/-- A resolver rejection has one fault successor. It cannot be represented as
`none` scheduler stutter or leave an application frame queued. -/
theorem reject_resolver_step_begins_cancellation
    {config next : GlobalConfig} {role conflict resolver : Nat}
    (stepped : config.rejectResolverStep? role conflict resolver = some next) :
    ((∃ awaiting, next.status = .cancelling .protocolViolation awaiting) \/
      next.status = .retired .protocolViolation) /\
    ∀ globalId, next.queue globalId = none := by
  unfold GlobalConfig.rejectResolverStep? at stepped
  cases statusCase : config.status with
  | cancelling cause awaiting => simp [statusCase] at stepped
  | retired cause => simp [statusCase] at stepped
  | live =>
      cases selectionCase : config.routeSelection conflict with
      | some selected => simp [statusCase, selectionCase] at stepped
      | none =>
          cases resolverCase : applyResolver
              (projectGraph role config.choreo)
              (config.localState role)
              conflict resolver .reject with
          | none => simp [statusCase, selectionCase, resolverCase] at stepped
          | some result =>
              cases result with
              | selected localState =>
                  simp [statusCase, selectionCase, resolverCase] at stepped
              | rejected =>
                  have nextEq : config.beginCancellation role .protocolViolation = next := by
                    exact Option.some.inj (by
                      simpa [statusCase, selectionCase, resolverCase] using stepped)
                  subst next
                  cases pendingCase : removeAwaitingRole
                      (List.range config.roleCount) role <;>
                    simp [GlobalConfig.beginCancellation, statusCase, pendingCase]
theorem cancellation_observation_preserves_protocol
    {config next : GlobalConfig} {role : Nat}
    (observed : config.observeCancellation? role = some next) :
    next.session = config.session ∧
    next.roleCount = config.roleCount ∧
    next.choreo = config.choreo ∧
    next.epoch = config.epoch ∧
    next.phase = config.phase ∧
    next.queue = config.queue ∧
    next.localState = config.localState ∧
    next.routeSelection = config.routeSelection := by
  unfold GlobalConfig.observeCancellation? at observed
  cases statusCase : config.status with
  | live => simp [statusCase] at observed
  | retired existing => simp [statusCase] at observed
  | cancelling cause awaiting =>
      by_cases member : role ∈ awaiting
      · cases remainingCase : removeAwaitingRole awaiting role with
        | nil =>
            have nextEq :
                { config with status := .retired cause, resources := .empty } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            exact ⟨rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl⟩
        | cons head tail =>
            have nextEq :
                { config with
                  status := .cancelling cause (head :: tail)
                  resources := .forRoles (head :: tail) } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            exact ⟨rfl, rfl, rfl, rfl, rfl, rfl, rfl, rfl⟩
      · simp [statusCase, member] at observed

theorem cancellation_observation_strictly_decreases
    {config next : GlobalConfig} {role : Nat}
    (observed : config.observeCancellation? role = some next) :
    next.cancellationMeasure < config.cancellationMeasure := by
  unfold GlobalConfig.observeCancellation? at observed
  cases statusCase : config.status with
  | live => simp [statusCase] at observed
  | retired existing => simp [statusCase] at observed
  | cancelling cause awaiting =>
      by_cases member : role ∈ awaiting
      · have shorter : (removeAwaitingRole awaiting role).length < awaiting.length := by
          unfold removeAwaitingRole
          rw [List.length_filter_lt_length_iff_exists]
          exact ⟨role, member, by simp⟩
        cases remainingCase : removeAwaitingRole awaiting role with
        | nil =>
            have nextEq :
                { config with status := .retired cause, resources := .empty } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            simp [GlobalConfig.cancellationMeasure, statusCase]
            simpa [remainingCase] using shorter
        | cons head tail =>
            have nextEq :
                { config with
                  status := .cancelling cause (head :: tail)
                  resources := .forRoles (head :: tail) } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            simp [GlobalConfig.cancellationMeasure, statusCase]
            simpa [remainingCase] using shorter
      · simp [statusCase, member] at observed

theorem retired_config_retires_resources
    {config : GlobalConfig} {cause : SessionFault}
    (retired : config.status = .retired cause)
    (queueEmpty : ∀ globalId, config.queue globalId = none)
    (ownersEmpty : config.resources = .empty) :
    config.resourcesRetired :=
  ⟨cause, retired, queueEmpty, ownersEmpty⟩

theorem cancellation_observation_is_affine
    {config next : GlobalConfig} {role : Nat}
    (observed : config.observeCancellation? role = some next) :
    next.observeCancellation? role = none /\
    role ∉ next.resources.transports /\
    role ∉ next.resources.waiters /\
    role ∉ next.resources.leases := by
  unfold GlobalConfig.observeCancellation? at observed ⊢
  cases statusCase : config.status with
  | live => simp [statusCase] at observed
  | retired existing => simp [statusCase] at observed
  | cancelling cause awaiting =>
      by_cases member : role ∈ awaiting
      · cases remainingCase : removeAwaitingRole awaiting role with
        | nil =>
            have nextEq :
                { config with status := .retired cause, resources := .empty } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            simp [ParticipantResourceOwners.empty,
              ParticipantResourceOwners.forRoles]
        | cons head tail =>
            have nextEq :
                { config with
                  status := .cancelling cause (head :: tail)
                  resources := .forRoles (head :: tail) } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            have absent : role ∉ head :: tail := by
              have filtered : role ∉ removeAwaitingRole awaiting role := by
                simp [removeAwaitingRole]
              simpa [remainingCase] using filtered
            simp [absent, ParticipantResourceOwners.forRoles]
      · simp [statusCase, member] at observed

theorem cancellation_observation_cannot_restore_live
    {config next : GlobalConfig} {role : Nat}
    (observed : config.observeCancellation? role = some next) :
    next.status ≠ .live := by
  unfold GlobalConfig.observeCancellation? at observed
  cases statusCase : config.status with
  | live => simp [statusCase] at observed
  | retired existing => simp [statusCase] at observed
  | cancelling cause awaiting =>
      by_cases member : role ∈ awaiting
      · cases remainingCase : removeAwaitingRole awaiting role with
        | nil =>
            have nextEq :
                { config with status := .retired cause, resources := .empty } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            simp
        | cons head tail =>
            have nextEq :
                { config with
                  status := .cancelling cause (head :: tail)
                  resources := .forRoles (head :: tail) } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            simp
      · simp [statusCase, member] at observed

inductive CancellationReachable
    (start : GlobalConfig) : GlobalConfig -> Prop where
  | refl : CancellationReachable start start
  | observe
      {current next : GlobalConfig} {role : Nat}
      (path : CancellationReachable start current)
      (observed : current.observeCancellation? role = some next) :
      CancellationReachable start next

theorem CancellationReachable.trans
    {start middle finish : GlobalConfig}
    (path : CancellationReachable start middle)
    (suffix : CancellationReachable middle finish) :
    CancellationReachable start finish := by
  induction suffix with
  | refl => exact path
  | observe suffixPrefix observed inductionHypothesis =>
      exact .observe inductionHypothesis observed

theorem CancellationReachable.preserves_queue
    {start finish : GlobalConfig}
    (path : CancellationReachable start finish) :
    finish.queue = start.queue := by
  induction path with
  | refl => rfl
  | observe pathPrefix observed inductionHypothesis =>
      have queueEq :=
        (cancellation_observation_preserves_protocol observed).2.2.2.2.2.1
      exact queueEq.trans inductionHypothesis

private theorem cancellation_observations_reach_retired_with_fuel
    (fuel : Nat)
    {config : GlobalConfig} {cause : SessionFault} {awaiting : List Nat}
    (cancelling : config.status = .cancelling cause awaiting)
    (bounded : awaiting.length ≤ fuel)
    (pending : awaiting ≠ []) :
    ∃ retired,
      CancellationReachable config retired /\
      retired.status = .retired cause /\
      retired.resources = .empty := by
  induction fuel generalizing config awaiting with
  | zero =>
      have empty : awaiting = [] :=
        List.eq_nil_of_length_eq_zero (Nat.eq_zero_of_le_zero bounded)
      exact False.elim (pending empty)
  | succ fuel inductionHypothesis =>
      cases awaiting with
      | nil => exact False.elim (pending rfl)
      | cons head tail =>
          have shorter :
              (removeAwaitingRole (head :: tail) head).length <
                (head :: tail).length := by
            unfold removeAwaitingRole
            rw [List.length_filter_lt_length_iff_exists]
            exact ⟨head, by simp, by simp⟩
          cases remainingCase : removeAwaitingRole (head :: tail) head with
          | nil =>
              let retired := {
                config with status := .retired cause, resources := .empty
              }
              have observed : config.observeCancellation? head = some retired := by
                simp [GlobalConfig.observeCancellation?, cancelling,
                  remainingCase, retired]
              exact ⟨retired, .observe .refl observed, rfl, rfl⟩
          | cons nextHead nextTail =>
              let next := {
                config with
                status := .cancelling cause (nextHead :: nextTail)
                resources := .forRoles (nextHead :: nextTail)
              }
              have observed : config.observeCancellation? head = some next := by
                simp [GlobalConfig.observeCancellation?, cancelling,
                  remainingCase, next]
              have nextBound : (nextHead :: nextTail).length ≤ fuel := by
                have remainingBound :
                    (removeAwaitingRole (head :: tail) head).length ≤ fuel := by
                  omega
                simpa [remainingCase] using remainingBound
              obtain ⟨retired, reachable, retiredStatus, ownersEmpty⟩ :=
                inductionHypothesis
                  (config := next)
                  (awaiting := nextHead :: nextTail)
                  (by rfl)
                  nextBound
                  (by simp)
              exact ⟨retired,
                CancellationReachable.trans (.observe .refl observed) reachable,
                retiredStatus, ownersEmpty⟩

theorem finite_cancellation_observations_reach_retired
    {config : GlobalConfig} {cause : SessionFault} {awaiting : List Nat}
    (cancelling : config.status = .cancelling cause awaiting)
    (pending : awaiting ≠ []) :
    ∃ retired,
      CancellationReachable config retired /\
      retired.status = .retired cause /\
      retired.resources = .empty :=
  cancellation_observations_reach_retired_with_fuel
    awaiting.length cancelling (Nat.le_refl _) pending

theorem begun_cancellation_has_finite_retirement_trace
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (live : config.status = .live) :
    ∃ retired,
      CancellationReachable (config.beginCancellation reporter cause) retired /\
      retired.status = .retired cause /\
      retired.resources = .empty := by
  cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter with
  | nil =>
      let started := config.beginCancellation reporter cause
      have retired : started.status = .retired cause := by
        simp [started, GlobalConfig.beginCancellation, live, pendingCase]
      have ownersEmpty : started.resources = .empty := by
        simp [started, GlobalConfig.beginCancellation, live, pendingCase]
      exact ⟨started, .refl, retired, ownersEmpty⟩
  | cons head tail =>
      let started := config.beginCancellation reporter cause
      have cancelling :
          started.status = .cancelling cause (head :: tail) := by
        simp [started, GlobalConfig.beginCancellation, live, pendingCase]
      exact finite_cancellation_observations_reach_retired
        cancelling (by simp)

theorem begun_cancellation_reaches_resource_retirement
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (live : config.status = .live) :
    ∃ retired,
      CancellationReachable (config.beginCancellation reporter cause) retired /\
      retired.resourcesRetired := by
  obtain ⟨retired, reachable, retiredStatus, ownersEmpty⟩ :=
    begun_cancellation_has_finite_retirement_trace live
  refine ⟨retired, reachable,
    retired_config_retires_resources retiredStatus ?_ ownersEmpty⟩
  intro globalId
  rw [reachable.preserves_queue]
  exact begin_cancellation_clears_queue live globalId

def GlobalConfig.CancellationWellFormed (config : GlobalConfig) : Prop :=
  match config.status with
  | .live => True
  | .cancelling _ awaiting =>
      awaiting ≠ [] /\
      config.resources = .forRoles awaiting /\
      ∀ globalId, config.queue globalId = none
  | .retired _ =>
      config.resources = .empty /\
      ∀ globalId, config.queue globalId = none

theorem begin_cancellation_is_well_formed
    {config : GlobalConfig} {reporter : Nat} {cause : SessionFault}
    (live : config.status = .live) :
    (config.beginCancellation reporter cause).CancellationWellFormed := by
  cases pendingCase : removeAwaitingRole (List.range config.roleCount) reporter with
  | nil =>
      simp [GlobalConfig.CancellationWellFormed,
        GlobalConfig.beginCancellation, live, pendingCase]
  | cons head tail =>
      simp [GlobalConfig.CancellationWellFormed,
        GlobalConfig.beginCancellation, live, pendingCase]

theorem cancellation_observation_preserves_well_formed
    {config next : GlobalConfig} {role : Nat}
    (wellFormed : config.CancellationWellFormed)
    (observed : config.observeCancellation? role = some next) :
    next.CancellationWellFormed := by
  unfold GlobalConfig.observeCancellation? at observed
  cases statusCase : config.status with
  | live => simp [statusCase] at observed
  | retired cause => simp [statusCase] at observed
  | cancelling cause awaiting =>
      have currentWellFormed :
          awaiting ≠ [] /\
          config.resources = .forRoles awaiting /\
          ∀ globalId, config.queue globalId = none := by
        simpa [GlobalConfig.CancellationWellFormed, statusCase] using wellFormed
      by_cases member : role ∈ awaiting
      · cases remainingCase : removeAwaitingRole awaiting role with
        | nil =>
            have nextEq :
                { config with status := .retired cause, resources := .empty } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            exact ⟨rfl, currentWellFormed.2.2⟩
        | cons head tail =>
            have nextEq :
                { config with
                  status := .cancelling cause (head :: tail)
                  resources := .forRoles (head :: tail) } = next := by
              exact Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            exact ⟨by simp, rfl, currentWellFormed.2.2⟩
      · simp [statusCase, member] at observed

theorem cancellation_observation_preserves_global_invariant
    {config next : GlobalConfig} {role : Nat}
    (invariant : GlobalInvariant config)
    (observed : config.observeCancellation? role = some next) :
    GlobalInvariant next := by
  unfold GlobalConfig.observeCancellation? at observed
  cases statusCase : config.status with
  | live => simp [statusCase] at observed
  | retired cause => simp [statusCase] at observed
  | cancelling cause awaiting =>
      have queueEmpty : ∀ globalId, config.queue globalId = none := by
        simpa [SessionFidelity, statusCase] using invariant.2.1
      by_cases member : role ∈ awaiting
      · cases remainingCase : removeAwaitingRole awaiting role with
        | nil =>
            have nextEq :
                { config with status := .retired cause, resources := .empty } = next :=
              Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            refine ⟨?_, ?_, ?_⟩
            · simpa [GlobalConfig.WellFormed] using invariant.1
            · simp [SessionFidelity, queueEmpty]
            · simpa [RouteSelectionFidelity] using invariant.2.2
        | cons head tail =>
            have nextEq :
                { config with
                  status := .cancelling cause (head :: tail)
                  resources := .forRoles (head :: tail) } = next :=
              Option.some.inj (by
                simpa [statusCase, member, remainingCase] using observed)
            subst next
            refine ⟨?_, ?_, ?_⟩
            · simpa [GlobalConfig.WellFormed] using invariant.1
            · simp [SessionFidelity, queueEmpty]
            · simpa [RouteSelectionFidelity] using invariant.2.2
      · simp [statusCase, member] at observed

/-- Fair cancellation delivery is stated explicitly: if every pending role is
eventually allowed to observe, the strictly decreasing finite measure reaches
the retired state. -/
theorem cancellation_fair_delivery_reaches_retirement
    (config : GlobalConfig)
    (fair : ∀ (current : GlobalConfig),
      current.cancellationMeasure > 0 →
      (∃ cause awaiting, current.status = .cancelling cause awaiting) →
      ∃ (role : Nat) (next : GlobalConfig),
        current.observeCancellation? role = some next) :
    ∀ fuel, config.cancellationMeasure ≤ fuel →
      config.status ≠ .live →
      config.CancellationWellFormed →
      ∃ (retired : GlobalConfig),
        CancellationReachable config retired ∧
        retired.cancellationMeasure = 0 ∧
        (∃ cause, retired.status = .retired cause) ∧
        retired.resourcesRetired := by
  intro fuel
  induction fuel generalizing config with
  | zero =>
      intro bounded notLive wellFormed
      have zero : config.cancellationMeasure = 0 := Nat.eq_zero_of_le_zero bounded
      cases statusCase : config.status with
      | live => exact False.elim (notLive statusCase)
      | retired cause =>
          have retiredWellFormed :
              config.resources = .empty ∧
              ∀ globalId, config.queue globalId = none := by
            simpa [GlobalConfig.CancellationWellFormed, statusCase] using wellFormed
          exact ⟨config, .refl, zero, ⟨cause, statusCase⟩,
            retired_config_retires_resources statusCase
              retiredWellFormed.2 retiredWellFormed.1⟩
      | cancelling cause awaiting =>
          have pending : awaiting ≠ [] := by
            have exact :
                awaiting ≠ [] ∧
                config.resources = .forRoles awaiting ∧
                ∀ globalId, config.queue globalId = none := by
              simpa [GlobalConfig.CancellationWellFormed, statusCase] using wellFormed
            exact exact.1
          have empty : awaiting = [] := by
            apply List.eq_nil_of_length_eq_zero
            simpa [GlobalConfig.cancellationMeasure, statusCase] using zero
          exact False.elim (pending empty)
  | succ fuel inductionHypothesis =>
      intro bounded notLive wellFormed
      cases statusCase : config.status with
      | live => exact False.elim (notLive statusCase)
      | retired cause =>
          have retiredWellFormed :
              config.resources = .empty ∧
              ∀ globalId, config.queue globalId = none := by
            simpa [GlobalConfig.CancellationWellFormed, statusCase] using wellFormed
          exact ⟨config, .refl,
            by simp [GlobalConfig.cancellationMeasure, statusCase],
            ⟨cause, statusCase⟩,
            retired_config_retires_resources statusCase
              retiredWellFormed.2 retiredWellFormed.1⟩
      | cancelling cause awaiting =>
          cases awaiting with
          | nil =>
              simp [GlobalConfig.CancellationWellFormed, statusCase] at wellFormed
          | cons head tail =>
              have positive : config.cancellationMeasure > 0 := by
                simp [GlobalConfig.cancellationMeasure, statusCase]
              obtain ⟨role, next, observed⟩ :=
                fair config positive ⟨cause, head :: tail, statusCase⟩
              have decreases := cancellation_observation_strictly_decreases observed
              have nextBound : next.cancellationMeasure ≤ fuel := by omega
              have nextNotLive := cancellation_observation_cannot_restore_live observed
              have nextWellFormed :=
                cancellation_observation_preserves_well_formed wellFormed observed
              obtain ⟨retired, reachable, zero, retiredStatus, resourcesRetired⟩ :=
                inductionHypothesis next nextBound nextNotLive nextWellFormed
              exact ⟨retired,
                CancellationReachable.trans
                  (CancellationReachable.observe .refl observed) reachable,
                zero, retiredStatus, resourcesRetired⟩

end Hibana
