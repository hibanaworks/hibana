import Hibana.ActorProgress
import Init.Data.List.MinMaxOn

namespace Hibana
open EndpointDelegation

/-- Source dependencies within one selected visit always point forward.
Parallel siblings add no order; repetition reuses the body's order per visit. -/
theorem source_order_strictly_increases_event
    {source : Source} {before after : Nat}
    (ordered : source.before before after = true) : before < after := by
  induction source generalizing before after with
  | send message => simp [Source.before] at ordered
  | seq left right leftIH rightIH =>
      simp only [Source.before] at ordered
      split at ordered
      next bothLeft => exact leftIH ordered
      next notBothLeft =>
        split at ordered
        next bothRight =>
          have bounds : left.eventCount ≤ before ∧ left.eventCount ≤ after := by
            simpa using bothRight
          have below := rightIH ordered
          omega
        next notBothRight =>
          have crosses : before < left.eventCount ∧ left.eventCount ≤ after := by
            simpa using ordered
          omega
  | par left right leftIH rightIH | route resolver left right leftIH rightIH =>
      simp only [Source.before] at ordered
      split at ordered
      next bothLeft => exact leftIH ordered
      next notBothLeft =>
        split at ordered
        next bothRight =>
          have bounds : left.eventCount ≤ before ∧ left.eventCount ≤ after := by
            simpa using bothRight
          have below := rightIH ordered
          omega
        next notBothRight => contradiction
  | roll body bodyIH => exact bodyIH ordered

/-- Co-resident logical sender and receiver are two distinct operations.
Sharing an actor never collapses their send-before-receive dependency. -/
theorem actor_order_has_source_order_or_message_halves
    {source : Source} {bindings : List (Nat × Binding)} {before after : ActionId}
    (ordered : precedes source bindings before after = true) :
    before.event < after.event ∨
      (before.event = after.event ∧ before.side = .send ∧ after.side = .receive) := by
  unfold precedes at ordered
  cases leftAt : action? bindings before with
  | none => simp [leftAt] at ordered
  | some left =>
      cases rightAt : action? bindings after with
      | none => simp [leftAt, rightAt] at ordered
      | some right =>
          simp only [leftAt, rightAt, Bool.and_eq_true] at ordered
          have shape := ordered.2
          by_cases same : before.event = after.event
          · right
            have chosen : (before.event == after.event) = true := beq_iff_eq.mpr same
            rw [if_pos chosen] at shape
            have choices : (before.side == .send) = true ∧
                (after.side == .receive) = true := by
              simpa only [Bool.and_eq_true] using shape
            have exactSide : ∀ left right : Side, (left == right) = true → left = right := by
              intro left right
              cases left <;> cases right <;> decide
            exact ⟨same, exactSide _ _ choices.1, exactSide _ _ choices.2⟩
          · left
            exact source_order_strictly_increases_event (by simpa [same] using shape)

/-- Ghost observation points of the real send/receive boundary. Transport
acceptance can precede the sender's local publication; the receiver need not
wait for that publication. These points are not another runtime progress store. -/
inductive SourcePoint where
  | accepted (event : Nat)
  | committed (action : ActionId)
  deriving Repr, DecidableEq

def SourcePoint.rank : SourcePoint → Nat
  | .accepted event => 4 * event
  | .committed action => 4 * action.event + match action.side with
      | .send => 1
      | .receive => 2
      | .local => 3

def EndpointDelegation.ActionId.entryPoint (action : ActionId) : SourcePoint :=
  match action.side with
  | .send => .accepted action.event
  | .receive | .local => .committed action

/-- Necessary dependencies of a selected source visit. The finite active
domain excludes acceptance points for local operations and nonexistent rows.
The wire edge uses transport acceptance, while actor-local successors use
committed progress. In particular no sender-publication-to-remote-receive
edge is inserted. -/
inductive SourcePointEdge (source : Source) (bindings : List (Nat × Binding)) :
    SourcePoint → SourcePoint → Prop where
  | publish (event : Nat) :
      SourcePointEdge source bindings (.accepted event) (.committed ⟨event, .send⟩)
  | deliver (event : Nat) :
      SourcePointEdge source bindings (.accepted event) (.committed ⟨event, .receive⟩)
  | ordered {before after : ActionId} (ordered : precedes source bindings before after = true) :
      SourcePointEdge source bindings (.committed before) after.entryPoint

theorem source_point_dependency_increases_rank
    {source : Source} {bindings : List (Nat × Binding)} {before after : SourcePoint}
    (edge : SourcePointEdge source bindings before after) : before.rank < after.rank := by
  cases edge with
  | publish event => simp [SourcePoint.rank]
  | deliver event => simp [SourcePoint.rank]
  | @ordered before after ordered =>
      rcases actor_order_has_source_order_or_message_halves ordered with earlier | same
      · cases beforeSide : before.side <;> cases afterSide : after.side <;>
          simp only [SourcePoint.rank, ActionId.entryPoint, beforeSide, afterSide] <;> omega
      · obtain ⟨event, send, receive⟩ := same
        simp [SourcePoint.rank, ActionId.entryPoint, send, receive, event]

theorem source_point_dependencies_have_no_cycle
    {source : Source} {bindings : List (Nat × Binding)} (point : SourcePoint) :
    ¬ Relation.TransGen (SourcePointEdge source bindings) point point := by
  have increasing : ∀ {before after},
      Relation.TransGen (SourcePointEdge source bindings) before after →
      before.rank < after.rank := by
    intro before after path
    induction path with
    | single edge => exact source_point_dependency_increases_rank edge
    | tail path edge ih => exact Nat.lt_trans ih (source_point_dependency_increases_rank edge)
  intro cycle
  exact Nat.lt_irrefl point.rank (increasing cycle)

/-- A finite active source domain always has an unfinished point whose
dependencies have finished. `observed` is an interpretation of existing
runtime/transport observations, not a new owner table or completion ledger.
This is structural readiness, not yet a Rust or distributed-liveness theorem:
actual admission, route knowledge, transport delivery and fresh visits must
still be shown to realize these dependencies. -/
theorem unfinished_source_visit_has_ready_point
    {source : Source} {bindings : List (Nat × Binding)}
    {domain : List SourcePoint} {observed : SourcePoint → Bool}
    (closed : ∀ before after, after ∈ domain →
      SourcePointEdge source bindings before after →
      observed before = false → before ∈ domain)
    (unfinished : ∃ point ∈ domain, observed point = false) :
    ∃ point ∈ domain, observed point = false ∧
      ∀ before, SourcePointEdge source bindings before point → observed before = true := by
  let remaining := domain.filter fun point => !observed point
  have nonempty : remaining ≠ [] := by
    obtain ⟨point, member, pending⟩ := unfinished
    have belongs : point ∈ remaining := by simp [remaining, member, pending]
    intro empty
    simp [empty] at belongs
  let point := remaining.minOn SourcePoint.rank nonempty
  have member : point ∈ remaining := List.minOn_mem
  have fields : point ∈ domain ∧ observed point = false := by
    simpa [remaining] using member
  refine ⟨point, fields.1, fields.2, ?_⟩
  intro before edge
  cases observedAt : observed before with
  | true => rfl
  | false =>
      have belongs := closed before point fields.1 edge observedAt
      have beforeMember : before ∈ remaining := by simp [remaining, belongs, observedAt]
      have minimal : point.rank ≤ before.rank :=
        List.apply_minOn_le_of_mem beforeMember
      exact False.elim (Nat.not_le_of_gt (source_point_dependency_increases_rank edge) minimal)

end Hibana
