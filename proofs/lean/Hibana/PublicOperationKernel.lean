namespace Hibana

/-- Exact proof vocabulary for the production endpoint operation phase. It is
used only to validate the generated classifier table. -/
inductive PublicOperationPhase where
  | idle
  | poisoned
  | send
  | recv
  | offer
  | routeBranch
  | restoredRouteBranch
  | branchRecv
  | branchSend
  deriving Repr, DecidableEq

inductive PublicOperationLease where
  | rejected
  | held
  | faulted
  deriving Repr, DecidableEq

structure PublicOperationTransition where
  lease : PublicOperationLease
  phase : PublicOperationPhase
  deriving Repr, DecidableEq

inductive PublicOperationEdge where
  | beginOffer
  | resumeOffer
  | publishRouteBranch
  | finishOffer
  | parkOffer
  | parkRouteBranch
  | beginSend
  | beginBranchSend
  | finishSend
  | finishBranchSend
  | parkBranchSend
  | beginRecv
  | beginBranchRecv
  | finishRecv
  | finishBranchRecv
  | parkBranchRecv
  deriving Repr, DecidableEq

def PublicOperationEdge.expected : PublicOperationEdge → PublicOperationPhase
  | .beginOffer | .beginSend | .beginRecv => .idle
  | .resumeOffer => .restoredRouteBranch
  | .publishRouteBranch | .finishOffer | .parkOffer => .offer
  | .parkRouteBranch | .beginBranchSend | .beginBranchRecv => .routeBranch
  | .finishSend => .send
  | .finishBranchSend | .parkBranchSend => .branchSend
  | .finishRecv => .recv
  | .finishBranchRecv | .parkBranchRecv => .branchRecv

def PublicOperationEdge.next : PublicOperationEdge → PublicOperationPhase
  | .beginOffer | .resumeOffer => .offer
  | .publishRouteBranch => .routeBranch
  | .parkOffer | .parkRouteBranch | .parkBranchSend | .parkBranchRecv =>
      .restoredRouteBranch
  | .beginSend => .send
  | .beginBranchSend => .branchSend
  | .beginRecv => .recv
  | .beginBranchRecv => .branchRecv
  | .finishOffer | .finishSend | .finishBranchSend | .finishRecv |
      .finishBranchRecv => .idle

def transitionPublicOperation
    (current : PublicOperationPhase)
    (edge : PublicOperationEdge) : PublicOperationTransition :=
  if current = .poisoned then
    { lease := .faulted, phase := .poisoned }
  else if current = edge.expected then
    { lease := .held, phase := edge.next }
  else
    { lease := .rejected, phase := .poisoned }

def clearPublicOperationIfCurrent
    (current expected : PublicOperationPhase) : PublicOperationPhase :=
  if current = expected then .idle else current

def clearPublicOperationTerminal
    (_current : PublicOperationPhase) : PublicOperationPhase :=
  .idle

def faultPublicOperation
    (_current : PublicOperationPhase) : PublicOperationPhase :=
  .poisoned

def publicOperationPhases : List PublicOperationPhase := [
  .idle,
  .poisoned,
  .send,
  .recv,
  .offer,
  .routeBranch,
  .restoredRouteBranch,
  .branchRecv,
  .branchSend
]

def publicOperationEdges : List PublicOperationEdge := [
  .beginOffer,
  .resumeOffer,
  .publishRouteBranch,
  .finishOffer,
  .parkOffer,
  .parkRouteBranch,
  .beginSend,
  .beginBranchSend,
  .finishSend,
  .finishBranchSend,
  .parkBranchSend,
  .beginRecv,
  .beginBranchRecv,
  .finishRecv,
  .finishBranchRecv,
  .parkBranchRecv
]

theorem public_operation_phase_inventory_exact :
    publicOperationPhases.Nodup ∧
      ∀ phase : PublicOperationPhase, phase ∈ publicOperationPhases := by
  constructor
  · decide
  · intro phase
    cases phase <;> decide

theorem public_operation_edge_inventory_exact :
    publicOperationEdges.Nodup ∧
      ∀ edge : PublicOperationEdge, edge ∈ publicOperationEdges := by
  constructor
  · decide
  · intro edge
    cases edge <;> decide

theorem public_operation_edge_expected_is_live
    (edge : PublicOperationEdge) : edge.expected ≠ .poisoned := by
  cases edge <;> decide

theorem public_operation_edge_success_is_live
    (edge : PublicOperationEdge) : edge.next ≠ .poisoned := by
  cases edge <;> decide

def exactPublicOperationTable : List PublicOperationTransition :=
  publicOperationPhases.flatMap fun current =>
    publicOperationEdges.map fun edge =>
      transitionPublicOperation current edge

theorem exact_public_operation_table_covers_every_transition
    (current : PublicOperationPhase) (edge : PublicOperationEdge) :
    transitionPublicOperation current edge ∈ exactPublicOperationTable := by
  unfold exactPublicOperationTable
  apply List.mem_flatMap.mpr
  exact ⟨current, public_operation_phase_inventory_exact.2 current,
    List.mem_map.mpr ⟨edge, public_operation_edge_inventory_exact.2 edge, rfl⟩⟩

def PublicOperationTableExact
    (table : List PublicOperationTransition) : Prop :=
  table = exactPublicOperationTable

instance (table : List PublicOperationTransition) :
    Decidable (PublicOperationTableExact table) := by
  unfold PublicOperationTableExact
  infer_instance

def checkPublicOperationTable
    (table : List PublicOperationTransition) : Bool :=
  decide (PublicOperationTableExact table)

theorem public_operation_table_certificate_sound
    {table : List PublicOperationTransition}
    (accepted : checkPublicOperationTable table = true) :
    PublicOperationTableExact table :=
  of_decide_eq_true accepted

theorem poisoned_public_operation_remains_faulted
    (edge : PublicOperationEdge) :
    transitionPublicOperation .poisoned edge =
      { lease := .faulted, phase := .poisoned } := by
  simp [transitionPublicOperation]

theorem matching_live_public_operation_transitions_exactly
    (edge : PublicOperationEdge) :
    transitionPublicOperation edge.expected edge =
      { lease := .held, phase := edge.next } := by
  simp [transitionPublicOperation, public_operation_edge_expected_is_live]

theorem mismatched_live_public_operation_fails_closed
    {current : PublicOperationPhase}
    (live : current ≠ .poisoned)
    (edge : PublicOperationEdge)
    (different : current ≠ edge.expected) :
    transitionPublicOperation current edge =
      { lease := .rejected, phase := .poisoned } := by
  simp [transitionPublicOperation, live, different]

theorem conditional_public_operation_clear_is_exact
    (current expected : PublicOperationPhase) :
    clearPublicOperationIfCurrent current expected =
      if current = expected then .idle else current := by
  rfl

theorem terminal_public_operation_clear_is_exact
    (current : PublicOperationPhase) :
    clearPublicOperationTerminal current = .idle := by
  rfl

theorem public_operation_fault_is_exact
    (current : PublicOperationPhase) :
    faultPublicOperation current = .poisoned := by
  rfl

end Hibana
