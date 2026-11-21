import AlternateVisitWait
import Hibana.OwnershipDescriptor

open Hibana Hibana.EndpointDelegation Hibana.AlternateVisitWait

private def key (r : Nat) : EndpointKey := ⟨0, r⟩
private def send (fromRole toRole label : Nat) (payload : Payload) : Source :=
  .send ⟨key fromRole, key toRole, label, payload⟩
private def initialLoans (next : Source) : Source :=
  .seq (send 0 1 2 (.endpoint (key 2))) (.seq (send 0 1 3 (.endpoint (key 3))) next)
private def loan (role : Nat) := 4294967296 + role
private def gaugeBody : Choreo :=
  .seq (.route .intrinsic (.send 2 0 10 2) (.send 2 0 11 2))
    (.seq (.send 0 2 12 2) (.send 2 3 12 2))
private def grant (bench label : Nat) : Choreo :=
  .seq (.send 0 bench label (loan 4))
    (.seq (.send 4 bench 13 2) (.send bench 0 12 (loan 4)))
private def tools := Choreo.route .intrinsic (grant 2 10) (grant 3 11)

private def cuts {source expanded : Source} (unfolding : SourceUnfolding source expanded) :
    Option (List (SourceOccurrenceCut source)) := do
  match hOriginal : normalize source with
  | none => none
  | some original =>
    match hNorm : normalizeExpansion original.issuance expanded with
    | none => none
    | some normalized =>
      if same : normalized.issuance = original.issuance then
        normalized.paths.attach.mapM fun ⟨path, selected⟩ => do
          if sep : unfolding.parallelCompatible path.bindings then
            if guardOK : unfolding.guardsCompatible normalized.issuance path.bindings then
              pure { original, originalAccepted := hOriginal, expanded, unfolding, normalized,
                     accepted := hNorm, sameIssuance := same, path, selected,
                     separated := sep, guarded := guardOK, observed := fun _ => false }
            else none
          else none
      else none

private def delivered (event : Nat) : List SourcePoint :=
  [.accepted event, .committed ⟨event, .send⟩, .committed ⟨event, .receive⟩]
private def onWire (event : Nat) : List SourcePoint :=
  [.accepted event, .committed ⟨event, .send⟩]

private def word : PollVerdict → String
  | .ready => "ready"
  | .pending => "pending"
  | .unavailable => "unavailable"

private def reachable (plan : SourceOccurrenceCut source) (points : List SourcePoint) : Bool :=
  (points.foldl (fun (state : Bool × List SourcePoint) p =>
    (state.1 && enabled plan state.2 p, p :: state.2)) (true, [])).1

/-- Encode the full checked prefix, including the actor executing an operation
on a delegated view and the original wire identity of each expanded occurrence. -/
private def observation (plan : SourceOccurrenceCut source) (point : SourcePoint) : Option String := do
  let (mark, id) : String × ActionId := match point with
    | .accepted event => ("a", ⟨event, Side.send⟩)
    | .committed id => (if id.side == .send then "s" else "r", id)
  let (origin, m) ← (plan.unfolding.occurrences 0)[id.event]?
  let action ← action? plan.path.bindings id
  let dest := if mark == "a" then s!">{m.receiver.role}" else ""
  return s!"{mark}{action.actor}/{action.endpoint.role}/{origin}{dest}"

private def expected (body : Source) (receiving : Bool) : Option (List String) := do
  let plans ← cuts (.seq (.send _) (.seq (.send _)
    (.rollMore (.rollOnce (.oneCopy body)) (.oneCopy body))) :
      SourceUnfolding (initialLoans (.roll body)) _)
  let mut rows := []
  for plan in plans do
    let [(a, _), (b, _), (first, arm), (reply, _), (last, _), (next, nextArm), _, _] := plan.path.bindings | none
    if !receiving || (arm.message.label == 10 && nextArm.message.label == 11) then
      let initial := delivered a ++ delivered b ++ delivered first
      let (view, histories) := if receiving then
        let returned := initial ++ delivered reply ++ delivered last
        (3, [initial, returned, returned ++ onWire next])
      else
        let beforeRead := initial ++ onWire reply
        let beforeFile := initial ++ delivered reply ++ onWire last
        (2, [beforeRead, beforeFile, initial ++ delivered reply ++ delivered last])
      let label := nextArm.message.label
      let call : WireCall := if receiving then .receive (.endpoint (key 4)) else .send 2
      let outcomes := histories.map fun history =>
        word (publicCallVerdict plan 1 ⟨key view, none⟩ label call history)
      let some lastHistory := histories.getLast? | none
      let completed := lastHistory ++ (if receiving then [.committed ⟨next, .receive⟩] else onWire next)
      let histories := histories ++ [completed]
      if !(histories.all (reachable plan)) then none
      let observations ← histories.mapM fun h => String.intercalate "," <$> h.mapM (observation plan)
      let direction := if receiving then "recv" else "send"
      let polls := String.intercalate " "
        ([direction, toString arm.message.label, toString label] ++ outcomes)
      rows := rows ++ [String.intercalate " | " (polls :: observations)]
  pure rows

