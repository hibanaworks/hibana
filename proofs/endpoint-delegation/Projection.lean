import Std

namespace Hibana.EndpointDelegation

structure EndpointKey where
  component : Nat
  role : Nat
  deriving Repr, DecidableEq

instance : BEq EndpointKey := ⟨fun left right => decide (left = right)⟩
instance : ReflBEq EndpointKey where
  rfl := by intro value; exact decide_eq_true rfl
instance : LawfulBEq EndpointKey where
  eq_of_beq := by intro left right equal; exact of_decide_eq_true equal

inductive Payload where
  | wire (schema : Nat)
  | endpoint (key : EndpointKey)
  deriving Repr, DecidableEq, BEq

structure Message where
  sender : EndpointKey
  receiver : EndpointKey
  label : Nat
  payload : Payload
  deriving Repr, DecidableEq, BEq

inductive Source where
  | send (message : Message)
  | seq (left right : Source)
  | par (left right : Source)
  | route (resolver : Option Nat) (left right : Source)
  | roll (body : Source)
  deriving Repr, DecidableEq

def Source.eventCount : Source → Nat
  | .send _ => 1
  | .seq left right | .par left right | .route _ left right =>
      left.eventCount + right.eventCount
  | .roll body => body.eventCount

def Source.resources : Source → List EndpointKey
  | .send message => match message.payload with
      | .wire _ => []
      | .endpoint key => [key]
  | .seq left right | .par left right | .route _ left right =>
      (left.resources ++ right.resources).eraseDups
  | .roll body => body.resources

def Source.uses : Source → List EndpointKey
  | .send message =>
      [message.sender, message.receiver] ++ match message.payload with
        | .wire _ => []
        | .endpoint key => [key]
  | .seq left right | .par left right | .route _ left right =>
      (left.uses ++ right.uses).eraseDups
  | .roll body => body.uses

inductive OwnerExpr where
  | actor (actor : Nat)
  | initial (endpoint : EndpointKey)
  deriving Repr, DecidableEq

instance : BEq OwnerExpr := ⟨fun left right => decide (left = right)⟩
instance : ReflBEq OwnerExpr where
  rfl := by intro value; exact decide_eq_true rfl
instance : LawfulBEq OwnerExpr where
  eq_of_beq := by intro left right equal; exact of_decide_eq_true equal

abbrev OwnerMap := EndpointKey → OwnerExpr
abbrev Equation := OwnerExpr × OwnerExpr

def initialOwners (resources : List EndpointKey) : OwnerMap := fun key =>
  if resources.contains key then .initial key else .actor key.role

def withOwner (owners : OwnerMap) (key : EndpointKey) (owner : OwnerExpr) : OwnerMap :=
  fun candidate => if candidate = key then owner else owners candidate

structure SymbolicBinding where
  message : Message
  sender : OwnerExpr
  receiver : OwnerExpr
  deriving Repr, DecidableEq

structure Scope where
  start : Nat
  split : Nat
  stop : Nat
  deriving Repr, DecidableEq, BEq

inductive Arm where
  | left | right
  deriving Repr, DecidableEq, BEq

structure Selection where
  scope : Scope
  arm : Arm
  deriving Repr, DecidableEq, BEq

def Source.scopes : Source → Nat → List Scope
  | .send _, _ => []
  | .seq left right, start | .par left right, start =>
      left.scopes start ++ right.scopes (start + left.eventCount)
  | .route _ left right, start =>
      let split := start + left.eventCount
      { start, split, stop := split + right.eventCount } ::
        (left.scopes start ++ right.scopes split)
  | .roll body, start => body.scopes start

def paths : List Scope → List (List Selection)
  | [] => [[]]
  | scope :: rest =>
      let suffixes := paths rest
      (suffixes.map fun tail => { scope, arm := .left } :: tail) ++
        (suffixes.map fun tail => { scope, arm := .right } :: tail)

def chosen? (path : List Selection) (scope : Scope) : Option Arm :=
  (path.find? fun selection => selection.scope == scope).map Selection.arm

structure Elaboration where
  owners : OwnerMap
  bindings : List (Nat × SymbolicBinding)
  equations : List Equation

def incompatibleParallel (left right : Source) : Bool :=
  left.resources.any right.uses.contains || right.resources.any left.uses.contains

