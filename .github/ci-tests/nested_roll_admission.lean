-- Appended to endpoint_conformance.lean to reuse checkedPlans.
namespace NestedRollAdmission

abbrev Expansion (source : Source) := Σ expanded : Source, SourceUnfolding source expanded

def message (sender receiver label : Nat) : Source :=
  .send ⟨⟨0, sender⟩, ⟨0, receiver⟩, label, .wire 0⟩

def prefixMessage := message 0 1 231
def exchange := Source.seq (message 0 1 232) (message 1 0 233)
def tickMessage := message 0 2 234
def middle := Source.par (.roll exchange) tickMessage
def body := Source.seq prefixMessage (.roll middle)
def family := Source.roll body

def one (source : Source) : Expansion source := ⟨source.oneCopy, .oneCopy source⟩

-- Every generated expansion carries the existing model's unfolding witness.
-- Bounds count message occurrences, not runtime resets or successful calls.
def rolls (source : Source) (parts : List (Expansion source)) :
    Nat → Nat → List (Expansion (.roll source))
  | 0, _ => []
  | fuel + 1, budget => parts.flatMap fun part =>
      if part.1.eventCount == 0 || part.1.eventCount > budget then [] else
      ⟨part.1, .rollOnce part.2⟩ ::
        (rolls source parts fuel (budget - part.1.eventCount)).map fun earlier =>
          ⟨.seq earlier.1 part.1, .rollMore earlier.2 part.2⟩

def expansions (budget : Nat) : List (Expansion family) :=
  let inner := rolls exchange [one exchange] budget budget
  let middles : List (Expansion middle) := inner.map fun plan =>
    ⟨.par plan.1 tickMessage.oneCopy, .par plan.2 (.oneCopy tickMessage)⟩
  let repeated := rolls middle middles budget budget
  let bodies : List (Expansion body) := repeated.map fun plan =>
    ⟨.seq prefixMessage.oneCopy plan.1, .seq (.oneCopy prefixMessage) plan.2⟩
  rolls body bodies budget budget

abbrev State := SourceOccurrenceCut family × List SourcePoint

def label (letter : Nat) : Nat :=
  if letter == 0 then 231 else if letter == 1 then 232 else 234

def completeMessage (plan : SourceOccurrenceCut family) (points : List SourcePoint)
    (event : Nat) : Option (List SourcePoint) :=
  [.accepted event, .committed ⟨event, .send⟩, .committed ⟨event, .receive⟩].foldlM
    (fun points point =>
      if (occurrenceCut plan points).ready point then some (points ++ [point]) else none) points

def permitted (states : List State) (letter : Nat) : Bool :=
  states.any fun (plan, points) => plan.path.bindings.any fun (event, binding) =>
    binding.message.label == label letter &&
      (occurrenceCut plan points).ready (.accepted event)

def advance (states : List State) (letter : Nat) : List State :=
  states.flatMap fun (plan, points) => plan.path.bindings.filterMap fun (event, binding) => do
    if binding.message.label != label letter then none else do
      let points ← completeMessage plan points event
      let points ← if letter == 1 then do
        let (_, reply) ← plan.path.bindings.find? (fun entry => entry.1 == event + 1)
        if reply.message.label != 233 then none else completeMessage plan points (event + 1)
        else some points
      pure (plan, points)

structure Query where
  «prefix» : List Nat
  probe : Nat
  permitted : Bool
  deriving BEq, Inhabited

def parallelAt (source : Source) (start event : Nat) : Option Nat :=
  match source with
  | .send _ => none
  | .seq left right | .route _ left right =>
      if event < start + left.eventCount then parallelAt left start event
      else parallelAt right (start + left.eventCount) event
  | .par left right =>
      let nested := if event < start + left.eventCount then parallelAt left start event
        else parallelAt right (start + left.eventCount) event
      match nested with
      | some group => some group
      | none => some (start + 1)
  | .roll inner => parallelAt inner start event

def inputEvents (state : State) : List Nat :=
  state.2.filterMap fun point =>
    match point with
    | .committed _ => none
    | .accepted event =>
        match state.1.path.bindings.find? (fun pair => pair.1 == event) with
        | none => none
        | some (_, binding) =>
            if binding.message.label == 233 then none else some event

def group (plan : SourceOccurrenceCut family) (event : Nat) : Nat :=
  match parallelAt plan.expanded 0 event with
  | some group => group
  | none => 0

structure ScopedQuery where
  query : Query
  groups : List Nat
  deriving BEq

def canonicalGroups (groups : List Nat) : List Nat := Id.run do
  let mut seen := []
  let mut result := []
  for group in groups do
    if group == 0 then result := result ++ [0] else
      if !seen.contains group then seen := seen ++ [group]
      result := result ++ [seen.idxOf group + 1]
  return result

