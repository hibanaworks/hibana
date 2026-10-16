import Projection

namespace Hibana.EndpointDelegation

def Scope.contains (scope : Scope) (event : Nat) : Bool :=
  scope.start ≤ event && event < scope.stop

/-- The reference uses fully interpreted source paths, not Rust's actor DAG or
borrowed query contexts. A receive teaches only after its causal predecessor
has completed inside the branch whose choice is being checked. -/
def receivesChoiceBefore (source : Source) (scope : Scope) (actor event : Nat)
    (row : Nat × Binding) : Bool :=
  scope.contains row.1 && row.2.message.sender != row.2.message.receiver &&
    row.2.receiverActor == actor && source.before row.1 event

def senderKnowsChoice (source : Source) (scope : Scope) (controller : Nat)
    (bindings : List (Nat × Binding)) (row : Nat × Binding) : Bool :=
  row.2.senderActor == controller ||
    bindings.any (receivesChoiceBefore source scope row.2.senderActor row.1)

def participants (scope : Scope) (bindings : List (Nat × Binding)) : List Nat :=
  (bindings.filter (fun row => scope.contains row.1)).flatMap fun row =>
    [row.2.senderActor, row.2.receiverActor]

def otherArm : Arm → Arm
  | .left => .right
  | .right => .left

def flipSelection (path : List Selection) (scope : Scope) : List Selection :=
  path.map fun selection =>
    if selection.scope == scope then { selection with arm := otherArm selection.arm }
    else selection

def sameParticipants (scope : Scope) (controller : Nat)
    (left right : List (Nat × Binding)) : Bool :=
  let leftActors := participants scope left
  let rightActors := participants scope right
  (leftActors ++ rightActors).all fun actor =>
    actor == controller || (leftActors.contains actor == rightActors.contains actor)

def pathKnowsChoices (source : Source) (runs : List NormalizedPath)
    (path : NormalizedPath) : Bool :=
  (source.scopes 0).all fun scope =>
    match path.bindings.find? (fun row => scope.contains row.1) with
    | none => true -- The scope is inside an unselected outer alternative.
    | some (_, first) =>
        match runs.find? (fun other => other.selections == flipSelection path.selections scope) with
        | none => false
        | some other =>
            sameParticipants scope first.senderActor path.bindings other.bindings &&
            path.bindings.all fun row =>
              !scope.contains row.1 || senderKnowsChoice source scope first.senderActor path.bindings row

def branchKnowledge (source : Source) (normalization : Normalization) : Bool :=
  normalization.paths.all (pathKnowsChoices source normalization.paths)

theorem a_received_choice_applies_to_every_view_of_the_same_actor
    (source : Source) (scope : Scope) (actor other event : Nat)
    (row : Nat × Binding) (sameActor : actor = other) :
    receivesChoiceBefore source scope actor event row =
      receivesChoiceBefore source scope other event row := by
  subst other
  rfl

theorem a_receive_outside_the_choice_cannot_teach_it
    (source : Source) (scope : Scope) (actor event : Nat) (row : Nat × Binding)
    (outside : scope.contains row.1 = false) :
    receivesChoiceBefore source scope actor event row = false := by
  simp [receivesChoiceBefore, outside]

theorem a_parallel_siblings_receive_cannot_teach_an_independent_send
    (left right : Source) (scope : Scope) (actor : Nat) (row : Nat × Binding)
    (event : Nat) (inLeft : row.1 < left.eventCount) (inRight : left.eventCount ≤ event) :
    receivesChoiceBefore (.par left right) scope actor event row = false := by
  simp [receivesChoiceBefore,
    independent_parallel_siblings_have_no_local_order left right inLeft inRight]

#print axioms a_received_choice_applies_to_every_view_of_the_same_actor
#print axioms a_receive_outside_the_choice_cannot_teach_it
#print axioms a_parallel_siblings_receive_cannot_teach_an_independent_send

end Hibana.EndpointDelegation
