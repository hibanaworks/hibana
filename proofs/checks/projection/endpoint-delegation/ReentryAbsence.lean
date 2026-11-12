import Std

namespace Hibana.ConstReentry

/-- Dependency closure can only mark uses already present in the source
interval. An outgoing resource absent even from the union of route arms
therefore cannot obstruct the located return, for any needed-bit assignment.
The return's existence is still checked independently by the Rust validator. -/
theorem an_absent_source_resource_has_no_needed_return_use
    {Key Event : Type} {outgoing : Key → Prop} {interval : Event → Prop}
    {uses : Key → Event → Prop} {needed : Event → Prop}
    (absent : ¬ ∃ key, outgoing key ∧ ∃ event, interval event ∧ uses key event) :
    ¬ ∃ key, outgoing key ∧ ∃ event, interval event ∧ uses key event ∧ needed event := by
  rintro ⟨key, sent, event, within, used, _⟩
  exact absent ⟨key, sent, event, within, used⟩

/-- Marking every present event in seeded actor components is already closed
under actor-sharing predecessors. Source-order predecessors are a subset of
these pairs, so the backward dependency pass cannot add an event outside this
cover. This concerns one actual choice/holder assignment; it does not merge
possible holders or supply permission between parallel siblings. -/
theorem component_event_cover_is_predecessor_closed
    {Event Actor : Type} {present : Event → Prop} {connected : Actor → Prop}
    {sender receiver : Event → Actor}
    (edge : ∀ event, present event →
      (connected (sender event) ↔ connected (receiver event)))
    {before after : Event}
    (beforePresent : present before)
    (afterCovered : present after ∧ connected (sender after))
    (shared : ∃ actor,
      (actor = sender before ∨ actor = receiver before) ∧
      (actor = sender after ∨ actor = receiver after)) :
    present before ∧ connected (sender before) := by
  obtain ⟨actor, beforeSide, afterSide⟩ := shared
  have actorConnected : connected actor := by
    rcases afterSide with h | h
    · simpa only [h] using afterCovered.2
    · simpa only [h] using (edge after afterCovered.1).mp afterCovered.2
  refine ⟨beforePresent, ?_⟩
  rcases beforeSide with h | h
  · simpa only [h] using actorConnected
  · exact (edge before beforePresent).mpr (by simpa only [h] using actorConnected)

end Hibana.ConstReentry

#print axioms Hibana.ConstReentry.an_absent_source_resource_has_no_needed_return_use
#print axioms Hibana.ConstReentry.component_event_cover_is_predecessor_closed