def validKey (key : EndpointKey) : Bool := key.component < 65536 && key.role < 256

def validMessage (message : Message) : Bool :=
  validKey message.sender && validKey message.receiver && message.label < 256 &&
  match message.payload with
  | .wire schema => schema < 4294967296 &&
      (message.sender != message.receiver || schema == 0)
  | .endpoint key => validKey key && key != message.sender && key != message.receiver &&
      message.sender != message.receiver

/-- Independent functional execution along one complete source selection.
The reference explores source paths, whereas Rust queries ownership from
source prefixes and only splits equations on the decisions they need. -/
def elaborate (resources : List EndpointKey) (owners : OwnerMap)
    (path : List Selection) : Source → Nat → Option Elaboration
  | .send message, event => do
      if !validMessage message then none else
      let binding : SymbolicBinding := {
        message, sender := owners message.sender, receiver := owners message.receiver
      }
      match message.payload with
      | .wire _ => some { owners, bindings := [(event, binding)], equations := [] }
      | .endpoint key => some {
          owners := withOwner owners key binding.receiver
          bindings := [(event, binding)]
          equations := [(owners key, binding.sender)]
        }
  | .seq left right, start => do
      let first ← elaborate resources owners path left start
      let second ← elaborate resources first.owners path right (start + left.eventCount)
      some {
        owners := second.owners
        bindings := first.bindings ++ second.bindings
        equations := first.equations ++ second.equations
      }
  | .par left right, start => do
      if incompatibleParallel left right then none else
      let first ← elaborate resources owners path left start
      let second ← elaborate resources owners path right (start + left.eventCount)
      some {
        owners := fun key =>
          if left.resources.contains key then first.owners key else second.owners key
        bindings := first.bindings ++ second.bindings
        equations := first.equations ++ second.equations
      }
  | .route resolver left right, start => do
      if resolver.any (fun value => value >= 65536) then none else
      let split := start + left.eventCount
      let arm ← chosen? path { start, split, stop := split + right.eventCount }
      match arm with
      | .left => elaborate resources owners path left start
      | .right => elaborate resources owners path right split
  | .roll body, start => do
      let inner ← elaborate resources owners path body start
      some {
        owners := inner.owners
        bindings := inner.bindings
        equations := inner.equations ++
          resources.map (fun key => (owners key, inner.owners key))
      }

def equationVertices (equations : List Equation) : List OwnerExpr :=
  (equations.flatMap (fun pair => [pair.1, pair.2])).eraseDups

def expand (equations : List Equation) (seen : List OwnerExpr) : List OwnerExpr :=
  (seen ++ equations.flatMap (fun pair =>
    (if seen.contains pair.1 then [pair.2] else []) ++
    (if seen.contains pair.2 then [pair.1] else []))).eraseDups

def closure (equations : List Equation) (start : OwnerExpr) : Nat → List OwnerExpr
  | 0 => [start]
  | fuel + 1 => expand equations (closure equations start fuel)

def singleActor? : List Nat → Option Nat
  | [actor] => some actor
  | _ => none

/-- An initial owner must be connected to exactly one concrete actor by the
global's equations. An unrelated actor elsewhere in the family is not a
default issuer. No actor is guessed for an unrooted component. -/
def initialActor? (equations : List Equation) (key : EndpointKey) : Option Nat :=
  let reached := closure equations (.initial key) ((equationVertices equations).length + 1)
  singleActor? ((reached.filterMap fun vertex => match vertex with
      | .actor actor => some actor
      | .initial _ => none).eraseDups)

structure Issuance where
  endpoint : EndpointKey
  actor : Nat
  deriving Repr, DecidableEq

def issuer? (issuance : List Issuance) (key : EndpointKey) : Option Nat :=
  (issuance.find? (fun entry => entry.endpoint == key)).map Issuance.actor

def instantiate (issuance : List Issuance) : OwnerExpr → Option Nat
  | .actor actor => some actor
  | .initial key => issuer? issuance key

def equationAccepted (issuance : List Issuance) (equation : Equation) : Bool :=
  match instantiate issuance equation.1, instantiate issuance equation.2 with
  | some left, some right => left == right
  | _, _ => false

structure Binding where
  message : Message
  senderActor : Nat
  receiverActor : Nat
  deriving Repr, DecidableEq