def scopedQueries (query : Query) (state : State) : List ScopedQuery := Id.run do
  let events := inputEvents state
  let supplied := events.filterMap fun event =>
    (state.1.path.bindings.find? (fun pair => pair.1 == event)).map (·.2.message.label)
  if supplied != query.prefix.map label then panic! "source witness prefix mismatch"
  let groups := events.map (group state.1)
  let probes := if query.permitted then
      state.1.path.bindings.filterMap fun (event, binding) =>
        if binding.message.label == label query.probe &&
          (occurrenceCut state.1 state.2).ready (.accepted event)
        then some (group state.1 event) else none
    else [groups.getLast?.getD 0]
  return probes.map fun probe => ⟨query, canonicalGroups (groups ++ [probe])⟩

def reference (bound : Nat) : List Query × List ScopedQuery × Nat := Id.run do
  -- A prefix with at most bound+1 letters can finish with at most two more
  -- letters. An exchange contributes two message occurrences.
  let plans := (expansions (2 * (bound + 3))).flatMap fun plan =>
    checkedPlans family plan.1 plan.2
  let mut level : List (List Nat × List State) := [([], plans.map (·, []))]
  let mut queries := []
  let mut witnesses := []
  for depth in List.range (bound + 1) do
    let mut next := []
    for (word, states) in level do
      for letter in [0, 1, 2] do
        let query := ⟨word, letter, permitted states letter⟩
        queries := queries ++ [query]
        let supplied := (states.flatMap (scopedQueries query)).eraseDups
        if supplied.isEmpty then panic! "no accepted source witness"
        witnesses := witnesses ++ supplied
        if depth < bound then
          let advanced := advance states letter
          if !advanced.isEmpty then next := next ++ [(word ++ [letter], advanced)]
    level := next
  return (queries, witnesses, plans.length)

def keyLess (left right : Query) : Bool :=
  if left.prefix == right.prefix then left.probe < right.probe
  else decide (left.prefix < right.prefix)

def compare (expected actual : List Query) : Except String Unit := do
  let expected := expected.mergeSort keyLess
  let actual := actual.mergeSort keyLess
  if expected.map (fun q => (q.prefix, q.probe)) !=
      actual.map (fun q => (q.prefix, q.probe)) then
    throw "incomplete, extra or duplicate prefix/query inventory"
  for (model, supplied) in expected.zip actual do
    if model.permitted != supplied.permitted then
      throw s!"prefix={model.prefix} probe={model.probe}: source={model.permitted} reference={supplied.permitted}"

def encode (queries : List Query) : ByteArray := Id.run do
  let mut bytes := ByteArray.mk #[72, 78, 82, 1]
  for shift in [24, 16, 8, 0] do
    bytes := bytes.push (UInt8.ofNat (queries.length / 2 ^ shift % 256))
  for query in queries do
    bytes := bytes.push (UInt8.ofNat query.prefix.length)
    for letter in query.prefix do bytes := bytes.push (UInt8.ofNat letter)
    bytes := bytes.push (UInt8.ofNat query.probe)
    bytes := bytes.push (if query.permitted then 1 else 0)
  return bytes

def encodeScoped (queries : List ScopedQuery) : ByteArray := Id.run do
  let mut bytes := ByteArray.mk #[72, 78, 83, 1]
  for shift in [24, 16, 8, 0] do
    bytes := bytes.push (UInt8.ofNat (queries.length / 2 ^ shift % 256))
  for witnesses in queries do
    let query := witnesses.query
    bytes := bytes.push (UInt8.ofNat query.prefix.length)
    for letter in query.prefix do bytes := bytes.push (UInt8.ofNat letter)
    bytes := bytes.push (UInt8.ofNat query.probe)
    bytes := bytes.push (if query.permitted then 1 else 0)
    if witnesses.groups.length != query.prefix.length + 1 then panic! "invalid source grouping"
    for group in witnesses.groups do
      if group > 65535 then panic! "source group exceeds encoding"
      bytes := (bytes.push (UInt8.ofNat (group / 256))).push (UInt8.ofNat (group % 256))
  return bytes

def decodeRows : Nat → List Nat → Except String (List Query)
  | 0, [] => pure []
  | 0, _ => throw "surplus encoded queries"
  | count + 1, size :: bytes => do
      let word := bytes.take size
      let probe :: result :: tail := bytes.drop size | throw "truncated encoded query"
      if size > 7 || word.length != size || !word.all (· < 3) || probe > 2 || result > 1 then
        throw "invalid encoded query"
      let rest ← decodeRows count tail
      pure (⟨word, probe, result == 1⟩ :: rest)
  | _ + 1, [] => throw "missing encoded query"

