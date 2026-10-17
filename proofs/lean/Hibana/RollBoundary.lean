import Hibana.Commit

namespace Hibana

def rollBoundaryChoice : Choreo := .route .intrinsic
  (.seq (.send 0 1 41 0) (.send 1 0 43 0))
  (.seq (.send 0 1 42 0) (.send 1 0 43 0))

def rollBoundaryOuter : Choreo :=
  .roll (.seq (.send 0 1 40 0) (.seq rollBoundaryChoice (.send 0 1 44 0)))

def rollBoundaryNested : Choreo :=
  .roll (.seq (.send 0 1 40 0) (.seq (.roll rollBoundaryChoice) (.send 0 1 44 0)))

/-- Execute the two public Rust regressions' local event sequences through
the canonical commit function. This is proof data, never a runtime cursor. -/
def rollBoundaryRun (source : Choreo) (role : Nat) (events : List Nat) : Option CommitState :=
  events.foldlM (commitEvent (projectGraph role source)) CommitState.initial

theorem roll_boundary_sources_have_valid_projections :
    [0, 1].all (fun role => (projectGraph role rollBoundaryOuter).check &&
      (projectGraph role rollBoundaryNested).check) = true := by decide

theorem unrolled_child_cannot_repeat_before_its_parent_suffix :
    [0, 1].all (fun role => [1, 3].all (fun event =>
      !((rollBoundaryRun rollBoundaryOuter role [0, 1, 2]).bind
        (fun state => commitEvent (projectGraph role rollBoundaryOuter) state event)).isSome)) = true := by
  decide

theorem completed_parent_roll_can_restart_at_its_prefix :
    [0, 1].all (fun role => ((rollBoundaryRun rollBoundaryOuter role [0, 1, 2, 5]).bind
      (fun state => commitEvent (projectGraph role rollBoundaryOuter) state 0)).isSome) = true := by
  decide

theorem explicit_child_roll_can_change_arm_before_its_parent_suffix :
    [0, 1].all (fun role => ((rollBoundaryRun rollBoundaryNested role [0, 1, 2]).bind
      (fun state => commitEvent (projectGraph role rollBoundaryNested) state 3)).isSome) = true := by
  decide

theorem explicit_child_roll_keeps_its_parent_prefix :
    [0, 1].all (fun role => ((rollBoundaryRun rollBoundaryNested role [0, 1, 2, 3]).map
      (fun state => state.done 0)).getD false) = true := by decide

theorem nested_roll_boundary_trace_can_run_twice :
    [0, 1].all (fun role => (rollBoundaryRun rollBoundaryNested role
      [0, 1, 2, 3, 4, 3, 4, 5, 0, 1, 2, 3, 4, 3, 4, 5]).isSome) = true := by decide

end Hibana