def instantiateBinding (issuance : List Issuance) (binding : SymbolicBinding) :
    Option Binding := do
  let senderActor ← instantiate issuance binding.sender
  let receiverActor ← instantiate issuance binding.receiver
  some { message := binding.message, senderActor, receiverActor }

structure NormalizedPath where
  selections : List Selection
  bindings : List (Nat × Binding)
  deriving Repr, DecidableEq

structure Normalization where
  issuance : List Issuance
  paths : List NormalizedPath
  deriving Repr, DecidableEq

def issuanceOrder (left right : Issuance) : Bool :=
  decide (left.endpoint.component < right.endpoint.component ∨
    (left.endpoint.component = right.endpoint.component ∧ left.endpoint.role ≤ right.endpoint.role))

def insertIssuance (value : Issuance) : List Issuance → List Issuance
  | [] => [value]
  | head :: tail =>
      if issuanceOrder value head then value :: head :: tail
      else head :: insertIssuance value tail

def sortIssuance : List Issuance → List Issuance
  | [] => []
  | head :: tail => insertIssuance head (sortIssuance tail)

/-- Compile all source paths against one initial issuance. Paths may differ in
their later holders; they cannot infer different initial owners. Functional
maps and graph closure are independent of Rust's source-prefix query engine. -/
def normalize (source : Source) : Option Normalization := do
  let resources := source.resources
  let runs ← (paths (source.scopes 0)).mapM fun selections => do
    let elaborated ← elaborate resources (initialOwners resources) selections source 0
    some (selections, elaborated)
  let equations := runs.flatMap fun run => run.2.equations
  let issuance ← resources.mapM fun endpoint => do
    let actor ← initialActor? equations endpoint
    some ({ endpoint, actor } : Issuance)
  if !(equations.all (equationAccepted issuance)) then none else
  let normalized ← runs.mapM fun (selections, elaborated) => do
    let bindings ← elaborated.bindings.mapM fun (event, symbolic) => do
      let binding ← instantiateBinding issuance symbolic
      some (event, binding)
    some ({ selections, bindings } : NormalizedPath)
  some { issuance := sortIssuance issuance, paths := normalized }

inductive Side where
  | send | receive | local
  deriving Repr, DecidableEq, BEq

structure ActionId where
  event : Nat
  side : Side
  deriving Repr, DecidableEq

structure Action where
  actor : Nat
  endpoint : EndpointKey
  side : Side
  deriving Repr, DecidableEq

def action? (bindings : List (Nat × Binding)) (id : ActionId) : Option Action := do
  let (_, binding) ← bindings.find? fun row => row.1 == id.event
  let isLocal := binding.message.sender == binding.message.receiver
  match id.side with
  | .send => if isLocal then none else some {
      actor := binding.senderActor, endpoint := binding.message.sender, side := id.side
    }
  | .receive => if isLocal then none else some {
      actor := binding.receiverActor, endpoint := binding.message.receiver, side := id.side
    }
  | .local =>
      if isLocal then
        some { actor := binding.senderActor, endpoint := binding.message.sender, side := id.side }
      else none

def Source.before : Source → Nat → Nat → Bool
  | .send _, _, _ => false
  | .seq left right, before, after =>
      if before < left.eventCount && after < left.eventCount then
        left.before before after
      else if before ≥ left.eventCount && after ≥ left.eventCount then
        right.before (before - left.eventCount) (after - left.eventCount)
      else before < left.eventCount && after ≥ left.eventCount
  | .par left right, before, after | .route _ left right, before, after =>
      if before < left.eventCount && after < left.eventCount then
        left.before before after
      else if before ≥ left.eventCount && after ≥ left.eventCount then
        right.before (before - left.eventCount) (after - left.eventCount)
      else false
  | .roll body, before, after => body.before before after

def precedes (source : Source) (bindings : List (Nat × Binding)) (before after : ActionId) : Bool :=
  match action? bindings before, action? bindings after with
  | some left, some right =>
      left.actor == right.actor &&
      if before.event == after.event then before.side == .send && after.side == .receive
      else source.before before.event after.event
  | _, _ => false

def decodeActionId (encoded : Nat) : ActionId := {
  event := encoded / 3
  side := match encoded % 3 with | 0 => .send | 1 => .receive | _ => .local
}

