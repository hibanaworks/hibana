import Hibana.ActorCommit

import Hibana.ActorReentryCommit

namespace Hibana

private theorem a_published_choice_keeps_its_local_scope
    {memberships : List (Nat × Nat)} {entry result : Nat × Option RouteArm}
    (published : publishObservedChoice? memberships entry = some result) :
    result.1 = entry.1 := by
  rcases entry with ⟨scope, previous⟩
  unfold publishObservedChoice? at published
  cases atScope : memberships.find? (fun member => member.1 == scope) with
  | none =>
    simp only [atScope, Option.pure_def, Option.some.injEq] at published
    exact (congrArg Prod.fst published).symm
  | some member =>
    simp only [atScope] at published
    split at published <;>
      simp only [Option.bind_eq_bind, Option.bind_some, Option.bind_none, Option.pure_def] at published
    all_goals first | contradiction | (split at published <;> simp_all <;> cases published <;> rfl)

theorem a_published_occurrence_teaches_its_local_route_arm
    {memberships : List (Nat × Nat)} {scope : Nat} {previous : Option RouteArm}
    {arm : RouteArm} {result : Nat × Option RouteArm}
    (membership : memberships.find? (fun member => member.1 == scope) =
      some (scope, encodeRouteArm arm))
    (published : publishObservedChoice? memberships (scope, previous) = some result) :
    result = (scope, some arm) := by
  cases arm <;>
    simp only [publishObservedChoice?, membership, encodeRouteArm, Option.bind_eq_bind, Option.bind_some] at published <;>
    split at published <;> simp_all

private theorem successful_mapM_preserves_first_key
    {f : (Nat × Option RouteArm) → Option (Nat × Option RouteArm)}
    {before after : List (Nat × Option RouteArm)} {scope : Nat} {entry : Nat × Option RouteArm}
    (keepsKey : ∀ input output, f input = some output → output.1 = input.1)
    (mapped : before.mapM f = some after)
    (found : before.find? (fun row => row.1 == scope) = some entry) :
    ∃ result, f entry = some result ∧
      after.find? (fun row => row.1 == scope) = some result := by
  induction before generalizing after with
  | nil => simp at found
  | cons head tail ih =>
    cases headAt : f head with
    | none => simp [headAt] at mapped
    | some output =>
      cases tailAt : tail.mapM f with
      | none => simp [headAt, tailAt] at mapped
      | some rest =>
        have same : output :: rest = after := Option.some.inj (by simpa [headAt, tailAt] using mapped)
        subst after
        have key := keepsKey head output headAt
        cases sameScope : head.1 == scope with
        | true =>
          have entrySame : head = entry := by
            simpa only [List.find?_cons, sameScope, if_true, Option.some.injEq] using found
          subst entry
          exact ⟨output, headAt, by simp only [List.find?_cons, key, sameScope]⟩
        | false =>
          have remains : tail.find? (fun row => row.1 == scope) = some entry := by
            simpa only [List.find?_cons, sameScope, Bool.false_eq_true, if_false] using found
          obtain ⟨result, atEntry, inRest⟩ := ih tailAt remains
          exact ⟨result, atEntry, by simp only [List.find?_cons, key, sameScope, inRest]⟩

/-- The same source occurrence publishes only the branch knowledge represented
by this actor's local slots. Actors need not share all choices or one complete
source path, and no new decision is inferred from application payload bytes. -/
theorem a_source_occurrence_publishes_its_observed_local_choice
    {source : CanonicalProgramSource} {eff scope : Nat} {arm : RouteArm}
    {memberships : List (Nat × Nat)} {before after : List (Nat × Option RouteArm)}
    {previous : Option RouteArm}
    (pathAt : canonicalEventRoutePath? source eff = some memberships)
    (membership : memberships.find? (fun member => member.1 == scope) =
      some (scope, encodeRouteArm arm))
    (localSlot : before.find? (fun row => row.1 == scope) = some (scope, previous))
    (published : publishedObservedChoices? source eff before = some after) :
    after.find? (fun row => row.1 == scope) = some (scope, some arm) := by
  have mapped : before.mapM (publishObservedChoice? memberships) = some after := by
    simpa only [publishedObservedChoices?, pathAt, Option.bind_some] using published
  obtain ⟨result, atEntry, found⟩ := successful_mapM_preserves_first_key
    (fun _ _ => a_published_choice_keeps_its_local_scope) mapped localSlot
  rw [a_published_occurrence_teaches_its_local_route_arm membership atEntry] at found
  exact found

