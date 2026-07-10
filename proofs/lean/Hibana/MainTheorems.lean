import Hibana.Allocation
import Hibana.Authority
import Hibana.Commit
import Hibana.Generation
import Hibana.Layout
import Hibana.Progress
import Hibana.Refinement

namespace Hibana

theorem initial_state_has_no_selected_arm (conflict : Nat) :
    CommitState.initial.selected conflict = none := rfl

theorem initial_state_has_no_completed_event (event : Nat) :
    CommitState.initial.done event = false := rfl

private def resolverProofChoreo : Choreo :=
  .route (.dynamic 7) (.send 0 1 1) (.send 0 1 2)

theorem unresolved_dynamic_commit_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := [1, 2], action := .commit 1 }
    ] = false := by
  decide

theorem wrong_resolver_id_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := [1, 2], action := .resolve 0 8 .left }
    ] = false := by
  decide

theorem resolver_reuse_without_reset_rejected :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := [1, 2], action := .resolve 0 7 .left },
      { enabled := [1], action := .resolve 0 7 .right }
    ] = false := by
  decide

theorem resolver_reject_is_terminal :
    checkProgramTrace resolverProofChoreo 0 [
      { enabled := [1, 2], action := .reject 0 7 },
      { enabled := [1, 2], action := .stop }
    ] = false := by
  decide

end Hibana
