import Hibana.StaticProjectability
import Hibana.GlobalSemantics
import Hibana.EndpointOwnership
import Hibana.SourceOccurrenceProgress
namespace RollReentrySelectors
open Hibana

def ambiguous : Choreo := .seq (.roll (.send 0 1 3 0))
    (.seq (.send 1 0 24 0) (.send 0 1 3 0))
def execute : GlobalConfig → List GlobalOperation → Option GlobalConfig
  | current, [] => some current
  | current, operation :: rest => do
      let next ← current.step? operation
      execute next rest
def history : List GlobalOperation := [.send 0, .recv 0, .send 1, .recv 1]
def admits (tail : List GlobalOperation) : Bool :=
  (execute (GlobalConfig.initial 1 2 ambiguous) (history ++ tail)).isSome

theorem both_continuations_exist : admits [.send 2] = true ∧ admits [.roll 0, .send 0] = true := by decide

theorem indistinguishable_public_requests_cannot_select_both
    (choose : StaticEndpointSelector → Nat)
    (old next : StaticEndpointSelector) (same : old = next) :
    ¬ (choose old = 0 ∧ choose next = 1) := by
  subst next
  omega

theorem ambiguous_projection_is_rejected :
    checkStaticProjectability 2 ambiguous = false := by decide

theorem sequential_reuse_is_preserved :
    checkStaticProjectability 2 (.seq (.send 0 1 3 0) (.send 0 1 3 0)) = true := by decide

theorem receive_guarded_replies_are_preserved :
    checkStaticProjectability 2
      (.seq (.roll (.seq (.send 0 1 10 0) (.send 1 0 3 0)))
        (.seq (.send 0 1 20 0) (.send 1 0 3 0))) = true := by decide

theorem exclusive_arms_are_preserved :
    checkStaticProjectability 2
      (.route .intrinsic (.seq (.send 0 1 10 0) (.roll (.send 0 1 3 0)))
        (.seq (.send 0 1 20 0) (.send 0 1 3 0))) = true := by decide

def distinctContinuation : Choreo := .seq (.roll (.send 0 1 1 0)) (.send 0 1 2 0)
def continuationThenReentry : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .recv 1, .roll 0, .send 0, .recv 0]

theorem a_distinct_outbound_continuation_keeps_the_completed_roll_available :
    (execute (GlobalConfig.initial 1 2 distinctContinuation) continuationThenReentry).isSome = true := by
  decide

theorem restarting_the_body_does_not_repeat_the_one_shot_continuation :
    ((execute (GlobalConfig.initial 1 2 distinctContinuation) continuationThenReentry).bind
      (fun state => state.step? (.send 1))).isNone = true := by decide

theorem an_unrolled_prefix_cannot_restart_after_its_continuation :
    ((execute (GlobalConfig.initial 1 2 (.seq (.send 0 1 1 0) (.send 0 1 2 0)))
      [.send 0, .recv 0, .send 1, .recv 1]).bind
      (fun state => state.step? (.send 0))).isNone = true := by decide

namespace DistinctContinuation
open EndpointDelegation
def ping : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 1, .wire 0⟩
def tail : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 2, .wire 0⟩
def source : Source := .seq (.roll (.send ping)) (.send tail)
def expanded : Source := .seq (.seq (.send ping) (.send tail)) (.send ping)
def unfolding : SourceUnfolding source expanded :=
  .resumeAfterContinuation (.seq (.rollOnce (.send ping)) (.send tail)) (.send ping)

theorem the_occurrence_model_preserves_the_same_continuation_then_body_order :
    unfolding.origins 0 = [0, 1, 0] ∧ (normalize expanded).isSome = true := by decide
end DistinctContinuation

def erasedPrefix : Choreo :=
  .seq (.roll (.seq (.send 0 1 10 0) (.send 2 3 3 0)))
    (.seq (.send 0 1 20 0) (.send 2 3 3 0))

theorem unrelated_prefix_is_erased_for_the_sending_role :
    (Choreo.seq (.send 0 1 10 0) (.send 2 3 3 0)).firstRoleEndpointSelectorsFrom 2 0 =
      [.outbound 2 3 0] := by decide

theorem unrelated_prefix_cannot_disambiguate_reentry :
    erasedPrefix.checkRollReentryLinearityFrom 0 [] = false := by decide

