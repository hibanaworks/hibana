import Hibana.RuntimeRefinement

namespace Hibana

/-- Pure prepare/commit view of the production kernel. Rust supplies the
concrete `State`, `Input`, and `Prepared` domains; Kani checks the finite packed
operations and Miri checks provenance. Lean intentionally treats that
cross-tool result as an explicit premise instead of pretending to verify Rust
source. -/
structure PreparedKernelSemantics where
  State : Type
  Input : Type
  Prepared : Type
  prepare : State -> Input -> Option Prepared
  effect : Prepared -> RuntimeEffect
  commit : State -> Prepared -> State
  abstractState : State -> CompactGlobalState

def PreparedKernelSemantics.transition?
    (kernel : PreparedKernelSemantics)
    (state : kernel.State)
    (input : kernel.Input) :
    Option (RuntimeEffect × kernel.State) :=
  (kernel.prepare state input).map fun prepared =>
    (kernel.effect prepared, kernel.commit state prepared)

/-- Cross-tool production obligation. `stateCanonical` fixes the exact compact
snapshot boundary; `preparedCommitExact` says a successful commit is precisely
one normalized Lean effect. A rejected pure preparation has no commit value and
therefore denotes zero protocol transitions. -/
structure RustKernelRefinement
    (kernel : PreparedKernelSemantics)
    (session roleCount : Nat)
    (choreo : Choreo) : Prop where
  stateCanonical : forall state,
    (kernel.abstractState state).canonical roleCount choreo = true
  preparedCommitExact : forall state input prepared,
    kernel.prepare state input = some prepared ->
      applyRuntimeEffect? session roleCount choreo
          (kernel.abstractState state) (kernel.effect prepared) =
        some (kernel.abstractState (kernel.commit state prepared))

/-- Nonempty route rows carry the only lane authority accepted by the commit
boundary. Empty rows are represented by `none`; a second lane cannot silently
reinterpret or erase a prepared nonempty chain. -/
structure LaneBoundRouteRows (Row : Type) where
  lane : Nat
  rows : List Row
  nonempty : rows ≠ []

def finishRouteRowsForLane?
    (prepared : Option (LaneBoundRouteRows Row))
    (expectedLane : Nat) : Option (List Row) :=
  match prepared with
  | none => some []
  | some bound =>
      if bound.lane = expectedLane then some bound.rows else none

theorem empty_route_rows_finish_exactly :
    finishRouteRowsForLane? (Row := Row) none expectedLane = some [] := by
  rfl

theorem lane_bound_route_rows_finish_exactly
    (bound : LaneBoundRouteRows Row) :
    finishRouteRowsForLane? (some bound) bound.lane = some bound.rows := by
  simp [finishRouteRowsForLane?]

theorem lane_bound_route_rows_mismatch_is_rejected
    (bound : LaneBoundRouteRows Row)
    (different : bound.lane ≠ expectedLane) :
    finishRouteRowsForLane? (some bound) expectedLane = none := by
  simp [finishRouteRowsForLane?, different]

theorem rejected_kernel_preparation_is_zero_transition
    {kernel : PreparedKernelSemantics}
    {state : kernel.State}
    {input : kernel.Input}
    (rejected : kernel.prepare state input = none) :
    kernel.transition? state input = none := by
  simp [PreparedKernelSemantics.transition?, rejected]

theorem prepared_kernel_commit_has_accepted_transition_certificate
    {kernel : PreparedKernelSemantics}
    {session roleCount : Nat}
    {choreo : Choreo}
    (refinement : RustKernelRefinement kernel session roleCount choreo)
    {state : kernel.State}
    {input : kernel.Input}
    {prepared : kernel.Prepared}
    (preparedExact : kernel.prepare state input = some prepared) :
    let certificate : RuntimeTransitionCertificate := {
      before := kernel.abstractState state
      effect := kernel.effect prepared
      after := kernel.abstractState (kernel.commit state prepared)
    }
    certificate.check session roleCount choreo = true := by
  simp [RuntimeTransitionCertificate.check,
    refinement.stateCanonical,
    refinement.preparedCommitExact state input prepared preparedExact]

theorem prepared_kernel_commit_refines_exact_lean_effect
    {kernel : PreparedKernelSemantics}
    {session roleCount : Nat}
    {choreo : Choreo}
    (refinement : RustKernelRefinement kernel session roleCount choreo)
    {state : kernel.State}
    {input : kernel.Input}
    {prepared : kernel.Prepared}
    (preparedExact : kernel.prepare state input = some prepared) :
    let certificate : RuntimeTransitionCertificate := {
      before := kernel.abstractState state
      effect := kernel.effect prepared
      after := kernel.abstractState (kernel.commit state prepared)
    }
    certificate.Refines session roleCount choreo := by
  exact runtime_transition_certificate_sound
    (prepared_kernel_commit_has_accepted_transition_certificate
      refinement preparedExact)

theorem prepared_kernel_stutter_commit_erases_to_same_compact_state
    {kernel : PreparedKernelSemantics}
    {session roleCount : Nat}
    {choreo : Choreo}
    (refinement : RustKernelRefinement kernel session roleCount choreo)
    {state : kernel.State}
    {input : kernel.Input}
    {prepared : kernel.Prepared}
    (preparedExact : kernel.prepare state input = some prepared)
    (stutter : kernel.effect prepared = .preview \/
      kernel.effect prepared = .dropPending \/
      kernel.effect prepared = .requeue) :
    kernel.abstractState (kernel.commit state prepared) =
      kernel.abstractState state := by
  have relation := prepared_kernel_commit_refines_exact_lean_effect
    refinement preparedExact
  have semantic := relation.2.2
  cases decoded : (kernel.abstractState state).decode?
      session roleCount choreo with
  | none => simp [decoded] at semantic
  | some config =>
      rw [decoded] at semantic
      rcases stutter with preview | dropPending | requeue
      · simpa [preview] using semantic
      · simpa [dropPending] using semantic
      · simpa [requeue] using semantic

end Hibana
