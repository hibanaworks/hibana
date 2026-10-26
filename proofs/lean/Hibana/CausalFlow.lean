import Hibana.GlobalSyntax

namespace Hibana

/-- Pointwise causal facts. Indexed choreography queries respect `roleCount`;
Rust realizes the wire role domain with a fixed compile-time bitset. -/
abbrev CausalRoles := Nat → Bool

inductive CausalFlowExpr where
  | empty
  | seed (role : Nat)
  | relay (sender receiver : Nat)
  | seq (left right : CausalFlowExpr)
  | choice (left right : CausalFlowExpr)
  | parallel (left right : CausalFlowExpr)
  deriving Repr

def CausalRoles.insert (facts : CausalRoles) (role : Nat) : CausalRoles :=
  fun query => facts query || decide (query = role)

def CausalFlowExpr.eval : CausalFlowExpr → CausalRoles → CausalRoles
  | .empty, facts => facts
  | .seed role, facts => facts.insert role
  | .relay sender receiver, facts =>
      if facts sender then facts.insert receiver else facts
  | .seq left right, facts => right.eval (left.eval facts)
  | .choice left right, facts => fun role => left.eval facts role && right.eval facts role
  | .parallel left right, facts => fun role => left.eval facts role || right.eval facts role

def CausalRoles.Subset (left right : CausalRoles) : Prop :=
  ∀ role, left role = true → right role = true

theorem causal_flow_monotone (flow : CausalFlowExpr)
    {left right : CausalRoles} (subset : left.Subset right) :
    (flow.eval left).Subset (flow.eval right) := by
  induction flow generalizing left right with
  | empty => exact subset
  | seed role =>
      intro query found
      simp only [CausalFlowExpr.eval, CausalRoles.insert, Bool.or_eq_true] at found ⊢
      exact found.imp (subset query) id
  | relay sender receiver =>
      intro query found
      by_cases l : left sender = true
      · have r := subset sender l
        simpa [CausalFlowExpr.eval, l, r, CausalRoles.insert, Bool.or_eq_true] using
          (show left query = true ∨ query = receiver → right query = true ∨ query = receiver from
            fun h => h.imp (subset query) id) (by simpa [CausalFlowExpr.eval, l, CausalRoles.insert] using found)
      · by_cases r : right sender = true
        · simp only [CausalFlowExpr.eval, l, r, Bool.false_eq_true, ↓reduceIte] at found ⊢
          simp [CausalRoles.insert, subset query found]
        · simpa [CausalFlowExpr.eval, r] using subset query (by simpa [CausalFlowExpr.eval, l] using found)
  | seq leftFlow rightFlow leftIH rightIH => exact rightIH (leftIH subset)
  | choice leftFlow rightFlow leftIH rightIH =>
      intro role found
      simp only [CausalFlowExpr.eval, Bool.and_eq_true] at found ⊢
      exact ⟨leftIH subset role found.1, rightIH subset role found.2⟩
  | parallel leftFlow rightFlow leftIH rightIH =>
      intro role found
      simp only [CausalFlowExpr.eval, Bool.or_eq_true] at found ⊢
      exact found.imp (leftIH subset role) (rightIH subset role)

/-- An execution chooses one route arm. Parallel arms have the same input;
their outputs can meet only after the fork completes. -/
inductive CausalFlowRun : CausalFlowExpr → CausalRoles → CausalRoles → Prop where
  | empty (facts) : CausalFlowRun .empty facts facts
  | seed (facts role) : CausalFlowRun (.seed role) facts (facts.insert role)
  | relay (facts sender receiver) : CausalFlowRun (.relay sender receiver) facts
      ((.relay sender receiver : CausalFlowExpr).eval facts)
  | seq {left right before middle after}
      (first : CausalFlowRun left before middle) (second : CausalFlowRun right middle after) :
      CausalFlowRun (.seq left right) before after
  | left {left right before after} (run : CausalFlowRun left before after) :
      CausalFlowRun (.choice left right) before after
  | right {left right before after} (run : CausalFlowRun right before after) :
      CausalFlowRun (.choice left right) before after
  | parallel {left right before leftAfter rightAfter}
      (first : CausalFlowRun left before leftAfter) (second : CausalFlowRun right before rightAfter) :
      CausalFlowRun (.parallel left right) before (fun role => leftAfter role || rightAfter role)

