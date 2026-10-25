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

def smtTable (name : String) (values : Array Bool) : String :=
  let terms := values.toList.zipIdx.filterMap fun (value, index) =>
    if value then some s!"(= index {index})" else none
  s!"(define-fun {name} () Bool (or false {String.intercalate " " terms}))\n"

def main (args : List String) : IO UInt32 := do
  let [input, output] := args
    | throw (IO.userError "usage: CheckCompletion OBSERVATIONS.jsonl OUTPUT.smt2")
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
  let mut smt := "(set-logic QF_LIA)\n(declare-const index Int)\n(assert (and (<= 0 index) (< index 128)))\n"
  for (name, values) in [
      ("observed", actual.map (·.complete)), ("model", complete),
      ("observedA", actual.map (·.retainA)), ("modelA", actual.map (observedCredit · 0)),
      ("observedB", actual.map (·.retainB)), ("modelB", actual.map (observedCredit · 1)),
      ("strict", strict), ("loose", loose)] do
    smt := smt ++ smtTable name values
  for (left, right) in [("observed", "model"), ("observedA", "modelA"),
      ("observedB", "modelB"), ("strict", "model"), ("loose", "model")] do
    smt := smt ++ s!"(push)\n(assert (distinct {left} {right}))\n(check-sat)\n(pop)\n"
  -- The SMT truth tables are evaluated from this same executable Lean model;
  -- no separate SMT expression restates the completion rule.
  IO.FS.writeFile output smt
  for (o, index) in actual.toList.zipIdx do
    unless o.complete == observedCompletion o &&
        o.retainA == observedCredit o 0 && o.retainB == observedCredit o 1 do
      throw (IO.userError s!"completion mismatch at {index}: bits={o.bits} credits={o.credits} entry={o.entry}")
  IO.println "Parallel roll completion: 128 cursor states, 384 comparisons; strict and loose controls detected"
  pure 0
