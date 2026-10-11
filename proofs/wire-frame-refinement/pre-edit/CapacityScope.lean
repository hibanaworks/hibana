import Candidate

namespace Hibana.WireAllocatorCapacityScope
open Hibana.WireAllocatorCandidate

/-- An unrelated role can omit an invalid global row. Therefore per-role exact
    rejection requires the explicit retained-row premise below; it does not
    prove global Rust capacity rejection for arbitrary synthetic certificates. -/
theorem unrelated_role_can_omit_invalid_row :
    ([({ sender := 0, receiver := 1, label := 0, schema := 0,
          origin := 0, lane := 0, frameLabel := 256 } : ProgramAtomBody)].filterMap
      fun atom => if (Choreo.localAction? 2 atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) = [] := by decide

theorem participating_role_retains_invalid_row :
    ([({ sender := 0, receiver := 1, label := 0, schema := 0,
          origin := 0, lane := 0, frameLabel := 256 } : ProgramAtomBody)].filterMap
      fun atom => if (Choreo.localAction? 1 atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) = [256] := by decide

theorem retained_overflow_rejects_role_admission
    (image : RustDescriptorImage) (atoms : List ProgramAtomBody) (role : Nat)
    (retained : 256 ∈ atoms.filterMap fun atom =>
      if (Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) :
    image.decodeEventFrameLabels? ≠ some (atoms.filterMap fun atom =>
      if (Choreo.localAction? role atom.sender atom.receiver atom.label atom.schema).isSome
        then some atom.frameLabel else none) :=
  exhausted_row_rejects_exact_decoded_labels image _ retained

#print axioms unrelated_role_can_omit_invalid_row
#print axioms participating_role_retains_invalid_row
#print axioms retained_overflow_rejects_role_admission
end Hibana.WireAllocatorCapacityScope
