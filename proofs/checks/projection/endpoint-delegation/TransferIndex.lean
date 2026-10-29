import Std

namespace Hibana.TransferIndex

structure Key where
  component : BitVec 16
  role : BitVec 8
  deriving DecidableEq

/-- The subtree index records roles; leaves retain the complete endpoint key. -/
def mayTransfer (keys : List Key) (key : Key) : Bool :=
  decide (key.role ∈ keys.map Key.role)

def transfers (keys : List Key) (key : Key) : Bool := decide (key ∈ keys)

theorem exact_key_has_indexed_role (keys : List Key) (key : Key)
    (present : key ∈ keys) : mayTransfer keys key = true := by
  have member : key.role ∈ keys.map Key.role := List.mem_map.mpr ⟨key, present, rfl⟩
  simp [mayTransfer, member]

theorem role_pruning_preserves_complete_keys (keys : List Key) (key : Key) :
    (if mayTransfer keys key then transfers keys key else false) = transfers keys key := by
  by_cases present : key ∈ keys
  · simp [exact_key_has_indexed_role keys key present]
  · simp [transfers, present]

theorem merged_index_covers_exactly_both_children (left right : List Key) (key : Key) :
    mayTransfer (left ++ right) key = (mayTransfer left key || mayTransfer right key) := by
  simp [mayTransfer]

theorem a_repeated_body_preserves_the_index (keys : List Key) (key : Key) :
    mayTransfer (keys ++ keys) key = mayTransfer keys key := by
  simp [merged_index_covers_exactly_both_children]

end Hibana.TransferIndex

#print axioms Hibana.TransferIndex.exact_key_has_indexed_role
#print axioms Hibana.TransferIndex.role_pruning_preserves_complete_keys
#print axioms Hibana.TransferIndex.merged_index_covers_exactly_both_children
#print axioms Hibana.TransferIndex.a_repeated_body_preserves_the_index