def decode (bytes : ByteArray) : Except String (List Query) := do
  let 72 :: 78 :: 82 :: 1 :: a :: b :: c :: d :: rest := bytes.toList.map UInt8.toNat
    | throw "invalid encoded query header"
  let count := a * 16777216 + b * 65536 + c * 256 + d
  if count > rest.length / 3 then throw "invalid encoded query count"
  decodeRows count rest

def decodeScopedRows : Nat → List Nat → Except String (List ScopedQuery)
  | 0, [] => pure []
  | 0, _ => throw "surplus scoped queries"
  | count + 1, size :: bytes => do
      let word := bytes.take size
      let probe :: result :: tail := bytes.drop size | throw "truncated scoped query"
      if size > 7 || word.length != size || !word.all (· < 3) || probe > 2 || result > 1 then
        throw "invalid scoped query"
      let groupBytes := tail.take (2 * (size + 1))
      if groupBytes.length != 2 * (size + 1) then throw "truncated source groups"
      let groups := (List.range (size + 1)).map fun index =>
        groupBytes[2 * index]! * 256 + groupBytes[2 * index + 1]!
      if canonicalGroups groups != groups then throw "noncanonical source groups"
      let rest ← decodeScopedRows count (tail.drop groupBytes.length)
      pure (⟨⟨word, probe, result == 1⟩, groups⟩ :: rest)
  | _ + 1, [] => throw "missing scoped query"

def decodeScoped (bytes : ByteArray) : Except String (List ScopedQuery) := do
  let 72 :: 78 :: 83 :: 1 :: a :: b :: c :: d :: rest := bytes.toList.map UInt8.toNat
    | throw "invalid scoped query header"
  let count := a * 16777216 + b * 65536 + c * 256 + d
  if count > rest.length / 5 then throw "invalid scoped query count"
  decodeScopedRows count rest

end NestedRollAdmission

def main (args : List String) : IO UInt32 := do
  let [output, scopedOutput, resumedOutput] := args
    | throw (IO.userError "usage: nested-roll-admission OUTPUT.bin SCOPED.bin RESUMED.bin")
  let (expected, witnesses, plans) := NestedRollAdmission.reference 7
  let encoded := NestedRollAdmission.encode expected
  let queries ←
    match NestedRollAdmission.decode encoded with
    | .ok queries => pure queries
    | .error error => throw (IO.userError error)
  match NestedRollAdmission.compare expected queries with
  | .error error => throw (IO.userError error)
  | .ok () => pure ()
  let some first := queries.head? | throw (IO.userError "empty reference matrix")
  for invalid in [queries.tail!, first :: queries] do
    if (NestedRollAdmission.compare expected invalid).isOk then
      throw (IO.userError "invalid reference matrix was accepted")
  for outcome in [false, true] do
    let some changed := queries.find? (fun q => q.permitted == outcome)
      | throw (IO.userError "missing allowed or forbidden control")
    let invalid := queries.map fun q =>
      if q == changed then {q with permitted := !q.permitted} else q
    if (NestedRollAdmission.compare expected invalid).isOk then
      throw (IO.userError "flipped reference result was accepted")
  let scopedEncoded := NestedRollAdmission.encodeScoped witnesses
  let supplied ← match NestedRollAdmission.decodeScoped scopedEncoded with
    | .ok supplied => pure supplied
    | .error error => throw (IO.userError error)
  if supplied != witnesses || (witnesses.map (·.query)).eraseDups != expected then
    throw (IO.userError "incomplete scoped source inventory or codec mismatch")
  let some firstScoped := supplied.head? | throw (IO.userError "empty scoped matrix")
  for invalid in [supplied.tail!, firstScoped :: supplied,
      {firstScoped with groups := [65535]} :: supplied.tail!,
      {firstScoped with query := {firstScoped.query with permitted := !firstScoped.query.permitted}}
        :: supplied.tail!] do
    if invalid == witnesses then throw (IO.userError "invalid scoped matrix was accepted")
  if (NestedRollAdmission.decodeScoped (scopedEncoded.push 0)).isOk ||
      (NestedRollAdmission.decodeScoped (scopedEncoded.extract 0 (scopedEncoded.size - 1))).isOk then
    throw (IO.userError "invalid scoped encoding was accepted")
  IO.FS.writeBinFile output encoded
  IO.FS.writeBinFile scopedOutput scopedEncoded
  IO.println s!"Nested-roll source cases: plans={plans} queries={expected.length} scoped={witnesses.length} roundtrip=pass controls=pass"
  ResumedRollAdmission.write resumedOutput
  pure 0