def parallelTail : Choreo :=
  .seq (.par (.roll (.send 0 1 3 0)) (.send 2 3 10 0))
    (.seq (.send 1 0 24 0) (.roll (.send 0 1 3 0)))

def parallelHistory : List GlobalOperation :=
  [.send 0, .recv 0, .send 1, .recv 1, .send 2, .recv 2]

def parallelAdmits (tail : List GlobalOperation) : Bool :=
  (execute (GlobalConfig.initial 1 4 parallelTail) (parallelHistory ++ tail)).isSome

theorem parallel_join_does_not_retire_the_old_elastic_roll :
    parallelAdmits [.send 3] = true ∧ parallelAdmits [.roll 0, .send 0] = true := by decide

theorem parallel_continuation_ambiguity_is_rejected :
    parallelTail.checkRollReentryLinearityFrom 0 [] = false := by decide

open EndpointDelegation in
theorem an_actor_predecessor_is_preserved_across_distinct_logical_views
    (source : Source) (bindings : List (Nat × Binding))
    (before after : ActionId) (left right : Action)
    (prior : action? bindings before = some left)
    (next : action? bindings after = some right)
    (sameActor : left.actor = right.actor)
    (differentEvents : before.event ≠ after.event)
    (ordered : source.before before.event after.event = true) :
    precedes source bindings before after = true := by
  have different : (before.event == after.event) = false :=
    decide_eq_false differentEvents
  have same : (left.actor == right.actor) = true := decide_eq_true sameActor
  simp only [precedes, prior, next, same, Bool.true_and, different,
    Bool.false_eq_true, ↓reduceIte, ordered]

theorem a_choice_guards_its_continuation_exactly_when_both_arms_do (left right : Bool) :
    (∀ choice : Bool, (if choice then left else right) = true) ↔ (left && right) = true := by
  cases left <;> cases right <;> decide

namespace Delegated
open EndpointDelegation
def key (role : Nat) : EndpointKey := ⟨0, role⟩
def loan (sender receiver : Nat) : Source := .send ⟨key sender, key receiver, 1, .endpoint (key 2)⟩
def work : Source := .seq (.send ⟨key 2, key 3, 2, .wire 0⟩)
    (.send ⟨key 3, key 2, 3, .wire 0⟩)
def cycle : Source := .seq (loan 0 1) (.seq work (loan 1 0))
def source : Source := .seq (.roll cycle) work
def bindings : List (Nat × Binding) :=
  (normalize source).toList.flatMap fun normalized =>
    normalized.paths.flatMap NormalizedPath.bindings

theorem acquisition_guards_the_worker_request :
    bindings.length = 6 ∧
    (action? bindings ⟨0, .receive⟩).map Action.actor = some 1 ∧
    (action? bindings ⟨1, .send⟩).map Action.actor = some 1 ∧
    precedes source bindings ⟨0, .receive⟩ ⟨1, .send⟩ = true := by decide

theorem the_returned_view_keeps_the_owner_continuation :
    (action? bindings ⟨4, .send⟩).map Action.actor = some 0 ∧
    precedes source bindings ⟨3, .receive⟩ ⟨4, .send⟩ = true := by decide
end Delegated

#print axioms both_continuations_exist
#print axioms indistinguishable_public_requests_cannot_select_both
#print axioms ambiguous_projection_is_rejected
#print axioms sequential_reuse_is_preserved
#print axioms receive_guarded_replies_are_preserved
#print axioms exclusive_arms_are_preserved
#print axioms a_distinct_outbound_continuation_keeps_the_completed_roll_available
#print axioms restarting_the_body_does_not_repeat_the_one_shot_continuation
#print axioms an_unrolled_prefix_cannot_restart_after_its_continuation
#print axioms DistinctContinuation.the_occurrence_model_preserves_the_same_continuation_then_body_order
#print axioms unrelated_prefix_is_erased_for_the_sending_role
#print axioms unrelated_prefix_cannot_disambiguate_reentry
#print axioms parallel_join_does_not_retire_the_old_elastic_roll
#print axioms parallel_continuation_ambiguity_is_rejected
#print axioms an_actor_predecessor_is_preserved_across_distinct_logical_views
#print axioms a_choice_guards_its_continuation_exactly_when_both_arms_do
#print axioms Delegated.acquisition_guards_the_worker_request
#print axioms Delegated.the_returned_view_keeps_the_owner_continuation
end RollReentrySelectors