def orderEdges (source : Source) (bindings : List (Nat × Binding)) : List (Nat × Nat) :=
  let domain := List.range (3 * (source.eventCount + 1))
  domain.flatMap fun before => domain.filterMap fun after =>
    if precedes source bindings (decodeActionId before) (decodeActionId after)
    then some (before, after) else none

@[simp] theorem withOwner_same (owners : OwnerMap) (key : EndpointKey) (value : OwnerExpr) :
    withOwner owners key value key = value := by simp [withOwner]

theorem withOwner_other (owners : OwnerMap) (key other : EndpointKey) (value : OwnerExpr)
    (different : other ≠ key) : withOwner owners key value other = owners other := by
  simp [withOwner, different]

theorem independent_owner_moves_commute
    (owners : OwnerMap) (first second : EndpointKey) (left right : OwnerExpr)
    (different : first ≠ second) :
    withOwner (withOwner owners first left) second right =
      withOwner (withOwner owners second right) first left := by
  funext key
  by_cases atFirst : key = first
  · subst key
    simp [withOwner, different]
  · by_cases atSecond : key = second
    · subst key
      simp [withOwner, Ne.symm different]
    · simp [withOwner, atFirst, atSecond]

def OwnerExpr.eval (assignment : EndpointKey → Nat) : OwnerExpr → Nat
  | .actor value => value
  | .initial key => assignment key

def Solves (assignment : EndpointKey → Nat) (equations : List Equation) : Prop :=
  ∀ equation ∈ equations, equation.1.eval assignment = equation.2.eval assignment

def Edge (equations : List Equation) (left right : OwnerExpr) : Prop :=
  (left, right) ∈ equations ∨ (right, left) ∈ equations

inductive Connected (equations : List Equation) : OwnerExpr → OwnerExpr → Prop where
  | refl (value) : Connected equations value value
  | step {start middle stop} : Connected equations start middle →
      Edge equations middle stop → Connected equations start stop

theorem expand_preserves_connectivity (equations : List Equation) (seen : List OwnerExpr)
    (start : OwnerExpr)
    (connected : ∀ vertex ∈ seen, Connected equations start vertex) :
    ∀ vertex ∈ expand equations seen, Connected equations start vertex := by
  intro vertex member
  simp only [expand, List.mem_eraseDups, List.mem_append, List.mem_flatMap] at member
  rcases member with old | ⟨equation, inEquations, adjacent⟩
  · exact connected vertex old
  · rcases adjacent with forward | backward
    · split at forward
      next known =>
        simp only [List.mem_singleton] at forward
        subst vertex
        exact .step (connected equation.1 (by simpa using known)) (Or.inl inEquations)
      next => simp at forward
    · split at backward
      next known =>
        simp only [List.mem_singleton] at backward
        subst vertex
        exact .step (connected equation.2 (by simpa using known)) (Or.inr inEquations)
      next => simp at backward

theorem closure_contains_only_connected_vertices (equations : List Equation)
    (start : OwnerExpr) (fuel : Nat) :
    ∀ vertex ∈ closure equations start fuel, Connected equations start vertex := by
  induction fuel with
  | zero =>
      intro vertex member
      simp only [closure, List.mem_singleton] at member
      subst vertex
      exact .refl start
  | succ fuel inductionHypothesis =>
      exact expand_preserves_connectivity equations _ start inductionHypothesis

theorem inferred_initial_actor_is_connected (equations : List Equation)
    (key : EndpointKey) (actor : Nat) (accepted : initialActor? equations key = some actor) :
    Connected equations (.initial key) (.actor actor) := by
  have singleton_member : ∀ {values : List Nat}, singleActor? values = some actor →
      actor ∈ values := by
    intro values found
    cases values with
    | nil => simp [singleActor?] at found
    | cons first rest =>
        cases rest with
        | nil => simpa [singleActor?] using found.symm
        | cons second tail => simp [singleActor?] at found
  have member := singleton_member accepted
  simp only [List.mem_eraseDups, List.mem_filterMap] at member
  obtain ⟨vertex, reached, yields⟩ := member
  cases vertex with
  | initial _ => simp at yields
  | actor value =>
      simp only [Option.some.injEq] at yields
      subst value
      exact closure_contains_only_connected_vertices equations (.initial key) _ _ reached

