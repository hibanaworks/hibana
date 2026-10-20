-- Appended after the checker and small, regenerated source/image inputs.
-- Only observations arrive as JSON. No observed success selects a source plan.

def loadObservations (inputs : List ConformanceCase) (observations : List CaseObservations) :
    Except String (List ConformanceCase) := do
  unless (observations.map (·.name)).mergeSort (· ≤ ·) ==
      (inputs.map (·.name)).mergeSort (· ≤ ·) do
    throw "missing, duplicate or unknown conformance case"
  inputs.mapM fun input => do
    let some observed := resolveUnique? (observations.filter (·.name == input.name))
      | throw s!"ambiguous observations: {input.name}"
    pure { input with attempts := observed.attempts }

def readObservationFile [Lean.FromJson α] (path : String) : IO α := do
  let text ← IO.FS.readFile path
  let parsed := Lean.Json.parse text >>= Lean.fromJson?
  match parsed with
  | .ok value => pure value
  | .error error => throw (IO.userError s!"invalid observations in {path}: {error}")

def main (args : List String) : IO UInt32 := do
  match args with
  | ["completed", path] =>
    let observations : List CaseObservations ← readObservationFile path
    let cases ← match loadObservations conformanceInputs observations with
      | .ok cases => pure cases
      | .error error => throw (IO.userError error)
    -- Parsing or an empty/misnamed data file cannot silently omit a case.
    let some first := observations.head? | throw (IO.userError "empty observations")
    match loadObservations conformanceInputs observations.tail,
        loadObservations conformanceInputs (first :: observations) with
    | .error _, .error _ => pure ()
    | _, _ => throw (IO.userError "case inventory controls failed")
    checkCases cases
    pure 0
  | ["phased", path] =>
    let attempts : List PhaseAttempt ← readObservationFile path
    let some input := resolveUnique? (conformanceInputs.filter (·.name == "owned-par-roll"))
      | throw (IO.userError "missing phased source/image input")
    checkPhases input.choreo input.images attempts
    pure 0
  | _ => throw (IO.userError "usage: endpoint-conformance completed|phased OBSERVATIONS.json")