theorem a_source_occurrence_preserves_an_unrelated_local_choice
    {source : CanonicalProgramSource} {eff scope : Nat} {arm : Option RouteArm}
    {memberships : List (Nat × Nat)} {before after : List (Nat × Option RouteArm)}
    (pathAt : canonicalEventRoutePath? source eff = some memberships)
    (unrelated : memberships.find? (fun member => member.1 == scope) = none)
    (localSlot : before.find? (fun row => row.1 == scope) = some (scope, arm))
    (published : publishedObservedChoices? source eff before = some after) :
    after.find? (fun row => row.1 == scope) = some (scope, arm) := by
  have mapped : before.mapM (publishObservedChoice? memberships) = some after := by
    simpa only [publishedObservedChoices?, pathAt, Option.bind_some] using published
  obtain ⟨result, atEntry, found⟩ := successful_mapM_preserves_first_key
    (fun _ _ => a_published_choice_keeps_its_local_scope) mapped localSlot
  have same : result = (scope, arm) := by
    simpa only [publishObservedChoice?, unrelated, Option.pure_def, Option.some.injEq] using atEntry.symm
  simpa only [same] using found

private theorem successful_mapM_reflects_first_key
    {f : (Nat × Option RouteArm) → Option (Nat × Option RouteArm)}
    {before after : List (Nat × Option RouteArm)} {scope : Nat} {entry : Nat × Option RouteArm}
    (keepsKey : ∀ input output, f input = some output → output.1 = input.1)
    (mapped : before.mapM f = some after)
    (found : after.find? (fun row => row.1 == scope) = some entry) :
    ∃ input, before.find? (fun row => row.1 == scope) = some input ∧ f input = some entry := by
  induction before generalizing after with
  | nil => simp at mapped; subst after; simp at found
  | cons head tail ih =>
    cases headAt : f head with
    | none => simp [headAt] at mapped
    | some output =>
      cases tailAt : tail.mapM f with
      | none => simp [headAt, tailAt] at mapped
      | some rest =>
        have same : output :: rest = after := Option.some.inj (by simpa [headAt, tailAt] using mapped)
        subst after
        have key := keepsKey head output headAt
        cases sameScope : head.1 == scope with
        | true =>
          have entrySame : output = entry := by
            simpa only [List.find?_cons, key, sameScope, if_true, Option.some.injEq] using found
          exact ⟨head, by simp only [List.find?_cons, sameScope], entrySame ▸ headAt⟩
        | false =>
          have remains : rest.find? (fun row => row.1 == scope) = some entry := by
            simpa only [List.find?_cons, key, sameScope, Bool.false_eq_true, if_false] using found
          obtain ⟨input, inputAt, valueAt⟩ := ih tailAt remains
          exact ⟨input, by simp only [List.find?_cons, sameScope, inputAt], valueAt⟩

private theorem a_published_choice_retains_existing_knowledge
    {memberships : List (Nat × Nat)} {scope : Nat} {arm : RouteArm}
    {result : Nat × Option RouteArm}
    (published : publishObservedChoice? memberships (scope, some arm) = some result) :
    result = (scope, some arm) := by
  unfold publishObservedChoice? at published
  cases found : memberships.find? (fun member => member.1 == scope) with
  | none => simpa only [found, Option.pure_def, Option.some.injEq] using published.symm
  | some member =>
    simp only [found] at published
    cases arm <;> split at published <;> simp_all
    all_goals
      have impossible := published.1
      change true = false at impossible
      cases impossible

private theorem a_newly_published_choice_has_its_source_membership
    {memberships : List (Nat × Nat)} {scope : Nat} {arm : RouteArm}
    (published : publishObservedChoice? memberships (scope, none) = some (scope, some arm)) :
    memberships.find? (fun member => member.1 == scope) = some (scope, encodeRouteArm arm) := by
  unfold publishObservedChoice? at published
  cases found : memberships.find? (fun member => member.1 == scope) with
  | none => simp [found] at published
  | some member =>
    have sameScope : member.1 = scope := beq_iff_eq.mp
      (List.find?_some (p := fun row : Nat × Nat => row.1 == scope) found)
    rcases member with ⟨memberScope, encoded⟩
    dsimp only at sameScope
    subst memberScope
    cases arm <;> cases encoded with
    | zero => simp_all [encodeRouteArm]
    | succ remaining => cases remaining <;> simp_all [encodeRouteArm]

