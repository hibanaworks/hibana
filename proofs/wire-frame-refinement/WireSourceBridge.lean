import WireFrameRefinement
import Generated
set_option maxRecDepth 100000

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

def staleRole0RollFlags : ExactDescriptorCertificate := {
  generatedNestedRolledProjectionRole0Exact with
  image := { generatedNestedRolledProjectionRole0Exact.image with
    roleBytes := generatedNestedRolledProjectionRole0Exact.image.roleBytes.set 9 0 }
}

def staleRole1RollFlags : ExactDescriptorCertificate := {
  generatedNestedRolledProjectionRole1Exact with
  image := { generatedNestedRolledProjectionRole1Exact.image with
    roleBytes := generatedNestedRolledProjectionRole1Exact.image.roleBytes.set 9 0 }
}

theorem stale_role0_roll_flags_rejected : staleRole0RollFlags.check = false := by decide

theorem stale_role1_roll_flags_rejected : staleRole1RollFlags.check = false := by decide

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
