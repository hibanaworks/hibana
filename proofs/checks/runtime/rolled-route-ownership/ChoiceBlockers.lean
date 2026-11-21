import Hibana.ActorCommit
import Hibana.ActorCommitHistory
import Hibana.PublicCandidateAdmission

namespace Hibana.ChoiceBlockers

/-- A read-only decision. Reading a choice twice consults the same assignment.
The key may include a visit when committed and fresh choices differ. -/
inductive Query (κ : Type) where
  | result (blocked : Bool)
  | read (key : κ) (resume : RouteArm → Query κ)

def Query.run (total : κ → RouteArm) : Query κ → Bool
  | .result blocked => blocked
  | .read key resume => (resume (total key)).run total

def record [DecidableEq κ] (known : κ → Option RouteArm) (key : κ) (arm : RouteArm) :=
  fun s => if s = key then some arm else known s

/-- Both cofactors must block to reject. A known arm takes just one branch. -/
def Query.covered [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) : Bool :=
  match q with
  | .result blocked => blocked
  | .read key resume => match known key with
    | some arm => (resume arm).covered known
    | none => (resume .left).covered (record known key .left) &&
        (resume .right).covered (record known key .right)

def Extends (known : κ → Option RouteArm) (total : κ → RouteArm) : Prop :=
  ∀ key arm, known key = some arm → total key = arm

private theorem extends_record [DecidableEq κ] {known : κ → Option RouteArm} {key : κ}
    {arm : RouteArm} {total : κ → RouteArm} (unknown : known key = none) :
    Extends (record known key arm) total ↔ Extends known total ∧ total key = arm := by
  constructor
  · intro h
    refine ⟨fun s a v => ?_, h key arm (by simp [record])⟩
    by_cases same : s = key
    · subst s; simp [unknown] at v
    · exact h s a (by simpa [record, same] using v)
  · rintro ⟨h, value⟩ s a atS
    by_cases same : s = key
    · subst s; exact value.trans (by simpa [record] using atS)
    · exact h s a (by simpa [record, same] using atS)

/-- Covers arbitrary nesting, repeated queries, and correlated guard and
actor tests. A compatible completion can use different blockers on each arm. -/
theorem covered_iff_all_completions [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) :
    q.covered known = true ↔ ∀ total, Extends known total → q.run total = true := by
  induction q generalizing known with
  | result blocked =>
    constructor
    · exact fun yes _ _ => yes
    · intro every
      exact every (fun k => (known k).getD .left) (fun k a atK => by simp [atK])
  | read key resume ih =>
    cases atKey : known key with
    | some arm =>
      simp only [Query.covered, atKey, ih]
      exact forall_congr' fun total => forall_congr' fun ex => by simp [Query.run, ex key arm atKey]
    | none =>
      simp only [Query.covered, atKey, Bool.and_eq_true, ih, extends_record atKey]
      constructor
      · rintro ⟨left, right⟩ total ex
        cases arm : total key with
        | left => simpa [Query.run, arm] using left total ⟨ex, arm⟩
        | right => simpa [Query.run, arm] using right total ⟨ex, arm⟩
      · intro every
        constructor <;> rintro total ⟨ex, arm⟩ <;> simpa [Query.run, arm] using every total ex

private def Query.orElse (first second : Query κ) : Query κ :=
  match first with
  | .result blocked => if blocked then .result true else second
  | .read key next => .read key (fun arm => (next arm).orElse second)

private theorem run_orElse (first second : Query κ) (total : κ → RouteArm) :
    (first.orElse second).run total = (first.run total || second.run total) := by
  induction first with
  | result b => cases b <;> rfl
  | read k next ih => exact ih (total k)

/-- Interpret a compiled decision against the listed descriptor scopes. A
continuation delays later reads until the preceding answer has been supplied. -/
private def inspect (key : Nat → κ) : List Nat → ((Nat → Option RouteArm) → Bool) → Query κ
  | [], evaluate => .result (evaluate (fun _ => none))
  | s :: rest, evaluate => .read (key s) (fun a => inspect key rest (fun evidence =>
      evaluate (record evidence s a)))

private theorem run_inspect (key : Nat → κ) (scopes : List Nat) (total : κ → RouteArm)
    (evaluate : (Nat → Option RouteArm) → Bool) :
    (inspect key scopes evaluate).run total =
      evaluate (fun s => if s ∈ scopes then some (total (key s)) else none) := by
  induction scopes generalizing evaluate with
  | nil => simp [inspect, Query.run]
  | cons s rest ih =>
    simp only [inspect, Query.run, ih]
    congr 1
    funext k
    by_cases same : k = s <;> simp [record, same]

private theorem route_agrees {path : List (Nat × Nat)} {first second : Nat → Option RouteArm}
    (same : ∀ entry ∈ path, first entry.1 = second entry.1) :
    routePathAllows path first (fun _ => none) = routePathAllows path second (fun _ => none) := by
  induction path with
  | nil => rfl
  | cons p rest ih =>
    obtain ⟨scope, arm⟩ := p
    have head := same (scope, arm) (by simp)
    have tail := ih (fun q qAt => same q (List.mem_cons_of_mem _ qAt))
    simpa only [routePathAllows, List.all_cons, head] using
      congrArg (fun b => (match (second scope).orElse (fun _ => none) with
        | none => true | some chosen => encodeRouteArm chosen == arm) && b) tail

/-- The same completion controls the canonical ancestor path and the physical
holder of the row. `rows` contains the unfinished preceding rows needing the
caller's borrow at this cut. Classification never adds a row to that input. -/
def rowQuery (columns : DecodedProgramColumns) (actor : Nat) (row : DecodedActorRow)
    (path : List (Nat × Nat)) (key : Nat → κ) : Query κ :=
  let scopes := path.map Prod.fst ++ columns.actorDecisions.map (·.scope)
  inspect key scopes fun selected => routePathAllows path selected (fun _ => none) &&
    columns.selectedActor? selected row.actorReference == some actor