theorem causal_flow_is_must_analysis
    {flow : CausalFlowExpr} {before after : CausalRoles}
    (run : CausalFlowRun flow before after) : (flow.eval before).Subset after := by
  induction run with
  | empty => exact fun _ h => h
  | seed => exact fun _ h => h
  | relay => exact fun _ h => h
  | seq _ _ firstIH secondIH =>
      exact fun role h => secondIH role (causal_flow_monotone _ firstIH role h)
  | left _ ih =>
      intro role h
      simp only [CausalFlowExpr.eval, Bool.and_eq_true] at h
      exact ih role h.1
  | right _ ih =>
      intro role h
      simp only [CausalFlowExpr.eval, Bool.and_eq_true] at h
      exact ih role h.2
  | parallel _ _ leftIH rightIH =>
      intro role h
      simp only [CausalFlowExpr.eval, Bool.or_eq_true] at h ⊢
      exact h.imp (leftIH role) (rightIH role)

private def flowRangeContains (base count index : Nat) : Bool :=
  base ≤ index && index < base + count

/-- The query prunes only an endpoint-fixed route arm or the target's own
parallel arm. Unknown routes remain intersections. A roll visit is one pass;
a reentry query composes two passes with different occurrence bases. -/
def Choreo.causalFlowAt (base earlier later roleCount : Nat) : Choreo → CausalFlowExpr
  | .send sender receiver _ _ =>
      if base < later && sender < roleCount && receiver < roleCount then
        if base = earlier then .seed receiver else .relay sender receiver
      else .empty
  | .seq left right => .seq (left.causalFlowAt base earlier later roleCount)
      (right.causalFlowAt (base + left.globalEvents.length) earlier later roleCount)
  | .route _ left right =>
      let split := base + left.globalEvents.length
      let leftFlow := left.causalFlowAt base earlier later roleCount
      let rightFlow := right.causalFlowAt split earlier later roleCount
      if flowRangeContains base left.globalEvents.length earlier ||
          flowRangeContains base left.globalEvents.length later then leftFlow
      else if flowRangeContains split right.globalEvents.length earlier ||
          flowRangeContains split right.globalEvents.length later then rightFlow
      else .choice leftFlow rightFlow
  | .par left right =>
      let split := base + left.globalEvents.length
      let leftFlow := left.causalFlowAt base earlier later roleCount
      let rightFlow := right.causalFlowAt split earlier later roleCount
      if flowRangeContains base left.globalEvents.length later then leftFlow
      else if flowRangeContains split right.globalEvents.length later then rightFlow
      else .parallel leftFlow rightFlow
  | .roll body => body.causalFlowAt base earlier later roleCount

def Choreo.causalFlow (choreo : Choreo) (earlier later roleCount : Nat) : CausalFlowExpr :=
  choreo.causalFlowAt 0 earlier later roleCount

def Choreo.rollCausalFlow (body : Choreo) (earlier later roleCount : Nat) : CausalFlowExpr :=
  .seq (body.causalFlowAt 0 earlier later roleCount)
    (body.causalFlowAt body.globalEvents.length earlier later roleCount)

theorem roll_causal_flow_composes_fresh_visits (body : Choreo) (earlier later roleCount : Nat)
    (facts : CausalRoles) :
    (body.rollCausalFlow earlier later roleCount).eval facts =
      (body.causalFlowAt body.globalEvents.length earlier later roleCount).eval
        ((body.causalFlowAt 0 earlier later roleCount).eval facts) := rfl

/-- No event in a subtree starting at the query cutoff can change its facts. -/
theorem causal_flow_after_cut_preserves_facts (choreo : Choreo)
    (base earlier cutoff roleCount : Nat) (facts : CausalRoles)
    (past : cutoff ≤ base) :
    (choreo.causalFlowAt base earlier cutoff roleCount).eval facts = facts := by
  induction choreo generalizing base with
  | send sender receiver label schema =>
      simp [Choreo.causalFlowAt, Nat.not_lt.mpr past, CausalFlowExpr.eval]
  | seq left right leftIH rightIH =>
      simp only [Choreo.causalFlowAt, CausalFlowExpr.eval]
      rw [leftIH base past, rightIH (base + left.globalEvents.length) (by omega)]
  | route authority left right leftIH rightIH =>
      have l := leftIH base past
      have r := rightIH (base + left.globalEvents.length) (by omega)
      simp only [Choreo.causalFlowAt]
      split
      · exact l
      · split
        · exact r
        · simp only [CausalFlowExpr.eval, l, r, Bool.and_self]
  | par left right leftIH rightIH =>
      have l := leftIH base past
      have r := rightIH (base + left.globalEvents.length) (by omega)
      simp only [Choreo.causalFlowAt]
      split
      · exact l
      · split
        · exact r
        · simp only [CausalFlowExpr.eval, l, r, Bool.or_self]
  | roll body ih => exact ih base past