theorem connected_expressions_have_equal_owners
    {assignment : EndpointKey → Nat} {equations : List Equation}
    (solves : Solves assignment equations) {left right : OwnerExpr}
    (connected : Connected equations left right) :
    left.eval assignment = right.eval assignment := by
  induction connected with
  | refl => rfl
  | step prior edge inductionHypothesis =>
      rcases edge with forward | backward
      · exact inductionHypothesis.trans (solves _ forward)
      · exact inductionHypothesis.trans (solves _ backward).symm

theorem rooted_initial_owner_is_forced_by_the_global
    {assignment : EndpointKey → Nat} {equations : List Equation}
    (solves : Solves assignment equations) {key : EndpointKey} {actor : Nat}
    (rooted : Connected equations (.initial key) (.actor actor)) :
    assignment key = actor := by
  simpa [OwnerExpr.eval] using connected_expressions_have_equal_owners solves rooted

theorem inferred_initial_owner_is_forced_by_every_solution
    {assignment : EndpointKey → Nat} {equations : List Equation}
    (solves : Solves assignment equations) {key : EndpointKey} {actor : Nat}
    (accepted : initialActor? equations key = some actor) : assignment key = actor := by
  exact rooted_initial_owner_is_forced_by_the_global solves
    (inferred_initial_actor_is_connected equations key actor accepted)

theorem distinct_roots_reject_every_initial_assignment
    {equations : List Equation} {key : EndpointKey} {left right : Nat}
    (first : Connected equations (.initial key) (.actor left))
    (second : Connected equations (.initial key) (.actor right))
    (different : left ≠ right) : ¬∃ assignment, Solves assignment equations := by
  rintro ⟨assignment, solves⟩
  exact different ((rooted_initial_owner_is_forced_by_the_global solves first).symm.trans
    (rooted_initial_owner_is_forced_by_the_global solves second))

theorem binding_preserves_wire_identity
    {issuance : List Issuance} {symbolic : SymbolicBinding} {binding : Binding}
    (accepted : instantiateBinding issuance symbolic = some binding) :
    binding.message = symbolic.message := by
  unfold instantiateBinding at accepted
  cases senderCase : instantiate issuance symbolic.sender with
  | none => simp [senderCase] at accepted
  | some sender =>
      cases receiverCase : instantiate issuance symbolic.receiver with
      | none => simp [senderCase, receiverCase] at accepted
      | some receiver =>
          have exactBinding : binding =
              { message := symbolic.message
                senderActor := sender
                receiverActor := receiver } := by
            exact (Option.some.inj (by simpa [senderCase, receiverCase] using accepted)).symm
          simp [exactBinding]

theorem sequential_actor_work_precedes_its_continuation
    (left right : Source) {before after : Nat}
    (inLeft : before < left.eventCount) (inRight : left.eventCount ≤ after) :
    (Source.seq left right).before before after = true := by
  simp [Source.before, inLeft, Nat.not_lt.mpr inRight, Nat.not_le.mpr inLeft, inRight]

theorem independent_parallel_siblings_have_no_local_order
    (left right : Source) {before after : Nat}
    (inLeft : before < left.eventCount) (inRight : left.eventCount ≤ after) :
    (Source.par left right).before before after = false := by
  simp [Source.before, inLeft, Nat.not_lt.mpr inRight, Nat.not_le.mpr inLeft, inRight]

theorem roll_keeps_visit_local_order (body : Source) (before after : Nat) :
    (Source.roll body).before before after = body.before before after := rfl

#print axioms withOwner_same
#print axioms withOwner_other
#print axioms independent_owner_moves_commute
#print axioms expand_preserves_connectivity
#print axioms closure_contains_only_connected_vertices
#print axioms inferred_initial_actor_is_connected
#print axioms connected_expressions_have_equal_owners
#print axioms rooted_initial_owner_is_forced_by_the_global
#print axioms inferred_initial_owner_is_forced_by_every_solution
#print axioms distinct_roots_reject_every_initial_assignment
#print axioms binding_preserves_wire_identity
#print axioms sequential_actor_work_precedes_its_continuation
#print axioms independent_parallel_siblings_have_no_local_order
#print axioms roll_keeps_visit_local_order

end Hibana.EndpointDelegation
