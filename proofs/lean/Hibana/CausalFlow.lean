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

end Hibana