/-- A repeated region relevant to a receive pair starts before both events. -/
theorem a_roll_containing_both_receives_precedes_their_earlier_offset
    (start stop earlier later : Nat)
    (first : start ≤ earlier ∧ earlier < stop)
    (second : start ≤ later ∧ later < stop) : start ≤ min earlier later := by
  omega

/-- Once ordered scope starts pass either event, the remaining scopes cannot
contain the pair, regardless of the order of their two visits. -/
theorem later_scope_starts_cannot_contain_the_receive_pair
    (start following stop earlier later : Nat)
    (ordered : start ≤ following)
    (past : earlier < start ∨ later < start) :
    ¬ (following ≤ earlier ∧ earlier < stop ∧ following ≤ later ∧ later < stop) := by
  omega

/-- Receives in one direction, ordered from latest to earliest. -/
def summarizeReceiveDirection : List (Nat × Nat) → Option ((Nat × Nat) × Option Nat)
  | [] => none
  | latest :: rest => some (latest,
      (rest.find? (fun event => event.1 != latest.1)).map Prod.snd)

def lookupReceiveDirection
    (summary : Option ((Nat × Nat) × Option Nat)) (sender : Nat) : Option Nat :=
  match summary with
  | none => none
  | some (latest, other) => if latest.1 = sender then other else some latest.2

theorem receive_direction_summary_preserves_latest_sender_change
    (events : List (Nat × Nat)) (sender : Nat) :
    lookupReceiveDirection (summarizeReceiveDirection events) sender =
      (events.find? (fun event => event.1 != sender)).map Prod.snd := by
  cases events with
  | nil => rfl
  | cons latest rest =>
      rcases latest with ⟨source, position⟩
      by_cases same : source = sender
      · subst sender
        simp [summarizeReceiveDirection, lookupReceiveDirection, List.find?]
      · have different : (source != sender) = true := by
          rw [bne_eq, beq_false_of_ne same]
          rfl
        simp [summarizeReceiveDirection, lookupReceiveDirection, List.find?, same, different]

def CausalFlowExpr.seedless : CausalFlowExpr → Bool
  | .empty | .relay _ _ => true
  | .seed _ => false
  | .seq left right | .choice left right | .parallel left right =>
      left.seedless && right.seedless

theorem seedless_flow_preserves_empty_facts (flow : CausalFlowExpr)
    (noSeed : flow.seedless = true) :
    flow.eval (fun _ => false) = (fun _ => false) := by
  induction flow with
  | empty => rfl
  | seed role => simp [CausalFlowExpr.seedless] at noSeed
  | relay sender receiver => simp [CausalFlowExpr.eval]
  | seq left right leftIH rightIH =>
      simp only [CausalFlowExpr.seedless, Bool.and_eq_true] at noSeed
      simp only [CausalFlowExpr.eval, leftIH noSeed.1, rightIH noSeed.2]
  | choice left right leftIH rightIH =>
      simp only [CausalFlowExpr.seedless, Bool.and_eq_true] at noSeed
      funext role
      simp [CausalFlowExpr.eval, leftIH noSeed.1, rightIH noSeed.2]
  | parallel left right leftIH rightIH =>
      simp only [CausalFlowExpr.seedless, Bool.and_eq_true] at noSeed
      funext role
      simp [CausalFlowExpr.eval, leftIH noSeed.1, rightIH noSeed.2]

theorem seedless_sequence_prefix_can_be_skipped (front body : CausalFlowExpr)
    (noSeed : front.seedless = true) :
    (.seq front body : CausalFlowExpr).eval (fun _ => false) =
      body.eval (fun _ => false) := by
  simp only [CausalFlowExpr.eval, seedless_flow_preserves_empty_facts front noSeed]

theorem seedless_parallel_left_can_be_skipped (left right : CausalFlowExpr)
    (noSeed : left.seedless = true) :
    (.parallel left right : CausalFlowExpr).eval (fun _ => false) =
      right.eval (fun _ => false) := by
  funext role
  simp [CausalFlowExpr.eval, seedless_flow_preserves_empty_facts left noSeed]

/-- Every possible receiver already has the causal fact. -/
def CausalFlowExpr.coveredBy (facts : CausalRoles) : CausalFlowExpr → Bool
  | .empty => true
  | .seed role => facts role
  | .relay _ receiver => facts receiver
  | .seq left right | .choice left right | .parallel left right =>
      left.coveredBy facts && right.coveredBy facts

