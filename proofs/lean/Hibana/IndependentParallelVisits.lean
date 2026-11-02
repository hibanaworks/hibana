import Hibana.SourceResumptionProgress

namespace Hibana
open EndpointDelegation

private def resource : EndpointKey := ⟨0, 3⟩
private def loan : Message := ⟨⟨0, 0⟩, ⟨0, 1⟩, 0, .endpoint resource⟩
private def work : Message := ⟨resource, ⟨0, 2⟩, 1, .wire 0⟩
private def control : Message := ⟨⟨0, 4⟩, ⟨0, 5⟩, 2, .wire 0⟩
private def returned : Message := ⟨⟨0, 1⟩, ⟨0, 0⟩, 3, .endpoint resource⟩
private def source : Source := .seq (.send loan)
  (.seq (.roll (.par (.send work) (.send control))) (.send returned))
private def expanded : Source := .seq (.send loan)
  (.seq (.par (.send work) (.seq (.send control) (.send control))) (.send returned))
private def unfolding : SourceUnfolding source expanded :=
  .seq (.send _) (.seq
    (.rollParallel (.rollOnce (.send _)) (.rollMore (.rollOnce (.send _)) (.send _)))
    (.send _))
private def original : Normalization := (normalize source).get (by decide)
private def normalized : Normalization :=
  (normalizeExpansion original.issuance expanded).get (by decide)
private def plan : SourceOccurrenceCut source := {
  original
  originalAccepted := by decide
  expanded
  unfolding
  normalized
  accepted := by decide
  sameIssuance := by decide
  path := normalized.paths[0]'(by decide)
  selected := by decide
  separated := by decide
  guarded := by decide
  observed := fun _ => false
}
private def prefixPoints : List SourcePoint := [0, 1, 2, 4].flatMap fun event =>
  [.accepted event, .committed ⟨event, .send⟩, .committed ⟨event, .receive⟩]

/-- This is the source of the real Endpoint regression: the resource branch
has finished and returned its Endpoint; a disjoint ordinary branch revisits. -/
theorem IndependentParallelVisits.the_resource_return_prefix_is_legal_with_unequal_parallel_visits :
    (prefixPoints.zipIdx.all fun (point, index) =>
      (occurrenceCut plan (prefixPoints.take index)).ready point) = true := by decide

theorem IndependentParallelVisits.a_disjoint_branch_can_revisit_after_the_resource_returns :
    ((occurrenceCut plan prefixPoints).ready (.accepted 3) &&
      (occurrenceCut plan prefixPoints).resumptionReady (.accepted 3)) = true := by decide

theorem IndependentParallelVisits.unequal_visits_keep_the_original_source_and_resource_actor :
    (unfolding.origins 0) = [0, 1, 2, 2, 3] ∧
      (plan.path.bindings.find? (fun entry => entry.1 == 1)).map
        (fun entry => entry.2.senderActor) = some 1 := by decide

theorem IndependentParallelVisits.another_visit_does_not_reuse_the_first_occurrence :
    (occurrenceCut plan prefixPoints).ready (.accepted 2) = false := by decide

/-- The logical endpoints {3,2} and {1,5} are distinct, but role 1 currently
holds resource 3. They therefore cannot receive independent visit counts. -/
theorem IndependentParallelVisits.delegated_aliases_do_not_establish_physical_separation :
    (SourceParallelVisits.check ⟨[0], [1]⟩
      [(0, ⟨work, 1, 2⟩), (1, ⟨⟨⟨0, 1⟩, ⟨0, 5⟩, 9, .wire 0⟩, 1, 5⟩)]) = false := by decide

#print axioms IndependentParallelVisits.the_resource_return_prefix_is_legal_with_unequal_parallel_visits
#print axioms IndependentParallelVisits.a_disjoint_branch_can_revisit_after_the_resource_returns
#print axioms IndependentParallelVisits.unequal_visits_keep_the_original_source_and_resource_actor
#print axioms IndependentParallelVisits.another_visit_does_not_reuse_the_first_occurrence
#print axioms IndependentParallelVisits.delegated_aliases_do_not_establish_physical_separation

end Hibana
