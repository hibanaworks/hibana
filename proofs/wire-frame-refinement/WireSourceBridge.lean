import WireFrameRefinement
import Hibana.DescriptorRefinement
set_option maxRecDepth 100000

def generatedNestedRolledChoreo : Hibana.Choreo :=
  Hibana.Choreo.roll (Hibana.Choreo.seq (Hibana.Choreo.roll (Hibana.Choreo.seq (Hibana.Choreo.send 0 1 71 0) (Hibana.Choreo.send 0 1 72 0))) (Hibana.Choreo.send 0 1 73 0))


def generatedNestedRolledProjectionRole0Exact : Hibana.ExactDescriptorCertificate := {
  image := {
    roleCount := 2
    role := 0
    logicalLaneCount := 1
    activeLaneCount := 1
    endpointLaneSlotCount := 1
    maxRouteCommitCount := 0
    firstActiveLane := 0
    activeLaneStart := 0
    activeLaneLength := 1
    atomCount := 3
    routeResolverCount := 0
    routeParticipantCount := 0
    scopeMarkerCount := 4
    eventCount := 3
    dependencyRowCount := 0
    conflictRowCount := 0
    routeScopeCount := 0
    residentBoundaryCount := 2
    laneBitCount := 1
    routeArmLaneStepCount := 0
    routeCommitRowCount := 0
    rollScopeCount := 2
    programBytes := [0, 1, 71, 0, 0, 0, 0, 0, 0, 0, 1, 72, 0, 0, 0, 0, 0, 0, 0, 1, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 0, 0, 0, 1, 32, 0, 2, 0, 1, 32, 2, 3, 0, 0, 32, 2]
    roleBytes := [0, 0, 255, 255, 255, 255, 1, 32, 0, 0, 1, 0, 255, 255, 255, 255, 1, 32, 0, 0, 2, 0, 255, 255, 255, 255, 0, 32, 1, 0, 0, 0, 0, 0, 0, 3, 0, 1, 0, 0, 3, 0, 0, 0, 1, 0, 2, 0, 0, 0]
  }
  choreo := generatedNestedRolledChoreo
}


def generatedNestedRolledProjectionRole1Exact : Hibana.ExactDescriptorCertificate := {
  image := {
    roleCount := 2
    role := 1
    logicalLaneCount := 1
    activeLaneCount := 1
    endpointLaneSlotCount := 1
    maxRouteCommitCount := 0
    firstActiveLane := 0
    activeLaneStart := 0
    activeLaneLength := 1
    atomCount := 3
    routeResolverCount := 0
    routeParticipantCount := 0
    scopeMarkerCount := 4
    eventCount := 3
    dependencyRowCount := 0
    conflictRowCount := 0
    routeScopeCount := 0
    residentBoundaryCount := 2
    laneBitCount := 1
    routeArmLaneStepCount := 0
    routeCommitRowCount := 0
    rollScopeCount := 2
    programBytes := [0, 1, 71, 0, 0, 0, 0, 0, 0, 0, 1, 72, 0, 0, 0, 0, 0, 0, 0, 1, 73, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 0, 0, 0, 1, 32, 0, 2, 0, 1, 32, 2, 3, 0, 0, 32, 2]
    roleBytes := [0, 0, 255, 255, 255, 255, 1, 32, 0, 0, 1, 0, 255, 255, 255, 255, 1, 32, 0, 0, 2, 0, 255, 255, 255, 255, 0, 32, 1, 0, 0, 0, 0, 0, 0, 3, 0, 1, 0, 0, 3, 0, 0, 0, 1, 0, 2, 0, 0, 0]
  }
  choreo := generatedNestedRolledChoreo
}

open Hibana Hibana.WireFrameRefinement
namespace Hibana.WireFrameSourceBridge

def candidateRoleLabels (choreo : Choreo) (role : Nat) : List Nat :=
  choreo.canonicalRoleFrameLabels role

theorem generated_role0_labels_match :
    generatedNestedRolledProjectionRole0Exact.image.decodeEventFrameLabels? =
      some (candidateRoleLabels generatedNestedRolledChoreo 0) := by decide

theorem generated_role1_labels_match :
    generatedNestedRolledProjectionRole1Exact.image.decodeEventFrameLabels? =
      some (candidateRoleLabels generatedNestedRolledChoreo 1) := by decide

theorem generated_event_labels_match :
    (List.range 3).map generatedNestedRolledChoreo.canonicalFrameLabel = [0, 0, 1] := by decide

theorem generated_role0_exact_accepted :
    generatedNestedRolledProjectionRole0Exact.check = true := by decide

theorem generated_role1_exact_accepted :
    generatedNestedRolledProjectionRole1Exact.check = true := by decide

theorem final_source_has_refined_labels :
    (canonicalProgramSource generatedNestedRolledChoreo).atoms.map
      ProgramAtomBody.frameLabel = [0, 0, 1] := by decide

theorem structural_baseline_mismatch_is_real :
    (generatedNestedRolledChoreo.compiledOccurrences.occurrences.map CompiledOccurrence.programAtomBody).map ProgramAtomBody.frameLabel = [0, 0, 0] := by decide

theorem source_emitted_nested_domains :
    (canonicalControlSource generatedNestedRolledChoreo).markers =
      [⟨0, 8192, 0⟩, ⟨0, 8193, 0⟩, ⟨2, 8193, 2⟩, ⟨3, 8192, 2⟩] := by decide

theorem source_owners_are_inner_inner_outer :
    let markers := (canonicalControlSource generatedNestedRolledChoreo).markers
    (List.range 3).map (elasticFrameOwner markers) = [2, 2, 1] := by decide

#print axioms generated_role0_labels_match
#print axioms generated_role1_labels_match
#print axioms structural_baseline_mismatch_is_real
#print axioms source_emitted_nested_domains
#print axioms source_owners_are_inner_inner_outer
end Hibana.WireFrameSourceBridge