theorem a_covered_flow_preserves_its_input (flow : CausalFlowExpr) (facts : CausalRoles)
    (covered : flow.coveredBy facts = true) : flow.eval facts = facts := by
  induction flow with
  | empty => rfl
  | seed role =>
      have known : facts role = true := covered
      funext query
      by_cases same : query = role
      · subst query
        simp [CausalFlowExpr.eval, CausalRoles.insert, known]
      · simp [CausalFlowExpr.eval, CausalRoles.insert, same]
  | relay sender receiver =>
      funext query
      have known : facts receiver = true := covered
      by_cases senderKnown : facts sender = true
      · by_cases same : query = receiver
        · subst query
          simp [CausalFlowExpr.eval, CausalRoles.insert, senderKnown, known]
        · simp [CausalFlowExpr.eval, CausalRoles.insert, senderKnown, same]
      · simp [CausalFlowExpr.eval, senderKnown]
  | seq left right leftIH rightIH =>
      simp only [CausalFlowExpr.coveredBy, Bool.and_eq_true] at covered
      simp only [CausalFlowExpr.eval, leftIH covered.1, rightIH covered.2]
  | choice left right leftIH rightIH =>
      simp only [CausalFlowExpr.coveredBy, Bool.and_eq_true] at covered
      funext role
      simp [CausalFlowExpr.eval, leftIH covered.1, rightIH covered.2]
  | parallel left right leftIH rightIH =>
      simp only [CausalFlowExpr.coveredBy, Bool.and_eq_true] at covered
      funext role
      simp [CausalFlowExpr.eval, leftIH covered.1, rightIH covered.2]

/-- Keep the earliest source position rejected by either possible owner path. -/
def firstCausalFailure : Option Nat → Option Nat → Option Nat
  | none, right => right
  | left, none => left
  | some left, some right => some (min left right)

theorem first_causal_failure_preserves_acceptance (left right : Option Nat) :
    firstCausalFailure left right = none ↔ left = none ∧ right = none := by
  cases left <;> cases right <;> simp [firstCausalFailure]

theorem first_causal_failure_is_earliest (left right : Option Nat) (event : Nat)
    (selected : firstCausalFailure left right = some event) :
    (left = some event ∨ right = some event) ∧
    (∀ earlier, left = some earlier ∨ right = some earlier → event ≤ earlier) := by
  cases left <;> cases right <;> simp_all [firstCausalFailure, Nat.min_def]
  split at * <;> simp_all <;> omega

/-- Relays, choices and parallel forks never remove an incoming fact. -/
theorem causal_flow_preserves_known_facts (flow : CausalFlowExpr) (facts : CausalRoles) :
    facts.Subset (flow.eval facts) := by
  induction flow with
  | empty => exact fun _ h => h
  | seed role =>
      intro query known
      simp [CausalFlowExpr.eval, CausalRoles.insert, known]
  | relay sender receiver =>
      intro query known
      by_cases h : facts sender = true <;>
        simp [CausalFlowExpr.eval, CausalRoles.insert, h, known]
  | seq left right leftIH rightIH =>
      intro role known
      exact causal_flow_monotone right leftIH role (rightIH role known)
  | choice left right leftIH rightIH =>
      intro role known
      simp [CausalFlowExpr.eval, leftIH role known, rightIH role known]
  | parallel left right leftIH rightIH =>
      intro role known
      simp [CausalFlowExpr.eval, leftIH role known]

/-- A target sender already known at the start stays known after every prefix. -/
theorem known_target_sender_needs_no_suffix_search
    (flow : CausalFlowExpr) (facts : CausalRoles) (sender : Nat)
    (known : facts sender = true) : flow.eval facts sender = true :=
  causal_flow_preserves_known_facts flow facts sender known

/-- Split seed queries while retaining every target of each query. -/
theorem receive_seed_partition_preserves_validation
    {α : Type} (seeds : List α) (cut : Nat) (valid : α → Bool) :
    seeds.all valid = ((seeds.take cut).all valid && (seeds.drop cut).all valid) := by
  rw [← List.all_append, List.take_append_drop]

/-- Source and occurrence cutoffs give the same order within one visit. -/
theorem source_cutoff_preserves_occurrence_order
    (source body iteration cutoff : Nat) (sourceIn : body ≤ source)
    (cutoffIn : iteration ≤ cutoff) :
    iteration + (source - body) < cutoff ↔ source < body + (cutoff - iteration) := by
  constructor
  · intro h
    have mid : source - body < cutoff - iteration := by
      apply Nat.lt_of_add_lt_add_left (n := iteration)
      simpa only [Nat.add_sub_of_le cutoffIn] using h
    simpa only [Nat.add_sub_of_le sourceIn] using Nat.add_lt_add_left mid body
  · intro h
    have mid : source - body < cutoff - iteration := by
      apply Nat.lt_of_add_lt_add_left (n := body)
      simpa only [Nat.add_sub_of_le sourceIn] using h
    simpa only [Nat.add_sub_of_le cutoffIn] using Nat.add_lt_add_left mid iteration

end Hibana
