namespace ParallelViewChoice

-- The source interval of a borrow must either fit within one arm or contain
-- the entire choice. A disjoint sibling cannot use its parked preview.
def visible (first split boundary limit start stop : Nat) : Prop :=
  start < stop ∧ stop ≤ limit ∧
    ((first ≤ start ∧ stop ≤ split) ∨
     (split ≤ start ∧ stop ≤ boundary) ∨
     (start ≤ first ∧ boundary ≤ stop))

def readable (sameActor live active scopeVisible : Bool) : Bool :=
  sameActor && live && active && scopeVisible

theorem held_views_share_a_visible_preview : readable true true true true = true := rfl

theorem retired_views_cannot_supply_a_preview (same active scope : Bool) :
    readable same false active scope = false := by cases same <;> rfl

theorem other_actors_cannot_supply_a_preview (live active scope : Bool) :
    readable false live active scope = false := rfl

theorem disjoint_scopes_cannot_supply_a_preview (same live active : Bool) :
    readable same live active false = false := by
  cases same <;> cases live <;> cases active <;> rfl

theorem a_borrow_inside_an_arm_can_read_the_choice
    (first split boundary limit start stop : Nat)
    (nonempty : start < stop) (bounded : stop ≤ limit)
    (arm : (first ≤ start ∧ stop ≤ split) ∨ (split ≤ start ∧ stop ≤ boundary)) :
    visible first split boundary limit start stop :=
  ⟨nonempty, bounded, arm.elim (fun left => Or.inl left) (fun right => Or.inr (Or.inl right))⟩

theorem a_borrow_containing_the_choice_can_read_it
    (first split boundary limit start stop : Nat)
    (nonempty : start < stop) (bounded : stop ≤ limit)
    (contains : start ≤ first ∧ boundary ≤ stop) :
    visible first split boundary limit start stop :=
  ⟨nonempty, bounded, Or.inr (Or.inr contains)⟩

theorem old_arm_only_rule_misses_an_enclosing_borrow :
    visible 1 3 5 8 0 6 ∧ ¬ ((1 ≤ 0 ∧ 6 ≤ 3) ∨ (3 ≤ 0 ∧ 6 ≤ 5)) := by
  unfold visible
  decide

-- An owned endpoint remains responsible for its future delegation. A borrow
-- must first finish its own operations and release its parent for that move.
def complete (ownsEndpoint operationsDone transferDone : Bool) : Bool :=
  operationsDone && (!ownsEndpoint || transferDone)

theorem borrowing_never_completes_unfinished_operations (owns transfer : Bool) :
    complete owns false transfer = false := rfl

theorem a_completed_borrow_can_end_before_the_parent_moves (transfer : Bool) :
    complete false true transfer = true := rfl

theorem an_owned_endpoint_still_requires_its_transfer (done : Bool) :
    complete true done false = false := by cases done <;> rfl

theorem a_transferred_endpoint_requires_its_operations (owns done : Bool) :
    complete owns done true = done := by cases owns <;> cases done <;> rfl

theorem old_completion_rule_blocks_release_before_transfer :
    complete true true false = false ∧ complete false true false = true := by decide

#print axioms held_views_share_a_visible_preview
#print axioms retired_views_cannot_supply_a_preview
#print axioms other_actors_cannot_supply_a_preview
#print axioms disjoint_scopes_cannot_supply_a_preview
#print axioms a_borrow_inside_an_arm_can_read_the_choice
#print axioms a_borrow_containing_the_choice_can_read_it
#print axioms old_arm_only_rule_misses_an_enclosing_borrow
#print axioms borrowing_never_completes_unfinished_operations
#print axioms a_completed_borrow_can_end_before_the_parent_moves
#print axioms an_owned_endpoint_still_requires_its_transfer
#print axioms a_transferred_endpoint_requires_its_operations
#print axioms old_completion_rule_blocks_release_before_transfer


-- Waiting on a separately owned payload blocks its move, but taking a
-- parallel borrow does not move that payload. Own-view prefixes still block.
inductive PrefixWait where
  | other
  | payload
  | own
  deriving DecidableEq

def prefixWait (own payload : Bool) : PrefixWait :=
  if own then .own else if payload then .payload else .other

def borrowCanWait : PrefixWait → Bool
  | .other | .payload => true
  | .own => false

def sendCanWait : PrefixWait → Bool
  | .other => true
  | .payload | .own => false

theorem send_wait_rule_is_unchanged (own payload : Bool) :
    sendCanWait (prefixWait own payload) = !(own || payload) := by
  cases own <;> cases payload <;> rfl

theorem borrowing_waits_only_for_other_views (own payload : Bool) :
    borrowCanWait (prefixWait own payload) = !own := by
  cases own <;> cases payload <;> rfl

theorem payload_wait_does_not_permit_a_send :
    borrowCanWait (prefixWait false true) = true ∧
    sendCanWait (prefixWait false true) = false := by decide

theorem an_own_prefix_blocks_both_calls (payload : Bool) :
    borrowCanWait (prefixWait true payload) = false ∧
    sendCanWait (prefixWait true payload) = false := by
  cases payload <;> decide

theorem every_send_wait_is_also_a_borrow_wait (wait : PrefixWait)
    (allowed : sendCanWait wait = true) : borrowCanWait wait = true := by
  cases wait <;> simp_all [sendCanWait, borrowCanWait]

#print axioms send_wait_rule_is_unchanged
#print axioms borrowing_waits_only_for_other_views
#print axioms payload_wait_does_not_permit_a_send
#print axioms an_own_prefix_blocks_both_calls
#print axioms every_send_wait_is_also_a_borrow_wait
end ParallelViewChoice
