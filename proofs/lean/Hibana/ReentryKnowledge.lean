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

end Hibana
