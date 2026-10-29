import Hibana.ActorSourceProgress

namespace Hibana
open EndpointDelegation

/-- A causally enclosing local resource stays absent throughout the remote
resource's absence before return acceptance. This uses only a causally closed cut: there is no shared
owner table, global clock or requirement to serialize independent actions.
Rust checks the two causal edges on selected source actions. -/
theorem a_causal_resource_guard_covers_the_remote_absence
    {Action : Type} {before : Action → Action → Prop} {occurred : Action → Prop}
    {guardSend resourceSend resourceReturn guardReceive : Action}
    (closed : ∀ {earlier later}, before earlier later → occurred later → occurred earlier)
    (departure : before guardSend resourceSend)
    (returned : before resourceReturn guardReceive)
    (away : occurred resourceSend ∧ ¬occurred resourceReturn) :
    occurred guardSend ∧ ¬occurred guardReceive := by
  exact ⟨closed departure away.1, fun guardBack => away.2 (closed returned guardBack)⟩

/-- Permission flows from the entry owner through this visit's actual messages
and strict actor-local source order. It is proof evidence, not runtime state. -/
inductive ReentryAuthority (source : Source) (sender receiver : Nat → Nat)
    (owner : Nat) : Nat → Prop where
  | owner {event : Nat} (owns : sender event = owner) :
      ReentryAuthority source sender receiver owner event
  | message {earlier event : Nat}
      (order : source.before earlier event = true)
      (recipient : receiver earlier = sender event)
      (prior : ReentryAuthority source sender receiver owner earlier) :
      ReentryAuthority source sender receiver owner event

/-- The source check follows receive-to-send edges backwards too. Thus every
incoming witness below also has its originating send in the same needed set.
An outside prefix is absent from both sets for the fresh visit. -/
theorem accepted_reentry_origins_have_owner_authority
    {source : Source} {sender receiver : Nat → Nat} {owner : Nat}
    {neededSend neededReceive : Nat → Prop}
    (wireClosed : ∀ event, neededReceive event → neededSend event)
    (origins : ∀ event, neededSend event → sender event = owner ∨
      ∃ earlier, neededReceive earlier ∧ source.before earlier event = true ∧
        receiver earlier = sender event) :
    ∀ event, neededSend event → ReentryAuthority source sender receiver owner event := by
  intro event
  induction event using Nat.strongRecOn with
  | ind event earlierIH =>
      intro needed
      rcases origins event needed with owns | ⟨earlier, incoming, order, recipient⟩
      · exact .owner owns
      · exact .message order recipient
          (earlierIH earlier (source_order_strictly_increases_event order)
            (wireClosed earlier incoming))

/-- Merely declaring a sibling permit in par gives no causal permission to
an independently enabled send. Sequencing an actual relay supplies that order. -/
theorem a_parallel_sibling_cannot_supply_reentry_authority
    (left right : Source) {earlier event : Nat}
    (inLeft : earlier < left.eventCount) (inRight : left.eventCount ≤ event) :
    (Source.par left right).before earlier event = false := by
  have notLeft : ¬event < left.eventCount := by omega
  have notRight : ¬left.eventCount ≤ earlier := by omega
  simp [Source.before, inLeft, notLeft, notRight]

/-- If no earlier incoming message can reach this actor, an accepted causal
origin must be the owner itself. A one-time prefix cannot grant a later visit. -/
theorem an_uninformed_reentry_origin_is_the_entry_owner
    {source : Source} {sender receiver : Nat → Nat} {owner event : Nat}
    (origin : sender event = owner ∨
      ∃ earlier, source.before earlier event = true ∧ receiver earlier = sender event)
    (uninformed : ∀ earlier, source.before earlier event = true →
      receiver earlier ≠ sender event) : sender event = owner := by
  rcases origin with owns | ⟨earlier, order, recipient⟩
  · exact owns
  · exact False.elim (uninformed earlier order recipient)

/-- Source-order edges increase the event number, and the wire edge keeps it.
Every intermediate point in a causal path therefore lies between its endpoints. -/
theorem causal_path_event_positions_are_monotone
    {Point : Type} {edge : Point → Point → Prop} (position : Point → Nat)
    (monotone : ∀ {a b}, edge a b → position a ≤ position b)
    {a b : Point} (path : Relation.TransGen edge a b) : position a ≤ position b := by
  induction path with
  | single step => exact monotone step
  | tail path step ih => exact Nat.le_trans ih (monotone step)

/-- A query about two actions needs no predecessor before its first event or
after its last event. This is a graph statement; Rust's bounded closure is
also compared with the complete scan over selected seq/par/route/roll sources. -/
theorem restricting_a_causal_query_to_its_event_interval_preserves_paths
    {Point : Type} {edge : Point → Point → Prop} (position : Point → Nat)
    (monotone : ∀ {a b}, edge a b → position a ≤ position b)
    (lower upper : Nat) {a b : Point}
    (starts : lower ≤ position a) (ends : position b ≤ upper) :
    Relation.TransGen edge a b ↔
      Relation.TransGen (fun x y => edge x y ∧ lower ≤ position x ∧ position y ≤ upper) a b := by
  constructor
  · intro path
    have restrict : ∀ {x y}, Relation.TransGen edge x y →
        lower ≤ position x → position y ≤ upper →
        Relation.TransGen (fun p q => edge p q ∧ lower ≤ position p ∧ position q ≤ upper) x y := by
      intro x y history
      induction history with
      | single step =>
          intro first last
          exact .single ⟨step, first, last⟩
      | tail priorPath step ih =>
          intro first last
          exact .tail (ih first (Nat.le_trans (monotone step) last))
            ⟨step, Nat.le_trans first (causal_path_event_positions_are_monotone position monotone priorPath), last⟩
    exact restrict path starts ends
  · intro path
    have forgetBounds : ∀ {x y},
        Relation.TransGen (fun p q => edge p q ∧ lower ≤ position p ∧ position q ≤ upper) x y →
        Relation.TransGen edge x y := by
      intro x y history
      induction history with
      | single step => exact .single step.1
      | tail _ step ih => exact .tail ih step.1
    exact forgetBounds path

/-- Rechecking the same resource under the same source path changes no verdict.
The representative retains the full endpoint key, including its component. -/
theorem checking_one_representative_per_resource_preserves_the_verdict
    {Index Key : Type} (key : Index → Key) (first : Index → Index)
    (same : ∀ i, key (first i) = key i)
    (stable : ∀ i, first (first i) = first i) (accepts : Key → Prop) :
    (∀ i, accepts (key i)) ↔ (∀ i, first i = i → accepts (key i)) := by
  constructor
  · intro all i _
    exact all i
  · intro representatives i
    have accepted := representatives (first i) (stable i)
    exact (same i) ▸ accepted

end Hibana
