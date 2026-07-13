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

def classifyPublicOperation
    (current expected : PublicOperationPhase) : PublicOperationLease :=
  if current = .poisoned then
    .faulted
  else if current = expected then
    .held
  else
    .rejected

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

def exactPublicOperationTable : List PublicOperationLease :=
  publicOperationPhases.flatMap fun current =>
    publicOperationPhases.map fun expected =>
      classifyPublicOperation current expected

def PublicOperationTableExact
    (table : List PublicOperationLease) : Prop :=
  table = exactPublicOperationTable

instance (table : List PublicOperationLease) :
    Decidable (PublicOperationTableExact table) := by
  unfold PublicOperationTableExact
  infer_instance

def checkPublicOperationTable
    (table : List PublicOperationLease) : Bool :=
  decide (PublicOperationTableExact table)

theorem public_operation_table_certificate_sound
    {table : List PublicOperationLease}
    (accepted : checkPublicOperationTable table = true) :
    PublicOperationTableExact table :=
  of_decide_eq_true accepted

theorem poisoned_public_operation_is_faulted
    (expected : PublicOperationPhase) :
    classifyPublicOperation .poisoned expected = .faulted := by
  simp [classifyPublicOperation]

theorem matching_live_public_operation_is_held
    {phase : PublicOperationPhase}
    (live : phase ≠ .poisoned) :
    classifyPublicOperation phase phase = .held := by
  simp [classifyPublicOperation, live]

theorem mismatched_live_public_operation_is_rejected
    {current expected : PublicOperationPhase}
    (live : current ≠ .poisoned)
    (different : current ≠ expected) :
    classifyPublicOperation current expected = .rejected := by
  simp [classifyPublicOperation, live, different]

end Hibana