private theorem run_rowQuery (columns : DecodedProgramColumns) (actor : Nat) (row : DecodedActorRow)
    (path : List (Nat × Nat)) (key : Nat → κ) (total : κ → RouteArm) :
    (rowQuery columns actor row path key).run total =
      (routePathAllows path (fun s => some (total (key s))) (fun _ => none) &&
       columns.selectedActor? (fun s => some (total (key s))) row.actorReference == some actor) := by
  let scopes := path.map Prod.fst ++ columns.actorDecisions.map (·.scope)
  let limited := fun s => if s ∈ scopes then some (total (key s)) else none
  change (inspect key scopes _).run total = _
  rw [run_inspect]
  have route : routePathAllows path limited (fun _ => none) = _ :=
    route_agrees (second := fun s => some (total (key s))) (fun p inside => by
      have present : p.1 ∈ scopes := List.mem_append_left _ (List.mem_map_of_mem inside)
      simp [limited, present])
  have owner : columns.selectedActor? limited row.actorReference = _ :=
    selected_actor_eq_of_same_decision_scopes (second := fun s => some (total (key s))) (fun d inside => by
      have present : d.scope ∈ scopes := List.mem_append_right _ (List.mem_map_of_mem inside)
      simp [limited, present]) _
  change (_root_.Hibana.routePathAllows path limited _ && columns.selectedActor? limited _ == _) = _
  rw [route, owner]

def rowsQuery (columns : DecodedProgramColumns) (actor : Nat) (path : DecodedActorRow → List (Nat × Nat))
    (key : DecodedActorRow → Nat → κ) : List DecodedActorRow → Query κ
  | [] => .result false
  | row :: rest => (rowQuery columns actor row (path row) (key row)).orElse (rowsQuery columns actor path key rest)

private theorem run_rows (columns : DecodedProgramColumns) (actor : Nat) (path : DecodedActorRow → List (Nat × Nat))
    (key : DecodedActorRow → Nat → κ) (rows : List DecodedActorRow) (total : κ → RouteArm) :
    (rowsQuery columns actor path key rows).run total = true ↔ ∃ row ∈ rows,
      let selected := fun s => some (total (key row s))
      routePathAllows (path row) selected (fun _ => none) = true ∧
        columns.selectedActor? selected row.actorReference = some actor := by
  induction rows with
  | nil => simp [rowsQuery, Query.run]
  | cons row rest ih =>
    simp only [rowsQuery, run_orElse, Bool.or_eq_true, run_rowQuery, Bool.and_eq_true, beq_iff_eq, ih]
    simp

/-- A refusal by the cofactor scan rules out every completion lacking a
held predecessor performed by this actor, and conversely. `known` includes
candidate-preview choices where the actual selected choice is still unknown.
Different visits may use distinct keys for the same static route scope. -/
theorem held_predecessors_cover_iff [DecidableEq κ] (columns : DecodedProgramColumns) (actor : Nat)
    (rows : List DecodedActorRow) (path : DecodedActorRow → List (Nat × Nat))
    (key : DecodedActorRow → Nat → κ) (known : κ → Option RouteArm) :
    (rowsQuery columns actor path key rows).covered known = true ↔
      ∀ total, Extends known total → ∃ row ∈ rows,
        let selected := fun s => some (total (key row s))
        routePathAllows (path row) selected (fun _ => none) = true ∧
          columns.selectedActor? selected row.actorReference = some actor := by
  rw [covered_iff_all_completions]
  exact forall_congr' fun total => forall_congr' fun _ => run_rows columns actor path key rows total

#print axioms covered_iff_all_completions
#print axioms held_predecessors_cover_iff

/-- Every decoded guard in this scan is the canonical guard of the original
source event. Validating the image supplies this equality for all rows and
all eventual completions of the currently known choices. -/
theorem checked_image_held_predecessors_cover_iff [DecidableEq κ] (columns : DecodedProgramColumns)
    {image : RustDescriptorImage} {choreo : Choreo} (validated : image.actorRouteCheck choreo = true)
    (rows : List DecodedActorRow) (path : DecodedActorRow → List (Nat × Nat))
    (present : ∀ row ∈ rows, row.eventId < image.eventCount)
    (decoded : ∀ row ∈ rows, image.eventRoutePath? row.eventId = some (path row))
    (key : DecodedActorRow → Nat → κ) (known : κ → Option RouteArm) :
    (rowsQuery columns image.role path key rows).covered known = true ↔
      ∀ total, Extends known total → ∃ row ∈ rows, ∃ origin canonical,
        image.eventEffIndex? row.eventId = some origin ∧
        canonicalEventRoutePath? (canonicalProgramSource choreo) origin = some canonical ∧
        let selected := fun s => some (total (key row s))
        routePathAllows canonical selected (fun _ => none) = true ∧
          columns.selectedActor? selected row.actorReference = some image.role := by
  rw [held_predecessors_cover_iff]
  apply forall_congr'
  intro total
  apply forall_congr'
  intro _
  have canonical (row : DecodedActorRow) (member : row ∈ rows) :=
    (accepted_actor_routes_bind_source_choice_filter validated).2 row.eventId
      (List.mem_range.mpr (present row member))
  constructor
  · rintro ⟨row, member, permitted⟩
    obtain ⟨origin, actual, eff, atRow, atSource, _⟩ := canonical row member
    have same := Option.some.inj (atRow.symm.trans (decoded row member))
    exact ⟨row, member, origin, path row, eff, same ▸ atSource, permitted⟩
  · rintro ⟨row, member, origin, original, eff, atSource, permitted⟩
    obtain ⟨other, actual, effAt, atRow, sourceAt, _⟩ := canonical row member
    have sameOrigin := Option.some.inj (effAt.symm.trans eff)
    subst other
    have same := Option.some.inj (atSource.symm.trans sourceAt) |>.trans
      (Option.some.inj (atRow.symm.trans (decoded row member)))
    exact ⟨row, member, same ▸ permitted⟩

#print axioms checked_image_held_predecessors_cover_iff