private def sourceRow (direction : String) (full : Choreo) (source : Source) : Option String := do
  let fields := (source.messageEntries 0).map fun (_, m) =>
    let (kind, value) := match m.payload with
      | .wire schema => (0, schema)
      | .endpoint target => (1, target.role)
    s!"{m.sender.role},{m.receiver.role},{m.label},{kind},{value}"
  let control := canonicalControlSource full
  let markers := control.markers.map fun m => s!"scope,{m.scope},{m.offset},{m.tag}"
  let choices ← control.resolvers.mapM fun r => do
    let controller ← r.controller
    let resolver := match r.authority with | .intrinsic => 0 | .dynamic n => n + 1
    return s!"choice,{r.scope},{controller},{resolver}"
  return String.intercalate " " (["source", direction] ++ fields ++ markers ++ choices)

private def fixture (direction : String) (body : Choreo) (receiving : Bool) : Option (List String) := do
  let full := Choreo.seq (.send 0 1 2 (loan 2)) (.seq (.send 0 1 3 (loan 3)) (.roll body))
  let inner ← body.ownershipSource?
  let source := initialLoans (.roll inner)
  guard (full.ownershipSource? == some source)
  return (← sourceRow direction full source) :: (← expected inner receiving)

private def openChoice : Option (List String) := do
  let full := Choreo.seq (.send 0 1 1 (loan 2))
    (.seq (.route .intrinsic (.seq (.send 0 1 2 0) (.send 2 0 4 0)) (.send 0 1 3 0))
      (.send 2 0 5 2))
  let source ← full.ownershipSource?
  let plans ← cuts (.oneCopy source)
  let initial := delivered 0
  guard (plans.length == 2 && plans.all fun plan => reachable plan initial)
  let results := plans.map fun plan => publicCallVerdict plan 1 ⟨key 2, none⟩ 5 (.send 2) initial
  -- One branch can finish without this view, the other needs it. Both share
  -- exactly the delivered-loan prefix at the first public poll.
  guard (results.contains .unavailable && results.contains .pending)
  return [← sourceRow "open-choice" full source,
    "open-choice " ++ word (if results.contains .ready then .ready
      else if results.contains .pending then .pending else .unavailable)]

private def repeatedFragment : Option (List String) := do
  let body : Choreo := .par (.send 0 1 234 0) (.seq (.send 0 1 232 0) (.send 0 1 233 0))
  let inner ← body.ownershipSource?
  let one := SourceUnfolding.oneCopy inner
  let [plan] ← cuts (.rollMore (.rollMore (.rollOnce one) one) one :
    SourceUnfolding (.roll inner) _) | none
  let [(tick₁, _), (work₁, _), (ack₁, _), (tick₂, _), (work₂, _), (ack₂, _), _, (work₃, _), _] :=
    plan.path.bindings | none
  let beforeOwn := delivered tick₁ ++ delivered work₁
  let beforeWait := beforeOwn ++ delivered ack₁ ++ delivered work₂ ++ delivered ack₂
  let beforeReady := beforeWait ++ delivered tick₂
  let histories := [beforeOwn, beforeWait, beforeReady, beforeReady ++ onWire work₃]
  guard (histories.all (reachable plan))
  let outcomes := [beforeOwn, beforeWait, beforeReady].map fun history =>
    word (publicCallVerdict plan 0 ⟨key 0, some (1, 3)⟩ 232 (.send 0) history)
  let traces ← histories.mapM fun h => String.intercalate "," <$> h.mapM (observation plan)
  return [← sourceRow "fragment" (.roll body) (.roll inner),
    String.intercalate " | " (String.intercalate " " ("fragment" :: outcomes) :: traces)]

/-- Compare as a multiset. A duplicate observation cannot fill a missing case. -/
def main (args : List String) : IO Unit := do
  let [file] := args | throw (IO.userError "expected the public Endpoint poll observations")
  let some sends := fixture "send" gaugeBody false | throw (IO.userError "unreachable gauge schedule")
  let some receives := fixture "recv" tools true | throw (IO.userError "unreachable tool schedule")
  unless sends.length == 5 && receives.length == 2 do
    throw (IO.userError "missing source arm combination")
  let lines := (← IO.FS.readFile file).splitOn "\n"
  let some choice := openChoice | throw (IO.userError "unresolved source alternatives disagree")
  let some fragment := repeatedFragment | throw (IO.userError "unreachable parallel fragment schedule")
  let wanted := (sends ++ receives ++ choice ++ fragment).mergeSort
  unless lines.getLast? == some "" && lines.dropLast.mergeSort == wanted do
    throw (IO.userError s!"Endpoint outcomes differ: observed {lines}, source {wanted}")
  IO.println "19 public Endpoint polls match the source across visits, fragments and unresolved choices"
