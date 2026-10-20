import Hibana.StaticProjectability
import Hibana.GlobalSemantics
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

#print axioms both_continuations_exist
#print axioms indistinguishable_public_requests_cannot_select_both
#print axioms ambiguous_projection_is_rejected
#print axioms sequential_reuse_is_preserved
#print axioms receive_guarded_replies_are_preserved
#print axioms exclusive_arms_are_preserved
#print axioms unrelated_prefix_is_erased_for_the_sending_role
#print axioms unrelated_prefix_cannot_disambiguate_reentry
#print axioms parallel_join_does_not_retire_the_old_elastic_roll
#print axioms parallel_continuation_ambiguity_is_rejected
end RollReentrySelectors