/-- Successful publication never changes an already committed local choice.
A different arm needs a real source-derived reset before this operation. -/
theorem a_source_publication_preserves_existing_choice
    {source : CanonicalProgramSource} {eff scope : Nat} {arm : RouteArm}
    {before after : List (Nat × Option RouteArm)}
    (published : publishedObservedChoices? source eff before = some after)
    (known : before.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    after.find? (fun row => row.1 == scope) = some (scope, some arm) := by
  unfold publishedObservedChoices? at published
  obtain ⟨memberships, _, mapped⟩ := Option.bind_eq_some_iff.mp published
  obtain ⟨result, atEntry, found⟩ := successful_mapM_preserves_first_key
    (fun _ _ => a_published_choice_keeps_its_local_scope) mapped known
  simpa only [a_published_choice_retains_existing_knowledge atEntry] using found

/-- A selected slot comes either from the same prior local knowledge or from
this operation's immutable source membership. The latter starts with `none`.
No whole source path or other actor's choices are assumed. -/
theorem a_source_publication_has_an_existing_or_new_choice_origin
    {source : CanonicalProgramSource} {eff scope : Nat} {arm : RouteArm}
    {before after : List (Nat × Option RouteArm)}
    (published : publishedObservedChoices? source eff before = some after)
    (known : after.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    before.find? (fun row => row.1 == scope) = some (scope, some arm) ∨
      (before.find? (fun row => row.1 == scope) = some (scope, none) ∧
        ∃ memberships, canonicalEventRoutePath? source eff = some memberships ∧
          memberships.find? (fun row => row.1 == scope) = some (scope, encodeRouteArm arm)) := by
  unfold publishedObservedChoices? at published
  obtain ⟨memberships, pathAt, mapped⟩ := Option.bind_eq_some_iff.mp published
  obtain ⟨input, inputAt, valueAt⟩ := successful_mapM_reflects_first_key
    (fun _ _ => a_published_choice_keeps_its_local_scope) mapped known
  have sameScope : input.1 = scope := beq_iff_eq.mp
    (List.find?_some (p := fun row : Nat × Option RouteArm => row.1 == scope) inputAt)
  rcases input with ⟨inputScope, previous⟩
  dsimp only at sameScope
  subst inputScope
  cases previous with
  | none => exact .inr ⟨inputAt, memberships, pathAt,
      a_newly_published_choice_has_its_source_membership valueAt⟩
  | some previous =>
    have same := a_published_choice_retains_existing_knowledge valueAt
    have selected : some arm = some previous := congrArg Prod.snd same
    have armAt := Option.some.inj selected
    subst previous
    exact .inl inputAt

private theorem a_choice_reset_keeps_its_local_scope
    {source : CanonicalProgramSource} {body : Nat} {input output : Nat × Option RouteArm}
    (reset : (do let clear ← sourceRollResetsRoute? source body input.1
                 pure (input.1, if clear then none else input.2)) = some output) :
    output.1 = input.1 := by
  obtain ⟨clear, _, valueAt⟩ := Option.bind_eq_some_iff.mp reset
  exact (congrArg Prod.fst (Option.some.inj valueAt)).symm

/-- Knowledge retained by a prospective reset belongs outside that body's
route scopes. Repeating the same arm inside a body therefore gets a fresh
origin even when the two visible snapshots contain equal selected values. -/
theorem a_source_choice_reset_only_retains_unreset_knowledge
    {source : CanonicalProgramSource} {body scope : Nat} {arm : RouteArm}
    {before after : List (Nat × Option RouteArm)}
    (reset : resetObservedChoices? source body before = some after)
    (known : after.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    sourceRollResetsRoute? source body scope = some false ∧
      before.find? (fun row => row.1 == scope) = some (scope, some arm) := by
  obtain ⟨input, inputAt, valueAt⟩ := successful_mapM_reflects_first_key
    (fun _ _ => a_choice_reset_keeps_its_local_scope) reset known
  have sameScope : input.1 = scope := beq_iff_eq.mp
    (List.find?_some (p := fun row : Nat × Option RouteArm => row.1 == scope) inputAt)
  rcases input with ⟨inputScope, previous⟩
  dsimp only at sameScope
  subst inputScope
  obtain ⟨clear, clearAt, selected⟩ := Option.bind_eq_some_iff.mp valueAt
  cases clear with
  | true => simp at selected
  | false =>
    have same : previous = some arm := congrArg Prod.snd (Option.some.inj selected)
    exact ⟨clearAt, by simpa only [same] using inputAt⟩

theorem a_source_choice_reset_preserves_outside_knowledge
    {source : CanonicalProgramSource} {body scope : Nat} {arm : RouteArm}
    {before after : List (Nat × Option RouteArm)}
    (reset : resetObservedChoices? source body before = some after)
    (outside : sourceRollResetsRoute? source body scope = some false)
    (known : before.find? (fun row => row.1 == scope) = some (scope, some arm)) :
    after.find? (fun row => row.1 == scope) = some (scope, some arm) := by
  obtain ⟨result, valueAt, found⟩ := successful_mapM_preserves_first_key
    (fun _ _ => a_choice_reset_keeps_its_local_scope) reset known
  have same : result = (scope, some arm) := by
    simpa [outside] using valueAt.symm
  simpa only [same] using found

end Hibana