open EndpointDelegation in
private theorem wire_action_actor_and_endpoint {row : DecodedActorRow} {binding : Binding}
    {path : NormalizedPath} {actor : Nat} (receive : Bool)
    (atBinding : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (side : row.actionId.side = if receive then .receive else .send)
    (action : action? path.bindings row.actionId = some (row.sourceAction actor).2) :
    (if receive then binding.receiverActor else binding.senderActor) = actor ∧
      (if receive then binding.message.receiver else binding.message.sender) =
        { component := 0, role := row.view } := by
  cases receive <;> simp at side ⊢ <;>
    by_cases same : binding.message.sender = binding.message.receiver <;>
    simp [action?, atBinding, side, same, DecodedActorRow.sourceAction] at action <;> exact action

open EndpointDelegation in
/-- On a normalized visit containing this operation, evaluate its real decoded
row with the same decision scopes as the source path. A successful query
identifies the canonical guard, physical performer and public local endpoint.
The same source occurrence retains its label and payload on every other
normalized path where it occurs. No equality of physical holders across paths
is needed. The choice-to-visit correspondence and the dynamic cut are distinct. -/
theorem checked_row_query_binds_source_public_contract
    {image : RustDescriptorImage} {choreo : Choreo} {data : DecodedActorOrder}
    {row : DecodedActorRow} {path : NormalizedPath} {binding : Binding}
    (checked : image.actorRouteCheck choreo = true)
    (decoded : image.decodeActorOrder? choreo = some data) (index : Fin image.eventCount)
    (located : data.rows[index.val]? = some row) (selected : path ∈ data.normalized.paths)
    (atBinding : path.bindings.find? (fun entry => entry.1 == row.actionId.event) =
      some (row.actionId.event, binding))
    (receive : Bool) (side : row.actionId.side = if receive then .receive else .send)
    (key : Nat → κ) (total : κ → RouteArm)
    (agrees : ∀ decision ∈ data.columns.actorDecisions,
      some (total (key decision.scope)) = sourcePathArm? data.markers path decision.scope) :
    ∃ origin guard, row.eventId = index.val ∧ image.eventEffIndex? index.val = some origin ∧
      image.eventRoutePath? index.val = some guard ∧
      canonicalEventRoutePath? (canonicalProgramSource choreo) origin = some guard ∧
      ((rowQuery data.columns image.role row guard key).run total = true ↔
        routePathAllows guard (fun s => some (total (key s))) (fun _ => none) = true ∧
        row ∈ data.activeRows path ∧
        (if receive then binding.receiverActor else binding.senderActor) = image.role ∧
        ∀ otherPath ∈ data.normalized.paths, ∀ other,
          (row.actionId.event, other) ∈ otherPath.bindings →
          publicMessageContract other.message receive =
            (binding.message.label, binding.message.payload, { component := 0, role := row.view })) := by
  have routes := accepted_actor_routes_bind_source_choice_filter checked
  obtain ⟨origin, guard, eff, compiled, canonical, _⟩ :=
    routes.2 index.val (List.mem_range.mpr index.isLt)
  obtain ⟨_, normalized, _, _, rowsAt, actorAt⟩ := decoded_actor_order_fields decoded
  refine ⟨origin, guard, decoded_actor_row_lookup_preserves_its_index rowsAt located,
    eff, compiled, canonical, ?_⟩
  have present := List.mem_of_find?_eq_some atBinding
  have binds : (path.bindings.any fun entry => entry.1 == row.actionId.event) = true :=
    List.any_eq_true.mpr ⟨_, present, by simp⟩
  have active : row ∈ data.activeRows path ↔
      data.columns.selectedActor? (sourcePathArm? data.markers path) row.actorReference =
        some data.actor := by
    simp [DecodedActorOrder.activeRows, DecodedProgramColumns.actorRowsForPath,
      List.mem_of_getElem? located, binds]
  rw [run_rowQuery, Bool.and_eq_true, beq_iff_eq,
    selected_actor_eq_of_same_decision_scopes agrees, ← actorAt]
  constructor
  · rintro ⟨allowed, localActor⟩
    have member := active.mpr localActor
    obtain ⟨actor, endpoint⟩ := wire_action_actor_and_endpoint receive atBinding side
      (active_decoded_row_has_its_exact_source_action routes.1 decoded selected member)
    refine ⟨allowed, member, actor, ?_⟩
    intro otherPath otherSelected other otherAt
    have message := normalized_paths_agree_on_a_shared_source_message normalized
      otherSelected selected otherAt present
    simp only [publicMessageContract, message, endpoint]
  · rintro ⟨allowed, member, _, _⟩
    exact ⟨allowed, active.mp member⟩

#print axioms checked_row_query_binds_source_public_contract

/-- At a forward borrow, dropping completed or committed-choice-excluded
rows of earlier rolls leaves every unfinished row of the current visit, in
order. `earlier` selects the source-sequential rolled prefix tested by Rust.
Each completion extends only the current visit's committed route knowledge. -/
theorem current_frontier_preserved_after_skipping_earlier_roll_alternatives
    {image : RustDescriptorImage} {choreo : Choreo}
    (validated : image.actorRouteCheck choreo = true) (rows : List DecodedActorRow)
    (present : ∀ row ∈ rows, row.eventId < image.eventCount)
    (earlier : DecodedActorRow → Bool) (before : ActorProgressObservation)
    (current : Nat → RouteArm) (agrees : Extends before.doneState.selected current) :
    let excluded := fun row : DecodedActorRow => (image.eventRoutePath? row.eventId).any fun guard =>
      !routePathAllows guard before.doneState.selected (fun _ => none)
    let outstanding := fun row : DecodedActorRow => !before.doneState.done row.eventId &&
      (image.eventEffIndex? row.eventId).any fun origin =>
        (canonicalEventRoutePath? (canonicalProgramSource choreo) origin).any fun guard =>
          routePathAllows guard (fun scope => some (current scope)) (fun _ => none)
    ((rows.filter fun row => !(earlier row && (before.doneState.done row.eventId || excluded row))).filter
      outstanding) = rows.filter outstanding := by
  dsimp only
  rw [List.filter_filter]
  apply List.filter_congr
  intro row member
  obtain ⟨origin, guard, effAt, pathAt, sourceAt, _⟩ :=
    (accepted_actor_routes_bind_source_choice_filter validated).2 row.eventId
      (List.mem_range.mpr (present row member))
  cases active : routePathAllows guard (fun scope => some (current scope)) (fun _ => none) with
  | false => simp [effAt, pathAt, sourceAt, active]
  | true =>
    have allowed : routePathAllows guard before.doneState.selected (fun _ => none) = true := by
      apply matching_choice_history_allows_route_path
      intro scope arm belongs chosen selected
      have known : before.doneState.selected scope = some chosen := by simpa using selected
      have agree := List.all_eq_true.mp active (scope, arm) belongs
      simpa [agrees scope chosen known] using agree
    cases before.doneState.done row.eventId <;>
      simp [effAt, pathAt, sourceAt, active, allowed]

#print axioms current_frontier_preserved_after_skipping_earlier_roll_alternatives

/-- The first-read decisions on one replay, in chronological order. Keys
already decided in this query take their remembered arm and add no bit. -/
def Query.firstReads [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm)
    (right : κ → Bool) : List (κ × RouteArm) :=
  match q with
  | .result _ => []
  | .read key resume => match known key with
    | some arm => (resume arm).firstReads known right
    | none =>
      let arm := if right key then RouteArm.right else .left
      (key, arm) :: (resume arm).firstReads (record known key arm) right

/-- Right bits used by an ordered path. -/
def rightKeys : List (κ × RouteArm) → List κ
  | [] => []
  | (_, .left) :: rest => rightKeys rest
  | (key, .right) :: rest => key :: rightKeys rest

/-- Flip the last first-read left and retain just the earlier right bits.
After finding that pivot no bit from the old suffix survives. -/
def nextRights : List (κ × RouteArm) → Option (List κ)
  | [] => none
  | (key, .left) :: rest => some ((nextRights rest).getD [key])
  | (key, .right) :: rest => (nextRights rest).map (key :: ·)

def Query.leafCount [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) : Nat :=
  match q with
  | .result _ => 1
  | .read key resume => match known key with
    | some arm => (resume arm).leafCount known
    | none => (resume .left).leafCount (record known key .left) +
        (resume .right).leafCount (record known key .right)

/-- The source order of consistent leaves. Only indices below `leafCount`
are used. Distinct paths reaching the same Boolean result remain distinct. -/
def Query.leafAt [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) (i : Nat) :
    List (κ × RouteArm) :=
  match q with
  | .result _ => []
  | .read key resume => match known key with
    | some arm => (resume arm).leafAt known i
    | none =>
      let leftCount := (resume .left).leafCount (record known key .left)
      if i < leftCount then (key, .left) :: (resume .left).leafAt (record known key .left) i
      else (key, .right) :: (resume .right).leafAt (record known key .right) (i - leftCount)

private theorem positive_leaf_count [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) :
    0 < q.leafCount known := by
  induction q generalizing known with
  | result => exact Nat.zero_lt_one
  | read key resume ih =>
    cases arm : known key with
    | some a => simpa [Query.leafCount, arm] using ih a known
    | none =>
      simp only [Query.leafCount, arm]
      exact Nat.add_pos_left (ih .left _) _

private theorem first_leaf_uses_no_right_bits [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) :
    rightKeys (q.leafAt known 0) = [] := by
  induction q generalizing known with
  | result => rfl
  | read key resume ih =>
    cases arm : known key with
    | some a => simpa [Query.leafAt, arm] using ih a known
    | none => simp [Query.leafAt, arm, positive_leaf_count, rightKeys, ih]

private theorem leaf_keys_are_new [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) (i : Nat) :
    ∀ entry ∈ q.leafAt known i, known entry.1 = none := by
  induction q generalizing known i with
  | result => simp [Query.leafAt]
  | read key resume ih =>
    cases arm : known key with
    | some a => simpa [Query.leafAt, arm] using ih a known i
    | none =>
      simp only [Query.leafAt, arm]
      split <;> intro p inside <;> rcases List.mem_cons.mp inside with rfl | member
      · exact arm
      · have h := ih .left _ _ p member
        by_cases eq : p.1 = key <;> simp_all [record]
      · exact arm
      · have h := ih .right _ _ p member
        by_cases eq : p.1 = key <;> simp_all [record]

private theorem leaf_keys_nodup [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) (i : Nat) :
    ((q.leafAt known i).map Prod.fst).Nodup := by
  induction q generalizing known i with
  | result => simp [Query.leafAt]
  | read key resume ih =>
    cases arm : known key with
    | some a => simpa [Query.leafAt, arm] using ih a known i
    | none =>
      simp only [Query.leafAt, arm]
      split <;> apply List.nodup_cons.mpr <;> refine ⟨?_, ih _ _ _⟩ <;>
        rintro member <;> obtain ⟨p, hp, atKey⟩ := List.mem_map.mp member <;>
        have fresh := leaf_keys_are_new _ _ _ p hp <;> simp [record, atKey] at fresh

private theorem key_determines_arm {path : List (κ × RouteArm)} (unique : (path.map Prod.fst).Nodup)
    (left : (key, first) ∈ path) (right : (key, second) ∈ path) : first = second := by
  induction path with
  | nil => simp at left
  | cons head tail ih =>
    obtain ⟨fresh, rest⟩ := List.nodup_cons.mp unique
    rcases List.mem_cons.mp left with a | a <;> rcases List.mem_cons.mp right with b | b
    · exact congrArg Prod.snd (a.trans b.symm)
    · exact False.elim (fresh (by simpa [← a] using List.mem_map_of_mem (f := Prod.fst) b))
    · exact False.elim (fresh (by simpa [← b] using List.mem_map_of_mem (f := Prod.fst) a))
    · exact ih rest a b

private theorem rightKeys_mem {key : κ} (path : List (κ × RouteArm)) :
    key ∈ rightKeys path ↔ (key, .right) ∈ path := by
  induction path with
  | nil => simp [rightKeys]
  | cons p rest ih => obtain ⟨k, arm⟩ := p; cases arm <;> simp [rightKeys, ih]

private theorem firstReads_of_agreement [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm)
    (i : Nat) (right : κ → Bool)
    (agrees : ∀ p ∈ q.leafAt known i, (if right p.1 then RouteArm.right else .left) = p.2) :
    q.firstReads known right = q.leafAt known i := by
  induction q generalizing known i with
  | result => rfl
  | read key resume ih =>
    cases arm : known key with
    | some a =>
      simp only [Query.leafAt, Query.firstReads, arm] at agrees ⊢
      exact ih a known i agrees
    | none =>
      simp only [Query.leafAt, arm] at agrees ⊢
      split at agrees <;> rename_i h <;> simp only [h, ↓reduceIte, Query.firstReads, arm]
      · rw [agrees (key, .left) (by simp)]
        exact congrArg ((key, RouteArm.left) :: ·) (ih _ _ _ (fun p hp => agrees p (List.mem_cons_of_mem _ hp)))
      · rw [agrees (key, .right) (by simp)]
        exact congrArg ((key, RouteArm.right) :: ·) (ih _ _ _ (fun p hp => agrees p (List.mem_cons_of_mem _ hp)))

/-- Replaying the right bits of an indexed leaf reaches that exact first-read
path. Undecided keys observed a second time use the value saved on first read. -/
theorem firstReads_replays_leaf [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) (i : Nat) :
    q.firstReads known (fun k => (rightKeys (q.leafAt known i)).contains k) = q.leafAt known i := by
  apply firstReads_of_agreement
  rintro ⟨k, arm⟩ inside
  cases arm with
  | right => simpa using (rightKeys_mem _).mpr inside
  | left =>
    have fresh := leaf_keys_nodup q known i
    have incompatible : (k, RouteArm.right) ∉ q.leafAt known i := by
      intro other
      have eq := key_determines_arm fresh inside other
      contradiction
    simp [rightKeys_mem, incompatible]

/-- The iterative flip advances by exactly one leaf. `none` occurs precisely
at the final leaf. The result is the right-bit assignment of the successor,
whose complete first-read path is recovered by `firstReads_replays_leaf`. -/
theorem last_left_visits_next_leaf [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm)
    (i : Nat) (valid : i < q.leafCount known) :
    nextRights (q.leafAt known i) =
      if i + 1 < q.leafCount known then some (rightKeys (q.leafAt known (i + 1))) else none := by
  induction q generalizing known i with
  | result => simp [Query.leafCount] at valid; subst i; rfl
  | read key resume ih =>
    cases arm : known key with
    | some a =>
      simp only [Query.leafCount, Query.leafAt, arm] at valid ⊢
      exact ih a known i valid
    | none =>
      let l := (resume .left).leafCount (record known key .left)
      let r := (resume .right).leafCount (record known key .right)
      have positive : 0 < r := positive_leaf_count _ _
      simp only [Query.leafAt, Query.leafCount, arm] at valid ⊢
      have hl : (resume .left).leafCount (record known key .left) = l := rfl
      have hr : (resume .right).leafCount (record known key .right) = r := rfl
      rw [hl, hr] at valid ⊢
      by_cases left : i < l
      · have step := ih .left (record known key .left) i left
        change nextRights _ = (if i + 1 < l then _ else none) at step
        by_cases same : i + 1 < l
        · simp [left, same, show i + 1 < l + r by omega, nextRights, step, rightKeys]
        · have border : i + 1 = l := by omega
          simp [left, nextRights, step, rightKeys, border, first_leaf_uses_no_right_bits]
          omega
      · have step := ih .right (record known key .right) (i - l) (by omega)
        change nextRights _ = (if i - l + 1 < r then _ else none) at step
        have align : i - l + 1 = i + 1 - l := by omega
        by_cases final : i + 1 < l + r
        · simp [left, show ¬ i + 1 < l by omega, final, nextRights, rightKeys, step,
            align]
          omega
        · simp [left, final, nextRights, step, show ¬ i - l + 1 < r by omega]

theorem firstReads_starts_at_leaf_zero [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) :
    q.firstReads known (fun _ => false) = q.leafAt known 0 := by
  simpa only [first_leaf_uses_no_right_bits, List.contains_nil] using firstReads_replays_leaf q known 0

/-- Leaf indices have distinct first-read paths, even when the results agree. -/
theorem leafAt_injective [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm)
    (i j : Fin (q.leafCount known)) (same : q.leafAt known i = q.leafAt known j) : i = j := by
  obtain ⟨i, hi⟩ := i
  obtain ⟨j, hj⟩ := j
  apply Fin.ext
  change i = j
  induction q generalizing known i j with
  | result => simp [Query.leafCount] at hi hj; omega
  | read key resume ih =>
    cases arm : known key with
    | some a =>
      simp only [Query.leafAt, Query.leafCount, arm] at hi hj same
      exact ih a known i hi j hj same
    | none =>
      simp only [Query.leafAt, Query.leafCount, arm] at hi hj same
      let l := (resume .left).leafCount (record known key .left)
      have hl : (resume .left).leafCount (record known key .left) = l := rfl
      rw [hl] at hi hj same
      by_cases il : i < l <;> by_cases jl : j < l <;> simp [il, jl] at same
      · exact ih .left (record known key .left) i il j jl same
      · have eq := ih .right (record known key .right) (i-l) (by omega) (j-l) (by omega) same
        omega

/-- The pivot remembered by the first pass over distinct read keys. -/
def lastLeft : List (κ × RouteArm) → Option κ
  | [] => none
  | (k, .left) :: rest => (lastLeft rest).orElse (fun _ => some k)
  | (_, .right) :: rest => lastLeft rest

private theorem pivot_in_path (path : List (κ × RouteArm)) (found : lastLeft path = some k) :
    (k, RouteArm.left) ∈ path := by
  induction path with
  | nil => simp [lastLeft] at found
  | cons p rest ih =>
    obtain ⟨head, arm⟩ := p
    cases arm with
    | right => exact List.mem_cons_of_mem _ (ih found)
    | left =>
      cases pivot : lastLeft rest with
      | none => have eq : head = k := by simpa [lastLeft, pivot] using found
                simp [eq]
      | some q => exact List.mem_cons_of_mem _ (ih (by simpa [lastLeft, pivot] using found))

/-- The second pass intersects the old right bits with the keys read before
`pivot` and sets the pivot's bit. Keeping the keys in first-read order gives
exactly the abstract successor. -/
theorem nextRights_eq_replayed_prefix [DecidableEq κ] (path : List (κ × RouteArm))
    (once : (path.map Prod.fst).Nodup) : nextRights path = (lastLeft path).map fun pivot =>
      rightKeys (path.takeWhile fun p => decide (p.1 ≠ pivot)) ++ [pivot] := by
  induction path with
  | nil => rfl
  | cons p rest ih =>
    obtain ⟨head, arm⟩ := p
    obtain ⟨fresh, tailUnique⟩ := List.nodup_cons.mp once
    have tailEq := ih tailUnique
    cases pivot : lastLeft rest with
    | none => cases arm <;> simp [lastLeft, nextRights, tailEq, pivot, rightKeys]
    | some k =>
      have different : head ≠ k := fun eq => fresh (by
        simpa [eq] using List.mem_map_of_mem (f := Prod.fst) (pivot_in_path rest pivot))
      cases arm <;> simp [lastLeft, nextRights, tailEq, pivot, rightKeys, different]

#print axioms nextRights_eq_replayed_prefix

#print axioms firstReads_starts_at_leaf_zero
#print axioms leafAt_injective
#print axioms firstReads_replays_leaf
#print axioms last_left_visits_next_leaf

/-- Every complete assignment follows an indexed leaf, even if the same key
is read repeatedly. Thus the ordered traversal covers every possible walk. -/
theorem every_first_read_path_has_an_index [DecidableEq κ] (q : Query κ)
    (known : κ → Option RouteArm) (right : κ → Bool) :
    ∃ i < q.leafCount known, q.firstReads known right = q.leafAt known i := by
  induction q generalizing known with
  | result => exact ⟨0, Nat.zero_lt_one, rfl⟩
  | read key resume ih =>
    cases arm : known key with
    | some a => simpa [Query.leafCount, Query.firstReads, Query.leafAt, arm] using ih a known
    | none =>
      let l := (resume .left).leafCount (record known key .left)
      let r := (resume .right).leafCount (record known key .right)
      cases selected : right key with
      | false =>
        obtain ⟨i, hi, atLeaf⟩ := ih .left (record known key .left)
        have within : i < l + r := by change i < l at hi; omega
        exact ⟨i, by simpa [Query.leafCount, arm, l, r] using within,
          by simp [Query.firstReads, Query.leafAt, arm, selected, hi, atLeaf]⟩
      | true =>
        obtain ⟨i, hi, atLeaf⟩ := ih .right (record known key .right)
        change i < r at hi
        refine ⟨l + i, ?_, ?_⟩
        · simpa [Query.leafCount, arm, l, r] using Nat.add_lt_add_left hi l
        · have outside : ¬ l + i < l := by omega
          have bound : (resume .left).leafCount (record known key .left) = l := rfl
          simp [Query.firstReads, Query.leafAt, arm, selected, bound, outside, atLeaf]

#print axioms every_first_read_path_has_an_index

/-- This is the Rust wait search's `scope.local_ordinal() * 2 + fresh`.
The bound uses the number of scopes of the sealed program. -/
private def decisionIndex (key : Fin scopes × Bool) : Nat := key.1.val * 2 + key.2.toNat

/-- Rust sizes its wait mask for one word, the square root of the full
word count, or the full scope domain. Each contains the entire local program. -/
theorem decision_address_bounds {scopes : Nat} (valid : scopes ≤ productionScopeCapacity)
    (key : Fin scopes × Bool) : let d := decisionIndex key
    let full := (2 * productionScopeCapacity + 63) / 64
    let need := (scopes * 2 + 63) / 64
    let mid := 16
    let words := if need ≤ 1 then 1 else if need ≤ mid then mid else full
    mid * mid = full ∧ d / 64 < words ∧ d % 64 < 64 := by
  refine ⟨by decide, ?_⟩
  obtain ⟨⟨n, h⟩, tag⟩ := key
  cases tag <;> simp only [decisionIndex, Bool.toNat_false, Bool.toNat_true,
    productionScopeCapacity, Nat.add_zero] at * <;>
    split <;> first | omega | split <;> omega

theorem decision_address_injective (a b : Fin scopes × Bool) :
    (decisionIndex a / 64, decisionIndex a % 64) = (decisionIndex b / 64, decisionIndex b % 64) ↔ a = b := by
  constructor
  · intro pair
    obtain ⟨hi, lo⟩ := Prod.mk.inj pair
    have word : decisionIndex a = decisionIndex b := by omega
    obtain ⟨⟨a, _⟩, fa⟩ := a
    obtain ⟨⟨b, _⟩, fb⟩ := b
    cases fa <;> cases fb <;> simp_all [decisionIndex] <;> omega
  · rintro rfl; rfl

private def packedChoice (words : Nat → BitVec 64) (key : Fin scopes × Bool) : Bool :=
  (words (decisionIndex key / 64)).getLsbD (decisionIndex key % 64)

/-- `BitVec.twoPow 64 b` is `1#64 <<< b`. After intersecting the two old
words, only the pivot word receives that OR mask. -/
private def flipPacked (right seen : Nat → BitVec 64) (pivot : Fin scopes × Bool) (word : Nat) : BitVec 64 :=
  let retained := right word &&& seen word
  if word = decisionIndex pivot / 64 then retained ||| BitVec.twoPow 64 (decisionIndex pivot % 64) else retained

theorem packed_pivot_update_exact (right seen : Nat → BitVec 64) (pivot query : Fin scopes × Bool) :
    packedChoice (flipPacked right seen pivot) query =
      ((packedChoice right query && packedChoice seen query) || decide (pivot = query)) := by
  by_cases sameWord : decisionIndex query / 64 = decisionIndex pivot / 64
  · have sameBit : (decisionIndex pivot % 64 = decisionIndex query % 64) ↔ pivot = query := by
      constructor
      · intro bit
        exact (decision_address_injective pivot query).mp (Prod.ext sameWord.symm bit)
      · rintro rfl; rfl
    have inside : decisionIndex pivot % 64 < 64 := Nat.mod_lt _ (by decide)
    simp [packedChoice, flipPacked, sameWord, BitVec.getLsbD_twoPow, inside, sameBit]
  · have different : pivot ≠ query := fun eq => sameWord (by rw [eq])
    simp [packedChoice, flipPacked, sameWord, different]

private theorem prefix_right_exact {path before : List (κ × RouteArm)}
    (unique : (path.map Prod.fst).Nodup) (included : before.Sublist path) (key : κ) :
    key ∈ rightKeys before ↔ key ∈ rightKeys path ∧ key ∈ before.map Prod.fst := by
  rw [rightKeys_mem, rightKeys_mem]
  constructor
  · intro h; exact ⟨included.subset h, List.mem_map_of_mem h⟩
  · rintro ⟨atRight, member⟩
    obtain ⟨⟨k, arm⟩, present, rfl⟩ := List.mem_map.mp member
    have same := key_determines_arm unique (included.subset present) atRight
    simpa [same] using present

/-- If `seen` records the first-read prefix of this very walk, the concrete
word update represents exactly the model's next DFS configuration. -/
theorem packed_update_realizes_next {scopes : Nat} (right seen : Nat → BitVec 64)
    (path : List ((Fin scopes × Bool) × RouteArm)) (pivot : Fin scopes × Bool)
    (unique : (path.map Prod.fst).Nodup) (found : lastLeft path = some pivot)
    (bits : ∀ k ∈ path.map Prod.fst, packedChoice right k = (rightKeys path).contains k)
    (seenPrefix : ∀ k, packedChoice seen k = ((path.takeWhile fun p => decide (p.1 ≠ pivot)).map Prod.fst).contains k) :
    ∃ next, nextRights path = some next ∧ ∀ k, packedChoice (flipPacked right seen pivot) k = next.contains k := by
  let before := path.takeWhile fun p => decide (p.1 ≠ pivot)
  have included : before.Sublist path := List.takeWhile_sublist _
  refine ⟨rightKeys before ++ [pivot], by simp [nextRights_eq_replayed_prefix path unique, found, before], ?_⟩
  intro k
  rw [packed_pivot_update_exact, seenPrefix]
  change ((packedChoice right k && (before.map Prod.fst).contains k) || decide (pivot = k)) = _
  by_cases member : k ∈ before.map Prod.fst
  · rw [bits k ((included.map Prod.fst).subset member)]
    have same := prefix_right_exact unique included k
    simp [same, member, eq_comm]
  · have absent : k ∉ rightKeys before := fun h => member ((prefix_right_exact unique included k).mp h).2
    simp [member, absent, eq_comm]

#print axioms decision_address_bounds
#print axioms decision_address_injective
#print axioms packed_pivot_update_exact
#print axioms packed_update_realizes_next

private abbrev WalkKey (n : Nat) := Fin n × Bool
private abbrev WalkWords := Nat → BitVec 64

private def armAt (right : WalkWords) (k : WalkKey n) :=
  if packedChoice right k then RouteArm.right else .left

private def markRead (seen : WalkWords) (k : WalkKey n) (w : Nat) : BitVec 64 :=
  if w = decisionIndex k / 64 then seen w ||| BitVec.twoPow 64 (decisionIndex k % 64) else seen w

private theorem markRead_exact (seen : WalkWords) (k s : WalkKey n) :
    packedChoice (markRead seen k) s = (packedChoice seen s || decide (k = s)) := by
  have ones : packedChoice (fun _ => BitVec.allOnes 64) s = true := by
    change (BitVec.allOnes 64).getLsbD (decisionIndex s % 64) = true
    rw [BitVec.getLsbD_allOnes]
    exact decide_eq_true (Nat.mod_lt _ (by decide))
  have update : flipPacked seen (fun _ => BitVec.allOnes 64) k = markRead seen k := by
    funext w; simp only [flipPacked, markRead, BitVec.and_allOnes]
  simpa only [ones, Bool.and_true, update] using packed_pivot_update_exact seen (fun _ => BitVec.allOnes 64) k s

/-- Project `walk` onto the two outputs that can change during its callback.
A repeated unknown still reads its fixed right bit. At the first pivot read,
all subsequent callbacks leave both outputs unchanged. -/
private def Query.walkBits (q : Query (WalkKey n)) (known : WalkKey n → Option RouteArm)
    (right : WalkWords) (stop : Option (WalkKey n)) (seen : WalkWords) (last : Option (WalkKey n)) :
    WalkWords × Option (WalkKey n) :=
  match q with
  | .result _ => (seen, last)
  | .read k resume => match known k with
    | some a => (resume a).walkBits known right stop seen last
    | none =>
      if stop = some k then (seen, last) else
      let next := resume (armAt right k)
      if packedChoice seen k then next.walkBits known right stop seen last
      else next.walkBits known right stop (markRead seen k) (if packedChoice right k then last else some k)

private def remembered (known : WalkKey n → Option RouteArm) (right seen : WalkWords) (k : WalkKey n) :=
  (known k).orElse (fun _ => if packedChoice seen k then some (armAt right k) else none)

private theorem remember_mark (known : WalkKey n → Option RouteArm) (right seen : WalkWords)
    (k : WalkKey n) (unknown : known k = none) :
    remembered known right (markRead seen k) = record (remembered known right seen) k (armAt right k) := by
  funext s
  by_cases equal : k = s
  · subst s; simp [remembered, record, unknown, markRead_exact]
  · simp [remembered, record, equal, Ne.symm equal, markRead_exact]

private def untilPivot [DecidableEq κ] (stop : Option κ) (path : List (κ × RouteArm)) :=
  path.takeWhile (fun p => decide (stop ≠ some p.1))

private theorem packed_walk_observations (q : Query (WalkKey n)) (known : WalkKey n → Option RouteArm)
    (right seen : WalkWords) (last stop : Option (WalkKey n))
    (fresh : ∀ s, stop = some s → packedChoice seen s = false) :
    let result := q.walkBits known right stop seen last
    let path := untilPivot stop (q.firstReads (remembered known right seen) (packedChoice right))
    (∀ s, packedChoice result.1 s = (packedChoice seen s || (path.map Prod.fst).contains s)) ∧
      result.2 = (lastLeft path).orElse (fun _ => last) := by
  induction q generalizing seen last with
  | result => simp [Query.walkBits, Query.firstReads, untilPivot, lastLeft]
  | read k resume ih =>
    cases old : known k with
    | some a =>
      have seenAt : remembered known right seen k = some a := by simp [remembered, old]
      simpa only [Query.walkBits, Query.firstReads, seenAt, old] using ih a seen last fresh
    | none =>
      cases already : packedChoice seen k with
      | true =>
        have notStop : stop ≠ some k := fun eq => by have h := fresh k eq; simp [already] at h
        have seenAt : remembered known right seen k = some (armAt right k) := by simp [remembered, old, already]
        simpa only [Query.walkBits, Query.firstReads, old, seenAt, notStop, if_false, already, if_true] using
          ih (armAt right k) seen last fresh
      | false =>
        have newAt : remembered known right seen k = none := by simp [remembered, old, already]
        by_cases atPivot : stop = some k
        · simp [Query.walkBits, Query.firstReads, old, newAt, atPivot, untilPivot, lastLeft]
        · have stays : ∀ s, stop = some s → packedChoice (markRead seen k) s = false := by
            intro s hit
            have other : k ≠ s := fun eq => atPivot (by simpa [eq] using hit)
            simp [markRead_exact, fresh s hit, other]
          let a := armAt right k
          let tail := untilPivot stop ((resume a).firstReads (record (remembered known right seen) k a) (packedChoice right))
          have prefixEq : untilPivot stop ((k, a) :: (resume a).firstReads
              (record (remembered known right seen) k a) (packedChoice right)) = (k, a) :: tail := by
            simp [untilPivot, atPivot, tail]
          obtain ⟨mask, pivot⟩ := ih a (markRead seen k) (if packedChoice right k then last else some k) stays
          rw [remember_mark _ _ _ _ old] at mask pivot
          change (∀ s, packedChoice ((Query.read k resume).walkBits known right stop seen last).1 s = _) ∧ _
          simp only [Query.walkBits, old, atPivot, if_false, already, Bool.false_eq_true,
            Query.firstReads, newAt]
          let suffixReads := (resume a).firstReads (record (remembered known right seen) k a) (packedChoice right)
          let next := (resume a).walkBits known right stop (markRead seen k) (if packedChoice right k then last else some k)
          change (∀ s, packedChoice next.1 s = (packedChoice seen s ||
              ((untilPivot stop ((k, a) :: suffixReads)).map Prod.fst).contains s)) ∧
            next.2 = (lastLeft (untilPivot stop ((k, a) :: suffixReads))).orElse (fun _ => last)
          change untilPivot stop ((k,a) :: suffixReads) = (k,a) :: tail at prefixEq
          rw [prefixEq]
          change (∀ s, packedChoice next.1 s = _) at mask
          change next.2 = _ at pivot
          constructor
          · intro s
            rw [mask, markRead_exact]
            change ((packedChoice seen s || decide (k = s)) || (tail.map Prod.fst).contains s) = _
            by_cases eq : s = k
            · subst s; simp
            · simp [Ne.symm eq, beq_eq_false_iff_ne.mpr eq]
          · rw [pivot]
            change (lastLeft tail).orElse _ = (lastLeft ((k, a) :: tail)).orElse _
            cases b : packedChoice right k <;> cases p : lastLeft tail <;> simp [lastLeft, a, armAt, b, p]

private theorem firstReads_match_bits [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) (bits : κ → Bool) :
    ∀ p ∈ q.firstReads known bits, p.2 = (if bits p.1 then RouteArm.right else .left) := by
  induction q generalizing known with
  | result => simp [Query.firstReads]
  | read k resume ih =>
    cases d : known k with
    | some a => simpa [Query.firstReads, d] using ih a known
    | none =>
      intro p member
      simp only [Query.firstReads, d] at member
      rcases List.mem_cons.mp member with rfl | rest
      · rfl
      · exact ih _ _ _ rest

private theorem actual_right_bits [DecidableEq κ] (q : Query κ) (known : κ → Option RouteArm) (bits : κ → Bool) :
    let path := q.firstReads known bits
    ∀ k ∈ path.map Prod.fst, bits k = (rightKeys path).contains k := by
  intro path k member
  obtain ⟨⟨k, a⟩, present, rfl⟩ := List.mem_map.mp member
  have arm := firstReads_match_bits q known bits (k, a) present
  cases atBit : bits k with
  | false =>
    have absent : (k, RouteArm.right) ∉ path := by
      intro h
      have nope := firstReads_match_bits q known bits (k, .right) h
      simp [atBit] at nope
    simp [rightKeys_mem, absent]
  | true =>
    have isRight : a = .right := by simpa [atBit] using arm
    subst a
    simpa [rightKeys_mem] using present

/-- Both calls to `walk` start with a cleared mask and read the same right
words. Their computed pivot and prefix mask suffice to derive the successor;
there is no input hypothesis about either observed mask or the chosen arms. -/
theorem actual_packed_walk_step (q : Query (WalkKey n)) (known : WalkKey n → Option RouteArm) (right : WalkWords) :
    let path := q.firstReads known (packedChoice right)
    let first := q.walkBits known right none (fun _ => 0) none
    first.2 = lastLeft path ∧ ∀ pivot, first.2 = some pivot →
      let replay := q.walkBits known right (some pivot) (fun _ => 0) none
      ∃ next, nextRights path = some next ∧ ∀ k, packedChoice (flipPacked right replay.1 pivot) k = next.contains k := by
  let path := q.firstReads known (packedChoice right)
  have empty : remembered known right (fun _ => 0) = known := by
    funext s; cases a : known s <;> simp [remembered, a, packedChoice]
  have allPath := packed_walk_observations q known right (fun _ => 0) none none (by simp)
  have first := allPath.2
  rw [empty] at first
  have noStop (xs : List (WalkKey n × RouteArm)) : untilPivot none xs = xs := by
    induction xs with
    | nil => rfl
    | cons head rest ih => simpa [untilPivot] using ih
  rw [noStop] at first
  have orNone (value : Option (WalkKey n)) : (value.orElse (fun _ => none)) = value := by cases value <;> rfl
  rw [orNone] at first
  refine ⟨first, ?_⟩
  intro pivot found
  have cut := packed_walk_observations q known right (fun _ => 0) none (some pivot) (by simp [packedChoice])
  rw [empty] at cut
  obtain ⟨i, _, indexAt⟩ := every_first_read_path_has_an_index q known (packedChoice right)
  have once : (path.map Prod.fst).Nodup := by rw [show path = _ from indexAt]; exact leaf_keys_nodup ..
  apply packed_update_realizes_next _ _ _ pivot once (first.symm.trans found)
  · intro k present
    have assigned := actual_right_bits q known (packedChoice right) k present
    cases b : packedChoice right k <;> simpa [path, b] using assigned
  · intro k
    simpa [untilPivot, packedChoice, eq_comm, path] using cut.1 k

#print axioms actual_packed_walk_step

end Hibana.ChoiceBlockers
