import Lean
import ResetAlignment

open ParallelVisitCompletion

structure CompletionObservation where
  bits : Nat
  credits : Nat
  entry : Nat
  complete : Bool
  retainA : Bool
  retainB : Bool
  deriving Lean.FromJson

-- The Rust fixture is roll(par(Prefix, par(roll(A), roll(B)))).
-- These indices come from that source, independently of the runtime's scope
-- and completion queries. Entry zero means no candidate has been selected.
def observedCompletion (o : CompletionObservation) : Bool :=
  completeAt [0, 1, 2]
    (fun row => o.bits / 2 ^ row % 2 == 1)
    (fun row => row > 0 && o.credits / 2 ^ (row - 1) % 2 == 1)
    (fun row entry => entry > 0 && row + 1 != entry) o.entry

def observedCredit (o : CompletionObservation) (child : Nat) : Bool :=
  o.credits / 2 ^ child % 2 == 1 || observedCompletion o

def main (args : List String) : IO UInt32 := do
  let [input] := args
    | throw (IO.userError "usage: CheckCompletion OBSERVATIONS.jsonl")
  let lines := (← IO.FS.readFile input).splitOn "\n"
  unless lines.getLast! == "" && lines.length == 129 do
    throw (IO.userError "missing or extra completion observation")
  let mut actual : Array CompletionObservation := #[]
  for (line, index) in (lines.take 128).zipIdx do
    let value ← match Lean.Json.parse line with
      | .ok value => pure value
      | .error error => throw (IO.userError error)
    let o : CompletionObservation ← match Lean.fromJson? value with
      | .ok value => pure value
      | .error error => throw (IO.userError error)
    unless o.bits == index / 16 && o.credits == index / 4 % 4 && o.entry == index % 4 do
      throw (IO.userError "wrong, duplicate or reordered completion coordinates")
    actual := actual.push o
  let complete := actual.map observedCompletion
  let strict := actual.map (fun o => o.bits == 7)
  let loose := actual.map fun o =>
    completeAt [0, 1, 2]
      (fun row => o.bits / 2 ^ row % 2 == 1)
      (fun row => row > 0 && o.credits / 2 ^ (row - 1) % 2 == 1)
      (fun _ _ => true) o.entry
  unless complete != strict && complete != loose do
    throw (IO.userError "completion inventory cannot detect both incorrect rules")
  for (o, index) in actual.toList.zipIdx do
    unless o.complete == observedCompletion o &&
        o.retainA == observedCredit o 0 && o.retainB == observedCredit o 1 do
      throw (IO.userError s!"completion mismatch at {index}: bits={o.bits} credits={o.credits} entry={o.entry}")
  IO.println "Parallel roll completion: 128 cursor states, 384 comparisons; strict and loose controls detected"
  pure 0
