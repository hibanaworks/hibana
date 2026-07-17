import Hibana.DistributedSemantics

namespace Hibana

private def malformedLocalQueueWitness : DistributedConfig :=
  let choreo : Choreo := .send 0 1 9 4
  DistributedConfig.fromGlobalConfig {
    (GlobalConfig.initial 3 2 choreo) with
    phase := fun globalId => if globalId = 0 then .queued 0 else .ready
    queue := fun globalId => if globalId = 0 then some {
      session := 3
      epoch := 0
      globalId := 1
      sender := 0
      receiver := 1
      lane := 0
      label := 9
      schema := 4
    } else none
  }

example : malformedLocalQueueWitness.localConfig? 1 = none := by
  native_decide

private def wrongLaneLocalQueueWitness : DistributedConfig :=
  let choreo : Choreo := .send 0 1 9 4
  DistributedConfig.fromGlobalConfig {
    (GlobalConfig.initial 3 2 choreo) with
    phase := fun globalId => if globalId = 0 then .queued 0 else .ready
    queue := fun globalId => if globalId = 0 then some {
      session := 3
      epoch := 0
      globalId := 0
      sender := 0
      receiver := 1
      lane := 1
      label := 9
      schema := 4
    } else none
  }

example : wrongLaneLocalQueueWitness.localConfig? 1 = none := by
  native_decide

private def routeEvidenceRegressionChoreo : Choreo :=
  .route .intrinsic (.send 0 1 11 4) (.send 0 1 12 4)

private def routeEvidenceRegression : Bool :=
  let initial := DistributedConfig.fromGlobalConfig
    (GlobalConfig.initial 7 2 routeEvidenceRegressionChoreo)
  match initial.protocolStep? 0 (.send 0) with
  | none => false
  | some queued =>
      !(queued.protocolStep? 1 (.recv 0)).isSome &&
        match queued.observeQueuedRouteEvidence? 1 0 with
        | none => false
        | some observed => (observed.protocolStep? 1 (.recv 0)).isSome

/-- A receiver cannot exploit ghost route authority. The exact queued frame is
the required local stutter before its protocol-visible receive. -/
example : routeEvidenceRegression = true := by
  native_decide

private def wrongRouteEvidenceRegression : Bool :=
  let initial := DistributedConfig.fromGlobalConfig
    (GlobalConfig.initial 7 2 routeEvidenceRegressionChoreo)
  match initial.protocolStep? 0 (.send 0) with
  | none => false
  | some queued => (queued.observeQueuedRouteEvidence? 1 1).isSome

example : wrongRouteEvidenceRegression = false := by
  native_decide

private def duplicateRouteEvidenceRegression : Bool :=
  let initial := DistributedConfig.fromGlobalConfig
    (GlobalConfig.initial 7 2 routeEvidenceRegressionChoreo)
  match initial.protocolStep? 0 (.send 0) with
  | none => false
  | some queued =>
      match queued.observeQueuedRouteEvidence? 1 0 with
      | none => false
      | some observed => (observed.observeQueuedRouteEvidence? 1 0).isSome

/-- Route evidence is affine: the same accepted occurrence cannot publish the
same route knowledge to one endpoint twice. -/
example : duplicateRouteEvidenceRegression = false := by
  native_decide

private def dynamicAuthorityRegressionChoreo : Choreo :=
  .route (.dynamic 0) (.send 0 1 21 4) (.send 0 1 22 4)

private def dynamicAuthorityRegression : Bool :=
  let initial := DistributedConfig.fromGlobalConfig
    (GlobalConfig.initial 9 2 dynamicAuthorityRegressionChoreo)
  !(initial.localCommitAuthority? 0 (.send 0)).isSome &&
    !(initial.protocolStep? 0 (.send 0)).isSome &&
      match initial.protocolStep? 0 (.resolve 0 0 0 .left) with
      | none => false
      | some selected => (selected.protocolStep? 0 (.send 0)).isSome

/-- Dynamic choice cannot borrow intrinsic first-event authority. Exact
resolver selection is required before the selected arm can commit. -/
example : dynamicAuthorityRegression = true := by
  native_decide

end Hibana
